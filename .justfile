clippy:
  cargo clippy --locked --workspace --all-targets
  cargo clippy --locked --workspace --all-targets --all-features
  cargo clippy --locked -p crono-web --target wasm32-unknown-unknown

# Verify formatting without modifying source files.
fmt-check:
  cargo fmt --all -- --check

# Run formatting, clippy and every workspace test; this is also the release's local check.
# The database tests use a throwaway PostgreSQL on a random loopback port that is removed
# on exit, so a running crono-postgres or another project's database on 5432 is never
# touched. Set CRONO_TEST_DATABASE_URL to use an initialized database of your own instead.
test: fmt-check clippy
  #!/usr/bin/env bash
  set -euo pipefail
  if [[ -z "${CRONO_TEST_DATABASE_URL:-}" ]]; then
    container="crono-test-postgres-$$"
    trap 'podman rm --force --time 0 "$container" >/dev/null 2>&1 || true' EXIT
    podman run --detach --rm --name "$container" \
      --env POSTGRES_HOST_AUTH_METHOD=trust \
      --publish 127.0.0.1::5432 \
      docker.io/library/postgres:18 >/dev/null

    # Ask over TCP: the image's temporary first-start server listens only on its socket.
    ready=false
    for ((attempt = 1; attempt <= 60; attempt++)); do
      if podman exec "$container" pg_isready --host 127.0.0.1 --username postgres >/dev/null 2>&1; then
        ready=true
        break
      fi
      sleep 0.5
    done
    if [[ "$ready" != true ]]; then
      podman logs --tail 50 "$container"
      echo "The test PostgreSQL did not become ready within 30 seconds" >&2
      exit 1
    fi

    # Copied rather than mounted: an SELinux relabel would lock crono-postgres out of db/sql.
    podman cp db/sql "$container:/tmp/crono-sql"
    podman exec --env PGOPTIONS=--client-min-messages=warning "$container" psql \
      --username postgres \
      --dbname postgres \
      --set ON_ERROR_STOP=1 \
      --file /tmp/crono-sql/00_init.sql >/dev/null
    bash tests/bootstrap.sh "$container"
    port="$(podman port "$container" 5432/tcp)"
    port="${port%%$'\n'*}"
    export CRONO_TEST_DATABASE_URL="postgres://crono_runtime@127.0.0.1:${port##*:}/crono"
  fi
  export CRONO_TEST_REQUIRE_DATABASE=1
  cargo test --locked --workspace
  cargo test --locked --workspace --all-features

# Regenerate the committed OpenAPI contract from the server's routes.
openapi:
  #!/usr/bin/env bash
  set -euo pipefail
  readonly spec="docs/openapi/crono-server.json"
  readonly staged="${spec}.tmp"
  trap 'rm -f "$staged"' EXIT
  mkdir -p "$(dirname "$spec")"
  cargo run --locked --quiet -p crono-server --bin crono-server-openapi > "$staged"
  mv "$staged" "$spec"
  echo "Wrote $spec"

# PostgreSQL runs in a disposable tmpfs container on port 55432 and the server
# on port 18080, so the development database is never touched. NATS points at
# a closed port: the fuzzer creates Shell Jobs and Runs, and a local worker on
# the development NATS would otherwise execute them.
[doc("Fuzz the API contract with Schemathesis against an isolated throwaway stack.")]
[positional-arguments]
schemathesis max-examples="":
  #!/usr/bin/env bash
  set -euo pipefail
  readonly container="crono-contract-postgres"
  readonly version="4.28.0"

  if podman container exists "$container"; then
    podman rm --force "$container" >/dev/null
  fi
  trap 'podman rm --force "$container" >/dev/null 2>&1 || true' EXIT
  podman run --detach --rm \
    --name "$container" \
    --env POSTGRES_HOST_AUTH_METHOD=trust \
    --publish 127.0.0.1:55432:5432 \
    --tmpfs /var/lib/postgresql \
    --volume "$PWD/db/sql:/db/sql:ro,z" \
    docker.io/library/postgres:18 >/dev/null

  initialized=false
  for ((attempt = 1; attempt <= 30; attempt++)); do
    if podman exec --env PGOPTIONS=--client-min-messages=warning "$container" psql \
      --host 127.0.0.1 --username postgres --dbname postgres \
      --set ON_ERROR_STOP=1 --file /db/sql/00_init.sql >/dev/null 2>&1; then
      initialized=true
      break
    fi
    sleep 1
  done
  if [[ "$initialized" != true ]]; then
    podman logs --tail 50 "$container"
    echo "contract PostgreSQL did not initialize within 30 seconds" >&2
    exit 1
  fi

  CRONO_DATABASE_URL="postgres://crono_runtime:change-me@127.0.0.1:55432/crono" \
    CRONO_NATS_URL="nats://127.0.0.1:1" \
    SCHEMATHESIS="uvx --from schemathesis==${version} st" \
    SCHEMATHESIS_MAX_EXAMPLES="$1" \
    bash scripts/schemathesis.sh

