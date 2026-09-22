clippy:
  cargo clippy --locked --workspace --all-targets
  cargo clippy --locked --workspace --all-targets --all-features
  cargo clippy --locked -p crono-web --target wasm32-unknown-unknown

test:
  cargo test --locked --workspace
  cargo test --locked --workspace --all-features

# Ensure the local PostgreSQL 18 container is running and apply the canonical schema.
postgres:
  #!/usr/bin/env bash
  set -euo pipefail
  readonly container="crono-postgres"

  if podman container exists "$container"; then
    podman start "$container" >/dev/null
  else
    podman run --detach \
      --name "$container" \
      --env POSTGRES_HOST_AUTH_METHOD=trust \
      --publish 127.0.0.1:5432:5432 \
      --volume crono-postgres-data:/var/lib/postgresql \
      --volume "$PWD/db/sql:/db/sql:ro,Z" \
      docker.io/library/postgres:18 >/dev/null
  fi

  ready=false
  for ((attempt = 1; attempt <= 30; attempt++)); do
    if podman exec "$container" pg_isready --username postgres --dbname postgres >/dev/null; then
      ready=true
      break
    fi
    sleep 1
  done
  if [[ "$ready" != true ]]; then
    podman logs --tail 50 "$container"
    echo "PostgreSQL did not become ready within 30 seconds" >&2
    exit 1
  fi

  podman exec "$container" psql \
    --username postgres \
    --dbname postgres \
    --set ON_ERROR_STOP=1 \
    --file /db/sql/00_init.sql >/dev/null
  echo "PostgreSQL is ready at 127.0.0.1:5432"

# Ensure the local NATS container is running with JetStream enabled.
nats:
  #!/usr/bin/env bash
  set -euo pipefail
  readonly container="crono-nats"

  if podman container exists "$container"; then
    podman start "$container" >/dev/null
  else
    podman run --detach \
      --name "$container" \
      --publish 127.0.0.1:4222:4222 \
      --publish 127.0.0.1:8222:8222 \
      --volume crono-nats-data:/data \
      docker.io/library/nats:2.14-alpine \
      --jetstream \
      --store_dir /data \
      --http_port 8222 >/dev/null
  fi

  ready=false
  for ((attempt = 1; attempt <= 30; attempt++)); do
    if curl --fail --silent --output /dev/null \
      "http://127.0.0.1:8222/healthz?js-enabled-only=true"; then
      ready=true
      break
    fi
    sleep 1
  done
  if [[ "$ready" != true ]]; then
    podman logs --tail 50 "$container"
    echo "NATS did not become ready within 30 seconds" >&2
    exit 1
  fi

  echo "NATS with JetStream is ready at nats://127.0.0.1:4222"

# Start all persistent services required by the development server.
dev-infra: postgres nats

# Start the Crono API server after its local infrastructure is ready.
[positional-arguments]
server port="8080" verbosity="-v": dev-infra
  cargo run --locked -p crono-server --bin crono-server -- "$2" --port "$1"

# Serve and live-reload crono-web for local or remote browser testing.
[positional-arguments]
web address="0.0.0.0" port="3000":
  cd apps/web && NO_COLOR=true trunk serve --address "$1" --port "$2"

[doc("Start the complete development stack on non-conflicting ports.")]
[parallel]
dev-start address="0.0.0.0" web-port="3000" server-port="8080" verbosity="-v": (server server-port verbosity) (web address web-port)

# Stop local infrastructure containers while preserving their data volumes.
dev-stop:
  #!/usr/bin/env bash
  set -euo pipefail
  for container in crono-postgres crono-nats; do
    if podman container exists "$container"; then
      podman stop "$container"
    fi
  done

# Run the Crono CLI, forwarding its arguments and connection configuration.
[positional-arguments]
cli *args:
  cargo run --locked -p crono-cli -- "$@"

db-bootstrap admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/00_init.sql

db-verify admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/check.sql
