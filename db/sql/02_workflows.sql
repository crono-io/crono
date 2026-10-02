-- Workflows orchestrate ordinary Runs. Definitions and immutable invocation
-- snapshots are separate; the event queue is committed with Run terminal state.
BEGIN;
CREATE TABLE IF NOT EXISTS crono.workflows (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    name text NOT NULL CHECK (name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    description text CHECK (char_length(description) <= 500),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    UNIQUE (namespace_id, name)
);
CREATE TABLE IF NOT EXISTS crono.workflow_nodes (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    workflow_id uuid NOT NULL REFERENCES crono.workflows(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    job_id uuid NOT NULL REFERENCES crono.jobs(id) ON DELETE RESTRICT,
    UNIQUE (workflow_id, name), UNIQUE (workflow_id, id)
);
CREATE TABLE IF NOT EXISTS crono.workflow_edges (
    workflow_id uuid NOT NULL REFERENCES crono.workflows(id) ON DELETE CASCADE,
    from_node_id uuid NOT NULL,
    to_node_id uuid NOT NULL,
    condition text NOT NULL CHECK (condition IN ('success', 'failure', 'always')),
    PRIMARY KEY (workflow_id, from_node_id, to_node_id),
    CHECK (from_node_id <> to_node_id),
    FOREIGN KEY (workflow_id, from_node_id) REFERENCES crono.workflow_nodes(workflow_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workflow_id, to_node_id) REFERENCES crono.workflow_nodes(workflow_id, id) ON DELETE CASCADE
);

-- Deferred validation also protects faulty adapters; replacement is one transaction.
CREATE OR REPLACE FUNCTION crono.validate_workflow_graph()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, crono AS $$
DECLARE
    graph_id uuid;
    graph_namespace uuid;
    node_count bigint;
BEGIN
    IF TG_TABLE_NAME = 'workflows' THEN graph_id := COALESCE(NEW.id, OLD.id);
    ELSE graph_id := COALESCE(NEW.workflow_id, OLD.workflow_id); END IF;
    SELECT namespace_id INTO graph_namespace FROM crono.workflows WHERE id = graph_id FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT count(*) INTO node_count FROM crono.workflow_nodes WHERE workflow_id = graph_id;
    IF node_count NOT BETWEEN 1 AND 64 OR
       (SELECT count(*) FROM crono.workflow_edges WHERE workflow_id = graph_id) > 256 OR
       EXISTS (SELECT 1 FROM crono.workflow_nodes n JOIN crono.jobs j ON j.id = n.job_id
                WHERE n.workflow_id = graph_id AND j.namespace_id <> graph_namespace) THEN
        RAISE EXCEPTION 'invalid Workflow nodes or Namespace references' USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        WITH RECURSIVE reach(origin, destination) AS (
            SELECT from_node_id, to_node_id FROM crono.workflow_edges WHERE workflow_id = graph_id
            UNION
            SELECT r.origin, e.to_node_id FROM reach r JOIN crono.workflow_edges e
              ON e.workflow_id = graph_id AND e.from_node_id = r.destination
        ) SELECT 1 FROM reach WHERE origin = destination
    ) THEN RAISE EXCEPTION 'Workflow contains a cycle' USING ERRCODE = '23514'; END IF;
    RETURN NULL;
END;
$$;
DROP TRIGGER IF EXISTS workflows_valid_graph ON crono.workflows;
CREATE CONSTRAINT TRIGGER workflows_valid_graph AFTER INSERT OR UPDATE ON crono.workflows
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION crono.validate_workflow_graph();
DROP TRIGGER IF EXISTS workflow_nodes_valid_graph ON crono.workflow_nodes;
CREATE CONSTRAINT TRIGGER workflow_nodes_valid_graph AFTER INSERT OR UPDATE OR DELETE ON crono.workflow_nodes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION crono.validate_workflow_graph();
DROP TRIGGER IF EXISTS workflow_edges_valid_graph ON crono.workflow_edges;
CREATE CONSTRAINT TRIGGER workflow_edges_valid_graph AFTER INSERT OR UPDATE OR DELETE ON crono.workflow_edges
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION crono.validate_workflow_graph();

CREATE TABLE IF NOT EXISTS crono.workflow_runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    request_id uuid NOT NULL UNIQUE,
    workflow_id uuid NOT NULL REFERENCES crono.workflows(id) ON DELETE RESTRICT,
    namespace_id uuid NOT NULL REFERENCES crono.namespaces(id) ON DELETE RESTRICT,
    definition_snapshot jsonb NOT NULL CHECK (jsonb_typeof(definition_snapshot) = 'object'),
    target_id uuid REFERENCES crono.targets(id) ON DELETE RESTRICT,
    target_set_id uuid REFERENCES crono.target_sets(id) ON DELETE RESTRICT,
    inputs jsonb NOT NULL CHECK (jsonb_typeof(inputs) = 'object'),
    state text NOT NULL DEFAULT 'running' CHECK (state IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    cancellation_requested boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    started_at timestamptz,
    finished_at timestamptz,
    CHECK ((target_id IS NOT NULL)::integer + (target_set_id IS NOT NULL)::integer = 1)
);
CREATE INDEX IF NOT EXISTS workflow_runs_history_idx ON crono.workflow_runs (workflow_id, id DESC);
CREATE TABLE IF NOT EXISTS crono.workflow_node_runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    workflow_run_id uuid NOT NULL REFERENCES crono.workflow_runs(id) ON DELETE RESTRICT,
    workflow_node_id uuid NOT NULL,
    name text NOT NULL,
    job_id uuid NOT NULL REFERENCES crono.jobs(id) ON DELETE RESTRICT,
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'ready', 'running', 'succeeded', 'failed', 'skipped', 'cancelled', 'unknown')),
    started_at timestamptz,
    finished_at timestamptz,
    UNIQUE (workflow_run_id, workflow_node_id), UNIQUE (workflow_run_id, id)
);
CREATE TABLE IF NOT EXISTS crono.workflow_run_edges (
    workflow_run_id uuid NOT NULL REFERENCES crono.workflow_runs(id) ON DELETE RESTRICT,
    from_node_run_id uuid NOT NULL,
    to_node_run_id uuid NOT NULL,
    condition text NOT NULL CHECK (condition IN ('success', 'failure', 'always')),
    PRIMARY KEY (workflow_run_id, from_node_run_id, to_node_run_id),
    FOREIGN KEY (workflow_run_id, from_node_run_id) REFERENCES crono.workflow_node_runs(workflow_run_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (workflow_run_id, to_node_run_id) REFERENCES crono.workflow_node_runs(workflow_run_id, id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS workflow_run_edges_incoming_idx ON crono.workflow_run_edges (to_node_run_id);
CREATE TABLE IF NOT EXISTS crono.workflow_node_executions (
    node_run_id uuid NOT NULL REFERENCES crono.workflow_node_runs(id) ON DELETE RESTRICT,
    target_id uuid NOT NULL REFERENCES crono.targets(id) ON DELETE RESTRICT,
    prospective_run_id uuid NOT NULL UNIQUE,
    run_id uuid UNIQUE REFERENCES crono.runs(id) ON DELETE RESTRICT,
    queue_id uuid NOT NULL REFERENCES crono.queues(id) ON DELETE RESTRICT,
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 100),
    execution_snapshot jsonb NOT NULL CHECK (jsonb_typeof(execution_snapshot) = 'object'),
    PRIMARY KEY (node_run_id, target_id)
);
CREATE TABLE IF NOT EXISTS crono.workflow_completion_events (
    run_id uuid PRIMARY KEY REFERENCES crono.runs(id) ON DELETE RESTRICT,
    workflow_run_id uuid NOT NULL REFERENCES crono.workflow_runs(id) ON DELETE RESTRICT,
    node_run_id uuid NOT NULL REFERENCES crono.workflow_node_runs(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT statement_timestamp()
);
CREATE INDEX IF NOT EXISTS workflow_completion_events_pending_idx ON crono.workflow_completion_events (workflow_run_id, node_run_id);

-- All ordinary terminal paths (completion, dispatch expiry, ambiguous lease)
-- enqueue orchestration in the same transaction, without workflow-aware workers.
CREATE OR REPLACE FUNCTION crono.enqueue_workflow_completion()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, crono AS $$
BEGIN
    IF NEW.status <> OLD.status AND NEW.status IN ('succeeded', 'failed', 'dead', 'skipped', 'cancelled', 'unknown') THEN
        INSERT INTO crono.workflow_completion_events (run_id, workflow_run_id, node_run_id)
        SELECT NEW.id, n.workflow_run_id, n.id FROM crono.workflow_node_executions e
          JOIN crono.workflow_node_runs n ON n.id = e.node_run_id WHERE e.run_id = NEW.id
        ON CONFLICT (run_id) DO NOTHING;
    END IF;
    RETURN NULL;
END;
$$;
DROP TRIGGER IF EXISTS runs_workflow_completion ON crono.runs;
CREATE TRIGGER runs_workflow_completion AFTER UPDATE OF status ON crono.runs
FOR EACH ROW EXECUTE FUNCTION crono.enqueue_workflow_completion();
COMMIT;
