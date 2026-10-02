-- Post-bootstrap verification for Crono.
--
-- Run as a PostgreSQL administrator against the postgres database:
--   psql "postgres://<admin>@<host>:5432/postgres" \
--     -v ON_ERROR_STOP=1 -f db/sql/check.sql

\set ON_ERROR_STOP 1

\echo 'Checking Crono database and roles...'

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'crono') THEN
        RAISE EXCEPTION 'Missing database: crono';
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM pg_database AS d
        JOIN pg_roles AS r ON r.oid = d.datdba
        WHERE d.datname = 'crono'
          AND r.rolname = 'crono_owner'
    ) THEN
        RAISE EXCEPTION 'Database crono is not owned by crono_owner';
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM pg_roles
        WHERE rolname = 'crono_owner'
          AND NOT rolcanlogin
          AND NOT rolsuper
          AND NOT rolcreatedb
          AND NOT rolcreaterole
          AND NOT rolreplication
          AND NOT rolbypassrls
    ) THEN
        RAISE EXCEPTION 'Missing or misconfigured role: crono_owner';
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM pg_roles
        WHERE rolname = 'crono_runtime'
          AND rolcanlogin
          AND NOT rolsuper
          AND NOT rolcreatedb
          AND NOT rolcreaterole
          AND NOT rolreplication
          AND NOT rolbypassrls
    ) THEN
        RAISE EXCEPTION 'Missing or over-privileged role: crono_runtime';
    END IF;
    IF NOT has_database_privilege('crono_runtime', 'crono', 'CONNECT') THEN
        RAISE EXCEPTION 'Missing grant: crono_runtime CONNECT on crono';
    END IF;
    IF NOT has_database_privilege('crono_runtime', 'crono', 'TEMPORARY') THEN
        RAISE EXCEPTION 'Missing grant: crono_runtime TEMPORARY on crono';
    END IF;
END;
$$;

\connect crono

\echo 'Checking Crono schema and grants...'

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_namespace AS n
        JOIN pg_roles AS r ON r.oid = n.nspowner
        WHERE n.nspname = 'crono'
          AND r.rolname = 'crono_owner'
    ) THEN
        RAISE EXCEPTION 'Missing or misowned schema: crono';
    END IF;
    IF NOT has_schema_privilege('crono_runtime', 'crono', 'USAGE') THEN
        RAISE EXCEPTION 'Missing grant: crono_runtime USAGE on schema crono';
    END IF;
    IF has_schema_privilege('crono_runtime', 'crono', 'CREATE') THEN
        RAISE EXCEPTION 'Unexpected grant: crono_runtime CREATE on schema crono';
    END IF;
END;
$$;

DO $$
DECLARE
    relation text;
BEGIN
    FOREACH relation IN ARRAY ARRAY[
        'crono.namespaces',
        'crono.queues',
        'crono.jobs',
        'crono.targets',
        'crono.target_sets',
        'crono.target_set_members',
        'crono.schedules',
        'crono.run_requests',
        'crono.runs',
        'crono.run_attempts',
        'crono.worker_presence',
        'crono.outbox',
        'crono.run_events',
        'crono.schedule_events',
        'crono.workflows', 'crono.workflow_nodes', 'crono.workflow_edges',
        'crono.workflow_runs', 'crono.workflow_node_runs', 'crono.workflow_run_edges',
        'crono.workflow_node_executions', 'crono.workflow_completion_events'
    ] LOOP
        IF to_regclass(relation) IS NULL THEN
            RAISE EXCEPTION 'Missing relation: %', relation;
        END IF;
        IF NOT has_table_privilege('crono_runtime', relation, 'SELECT, INSERT, UPDATE, DELETE') THEN
            RAISE EXCEPTION 'Missing runtime grants on relation: %', relation;
        END IF;
    END LOOP;
END;
$$;

DO $$
DECLARE
    required_column text;
BEGIN
    FOREACH required_column IN ARRAY ARRAY[
        'jobs.inputs',
        'targets.inputs',
        'target_sets.inputs',
        'target_sets.updated_at',
        'schedules.inputs',
        'schedules.target_set_id'
    ] LOOP
        IF NOT EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = 'crono'
              AND table_name = split_part(required_column, '.', 1)
              AND column_name = split_part(required_column, '.', 2)
        ) THEN
            RAISE EXCEPTION 'Missing compatibility column: crono.%', required_column;
        END IF;
    END LOOP;

    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'crono'
          AND table_name = 'schedules'
          AND column_name = 'target_id'
          AND is_nullable = 'NO'
    ) THEN
        RAISE EXCEPTION 'schedules.target_id still requires a Target';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'crono.runs'::regclass
          AND conname = 'runs_request_id_key'
    ) THEN
        RAISE EXCEPTION 'runs.request_id still prevents Target Set fan-out';
    END IF;
    IF to_regclass('crono.runs_schedule_occurrence_target_unique') IS NULL THEN
        RAISE EXCEPTION 'Missing per-Target Schedule occurrence index';
    END IF;
    IF to_regclass('crono.runs_schedule_occurrence_unique') IS NOT NULL THEN
        RAISE EXCEPTION 'Legacy Schedule occurrence index still prevents fan-out';
    END IF;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM crono.queues
        WHERE name = 'default' AND system AND enabled
    ) THEN
        RAISE EXCEPTION 'Missing enabled system default Queue';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM crono.namespaces WHERE name = 'default'
    ) THEN
        RAISE EXCEPTION 'Missing default Namespace';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM crono.targets AS t
        JOIN crono.namespaces AS n ON n.id = t.namespace_id
        WHERE n.name = 'default' AND t.name = 'default'
    ) THEN
        RAISE EXCEPTION 'Missing default Target';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = 'crono.targets'::regclass
          AND tgname = 'default_target_name'
          AND NOT tgisinternal
    ) THEN
        RAISE EXCEPTION 'Missing default Target name protection';
    END IF;
END;
$$;


DO $$
DECLARE
    required_trigger text;
BEGIN
    FOREACH required_trigger IN ARRAY ARRAY[
        'workflows.workflows_valid_graph',
        'workflow_nodes.workflow_nodes_valid_graph',
        'workflow_edges.workflow_edges_valid_graph',
        'runs.runs_workflow_completion'
    ] LOOP
        IF NOT EXISTS (
            SELECT 1 FROM pg_trigger t
            JOIN pg_class c ON c.oid = t.tgrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = 'crono' AND c.relname = split_part(required_trigger, '.', 1)
              AND t.tgname = split_part(required_trigger, '.', 2)
              AND NOT t.tgisinternal AND t.tgenabled = 'O'
        ) THEN RAISE EXCEPTION 'Missing Workflow durability trigger: %', required_trigger;
        END IF;
    END LOOP;
END;
$$;

\echo 'Crono database check complete.'
