# CLI architecture

Crono follows the modular CLI pattern from `cron-when`: clap defines arguments, dispatch converts
them into a typed action, and the binary executes that action. The `crono-server` and `crono-worker`
commands are daemon entrypoints; `crono` is an independent client of the server's public API.
Sharing a Cargo workspace does not allow the client or browser to depend on server internals.

## Daemon CLI layout

Each service has its own named binary. The API-driven server adds an `api` module and an OpenAPI
generator alongside the shared CLI layering:

```text
services/server/src/
├── api/
│   ├── handlers/health.rs
│   ├── handlers/mod.rs
│   ├── mod.rs
│   └── openapi.rs
├── bin/
│   ├── crono-server.rs
│   └── openapi.rs
├── lib.rs
└── cli/
    ├── mod.rs
    ├── commands/mod.rs
    ├── dispatch/mod.rs
    ├── actions/
    │   ├── mod.rs
    │   └── server.rs
    ├── start.rs
    └── telemetry.rs
```

The worker retains the same CLI directories with `actions/run.rs` and its intentionally unfinished
`run` command.

`commands::new()` contains clap definitions only. For the server it defines `--port` (also read from
`CRONO_SERVER_PORT`, default `8080`) and repeatable `-v`; invoking `crono-server` directly starts the
API. The worker still defines `run`. Both command trees handle help/version output before logging
starts, and invalid arguments exit with clap's status 2.

`-V` prints the package version; `--version` adds the full Git commit hash as `<version> - <commit>`. All three native commands use the root `build.rs` and the workspace's `built` build dependency with its `git2` feature. Generated metadata is exposed through `built_info`; source archives without Git metadata report `unknown`. When Git is installed, the build script watches HEAD, loose references, and packed references, including linked worktrees. A `LazyLock` stores the long version once per process. Help styling follows Permesi: bold yellow headings, bold green usage, bold blue literals, and green placeholders. Clap detects terminal color support automatically and honors `NO_COLOR`.

`dispatch::handler(&ArgMatches) -> Result<Action>` is the argument-to-action boundary. It owns
routing and any validation that spans multiple arguments. The server produces
`Action::Server(server::Args)`; the worker produces `Action::Run`.

`actions::server::execute()` starts the HTTP application and reports listener failures through
structured logging. `actions::run::execute()` in the worker continues to return the documented
unfinished-runtime error. Command definitions and startup orchestration stay free of application
logic.

`cli::start() -> Result<Action>` performs setup in order:

```text
binary creates Tokio runtime
    -> commands::new().get_matches()
    -> telemetry::init(verbosity)
    -> dispatch::handler(&matches)
    -> binary executes the typed Action asynchronously
    -> telemetry shutdown on a blocking thread
    -> return the action result
```

The binary preserves the action result even when trace shutdown fails. Startup errors also pass through shutdown; help, version, and argument errors occur before an exporter exists.

## API client CLI layout

The `crono-cli` package builds the user-facing `crono` executable and keeps command syntax,
configuration, and server communication separate:

```text
apps/cli/src/
├── bin/crono.rs
├── lib.rs
├── cli/
│   ├── commands/mod.rs
│   ├── dispatch/mod.rs
│   ├── actions/mod.rs
│   └── start.rs
├── client/
│   ├── config.rs
│   └── mod.rs
└── config/mod.rs
```

The client flow is:

```text
clap syntax
    -> resolve and validate client configuration
    -> typed Action
    -> action execution
    -> CronoClient
    -> future HTTPS request
    -> public crono-server API
```

`commands` knows only clap syntax. `config` applies explicit `--address`, then `CRONO_ADDR`, then
`http://127.0.0.1:8080` for local development. `client::config` parses the selected value with
`url::Url`, accepts HTTPS or loopback-only development HTTP with a host, and rejects user-info,
queries, and fragments. There is no TLS bypass option. The client does not log or persist
credentials.