# Static musl binaries for the host architecture, built in a rust:alpine
# container because `ring` needs a musl C toolchain; release CI builds the same
# targets natively with musl-tools. Output lands in target/musl/release.
[doc("Build static musl server, worker, and CLI binaries in a container.")]
musl-build:
  podman run --rm \
    --volume "$PWD:/src:z" \
    --volume "${CARGO_HOME:-$HOME/.cargo}/registry:/usr/local/cargo/registry:z" \
    --workdir /src docker.io/library/rust:1-alpine \
    sh -c 'apk add --no-cache musl-dev >/dev/null && cargo build --release --locked --target-dir target/musl -p crono-server -p crono-worker -p crono-cli'

# Stage the same build context layout the release workflow uses and build both
# images' `prebuilt` stage for the host platform, tagged
# localhost/crono-{server,web}:TAG. A plain `podman build -f <Dockerfile> .`
# from the repository root compiles from source instead.
[doc("Build the crono-server and crono-web images locally for the host platform.")]
images tag="dev": musl-build
  #!/usr/bin/env bash
  set -euo pipefail
  case "$(uname -m)" in
    x86_64) platform="linux/amd64" ;;
    aarch64 | arm64) platform="linux/arm64" ;;
    *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
  esac
  (cd apps/web && trunk build --release)
  readonly context="target/image-context"
  rm -rf "$context"
  mkdir -p "$context/$platform" "$context/web"
  cp target/musl/release/crono-server "$context/$platform/"
  cp -R apps/web/dist "$context/web/dist"
  cp -R apps/web/nginx "$context/web/nginx"
  podman build --platform "$platform" --target prebuilt -f services/server/Dockerfile -t "localhost/crono-server:{{ tag }}" "$context"
  podman build --platform "$platform" --target prebuilt -f apps/web/Dockerfile -t "localhost/crono-web:{{ tag }}" "$context"
  podman images --format '{{{{.Repository}}:{{{{.Tag}} {{{{.Size}}' | grep -E "crono-(server|web):{{ tag }}"

# Build .deb and .rpm packages for the host architecture into target/packages.
[doc("Build .deb and .rpm packages for the host architecture.")]
packages version="": musl-build
  #!/usr/bin/env bash
  set -euo pipefail
  case "$(uname -m)" in
    x86_64) arch="amd64" ;;
    aarch64 | arm64) arch="arm64" ;;
    *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
  esac
  version="{{ version }}"
  if [[ -z "$version" ]]; then
    version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
  fi
  bash scripts/package.sh "$version" "$arch" target/musl/release target/packages

# Preview the rendered API reference at http://127.0.0.1:8088.
[positional-arguments]
api-docs port="8088":
  python3 -m http.server --bind 127.0.0.1 --directory docs/openapi "$1"

# Fail on breaking API changes between a base (default origin/main) and HEAD.
[positional-arguments]
api-breaking base="origin/main":
  bash scripts/api-breaking.sh "$1" HEAD

# Run real failure-mode checks. This intentionally stops and restarts crono-nats.
integration-test: dev-infra
  CRONO_TEST_DATABASE_URL=postgres://crono_runtime:change-me@127.0.0.1:5432/crono \
    CRONO_TEST_NATS_URL=nats://127.0.0.1:4222 \
    cargo test --locked -p crono-server --test nats_outage_recovery -- --ignored --nocapture

