#!/usr/bin/env bash
# Exercise the development stack against an isolated fake checkout and recipes.
# No real Crono process or Podman container is touched.
set -euo pipefail

source_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
sandbox="$(mktemp -d)"
readonly source_root sandbox
declare -a launchers=()
declare -a outsiders=()

cleanup() {
  local pid
  bash "$sandbox/checkout/scripts/dev-stack.sh" stop-apps >/dev/null 2>&1 || true
  for pid in "${launchers[@]}" "${outsiders[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf -- "$sandbox"
}
trap cleanup EXIT

mkdir -p "$sandbox/checkout/scripts" "$sandbox/checkout/public" "$sandbox/bin"
cp "$source_root/scripts/dev-stack.sh" "$sandbox/checkout/scripts/dev-stack.sh"
cp "$source_root/tests/fixtures/dev-stack/just" "$sandbox/bin/just"
cp "$source_root/tests/fixtures/dev-stack/podman" "$sandbox/bin/podman"
chmod +x "$sandbox/bin/just" "$sandbox/bin/podman"
touch "$sandbox/checkout/public/ready"
export PATH="$sandbox/bin:$PATH"

read -r web_port api_port < <(python3 -c '
import socket
sockets = [socket.socket(), socket.socket()]
for sock in sockets:
    sock.bind(("127.0.0.1", 0))
print(*(sock.getsockname()[1] for sock in sockets))
for sock in sockets:
    sock.close()
')

wait_for_ready() {
  local attempt
  for ((attempt = 1; attempt <= 100; attempt++)); do
    if curl --fail --silent --output /dev/null "http://127.0.0.1:$api_port/ready" \
      && curl --fail --silent --output /dev/null "http://127.0.0.1:$web_port/"; then
      return 0
    fi
    sleep 0.1
  done
  echo "Timed out waiting for fake development stack" >&2
  return 1
}

env CRONO_DEV_STACK_ROOT="$sandbox/other" CRONO_DEV_ROLE=server sleep 60 &
outsider_pid=$!
outsiders+=("$outsider_pid")

bash "$sandbox/checkout/scripts/dev-stack.sh" start 127.0.0.1 \
  "$web_port" "$api_port" -v >"$sandbox/start-one.log" 2>&1 &
first_pid=$!
launchers+=("$first_pid")
wait_for_ready

bash "$sandbox/checkout/scripts/dev-stack.sh" start 127.0.0.1 \
  "$web_port" "$api_port" -v >"$sandbox/start-two.log" 2>&1 &
second_pid=$!
launchers+=("$second_pid")
for ((attempt = 1; attempt <= 50; attempt++)); do
  ! kill -0 "$first_pid" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "$first_pid" 2>/dev/null; then
  echo "Second start did not replace the first stack" >&2
  exit 1
fi
wait_for_ready
kill -0 "$second_pid"

# Stop must work without relying on the original just launcher or its traps.
kill -KILL "$second_pid" 2>/dev/null || true
wait "$second_pid" 2>/dev/null || true
bash "$sandbox/checkout/scripts/dev-stack.sh" stop
if curl --fail --silent --output /dev/null "http://127.0.0.1:$api_port/ready" \
  || curl --fail --silent --output /dev/null "http://127.0.0.1:$web_port/"; then
  echo "Development application survived independent stop" >&2
  exit 1
fi
kill -0 "$outsider_pid"

# A repeated stop should also be harmless.
bash "$sandbox/checkout/scripts/dev-stack.sh" stop
echo "Development stack start/stop regression checks passed"

# An unrelated listener must be reported, never terminated to free a port.
python3 -m http.server "$api_port" --bind 127.0.0.1 \
  --directory "$sandbox/checkout/public" >"$sandbox/unrelated.log" 2>&1 &
unrelated_port_pid=$!
outsiders+=("$unrelated_port_pid")
for ((attempt = 1; attempt <= 50; attempt++)); do
  if curl --fail --silent --output /dev/null "http://127.0.0.1:$api_port/ready"; then
    break
  fi
  sleep 0.1
done
if bash "$sandbox/checkout/scripts/dev-stack.sh" start 127.0.0.1 \
  "$web_port" "$api_port" -v >"$sandbox/port-conflict.log" 2>&1; then
  echo "Start ignored an unrelated port owner" >&2
  exit 1
fi
grep -q 'occupied after checkout cleanup' "$sandbox/port-conflict.log"
kill -0 "$unrelated_port_pid"
echo "Unrelated port-owner regression check passed"

kill -TERM "$unrelated_port_pid"
wait "$unrelated_port_pid" 2>/dev/null || true

# An IPv6-only listener also prevents the API's [::] dual-stack bind, even
# though connecting to 127.0.0.1 on the same port would fail.
python3 -m http.server "$api_port" --bind ::1 \
  --directory "$sandbox/checkout/public" >"$sandbox/ipv6.log" 2>&1 &
ipv6_pid=$!
outsiders+=("$ipv6_pid")
for ((attempt = 1; attempt <= 50; attempt++)); do
  if curl --noproxy '*' --fail --silent --output /dev/null \
    "http://[::1]:$api_port/ready"; then
    break
  fi
  sleep 0.1
done
if timeout 10 bash "$sandbox/checkout/scripts/dev-stack.sh" start 127.0.0.1 \
  "$web_port" "$api_port" -v >"$sandbox/ipv6-conflict.log" 2>&1; then
  echo "Start ignored an IPv6-only port owner" >&2
  exit 1
fi
if ! grep -q 'occupied after checkout cleanup' "$sandbox/ipv6-conflict.log"; then
  echo "IPv6-only listener was not detected before startup" >&2
  sed -n '1,20p' "$sandbox/ipv6-conflict.log" >&2
  exit 1
fi
kill -0 "$ipv6_pid"
kill -TERM "$ipv6_pid"
wait "$ipv6_pid" 2>/dev/null || true
echo "IPv6 port-owner regression check passed"

# Rebuilding while an older server runs leaves /proc/PID/exe suffixed with
# " (deleted)". Its checkout identity must still be recognized and stopped.
mkdir -p "$sandbox/checkout/target/debug"
cp "$(command -v sleep)" "$sandbox/checkout/target/debug/crono-server"
(
  cd "$sandbox/checkout"
  exec "$sandbox/checkout/target/debug/crono-server" 60
) &
stale_pid=$!
outsiders+=("$stale_pid")
sleep 0.1
rm -- "$sandbox/checkout/target/debug/crono-server"
bash "$sandbox/checkout/scripts/dev-stack.sh" stop-apps
if kill -0 "$stale_pid" 2>/dev/null; then
  echo "Deleted Crono binary survived checkout cleanup" >&2
  exit 1
fi
echo "Rebuilt-binary regression check passed"
