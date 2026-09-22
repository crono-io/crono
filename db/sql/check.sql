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
        'crono.jobs',
        'crono.job_versions',
        'crono.targets',
        'crono.runs',
        'crono.outbox'
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

\echo 'Crono database check complete.'
