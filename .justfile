clippy:
  cargo clippy --locked --workspace --all-targets
  cargo clippy --locked --workspace --all-targets --all-features
  cargo clippy --locked -p crono-web --target wasm32-unknown-unknown

test:
  cargo test --locked --workspace
  cargo test --locked --workspace --all-features

# Serve crono-web on every network interface for local or remote browser testing.
web address="0.0.0.0" port="8080":
  cd apps/web && NO_COLOR=true trunk serve --address "{{ address }}" --port "{{ port }}"

db-bootstrap admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/00_init.sql

db-verify admin-url="postgres://postgres@localhost:5432/postgres":
  psql "{{ admin-url }}" -v ON_ERROR_STOP=1 -f db/sql/check.sql
