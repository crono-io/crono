# Repository Guidelines

## Project Structure & Module Organization

- `services/server` and `services/worker`: native applications; entrypoints in `src/bin/`, CLI modules in `src/cli/`.
- `apps/web`: Leptos CSR frontend, with Rust in `src/` and HTML/CSS alongside `Trunk.toml`; generated `dist/` is ignored.
- `crates/telemetry`: shared native logging and optional OTLP export.
- `services/*/tests`: integration tests; `tests/support/telemetry.rs`: shared collector checks. Unit tests sit beside implementation code.

Follow `CLI_ARCHITECTURE.md`: commands define arguments, dispatch selects typed actions, and binaries execute them. The scaffold's `run` commands intentionally report unimplemented runtimes.

## Build, Test, and Development Commands

Use stable Rust, rustfmt, Clippy, Just, Trunk, and the `wasm32-unknown-unknown` target.

- `cargo build --locked -p crono-server -p crono-worker`: build native applications.
- `cargo fmt --all -- --check`: check formatting.
- `just clippy`: default/all-feature native linting plus WASM.
- `just test`: default/all-feature workspace tests.
- In `apps/web`, run `trunk serve` or `trunk build --release`.

## Coding Style & Strict Coding Rules

Use Rust 2024 and rustfmt's four-space indentation; Just recipes use two spaces. Use `snake_case` for modules/functions, `UpperCamelCase` for types, and `SCREAMING_SNAKE_CASE` for constants.

Propagate errors with `?`; handle missing data and unavailable dependencies. Avoid panic-prone shortcuts: use checked access and guard division by zero. Never hold locks across `.await`; bound concurrency, queues, and buffers. Define service CLI options/defaults in commands, validate them in dispatch, and keep business logic in action/application modules.

## Lint Contract

Every crate must inherit `[lints] workspace = true`. Root `Cargo.toml` enforces:

- Rust `warnings = "deny"` and `unsafe_code = "forbid"`.
- Clippy `all` and `pedantic` denied; `all` includes `correctness`, `suspicious`, `perf`, and `complexity`.
- Explicit denials: `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `await_holding_lock`, `needless_collect`, and `large_stack_arrays`.

Fix findings instead of weakening lints or adding production suppressions such as `#[allow(clippy::...)]`. Narrow test-only allowances require documented justification. Failing formatting, Clippy, or tests blocks review readiness.

## Testing Guidelines

Use Rust/Tokio tests with behavioral names such as `run_reports_unimplemented_runtime`. Cover failure paths and add regression tests for fixes. Telemetry tests require localhost sockets. Verify GUI changes with a Trunk build and browser inspection; native checks only compile its tooling entrypoint.

## Commit & Pull Request Guidelines

Use plain imperative subjects, e.g. `Refine messaging and execution architecture`. Never use prefixes such as `feat:`, `fix:`, `chore:`, or `docs:`. PRs explain behavior, validation, and relevant issues; include GUI screenshots. Update affected documentation.

## Versioning & Configuration

Inherit the root version; update `Cargo.lock` alongside dependency/version changes. Preserve independent builds, HTTPS API boundaries, and TLS verification. Never commit or log credentials. OTLP requires `--features telemetry` plus `OTEL_EXPORTER_OTLP_ENDPOINT`.
