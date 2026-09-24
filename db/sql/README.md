# Database SQL helpers

`db/sql/` is the canonical location for Crono's PostgreSQL bootstrap and schema.

## Files

- `00_init.sql` creates the `crono` database, owner/runtime roles, and grants,
  then loads the schema.
- `01_crono.sql` is the idempotent draft schema baseline for Namespaces, global
  worker Queues, direct Job and Target definitions, explicit Target Sets,
  Schedules, Runs, Attempts, worker presence, audit events, leases, and the
  transactional outbox.
- `container-entrypoint.sql` lets the official PostgreSQL image run the canonical
  bootstrap while keeping relative includes working.
- `check.sql` verifies database ownership, role safety, schema ownership, and
  runtime grants.
- `reset_all.sql` destroys the local database and roles for a clean rebuild.

## Bootstrap and verify

The commands below require the PostgreSQL `psql` client. With an administrator
available at the default local URL:

```sh
just db-bootstrap
just db-verify
```

For the normal workspace workflow, `just server` or `just dev-start` creates
and starts a loopback-only `crono-postgres` container from the official
`postgres:18` image. The recipe stores PostgreSQL 18's versioned data directory
under the `crono-postgres-data` named volume, waits for readiness, and applies
`00_init.sql` on every start so schema changes are picked up idempotently. This
container uses trust authentication only for local development; do not reuse
that configuration outside a developer workstation. `just dev-stop` stops the
container without deleting its data. This path requires Podman but does not
require a host installation of `psql` because the recipe runs the image's
client inside the container.

Pass another administrator URL when needed:

```sh
just db-bootstrap 'postgres://admin@db.example.test:5432/postgres'
just db-verify 'postgres://admin@db.example.test:5432/postgres'
```

The bootstrap is idempotent. It defaults the `crono_runtime` password to
`change-me` for local development. For any non-local environment, run the SQL
directly and provide a secret through a psql variable:

```sh
psql "postgres://<admin>@<host>:5432/postgres" \
  -v ON_ERROR_STOP=1 \
  -v crono_runtime_password='<secret>' \
  -f db/sql/00_init.sql
```

The application role is `crono_runtime`. It can connect and manipulate objects
created by `crono_owner`, but it cannot create schema objects. `crono_owner` is a
non-login role reserved for bootstrap and migrations.

Canonical names are DNS-1123 labels. Workload names are unique inside their
owning Namespace; Queue and Namespace names are globally unique. UUIDs are the
immutable keys for every relationship. Jobs, Runs, and worker presence retain
Queue UUIDs while Queue names remain editable display and lookup identifiers.
The bootstrap `default` Queue is system-managed and remains enabled so worker
configuration has a stable default. Constraint triggers reject cross-Namespace
Target Set membership and Job, Target,
Schedule, and Run combinations even if an adapter is faulty. A Run stores an immutable execution snapshot. The scheduler or
manual Run path inserts the Run, first Attempt, and outbox row in one
transaction. The dispatcher marks the outbox, Attempt, and Run queued only
after a JetStream persistence acknowledgement; failed sends remain pending
with bounded operational error text and do not consume execution retries.
Worker presence is refreshed through the server's NATS control handler rather
than by granting workers database access. The reconciler bounds retained
offline presence records to seven days.

This repository is still in its draft schema phase. There is no compatibility
or numbered schema series: reset older development databases before applying
the current baseline. `reset_all.sql` is the direct PostgreSQL-only reset path.
Production migration sequencing will be introduced before the schema is
declared stable; the runtime role intentionally has no DDL privileges.

The local PostgreSQL configuration lives in `db/config/postgres/postgresql.conf`.
When using the official image, mount `db/` at `/db` and mount
`db/sql/container-entrypoint.sql` into `/docker-entrypoint-initdb.d/00_init.sql`.
Create the ignored `db/log/postgres/` directory before starting a container with
that configuration file.

## Reset

For local development, `just dev-reset` removes the exact `crono-postgres` and
`crono-nats` containers and their `crono-postgres-data` and `crono-nats-data`
named volumes, then recreates both services and reapplies the canonical schema.
The recipe asks for confirmation because all local database and JetStream state
is permanently discarded. `just dev-stop` remains the non-destructive way to
stop the services while preserving their data.

To reset PostgreSQL directly without changing NATS state, run:

```sh
psql "postgres://postgres@localhost:5432/postgres" \
  -v ON_ERROR_STOP=1 \
  -f db/sql/reset_all.sql
```
