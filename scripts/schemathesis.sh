#!/usr/bin/env bash
# Build crono-server, run it against an already-initialized database, and fuzz
# it with Schemathesis using the committed OpenAPI contract.
#
# Required: CRONO_DATABASE_URL pointing at a throwaway database the fuzzer may
# fill with arbitrary data. Optional:
#   CRONO_NATS_URL               NATS for dispatch (unreachable is fine)
#   PORT                         server port (default 18080)
#   SCHEMATHESIS                 command that runs the `st` CLI (default `st`)
#   SCHEMATHESIS_MAX_EXAMPLES    override generation.max-examples
#   SCHEMATHESIS_SEED            reproduce a previous run
#
# Reports and the server log are written to target/schemathesis/.
set -euo pipefail

: "${CRONO_DATABASE_URL:?set CRONO_DATABASE_URL to a throwaway database}"
readonly port="${PORT:-18080}"
readonly base_url="http://127.0.0.1:${port}"
readonly report_dir="target/schemathesis"
read -r -a st <<< "${SCHEMATHESIS:-st}"

cd "$(git rev-parse --show-toplevel)"
mkdir -p "$report_dir"

cargo build --locked -p crono-server --bin crono-server

server_pid=""
stop_server() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid"
    wait "$server_pid" 2>/dev/null || true
  fi
}
trap stop_server EXIT

CRONO_DATABASE_URL="$CRONO_DATABASE_URL" \
  CRONO_NATS_URL="${CRONO_NATS_URL:-nats://127.0.0.1:4222}" \
  target/debug/crono-server --port "$port" -v > "$report_dir/server.log" 2>&1 &
server_pid=$!

for _ in $(seq 1 60); do
  if curl -fsS "${base_url}/ready" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$server_pid" 2>/dev/null; then
    echo "crono-server exited before becoming ready; see $report_dir/server.log" >&2
    exit 1
  fi
  sleep 1
done
curl -fsS "${base_url}/ready" >/dev/null

args=(run docs/openapi/crono-server.json --url "$base_url"
  --report junit --report-dir "$report_dir")
if [[ -n "${SCHEMATHESIS_MAX_EXAMPLES:-}" ]]; then
  args+=(--max-examples "$SCHEMATHESIS_MAX_EXAMPLES")
fi
if [[ -n "${SCHEMATHESIS_SEED:-}" ]]; then
  args+=(--seed "$SCHEMATHESIS_SEED")
fi

"${st[@]}" "${args[@]}"
