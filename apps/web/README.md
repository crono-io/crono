# Crono web

`crono-web` is the optional, independently built and deployed browser client
for Crono. It uses Leptos CSR, Leptos Router, Trunk, Tailwind CSS v4, and Google
Material Symbols Outlined. The server does not embed or serve these assets, and
Crono remains fully usable through its public API and the `crono` CLI without
the GUI.

The application uses a dark infrastructure-style sidebar, compact top toolbar,
and light routed workspace as its baseline visual language. Overview displays
authorized live counts. Namespaces, Jobs, and Targets provide creation and list
workflows, while Runs submits qualified resources and displays durable dispatch
state. Reserved pages continue to use truthful empty states.

## Development

Install a stable Rust toolchain, the `wasm32-unknown-unknown` target, and Trunk
0.21.14. The complete `dev-start` workflow also requires Podman and `curl` for
its local PostgreSQL and NATS containers:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk --version 0.21.14
```

From the workspace root, the development recipe listens on every network
interface by default so the UI can be tested from another machine:

```sh
just web
just web 127.0.0.1 3001
just dev-start
```

Open `http://<development-host>:3000` from the remote browser when using the
default port. Binding to `0.0.0.0` exposes the development server to reachable
networks; use `just web 127.0.0.1` when remote access is not wanted, and keep
host firewall rules appropriate for the environment. `just dev-start` runs the
frontend and `crono-server` together, using ports `3000` and `8080`
respectively. The server recipe also ensures the local PostgreSQL 18 and NATS
JetStream containers are initialized and running. `Trunk.toml` proxies
same-origin `/api/v1` requests to `http://127.0.0.1:8080`, avoiding development
CORS configuration while preserving the independently deployed client boundary.

`trunk serve`, used by both `just web` and `just dev-start`, already watches the
frontend's Rust, HTML, CSS, and asset inputs. Saving a change triggers a WASM
rebuild and automatically reloads connected browsers, so this workflow does not
need `cargo-watch` or a second compilation process.

The equivalent direct commands from `apps/web` are:

```sh
trunk serve --address 0.0.0.0 --port 3000
trunk build --release
```

Trunk's `tailwind-css` asset pipeline downloads and invokes the standalone
Tailwind CLI version pinned in `Trunk.toml`. The CSS-first source in `style.css`
imports Tailwind v4 and explicitly scans `src/`, so no Node, npm, Vite,
JavaScript configuration, generated stylesheet, or second watch process is
required. Release assets are written to the ignored `dist/` directory. Crono's
small semantic color layer is defined through Tailwind v4 theme variables in
that stylesheet so future palette changes do not require editing each component.

Material Symbols are loaded through one Google Fonts link in `index.html` and
rendered through the typed `Icon` component. The common outlined-family font
settings live in `style.css`, keeping the hosted font URL isolated so the asset
can be self-hosted later without changing page or navigation code. Trunk copies
the white Crono SVG from the repository-level `logo/` directory for the sidebar
brand mark; navigation and interface icons continue to use Material Symbols.

From the workspace root, validate Rust browser code with:

```sh
cargo check --locked -p crono-web --target wasm32-unknown-unknown
cargo clippy --locked -p crono-web --target wasm32-unknown-unknown
```

Native workspace tests verify the pure route/navigation model. A Trunk release
build verifies WASM binding, Tailwind compilation, and static asset assembly;
browser inspection remains necessary for layout and responsive behavior.

## Architecture boundary

The browser is a public API client:

```text
Page
  |
  v
browser API client
  |
  | HTTPS
  v
crono-server
```

HTTP behavior belongs in `src/api.rs` rather than being implemented separately
inside pages. `crono-web` must not depend on `crono-server`, `crono-worker`,
PostgreSQL, NATS, server repositories, or server domain/application types.
Transport-only wire types are shared through `crono-api`.

Credential authentication is intentionally not implemented. There is no login UI, OIDC,
OAuth, PKCE, cookie/session handling, token storage, JWT processing, or RBAC in
this foundation. The toolbar leaves room for future user and theme controls but
keeps those placeholders disabled so it does not imply an authentication or
preference contract. The server currently attaches its fixed development
principal and executes the complete authorization interface in permit-all mode.