# Measure bounded PostgreSQL scheduler/intent throughput (default: 100,000).
load-test jobs="100000": postgres
  CRONO_TEST_DATABASE_URL=postgres://crono_runtime:change-me@127.0.0.1:5432/crono \
    CRONO_LOAD_RUNS="{{ jobs }}" \
    cargo test --release --locked -p crono-server --test scheduler_load -- --ignored --nocapture

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

  podman exec --env PGOPTIONS=--client-min-messages=warning "$container" psql \
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

[doc("Start a worker and print its execution timeline (pretty or json).")]
[positional-arguments]
worker queue="default" worker-id="" concurrency="3" verbosity="-v" log-format="pretty":
  if [ -n "$2" ]; then cargo run --locked -p crono-worker -- "$4" --log-format "$5" run --queue "$1" --worker-id "$2" --concurrency "$3"; else cargo run --locked -p crono-worker -- "$4" --log-format "$5" run --queue "$1" --concurrency "$3"; fi

[doc("Start the local development stack with automatic Bearer authentication.")]
[positional-arguments]
dev-start address="0.0.0.0" web-port="3000" server-port="8080" verbosity="-v":
  bash scripts/dev-stack.sh start "$1" "$2" "$3" "$4"

# Stop this checkout's server, web, and worker processes plus local containers.
dev-stop:
  bash scripts/dev-stack.sh stop

# Delete and recreate Crono's local PostgreSQL and NATS state.
[confirm("Delete all local Crono PostgreSQL and NATS data and recreate clean services?")]
dev-reset:
  #!/usr/bin/env bash
  set -euo pipefail
  readonly containers=(crono-postgres crono-nats)
  readonly volumes=(crono-postgres-data crono-nats-data)

  for container in "${containers[@]}"; do
    if podman container exists "$container"; then
      podman rm --force "$container" >/dev/null
    fi
  done

  for volume in "${volumes[@]}"; do
    if podman volume exists "$volume"; then
      podman volume rm "$volume" >/dev/null
    fi
  done

  just dev-infra
  echo "Crono development PostgreSQL and NATS state was reset"

# Run the Crono CLI, forwarding its arguments and connection configuration.
[positional-arguments]
cli *args:
  cargo run --locked -p crono-cli -- "$@"

db-bootstrap admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/00_init.sql

db-verify admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/check.sql

# Releases run .github/workflows/release.yml on develop: it checks that the commit passed
# Test & Build and that the version (Cargo.toml, Cargo.lock and the OpenAPI contract's
# info.version) is new, then builds and publishes everything. `main` is moved here, after
# the run, because a protected branch refuses an update while a `CI OK` check of the same
# commit is still pending (develop's own Test & Build). The operator bumps the version
# beforehand, as the last commit of the work. See README "Releasing".

[doc("Release develop: start the Release workflow, follow it, then fast-forward main.")]
[script("bash")]
release:
  set -euo pipefail
  git fetch --quiet origin develop
  sha="$(git rev-parse origin/develop)"
  last="$(gh run list --workflow release.yml --branch develop --event workflow_dispatch --limit 1 --json databaseId --jq '.[0].databaseId // 0')"
  gh workflow run release.yml --ref develop
  # The dispatched run is the first new one for this commit; never just "the latest".
  run=""
  for _ in $(seq 30); do
    run="$(gh run list --workflow release.yml --branch develop --event workflow_dispatch --limit 10 --json databaseId,headSha --jq "[.[] | select(.databaseId > ${last} and .headSha == \"${sha}\")][0].databaseId // empty")"
    [[ -z "${run}" ]] || break
    sleep 2
  done
  [[ -n "${run}" ]] || { echo "The dispatched release run did not appear." >&2; exit 1; }
  gh run watch "${run}" --exit-status
  # develop's own Test & Build must have finished, and passed, before main can move.
  ci=""
  for _ in $(seq 30); do
    ci="$(gh run list --workflow build.yml --branch develop --commit "${sha}" --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
    [[ -z "${ci}" ]] || break
    sleep 2
  done
  [[ -n "${ci}" ]] || { echo "No Test & Build run of develop for ${sha}; push it to main yourself when CI OK passed." >&2; exit 1; }
  echo "Waiting for develop's Test & Build (run ${ci}) before moving main..."
  gh run watch "${ci}" --exit-status > /dev/null || { echo "Test & Build failed on ${sha}; re-run its failed jobs, then: git push origin ${sha}:refs/heads/main" >&2; exit 1; }
  git push origin "${sha}:refs/heads/main"
