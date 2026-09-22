-- PostgreSQL bootstrap for Crono.
--
-- This file is safe to re-run. It creates the database and roles before loading
-- the canonical schema from 01_crono.sql.
--
-- CREATE DATABASE cannot run in a transaction block, so psql's \gexec is used
-- to conditionally execute it.

\set ON_ERROR_STOP 1

\if :{?crono_runtime_password}
\else
\set crono_runtime_password 'change-me'
\endif

SELECT pg_advisory_lock(hashtext('crono-initdb'));

-- The owner role owns schemas and objects but cannot be used to log in.
SELECT format('CREATE ROLE %I NOLOGIN', 'crono_owner')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'crono_owner')\gexec

-- Local development uses the default password below. Supply
-- -v crono_runtime_password=... when bootstrapping another environment.
SELECT format(
    'CREATE ROLE %I LOGIN PASSWORD %L',
    'crono_runtime',
    :'crono_runtime_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'crono_runtime')\gexec

ALTER ROLE crono_owner
    NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE crono_runtime
    LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;

SELECT format('CREATE DATABASE %I OWNER %I', 'crono', 'crono_owner')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'crono')\gexec

ALTER DATABASE crono OWNER TO crono_owner;
REVOKE ALL ON DATABASE crono FROM PUBLIC;
GRANT CONNECT, TEMPORARY ON DATABASE crono TO crono_runtime;

ALTER ROLE crono_owner IN DATABASE crono SET search_path = crono, public;
ALTER ROLE crono_runtime IN DATABASE crono SET search_path = crono, public;

SELECT pg_advisory_unlock(hashtext('crono-initdb'));

\connect crono

-- Create schema objects as the non-login owner rather than as the bootstrap
-- superuser. Future migrations should follow the same ownership boundary.
SET ROLE crono_owner;
\ir 01_crono.sql
RESET ROLE;

REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA crono FROM PUBLIC;
GRANT USAGE ON SCHEMA crono TO crono_runtime;

GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA crono TO crono_runtime;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA crono TO crono_runtime;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA crono TO crono_runtime;

ALTER DEFAULT PRIVILEGES FOR ROLE crono_owner IN SCHEMA crono
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO crono_runtime;

ALTER DEFAULT PRIVILEGES FOR ROLE crono_owner IN SCHEMA crono
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO crono_runtime;

ALTER DEFAULT PRIVILEGES FOR ROLE crono_owner IN SCHEMA crono
    REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

ALTER DEFAULT PRIVILEGES FOR ROLE crono_owner IN SCHEMA crono
    GRANT EXECUTE ON FUNCTIONS TO crono_runtime;
