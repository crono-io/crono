# CLI architecture

Crono follows the modular CLI pattern from `cron-when`: clap defines arguments, dispatch converts them into a typed action, and the binary executes that action. The server and worker have separate command trees and application boundaries. Sharing a Cargo workspace does not make either binary depend on the other or on the GUI.

## Service layout

Each service has this structure, with its own named binary:

```text
src/
├── bin/crono-{server,worker}.rs
├── lib.rs
└── cli/
    ├── mod.rs
    ├── commands/mod.rs
    ├── dispatch/mod.rs
    ├── actions/
    │   ├── mod.rs
    │   └── run.rs
    ├── start.rs
    └── telemetry.rs
```

`commands::new()` contains clap definitions only. It supplies package metadata through `env!`, defines `run` and repeatable `-v`, and handles help/version output before logging starts. Missing commands and invalid arguments exit with clap's status 2.

`-V` prints the package version; `--version` adds the full Git commit hash as `<version> - <commit>`. Both services use the root `build.rs` and the workspace's `built` build dependency with its `git2` feature. Generated metadata is exposed through `built_info`; source archives without Git metadata report `unknown`. When Git is installed, the build script watches HEAD, loose references, and packed references, including linked worktrees. A `LazyLock` stores the long version once per process. Help styling follows Permesi: bold yellow headings, bold green usage, bold blue literals, and green placeholders. Clap detects terminal color support automatically and honors `NO_COLOR`.

`dispatch::handler(&ArgMatches) -> Result<Action>` is the argument-to-action boundary. It owns routing and any future validation that spans multiple arguments. `Action` is the typed contract; initially its only variant is `Run`.

`actions::run::execute()` owns the action's execution boundary. It currently emits an error and returns an unfinished-runtime error. Future actions will call application modules here; command definitions and startup orchestration stay free of business logic.

`cli::start() -> Result<Action>` performs setup in order:

```text
binary creates Tokio runtime
    -> commands::new().get_matches()
    -> telemetry::init(verbosity)
    -> dispatch::handler(&matches)
    -> binary matches Action and executes it
    -> telemetry shutdown on a blocking thread
    -> return the action result
```

The binary preserves the action result even when trace shutdown fails. Startup errors also pass through shutdown; help, version, and argument errors occur before an exporter exists.

## Telemetry

Each service keeps `cli/telemetry.rs` as a small adapter passing its own package name and version to `crono-telemetry`. This avoids reporting the shared library as the service identity. The library installs one global subscriber and retains an optional trace provider in `OnceLock` for explicit shutdown.

Local JSON logs always go to stderr. Verbosity defaults to error, with `-v` selecting info, `-vv` debug, and three or more occurrences trace. `RUST_LOG` directives override the default; invalid filter syntax is a startup error. This filter applies to trace spans as well as local logs.

The native packages expose a default-off `telemetry` Cargo feature that forwards to the library. It includes the OTLP dependency stack only when selected. A feature-enabled process starts an exporter only when `OTEL_EXPORTER_OTLP_ENDPOINT` exists at startup. An empty or invalid configured endpoint is an error. Changing configuration requires a restart; changing the compiled feature requires a rebuild.

The endpoint must be an explicit `http://` or `https://` URL. HTTPS uses certificate and hostname verification with WebPKI roots. Supply credentials through `OTEL_EXPORTER_OTLP_HEADERS`; endpoint user information is rejected. The SDK parses standard headers, including the trace-specific header override. Never place production credentials in checked-in configuration.

Export uses gRPC and gzip. Protocol environment variables, when present, must specify `grpc`. Crono explicitly selects the generic endpoint, a three-second export timeout, and gzip; signal-specific endpoint, timeout, and compression variables do not override these choices. Standard SDK batching and sampling settings still apply.

The SDK batches spans and adds `service.name` and the inherited `service.version`; W3C trace-context propagation is registered for future transports. Shutdown asks the batch processor to flush within five seconds on a blocking thread while Tokio remains alive to drive gRPC. There are no fixed sleeps. Export/shutdown failure can lose traces; it never establishes a successful application outcome.

## Browser boundary and future changes

`crono-web` is a Leptos CSR application with a WASM entrypoint and Trunk static output. It does not use the native CLI or telemetry library. Its native no-op entrypoint exists only so workspace checks can run without browser dependencies; validate the actual WASM target separately.

To add a service command, define its clap arguments, add a typed action, map it in dispatch, implement an action module, and extend the binary's match. Add application modules when there is real runtime behavior, and extract shared domain or wire contracts when both consumers need them. Package version inheritance does not require shared business logic or a universal configuration crate.
