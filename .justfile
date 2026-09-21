clippy:
  cargo clippy --locked --workspace --all-targets
  cargo clippy --locked --workspace --all-targets --all-features
  cargo clippy --locked -p crono-web --target wasm32-unknown-unknown

test:
  cargo test --locked --workspace
  cargo test --locked --workspace --all-features
