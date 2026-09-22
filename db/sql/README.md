# Database SQL helpers

`db/sql/` is the canonical location for Crono's PostgreSQL bootstrap and schema.

## Files

- `00_init.sql` creates the `crono` database, owner/runtime roles, and grants,
  then loads the schema.
- `01_crono.sql` is the idempotent schema baseline. Domain tables will be added
  after the conceptual model in the root README becomes an explicit schema and
  migration plan.
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

The local PostgreSQL configuration lives in `db/config/postgres/postgresql.conf`.
When using the official image, mount `db/` at `/db` and mount
`db/sql/container-entrypoint.sql` into `/docker-entrypoint-initdb.d/00_init.sql`.
Create the ignored `db/log/postgres/` directory before starting a container with
that configuration file.

## Reset

Reset is intentionally not exposed as a Just recipe because it is destructive.
For local development only:

```sh
psql "postgres://postgres@localhost:5432/postgres" \
  -v ON_ERROR_STOP=1 \
  -f db/sql/reset_all.sql
```
