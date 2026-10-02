#!/usr/bin/env bash
# Manage only this checkout's local development processes. Never kill an
# arbitrary listener merely because it happens to use a Crono port.
# Provision a private, reusable development credential for Trunk to supply to
# the API, which still verifies Bearer credentials normally. Every client that
# can reach this development web proxy receives full development access.
set +x
set -euo pipefail

project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly project_root
readonly lock_file="$project_root/target/dev-stack.lock"
declare -a matched_pids=()
declare -a matched_groups=()
declare -a child_pids=()
managed_kind=""

# The EXIT trap outlives start_stack's local variables. Keep the launchers in
# global state so a normal exit or signal always stops their process groups.
cleanup_children() {
  local pid attempt alive
  for pid in "${child_pids[@]}"; do
    kill -TERM -- "-$pid" 2>/dev/null || true
  done
  for ((attempt = 1; attempt <= 50; attempt++)); do
    alive=false
    for pid in "${child_pids[@]}"; do
      if kill -0 -- "-$pid" 2>/dev/null; then
        alive=true
      fi
    done
    [[ "$alive" == false ]] && break
    sleep 0.1
  done
  for pid in "${child_pids[@]}"; do
    if kill -0 -- "-$pid" 2>/dev/null; then
      kill -KILL -- "-$pid" 2>/dev/null || true
    fi
  done
  for pid in "${child_pids[@]}"; do
    wait "$pid" 2>/dev/null || true
  done
}

role_allowed() {
  local role="$1"
  local scope="$2"
  case "$role" in
    server|web) return 0 ;;
    worker) [[ "$scope" == all ]] ;;
    *) return 1 ;;
  esac
}

# A marker follows new launchers and all their children. Exact checkout and
# command checks also recover processes left by older, unmarked dev-start runs.
matches_checkout() {
  local pid="$1"
  local scope="$2"
  local proc="/proc/$pid"
  local cwd executable role
  local -a argv=()
  managed_kind=""
  [[ -r "$proc/cmdline" ]] || return 1
  mapfile -d '' -t argv < "$proc/cmdline" 2>/dev/null || return 1
  ((${#argv[@]} > 0)) || return 1

  if [[ -r "$proc/environ" ]] \
    && grep -azFqx "CRONO_DEV_STACK_ROOT=$project_root" "$proc/environ" 2>/dev/null; then
    for role in server web worker; do
      if grep -azFqx "CRONO_DEV_ROLE=$role" "$proc/environ" 2>/dev/null \
        && role_allowed "$role" "$scope"; then
        managed_kind=marked
        return 0
      fi
    done
  fi

  cwd="$(readlink -f -- "$proc/cwd" 2>/dev/null)" || return 1
  executable="$(readlink -f -- "$proc/exe" 2>/dev/null)" || return 1
  if [[ "$cwd" == "$project_root" && "${argv[0]##*/}" == just ]]; then
    role="${argv[1]-}"
    if role_allowed "$role" "$scope"; then
      managed_kind=just
      return 0
    fi
  fi
  if [[ "$cwd" == "$project_root" ]]; then
    case "$executable" in
      "$project_root/target/debug/crono-server"|"$project_root/target/debug/crono-server (deleted)"|"$project_root/target/release/crono-server"|"$project_root/target/release/crono-server (deleted)") role=server ;;
      "$project_root/target/debug/crono-worker"|"$project_root/target/debug/crono-worker (deleted)"|"$project_root/target/release/crono-worker"|"$project_root/target/release/crono-worker (deleted)") role=worker ;;
      *) role="" ;;
    esac
    if role_allowed "$role" "$scope"; then
      managed_kind=binary
      return 0
    fi
  fi
  if [[ "$cwd" == "$project_root/apps/web" \
    && "${argv[0]##*/}" == trunk && "${argv[1]-}" == serve ]] \
    && role_allowed web "$scope"; then
    managed_kind=web
    return 0
  fi
  return 1
}

