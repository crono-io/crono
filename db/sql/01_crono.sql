-- Canonical Crono schema bootstrap.
--
-- The initial schema stores catalog identities, immutable no-op Job versions,
-- Runs, and their transactional outbox. Keep this file idempotent so local
-- development can apply it whenever the server starts.

CREATE SCHEMA IF NOT EXISTS crono AUTHORIZATION crono_owner;

COMMENT ON SCHEMA crono IS 'Authoritative Crono control-plane data';

CREATE TABLE IF NOT EXISTS crono.namespaces (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    name text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT namespaces_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE TABLE IF NOT EXISTS crono.jobs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT jobs_namespace_name_unique UNIQUE (namespace_id, name),
    CONSTRAINT jobs_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE TABLE IF NOT EXISTS crono.job_versions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    job_id uuid NOT NULL REFERENCES crono.jobs(id) ON DELETE RESTRICT,
    version integer NOT NULL,
    executor text NOT NULL,
    queue text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT job_versions_job_version_unique UNIQUE (job_id, version),
    CONSTRAINT job_versions_positive_version CHECK (version > 0),
    CONSTRAINT job_versions_executor_supported CHECK (executor = 'noop'),
    CONSTRAINT job_versions_queue_canonical CHECK (
        queue ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE TABLE IF NOT EXISTS crono.targets (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT targets_namespace_name_unique UNIQUE (namespace_id, name),
    CONSTRAINT targets_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE TABLE IF NOT EXISTS crono.runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    request_id uuid NOT NULL UNIQUE,
    job_version_id uuid NOT NULL REFERENCES crono.job_versions(id) ON DELETE RESTRICT,
    target_id uuid NOT NULL REFERENCES crono.targets(id) ON DELETE RESTRICT,
    status text NOT NULL DEFAULT 'pending_dispatch',
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    dispatched_at timestamptz,
    nats_stream_sequence bigint,
    CONSTRAINT runs_status_supported CHECK (
        status IN ('pending_dispatch', 'dispatched')
    ),
    CONSTRAINT runs_dispatch_state_consistent CHECK (
        (status = 'pending_dispatch' AND dispatched_at IS NULL AND nats_stream_sequence IS NULL)
        OR
        (status = 'dispatched' AND dispatched_at IS NOT NULL AND nats_stream_sequence IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS crono.outbox (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL UNIQUE REFERENCES crono.runs(id) ON DELETE RESTRICT,
    subject text NOT NULL,
    payload jsonb NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    published_at timestamptz,
    CONSTRAINT outbox_attempts_nonnegative CHECK (attempts >= 0),
    CONSTRAINT outbox_error_bounded CHECK (
        last_error IS NULL OR char_length(last_error) <= 1024
    ),
    CONSTRAINT outbox_subject_dispatch CHECK (
        subject ~ '^crono[.]dispatch[.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE INDEX IF NOT EXISTS jobs_namespace_name_idx
    ON crono.jobs (namespace_id, name);
CREATE INDEX IF NOT EXISTS job_versions_job_version_idx
    ON crono.job_versions (job_id, version DESC);
CREATE INDEX IF NOT EXISTS targets_namespace_name_idx
    ON crono.targets (namespace_id, name);
CREATE INDEX IF NOT EXISTS runs_created_idx
    ON crono.runs (id DESC);
CREATE INDEX IF NOT EXISTS outbox_pending_idx
    ON crono.outbox (id) WHERE published_at IS NULL;

CREATE OR REPLACE FUNCTION crono.enforce_run_namespace_match()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, crono
AS $$
DECLARE
    job_namespace_id uuid;
    target_namespace_id uuid;
BEGIN
    SELECT jobs.namespace_id
      INTO STRICT job_namespace_id
      FROM crono.job_versions
      JOIN crono.jobs ON jobs.id = job_versions.job_id
     WHERE job_versions.id = NEW.job_version_id;

    SELECT targets.namespace_id
      INTO STRICT target_namespace_id
      FROM crono.targets
     WHERE targets.id = NEW.target_id;

    IF job_namespace_id <> target_namespace_id THEN
        RAISE EXCEPTION 'Run Job and Target must belong to the same Namespace'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS runs_namespace_match ON crono.runs;
CREATE CONSTRAINT TRIGGER runs_namespace_match
AFTER INSERT OR UPDATE OF job_version_id, target_id ON crono.runs
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION crono.enforce_run_namespace_match();
