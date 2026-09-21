# Crono web

An independently built Leptos CSR application. Trunk compiles it to browser WASM and static assets. The initial page displays Crono and the version inherited from the root workspace; it makes no API requests.

Install a stable Rust toolchain, the `wasm32-unknown-unknown` target, and Trunk 0.21.14. From this directory:

```sh
trunk serve
trunk build --release
```

The release assets are written to `dist/` and can be served by an ordinary static host. Trunk uses the workspace lockfile and obtains its matching `wasm-bindgen` tool as needed. No Node or CSS build pipeline is required.

From the workspace root, validate browser code with:

```sh
cargo check --locked -p crono-web --target wasm32-unknown-unknown
cargo clippy --locked -p crono-web --target wasm32-unknown-unknown
```

Native workspace checks only compile a tooling entrypoint; verify the Trunk build in a browser too. The GUI will eventually call the public server API over HTTPS. It must not depend on the server crate, receive NATS credentials, or access PostgreSQL.
