-- Canonical Crono schema bootstrap.
--
-- Domain tables intentionally remain out of this baseline until the conceptual
-- records in README.md are turned into an explicit schema and migration plan.
-- Keep this file idempotent so 00_init.sql can be re-run safely.

CREATE SCHEMA IF NOT EXISTS crono AUTHORIZATION crono_owner;

COMMENT ON SCHEMA crono IS 'Authoritative Crono control-plane data';
