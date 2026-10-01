#!/usr/bin/env bash
# Exercise repeat bootstrap and backfill inside just test's disposable PostgreSQL.
set -euo pipefail

container="$1"

query() {
  podman exec "$container" psql --username postgres --dbname crono \
    --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 --command "$1"
}

bootstrap() {
  podman exec --env PGOPTIONS=--client-min-messages=warning "$container" psql \
    --username postgres --dbname postgres --set ON_ERROR_STOP=1 \
    --file /tmp/crono-sql/00_init.sql >/dev/null
}

verify() {
  podman exec "$container" psql --username postgres --dbname postgres \
    --set ON_ERROR_STOP=1 --file /tmp/crono-sql/check.sql >/dev/null
}

verify
before="$(query "SELECT n.id || '|' || t.id || '|' || q.id
  FROM crono.namespaces n JOIN crono.targets t ON t.namespace_id = n.id
  CROSS JOIN crono.queues q
  WHERE n.name = 'default' AND t.name = 'default' AND q.name = 'default'")"
query "UPDATE crono.targets SET arguments = '[\"preserved\"]'::jsonb
  WHERE name = 'default' AND namespace_id =
    (SELECT id FROM crono.namespaces WHERE name = 'default')" >/dev/null
bootstrap
verify
after="$(query "SELECT n.id || '|' || t.id || '|' || q.id
  FROM crono.namespaces n JOIN crono.targets t ON t.namespace_id = n.id
  CROSS JOIN crono.queues q
  WHERE n.name = 'default' AND t.name = 'default' AND q.name = 'default'")"
[[ "$before" == "$after" ]] || { echo "Bootstrap replaced starter IDs" >&2; exit 1; }
preserved="$(query "SELECT arguments = '[\"preserved\"]'::jsonb
  FROM crono.targets WHERE name = 'default' AND namespace_id =
    (SELECT id FROM crono.namespaces WHERE name = 'default')")"
[[ "$preserved" == t ]] || { echo "Bootstrap replaced starter Target settings" >&2; exit 1; }

query "DELETE FROM crono.targets WHERE name = 'default' AND namespace_id =
  (SELECT id FROM crono.namespaces WHERE name = 'default')" >/dev/null
bootstrap
verify
namespace_after="$(query "SELECT id FROM crono.namespaces WHERE name = 'default'")"
[[ "$namespace_after" == "${before%%|*}" ]] || {
  echo "Backfilling the Target replaced the Namespace" >&2
  exit 1
}
query "DELETE FROM crono.targets WHERE name = 'default' AND namespace_id =
  (SELECT id FROM crono.namespaces WHERE name = 'default')" >/dev/null
query "DELETE FROM crono.namespaces WHERE name = 'default'" >/dev/null
bootstrap
verify
