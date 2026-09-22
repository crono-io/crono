# Crono web

`crono-web` is the optional, independently built and deployed browser client
for Crono. It uses Leptos CSR, Leptos Router, Trunk, Tailwind CSS v4, and Google
Material Symbols Outlined. The server does not embed or serve these assets, and
Crono remains fully usable through its public API and the `crono` CLI without
the GUI.

The application uses a dark infrastructure-style sidebar, compact top toolbar,
and light routed workspace as its baseline visual language. Overview combines
truthful zero-count resource summaries with operational empty states and concise
first-use guidance; the remaining routes reuse the same page-header, card, icon,
and empty-state primitives. It does not make API requests or fabricate backend
resources.

## Development

Install a stable Rust toolchain, the `wasm32-unknown-unknown` target, and Trunk
0.21.14:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk --version 0.21.14
```

From the workspace root, the development recipe listens on every network
interface by default so the UI can be tested from another machine:

```sh
just web
just web 0.0.0.0 3000
```

Open `http://<development-host>:8080` from the remote browser when using the
default port. Binding to `0.0.0.0` exposes the development server to reachable
networks; use `just web 127.0.0.1` when remote access is not wanted, and keep
host firewall rules appropriate for the environment.

The equivalent direct commands from `apps/web` are:

```sh
trunk serve --address 0.0.0.0 --port 8080
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
future API client
  |
  | HTTPS
  v
crono-server
```

Future HTTP behavior belongs in a dedicated client/service layer rather than
inside pages. `crono-web` must not depend on `crono-server`, `crono-worker`,
PostgreSQL, NATS, server repositories, or server domain/application types.
Shared wire types can be considered only after public contracts stabilize.

Authentication is intentionally not implemented. There is no login UI, OIDC,
OAuth, PKCE, cookie/session handling, token storage, JWT processing, or RBAC in
this foundation. The toolbar leaves room for future user and theme controls but
keeps those placeholders disabled so it does not imply an authentication or
preference contract.
