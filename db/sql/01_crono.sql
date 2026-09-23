-- Canonical draft schema for Crono.
--
-- PostgreSQL is the authority for schedules, execution intent, dispatch,
-- retries, leases, and audit history. JetStream is a rebuildable transport.
-- Reset databases created from the previous draft before applying this schema.

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
    executor text NOT NULL DEFAULT 'noop',
    queue text NOT NULL DEFAULT 'default',
    executable text,
    arguments jsonb NOT NULL DEFAULT '[]'::jsonb,
    idempotent boolean NOT NULL DEFAULT false,
    max_attempts integer NOT NULL DEFAULT 1,
    retry_initial_seconds integer NOT NULL DEFAULT 1,
    retry_max_seconds integer NOT NULL DEFAULT 60,
    retry_multiplier double precision NOT NULL DEFAULT 2.0,
    retry_jitter double precision NOT NULL DEFAULT 0.2,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT jobs_namespace_name_unique UNIQUE (namespace_id, name),
    CONSTRAINT jobs_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT jobs_queue_canonical CHECK (
        queue ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT jobs_executor_supported CHECK (executor IN ('noop', 'process')),
    CONSTRAINT jobs_process_configuration CHECK (
        (executor = 'noop' AND executable IS NULL)
        OR (executor = 'process' AND executable LIKE '/%')
    ),
    CONSTRAINT jobs_arguments_array CHECK (jsonb_typeof(arguments) = 'array'),
    CONSTRAINT jobs_retry_valid CHECK (
        max_attempts BETWEEN 1 AND 100
        AND retry_initial_seconds BETWEEN 1 AND 86400
        AND retry_max_seconds BETWEEN retry_initial_seconds AND 86400
        AND retry_multiplier >= 1.0
        AND retry_jitter BETWEEN 0.0 AND 1.0
    )
);

CREATE TABLE IF NOT EXISTS crono.targets (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    name text NOT NULL,
    arguments jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT targets_namespace_name_unique UNIQUE (namespace_id, name),
    CONSTRAINT targets_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT targets_arguments_array CHECK (jsonb_typeof(arguments) = 'array')
);

CREATE TABLE IF NOT EXISTS crono.schedules (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    job_id uuid NOT NULL REFERENCES crono.jobs(id) ON DELETE RESTRICT,
    target_id uuid NOT NULL REFERENCES crono.targets(id) ON DELETE RESTRICT,
    name text NOT NULL,
    schedule_type text NOT NULL,
    cron_expression text,
    execute_at timestamptz,
    timezone text NOT NULL DEFAULT 'UTC',
    enabled boolean NOT NULL DEFAULT true,
    next_run_at timestamptz,
    last_run_at timestamptz,
    misfire_policy text NOT NULL DEFAULT 'run_late',
    misfire_grace_seconds integer,
    catchup_policy text NOT NULL DEFAULT 'run_once',
    max_catchup_runs integer NOT NULL DEFAULT 100,
    max_catchup_age_seconds integer NOT NULL DEFAULT 86400,
    revision bigint NOT NULL DEFAULT 1,
    claim_owner uuid,
    claim_expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT schedules_namespace_name_unique UNIQUE (namespace_id, name),
    CONSTRAINT schedules_name_canonical CHECK (
        name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT schedules_shape CHECK (
        (schedule_type = 'cron' AND cron_expression IS NOT NULL AND execute_at IS NULL)
        OR (schedule_type = 'once' AND cron_expression IS NULL AND execute_at IS NOT NULL)
    ),
    CONSTRAINT schedules_misfire_supported CHECK (
        misfire_policy IN ('run_late', 'skip', 'grace_period')
    ),
    CONSTRAINT schedules_misfire_grace_valid CHECK (
        (misfire_policy = 'grace_period' AND misfire_grace_seconds >= 0)
        OR (misfire_policy <> 'grace_period' AND misfire_grace_seconds IS NULL)
    ),
    CONSTRAINT schedules_catchup_supported CHECK (
        catchup_policy IN ('skip', 'run_once', 'catch_up')
    ),
    CONSTRAINT schedules_catchup_bounds CHECK (
        max_catchup_runs BETWEEN 1 AND 1000
        AND max_catchup_age_seconds BETWEEN 60 AND 31536000
    ),
    CONSTRAINT schedules_revision_positive CHECK (revision > 0)
);

CREATE TABLE IF NOT EXISTS crono.runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    request_id uuid UNIQUE,
    schedule_id uuid REFERENCES crono.schedules(id) ON DELETE RESTRICT,
    job_id uuid NOT NULL REFERENCES crono.jobs(id) ON DELETE RESTRICT,
    target_id uuid NOT NULL REFERENCES crono.targets(id) ON DELETE RESTRICT,
    scheduled_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'pending_dispatch',
    execution_snapshot jsonb NOT NULL,
    attempt_count integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL,
    next_retry_at timestamptz,
    lateness_seconds bigint NOT NULL DEFAULT 0,
    terminal_reason text,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    queued_at timestamptz,
    started_at timestamptz,
    completed_at timestamptz,
    CONSTRAINT runs_status_supported CHECK (
        status IN (
            'pending_dispatch', 'queued', 'running', 'retry_wait',
            'succeeded', 'failed', 'dead', 'skipped', 'cancelled', 'unknown'
        )
    ),
    CONSTRAINT runs_attempt_bounds CHECK (
        attempt_count >= 0 AND max_attempts BETWEEN 1 AND 100
        AND attempt_count <= max_attempts
    ),
    CONSTRAINT runs_lateness_nonnegative CHECK (lateness_seconds >= 0),
    CONSTRAINT runs_snapshot_object CHECK (jsonb_typeof(execution_snapshot) = 'object')
);

