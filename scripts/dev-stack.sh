#!/usr/bin/env bash
# Manage only this checkout's local development processes. Never kill an
# arbitrary listener merely because it happens to use a Crono port.
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
      if [[ "$managed_kind" == just ]]; then
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

valid_port() {
  [[ "$1" =~ ^[0-9]+$ ]] && ((10#$1 >= 1 && 10#$1 <= 65535))
}

start_stack() {
  local address="$1"
  local web_port="$2"
  local server_port="$3"
  local verbosity="$4"
  local server_pid web_pid status attempt
  local ready=false

  valid_port "$web_port" && valid_port "$server_port" && [[ "$web_port" != "$server_port" ]] || {
    echo "Web and API ports must be distinct integers from 1 through 65535" >&2
    return 2
  }

  child_pids=()
  trap cleanup_children EXIT
  trap 'exit 130' INT TERM

  # The lock covers cleanup and readiness checks, not the lifetime of the
  # stack. A second start replaces the first; stop works from another shell.
  flock -w 130 -x 9 || { echo "Timed out waiting for Crono development lock" >&2; return 1; }
  stop_apps apps
  check_port_free "$server_port" API
  check_port_free "$web_port" Web

  setsid env CRONO_DEV_STACK_ROOT="$project_root" CRONO_DEV_ROLE=server \
    just server "$server_port" "$verbosity" 9>&- &
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
      return "$status"
    fi
    if curl --fail --silent --output /dev/null "http://127.0.0.1:${server_port}/ready"; then
      ready=true
      break
    fi
    sleep 1
  done
  [[ "$ready" == true ]] || { echo "Crono API did not become ready within 120 seconds" >&2; return 1; }

  setsid env CRONO_DEV_STACK_ROOT="$project_root" CRONO_DEV_ROLE=web \
    just web "$address" "$web_port" 9>&- &
  web_pid=$!
  child_pids+=("$web_pid")
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

  set +e
  wait -n "$server_pid" "$web_pid"
  status=$?
  set -e
  if ((status == 0)); then
    echo "A Crono development service stopped unexpectedly" >&2
    return 1
  fi
  return "$status"
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