`CronoClient` owns the validated base URL but intentionally has no HTTP dependency or request
methods. The server currently exposes health probes, not stable job/run contracts, so the CLI shell
returns an explicit unfinished-command error without opening a connection. It has no dependency on
`crono-server`, PostgreSQL, NATS, workers, server repositories, or authorization internals.

Once public contracts exist, commands may grow toward:

```text
crono job list                       crono run <job>
crono job show <job>                 crono run list
crono job create ...                 crono run show <run>
                                     crono run cancel <run>
crono worker list                    crono worker show <worker>
```

Every command must remain a thin client of the same public server application logic used by
`crono-web`; it must never create jobs, runs, or schedules through CLI-only logic. Stable wire types
or a separate SDK crate can be evaluated after the API exists, not before.

Future interactive authentication may use OIDC Authorization Code with PKCE and a system browser.
Browser sessions may independently use server-managed `HttpOnly` cookies, while automation may use
scoped service credentials. These mechanisms must resolve to the same server-side principal and
authorization model. This shell deliberately adds no token flag, token file, or credential store.

## Telemetry

Each service keeps `cli/telemetry.rs` as a small adapter passing its own package name and version to `crono-telemetry`. This avoids reporting the shared library as the service identity. The library installs one global subscriber and retains an optional trace provider in `OnceLock` for explicit shutdown.

Local JSON logs always go to stderr. Verbosity defaults to error, with `-v` selecting info, `-vv` debug, and three or more occurrences trace. `RUST_LOG` directives override the default; invalid filter syntax is a startup error. This filter applies to trace spans as well as local logs.

The native packages expose a default-off `telemetry` Cargo feature that forwards to the library. It includes the OTLP dependency stack only when selected. A feature-enabled process starts an exporter only when `OTEL_EXPORTER_OTLP_ENDPOINT` exists at startup. An empty or invalid configured endpoint is an error. Changing configuration requires a restart; changing the compiled feature requires a rebuild.

The endpoint must be an explicit `http://` or `https://` URL. HTTPS uses certificate and hostname verification with WebPKI roots. Supply credentials through `OTEL_EXPORTER_OTLP_HEADERS`; endpoint user information is rejected. The SDK parses standard headers, including the trace-specific header override. Never place production credentials in checked-in configuration.

Export uses gRPC and gzip. Protocol environment variables, when present, must specify `grpc`. Crono explicitly selects the generic endpoint, a three-second export timeout, and gzip; signal-specific endpoint, timeout, and compression variables do not override these choices. Standard SDK batching and sampling settings still apply.

The SDK batches spans and adds `service.name` and the inherited `service.version`; W3C trace-context propagation is registered for future transports. Shutdown asks the batch processor to flush within five seconds on a blocking thread while Tokio remains alive to drive gRPC. There are no fixed sleeps. Export/shutdown failure can lose traces; it never establishes a successful application outcome.

## Client boundaries and future changes

`crono-web` is a Leptos CSR application with a WASM entrypoint and Trunk static output. It is a peer
of `crono`, not an asset owned by it or by `crono-server`. The GUI remains independently built and
deployed; its native no-op entrypoint exists only so workspace checks can run without browser
dependencies. Crono remains usable through the server API and CLI when the GUI is absent.

Leptos Router selects page content inside one shared application shell. A pure typed navigation
model keeps route paths, labels, Material Symbols, sidebar sections, and active-state tests aligned.
The shell establishes Crono's dark navigation, compact toolbar, and light operational workspace;
reusable summaries and cards render only honest zero and empty states. Trunk invokes a pinned
standalone Tailwind CSS v4 CLI for CSS-first compilation. Future HTTP access belongs behind a
dedicated client/service boundary, not inside pages and never through server Rust internals.
Authentication remains intentionally undefined until the public server contract exists.

To add a daemon command, define its clap arguments, add a typed action, map it in dispatch, and
implement its action module. To add a client command, first define and implement the public server
contract, then add a `CronoClient` operation and a thin typed action. Documented HTTP routes belong
in the server's `OpenApiRouter` so runtime routing and generated contracts cannot drift. Package
version inheritance does not require shared business logic or a universal configuration crate.