CREATE UNIQUE INDEX IF NOT EXISTS runs_schedule_occurrence_unique
    ON crono.runs (schedule_id, scheduled_at) WHERE schedule_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS crono.run_attempts (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL REFERENCES crono.runs(id) ON DELETE RESTRICT,
    attempt integer NOT NULL,
    status text NOT NULL DEFAULT 'pending_dispatch',
    worker_id text,
    started_at timestamptz,
    heartbeat_at timestamptz,
    lease_expires_at timestamptz,
    completed_at timestamptz,
    exit_code integer,
    stdout_tail text,
    stderr_tail text,
    error text,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT run_attempts_run_attempt_unique UNIQUE (run_id, attempt),
    CONSTRAINT run_attempts_attempt_positive CHECK (attempt > 0),
    CONSTRAINT run_attempts_status_supported CHECK (
        status IN ('pending_dispatch', 'queued', 'running', 'succeeded', 'failed', 'dead', 'unknown')
    ),
    CONSTRAINT run_attempts_output_bounded CHECK (
        (stdout_tail IS NULL OR octet_length(stdout_tail) <= 65536)
        AND (stderr_tail IS NULL OR octet_length(stderr_tail) <= 65536)
        AND (error IS NULL OR char_length(error) <= 1024)
    )
);

