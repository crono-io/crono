-- Destructive reset for local development and tests.
--
-- WARNING: This permanently removes the Crono database and its bootstrap roles.
-- Run as a PostgreSQL administrator against the postgres database:
--   psql "postgres://<admin>@<host>:5432/postgres" \
--     -v ON_ERROR_STOP=1 -f db/sql/reset_all.sql

\set ON_ERROR_STOP 1

\echo 'Resetting Crono database and roles...'

SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = 'crono'
  AND pid <> pg_backend_pid();

SELECT format('DROP DATABASE %I', 'crono')
WHERE EXISTS (SELECT 1 FROM pg_database WHERE datname = 'crono')\gexec

SELECT format('DROP ROLE %I', 'crono_runtime')
WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'crono_runtime')\gexec

SELECT format('DROP ROLE %I', 'crono_owner')
WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'crono_owner')\gexec

\echo 'Crono reset complete.'
