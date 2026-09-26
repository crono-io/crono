#!/usr/bin/env bash
# Compare the committed OpenAPI contract between two commits with oasdiff.
#
# Usage: scripts/api-breaking.sh [BASE] [HEAD]
#   BASE defaults to origin/main and HEAD to the current commit, so running it
#   before `git push` checks exactly what the push would publish. CI passes the
#   pushed range instead.
#
# The script prints the API changelog, then fails on ERR-level breaking changes
# unless a commit in BASE..HEAD carries an `API-Breaking: <reason>` trailer,
# in which case the breaking changes are reported without failing. It skips
# cleanly when BASE is missing or predates the committed contract. When
# GITHUB_STEP_SUMMARY is set, the report is also written to the job summary.
set -euo pipefail

readonly spec="docs/openapi/crono-server.json"
readonly image="docker.io/tufin/oasdiff:v1.32.1"
readonly base_ref="${1:-origin/main}"
readonly head_ref="${2:-HEAD}"

report() {
  printf '%s\n' "$@"
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '%s\n' "$@" >> "$GITHUB_STEP_SUMMARY"
  fi
}

cd "$(git rev-parse --show-toplevel)"

if [[ "$base_ref" == origin/* ]]; then
  git fetch --quiet origin "${base_ref#origin/}" || true
fi
if [[ "$base_ref" =~ ^0+$ ]] || ! git rev-parse --verify --quiet "${base_ref}^{commit}" >/dev/null; then
  report "API contract comparison skipped: base ${base_ref} is not an available commit."
  exit 0
fi
if ! git cat-file -e "${base_ref}:${spec}" 2>/dev/null; then
  report "API contract comparison skipped: ${base_ref} has no committed ${spec}."
  exit 0
fi

engine="$(command -v podman || command -v docker || true)"
if [[ -z "$engine" ]]; then
  echo "podman or docker is required to run oasdiff" >&2
  exit 1
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
chmod 755 "$workdir"
git show "${base_ref}:${spec}" > "$workdir/base.json"
git show "${head_ref}:${spec}" > "$workdir/revision.json"

oasdiff() {
  "$engine" run --rm --volume "$workdir:/specs:ro,z" "$image" "$@"
}

report "### OpenAPI changes (${base_ref:0:12}..${head_ref:0:12})" ""
changelog="$(oasdiff changelog /specs/base.json /specs/revision.json --format markdown)"
report "${changelog:-No API changes.}" ""

reasons="$(git log --format='%(trailers:key=API-Breaking,valueonly)' "${base_ref}..${head_ref}" \
  | sed '/^[[:space:]]*$/d')"
fail_on=(--fail-on ERR)
if [[ -n "$reasons" ]]; then
  report "### Acknowledged breaking changes"
  while IFS= read -r reason; do
    report "- ${reason}"
  done <<< "$reasons"
  report ""
  fail_on=()
fi

status=0
breaking="$(oasdiff breaking /specs/base.json /specs/revision.json "${fail_on[@]}")" || status=$?
report "### Breaking changes" "" '```' "${breaking:-None.}" '```'
if (( status != 0 )); then
  report "" "Breaking API changes found. If they are intentional, add an" \
    "\`API-Breaking: <reason>\` trailer to the commit that makes them."
fi
exit "$status"