CREATE TABLE IF NOT EXISTS crono.worker_presence (
    worker_id text PRIMARY KEY,
    session_id uuid NOT NULL,
    queue text NOT NULL,
    concurrency integer NOT NULL,
    version text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT worker_presence_id_canonical CHECK (
        worker_id ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT worker_presence_queue_canonical CHECK (
        queue ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT worker_presence_concurrency_bounded CHECK (concurrency BETWEEN 1 AND 256),
    CONSTRAINT worker_presence_version_bounded CHECK (char_length(version) BETWEEN 1 AND 128),
    CONSTRAINT worker_presence_timestamps_ordered CHECK (last_seen_at >= started_at)
);

CREATE TABLE IF NOT EXISTS crono.outbox (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL REFERENCES crono.runs(id) ON DELETE RESTRICT,
    attempt_id uuid NOT NULL UNIQUE REFERENCES crono.run_attempts(id) ON DELETE RESTRICT,
    event_type text NOT NULL DEFAULT 'execute',
    subject text NOT NULL,
    payload jsonb NOT NULL,
    dispatch_deadline timestamptz,
    attempt_count integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    claimed_by uuid,
    claim_expires_at timestamptz,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    published_at timestamptz,
    cancelled_at timestamptz,
    nats_stream_sequence bigint,
    CONSTRAINT outbox_attempts_nonnegative CHECK (attempt_count >= 0),
    CONSTRAINT outbox_error_bounded CHECK (last_error IS NULL OR char_length(last_error) <= 1024),
    CONSTRAINT outbox_subject_dispatch CHECK (
        subject ~ '^crono[.]dispatch[.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT outbox_payload_object CHECK (jsonb_typeof(payload) = 'object')
);

CREATE TABLE IF NOT EXISTS crono.run_events (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL REFERENCES crono.runs(id) ON DELETE RESTRICT,
    event_type text NOT NULL,
    detail jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT run_events_detail_object CHECK (jsonb_typeof(detail) = 'object')
);

CREATE TABLE IF NOT EXISTS crono.schedule_events (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    schedule_id uuid NOT NULL REFERENCES crono.schedules(id) ON DELETE RESTRICT,
    event_type text NOT NULL,
    scheduled_at timestamptz,
    detail jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    CONSTRAINT schedule_events_detail_object CHECK (jsonb_typeof(detail) = 'object')
);

CREATE INDEX IF NOT EXISTS schedules_due_idx
    ON crono.schedules (next_run_at, id)
    WHERE enabled = true AND next_run_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS schedules_expired_claim_idx
    ON crono.schedules (claim_expires_at) WHERE claim_owner IS NOT NULL;
CREATE INDEX IF NOT EXISTS runs_created_idx ON crono.runs (id DESC);
CREATE INDEX IF NOT EXISTS runs_active_status_idx
    ON crono.runs (status)
    WHERE status IN ('pending_dispatch', 'queued', 'running');
CREATE INDEX IF NOT EXISTS runs_retry_due_idx
    ON crono.runs (next_retry_at, id) WHERE status = 'retry_wait';
CREATE INDEX IF NOT EXISTS run_attempts_expired_lease_idx
    ON crono.run_attempts (lease_expires_at, id) WHERE status = 'running';
CREATE INDEX IF NOT EXISTS worker_presence_last_seen_idx
    ON crono.worker_presence (last_seen_at);
CREATE INDEX IF NOT EXISTS outbox_pending_idx
    ON crono.outbox (next_attempt_at, id)
    WHERE published_at IS NULL AND cancelled_at IS NULL;
CREATE INDEX IF NOT EXISTS run_events_run_created_idx
    ON crono.run_events (run_id, created_at);
CREATE INDEX IF NOT EXISTS schedule_events_schedule_created_idx
    ON crono.schedule_events (schedule_id, created_at);

CREATE OR REPLACE FUNCTION crono.enforce_run_namespace_match()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, crono
AS $$
DECLARE
    job_namespace_id uuid;
    target_namespace_id uuid;
BEGIN
    SELECT namespace_id INTO STRICT job_namespace_id FROM crono.jobs WHERE id = NEW.job_id;
    SELECT namespace_id INTO STRICT target_namespace_id FROM crono.targets WHERE id = NEW.target_id;
    IF job_namespace_id <> target_namespace_id THEN
        RAISE EXCEPTION 'Job and Target must belong to the same Namespace'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION crono.enforce_schedule_namespace_match()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, crono
AS $$
DECLARE
    job_namespace_id uuid;
    target_namespace_id uuid;
BEGIN
    SELECT namespace_id INTO STRICT job_namespace_id FROM crono.jobs WHERE id = NEW.job_id;
    SELECT namespace_id INTO STRICT target_namespace_id FROM crono.targets WHERE id = NEW.target_id;
    IF NEW.namespace_id <> job_namespace_id OR job_namespace_id <> target_namespace_id THEN
        RAISE EXCEPTION 'Schedule, Job, and Target must belong to the same Namespace'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS schedules_namespace_match ON crono.schedules;
CREATE CONSTRAINT TRIGGER schedules_namespace_match
AFTER INSERT OR UPDATE OF namespace_id, job_id, target_id ON crono.schedules
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION crono.enforce_schedule_namespace_match();

DROP TRIGGER IF EXISTS runs_namespace_match ON crono.runs;
CREATE CONSTRAINT TRIGGER runs_namespace_match
AFTER INSERT OR UPDATE OF job_id, target_id ON crono.runs
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION crono.enforce_run_namespace_match();
