# Crono web

`crono-web` is the optional, independently built and deployed browser client
for Crono. It uses Leptos CSR, Leptos Router, Trunk, Tailwind CSS v4, and Google
Material Symbols Outlined. The server does not embed or serve these assets, and
Crono remains fully usable through its public API and the `crono` CLI without
the GUI.

The application uses a dark infrastructure-style sidebar, compact top toolbar,
and light routed workspace as its baseline visual language. Overview displays
authorized live counts with Material Symbols plus Recent Runs and Workers.
Namespaces, Queues, Jobs, Targets, and Target Sets provide creation and list
workflows. Queues additionally support rename, enable/disable, and guarded
deletion; the bootstrap `default` Queue is visibly protected while its
description remains editable. Runs selects existing resources and displays
durable dispatch state; Workers shows heartbeat-derived presence and capacity.
Reserved pages use truthful empty states.

Resource creation uses the shared DNS-1123 label rule from `crono-api` and
never rewrites user input. Existing relationships use client-filtered,
keyboard-accessible selectors that fetch each collection once and retain the
selected UUID rather than a display name. Target Sets use the same pattern for
multi-selection, and Job creation selects an enabled Queue by UUID. Loading,
empty, stale-selection, API, and field validation
states remain in the form so failed submissions do not discard entered values.
The Resources sidebar's Jobs submenu links to All Jobs (`/jobs`) and Create Job
(`/jobs/new`), never to individual Job records. Browsing requires a selected
Namespace and offers a Create Job action and an actionable empty state. Each
Job's Edit link opens `/jobs/{id}/edit`; the page reloads that Job by ID and
retains the existing update API. The shared create/edit form distinguishes
direct Process execution from a literal Shell script with an absolute
interpreter. Shell input templates are passed as positional arguments (`$1`,
`$2`, …), never inserted into shell syntax. Both modes include examples and a
preview of the command and merged inputs for a
selected Target or Target Set. The preview does not create a Run or include
later invocation inputs. Recent Runs load authorized Attempt stdout, stderr,
and errors when an operator opens their output. The Runs history keeps Job/Target,
Started, Finished, and Status aligned for comparison on wide screens and labels
those fields on narrow screens. Start and finish retain seconds and exact-time
tooltips; the finish cell labels duration as elapsed time. History actions use
decorative Material Symbols alongside visible Details, Output, and Re-run text,
and the Re-run confirmation expands beneath the action row on narrow screens.
Run details offer Refresh status only while the Run can still change. Opened
Attempt output likewise offers Refresh output only for nonterminal Runs; terminal
outcomes load output on demand without a redundant refresh control. Eligible
terminal Runs still offer Re-run.

During an active Attempt, the worker uploads bounded, redacted output tails
about once per second. Refresh output fetches that saved progress; a script
must print a line before a sleep if progress is expected during the sleep.
Worker names link from `/workers` to `/workers/{worker_id}`, where the same
WorkerRead authorization guards heartbeat-backed host/platform, default shell,
and allowlisted child-environment diagnostics. Arbitrary worker environment
values and credentials are never shown.

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
same-origin `/api` requests to `http://127.0.0.1:8080`, avoiding development
CORS configuration while preserving the independently deployed client boundary.
Running `just dev-start` again replaces stale API and web processes from the
same checkout. `just dev-stop` can be run independently to stop those processes,
any local worker from this checkout, and the two development containers without
deleting their volumes. An unrelated listener on either port is not killed.
The launcher requires Linux `/proc`, `flock`, and `ss`; `ss` catches IPv6-only
listeners that would otherwise prevent the API's dual-stack bind.

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
cargo test --locked -p crono-web --target wasm32-unknown-unknown
```

The browser test command requires the version-matched
`wasm-bindgen-test-runner` from `wasm-bindgen-cli` and a Chromium-compatible
browser. Native workspace tests verify the pure route/navigation model. A
Trunk release build verifies WASM binding, Tailwind compilation, and static
asset assembly; browser inspection remains necessary for layout and responsive
behavior.

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