scan_apps() {
  local scope="$1"
  local proc pid pgid
  matched_pids=()
  matched_groups=()
  for proc in /proc/[0-9]*; do
    pid="${proc##*/}"
    if matches_checkout "$pid" "$scope"; then
      matched_pids+=("$pid")
      if [[ "$managed_kind" == marked || "$managed_kind" == just ]]; then
        pgid="$(ps -o pgid= -p "$pid" 2>/dev/null)" || continue
        pgid="${pgid//[[:space:]]/}"
        if [[ "$pgid" == "$pid" ]]; then
          matched_groups+=("$pgid")
        fi
      fi
    fi
  done
}

signal_apps() {
  local signal="$1"
  local group pid
  for group in "${matched_groups[@]}"; do
    kill "-$signal" -- "-$group" 2>/dev/null || true
  done
  for pid in "${matched_pids[@]}"; do
    kill "-$signal" -- "$pid" 2>/dev/null || true
  done
}

stop_apps() {
  local scope="$1"
  local attempt
  scan_apps "$scope"
  ((${#matched_pids[@]} > 0)) || return 0
  printf 'Stopping Crono development processes from %s: %s\n' \
    "$project_root" "${matched_pids[*]}"
  signal_apps TERM
  for ((attempt = 1; attempt <= 50; attempt++)); do
    sleep 0.1
    scan_apps "$scope"
    ((${#matched_pids[@]} == 0)) && return 0
  done
  echo "Escalating remaining Crono development processes to SIGKILL: ${matched_pids[*]}" >&2
  signal_apps KILL
  for ((attempt = 1; attempt <= 20; attempt++)); do
    sleep 0.1
    scan_apps "$scope"
    ((${#matched_pids[@]} == 0)) && return 0
  done
  echo "Crono development processes did not stop: ${matched_pids[*]}" >&2
  return 1
}

port_is_open() {
  (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

# The API binds [::] as a dual-stack listener. A listener on ::1, a host IP,
# or another IPv6 address can make that bind fail even when 127.0.0.1 is free.
check_port_free() {
  local port="$1"
  local label="$2"
  local listeners
  if ! listeners="$(ss -H -ltnp "sport = :$port" 2>&1)"; then
    printf 'Cannot inspect %s port %s: %s\n' "$label" "$port" "$listeners" >&2
    return 1
  fi
  if [[ -n "$listeners" ]]; then
    printf '%s port %s is occupied after checkout cleanup:\n%s\n' \
      "$label" "$port" "$listeners" >&2
    echo "Stop that listener or choose another port; no unrelated process was killed." >&2
    return 1
  fi
}

# A failed bind is authoritative. Recheck after an early server exit because
# another process can acquire a port between a preflight check and bind(2).
describe_port_after_failure() {
  local port="$1"
  local listeners
  if listeners="$(ss -H -ltnp "sport = :$port" 2>/dev/null)"; then
    if [[ -n "$listeners" ]]; then
      printf 'API port %s now has a listener:\n%s\n' "$port" "$listeners" >&2
    else
      echo "No API listener is visible on port $port. If bind reported address-in-use, recently closed TCP connections may need time to expire." >&2
    fi
  fi
}

valid_port() {
  [[ "$1" =~ ^[0-9]+$ ]] && ((10#$1 >= 1 && 10#$1 <= 65535))
}

# Match the server's development-token length and RFC 6750 alphabet before
# stopping a working stack. Errors and shell traces must never contain a token.
validate_development_token() {
  local LC_ALL=C
  [[ ${#CRONO_AUTH_DEVELOPMENT_TOKEN} -ge 32 \
    && ${#CRONO_AUTH_DEVELOPMENT_TOKEN} -le 8192 \
    && "$CRONO_AUTH_DEVELOPMENT_TOKEN" =~ ^[a-zA-Z0-9._~+/-]+=*$ ]] || {
    echo "CRONO_AUTH_DEVELOPMENT_TOKEN must be a random Bearer token of 32 to 8192 bytes" >&2
    return 2
  }
}

# Called under the checkout lock. An explicit environment token takes priority;
# otherwise reuse the private file or generate 256 random bits once. Existing
# invalid credentials fail closed instead of being silently replaced. Only the
# ignored owner-only directory contains secrets; neither argv nor assets do.
prepare_development_auth() {
  local server_port="$1"
  local auth_dir="$project_root/target/dev-auth"
  local token_file="$auth_dir/token" staged
  [[ ! -L "$auth_dir" ]] || { echo "Development authentication directory must not be a symbolic link" >&2; return 2; }
  (umask 077; mkdir -p -- "$auth_dir")
  [[ -O "$auth_dir" ]] || { echo "Development authentication directory must belong to the current user" >&2; return 2; }
  chmod 700 -- "$auth_dir"

  if [[ ! ${CRONO_AUTH_DEVELOPMENT_TOKEN+x} ]]; then
    if [[ -e "$token_file" || -L "$token_file" ]]; then
      [[ -f "$token_file" && ! -L "$token_file" && -O "$token_file" ]] || {
        echo "Development token must be a regular file owned by the current user" >&2
        return 2
      }
      chmod 600 -- "$token_file"
      CRONO_AUTH_DEVELOPMENT_TOKEN="$(cat -- "$token_file")"
    else
      CRONO_AUTH_DEVELOPMENT_TOKEN="$(openssl rand -hex 32)"
      validate_development_token
      staged="$(mktemp "$auth_dir/token.XXXXXX")"
      printf '%s\n' "$CRONO_AUTH_DEVELOPMENT_TOKEN" > "$staged"
      mv -f -- "$staged" "$token_file"
    fi
  fi
  validate_development_token
  export CRONO_AUTH_DEVELOPMENT_TOKEN

  # Inherit the checked-in Trunk configuration, overriding only its API proxy.
  # The launch supplies absolute target/dist paths because this private config
  # lives outside apps/web. Disable redirects/system proxies to keep the secret
  # on the configured loopback API. This development-only proxy supplies the
  # configured credential for local and remote browsers; deployment hosting
  # continues to require credentials supplied by its caller.
  staged="$(mktemp "$auth_dir/Trunk.XXXXXX")"
  if ! awk -v api_port="$server_port" '
    /^\[\[proxy\]\]/ { in_proxy = 1; print; next }
    /^\[/ { in_proxy = 0 }
    in_proxy && /^backend[[:space:]]*=/ {
      print "backend = \"http://127.0.0.1:" api_port "/api/\""
      print "no_system_proxy = true"
      print "no_redirect = true"
      print "request_headers = { Authorization = \"Bearer " ENVIRON["CRONO_AUTH_DEVELOPMENT_TOKEN"] "\" }"
      next
    }
    { print }
  ' "$project_root/apps/web/Trunk.toml" > "$staged"; then
    rm -f -- "$staged"
    return 1
  fi
  mv -f -- "$staged" "$auth_dir/Trunk.toml"
}

start_stack() {
  local address="$1"
  local web_port="$2"
  local server_port="$3"
  local verbosity="$4"
  local server_pid web_pid status attempt
  local ready=false

  # Unsupported providers fail before touching a working development stack.
  case "${CRONO_AUTH_MODE-development}" in
    development) ;;
    oidc) echo "OIDC authentication is not implemented" >&2; return 2 ;;
    *) echo "CRONO_AUTH_MODE must be development or oidc" >&2; return 2 ;;
  esac

  valid_port "$web_port" && valid_port "$server_port" && [[ "$web_port" != "$server_port" ]] || {
    echo "Web and API ports must be distinct integers from 1 through 65535" >&2
    return 2
  }

  cd -- "$project_root"

  child_pids=()
  trap cleanup_children EXIT
  trap 'exit 130' INT TERM

  # The lock covers preparation, cleanup, and readiness, not stack lifetime.
  # A second start replaces the first; stop works from another shell.
  flock -w 130 -x 9 || { echo "Timed out waiting for Crono development lock" >&2; return 1; }
  prepare_development_auth "$server_port"
  stop_apps apps
  just dev-infra
  cargo build --locked -p crono-server --bin crono-server
  check_port_free "$server_port" API

  setsid env CRONO_DEV_STACK_ROOT="$project_root" CRONO_DEV_ROLE=server \
    "$project_root/target/debug/crono-server" "$verbosity" --port "$server_port" 9>&- &
  server_pid=$!
  child_pids+=("$server_pid")
  for ((attempt = 1; attempt <= 120; attempt++)); do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      if wait "$server_pid"; then
        status=1
      else
        status=$?
      fi
      echo "Crono API exited before becoming ready" >&2
      describe_port_after_failure "$server_port"
      return "$status"
    fi
    if curl --fail --silent --output /dev/null "http://127.0.0.1:${server_port}/ready"; then
      ready=true
      break
    fi
    sleep 1
  done
  [[ "$ready" == true ]] || { echo "Crono API did not become ready within 120 seconds" >&2; return 1; }

  check_port_free "$web_port" Web
  cd -- "$project_root/apps/web"
  # Trunk 0.21 logs configured proxy headers at info level. Force a filter that
  # disables that module, including when the parent shell requests trace logs.
  # The credential stays in the private config, never in command-line arguments.
  setsid env CRONO_DEV_STACK_ROOT="$project_root" CRONO_DEV_ROLE=web \
    NO_COLOR=true RUST_LOG=info,trunk::serve::proxy=off \
    trunk serve --config "$project_root/target/dev-auth/Trunk.toml" \
    --address "$address" --port "$web_port" \
    --dist "$project_root/apps/web/dist" "$project_root/apps/web/index.html" 9>&- &
  web_pid=$!
  child_pids+=("$web_pid")
  cd -- "$project_root"
  ready=false
  for ((attempt = 1; attempt <= 120; attempt++)); do
    if ! kill -0 "$web_pid" 2>/dev/null; then
      if wait "$web_pid"; then
        status=1
      else
        status=$?
      fi
      echo "Crono web exited before becoming ready" >&2
      return "$status"
    fi
    if port_is_open "$web_port"; then
      ready=true
      break
    fi
    sleep 1
  done
  [[ "$ready" == true ]] || { echo "Crono web did not become ready within 120 seconds" >&2; return 1; }
  flock -u 9
  echo "Crono development stack is ready: API :$server_port, web :$web_port"
  if [[ "$address" == 0.0.0.0 ]]; then
    echo "Open http://127.0.0.1:$web_port locally or http://<server-ip>:$web_port from your laptop"
  else
    echo "Open http://$address:$web_port"
  fi
  echo "Development web requests are authenticated automatically; clients reaching this web port have full development access"

  set +e
  wait -n "$server_pid" "$web_pid"
  status=$?
  set -e
  case "$status" in
    0|130|143)
      echo "Crono development stack stopped"
      return 0
      ;;
    *) return "$status" ;;
  esac
}

main() {
  local action="${1-}"
  local container_status stop_status
  mkdir -p -- "$project_root/target"
  exec 9>"$lock_file"
  case "$action" in
    start)
      (($# == 5)) || { echo "Usage: dev-stack.sh start ADDRESS WEB_PORT API_PORT VERBOSITY" >&2; return 2; }
      shift
      start_stack "$@"
      ;;
    stop)
      stop_status=0
      flock -w 130 -x 9 || { echo "Timed out waiting for Crono development lock" >&2; return 1; }
      stop_apps all || stop_status=1
      for container in crono-postgres crono-nats; do
        if podman container exists "$container"; then
          podman stop "$container" || stop_status=1
        else
          container_status=$?
          if ((container_status != 1)); then
            echo "Could not inspect development container $container (status $container_status)" >&2
            stop_status=1
          fi
        fi
      done
      return "$stop_status"
      ;;
    stop-apps)
      flock -w 130 -x 9 || { echo "Timed out waiting for Crono development lock" >&2; return 1; }
      stop_apps apps
      ;;
    *)
      echo "Usage: dev-stack.sh {start|stop|stop-apps}" >&2
      return 2
      ;;
  esac
}

main "$@"
