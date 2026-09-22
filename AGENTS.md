# Repository Guidelines

## Project Structure & Module Organization

- `services/server` and `services/worker`: native applications; entrypoints in `src/bin/`, CLI modules in `src/cli/`, the server HTTP API in `services/server/src/api/`, and pure control-plane concepts in `services/server/src/domain/`.
- `apps/cli`: native `crono` API client; argument parsing lives in `src/cli/`, while server address validation and the server-independent client boundary live outside clap modules.
- `apps/web`: independently deployed Leptos CSR frontend; routed pages and reusable layout primitives live in `src/`, while Trunk compiles the CSS-first Tailwind v4 stylesheet and generated `dist/` remains ignored.
- `crates/telemetry`: shared native logging and optional OTLP export.
- `services/*/tests` and `apps/cli/tests`: integration tests; `tests/support/telemetry.rs`: shared collector checks. Unit tests sit beside implementation code.

Follow `CLI_ARCHITECTURE.md`: commands define arguments, dispatch selects typed actions, and binaries execute them. The server starts its API directly; the worker's scaffolded `run` command intentionally reports an unimplemented runtime.

## Documentation Requirements

Documentation is mandatory. Code that changes behavior or flow without corresponding documentation updates is incomplete.

Style:

- Write documentation as concise narrative paragraphs.
- Do not repeat checklist-style labels such as "Purpose", "Context", "Rationale", or "Security" on every item.

Module-level docs (`//!`):

- Describe the module's end-to-end flow and responsibilities, not only its name.
- Explain why the design exists, including important trade-offs, constraints, and invariants.
- Highlight security and trust boundaries where relevant.
- For protocols or multi-step behavior, include a short "Flow Overview" section.

Item-level docs (`///`):

- Keep routine item docs concise, normally one to five lines.
- Focus on non-obvious behavior, invariants, side effects, and failure semantics.
- Expand documentation for security-critical, protocol-related, or correctness-sensitive items.
- Avoid restating module-level rationale on every struct or function.
- When adding or substantially changing a function, document its intent, key invariants, and any authorization or data-exposure behavior.

Detailed item documentation is required for cryptography and token handling, authentication or authorization transitions, role/scope decisions, protocol state machines, multi-step flows, parsing and validation precedence, fallback behavior, and unsafe assumptions.

Authorization helpers such as `can_*`, `is_*`, `*_allowed`, and `*_authorized` are access-control decisions. They must be side-effect free, and their docs must state what they authorize and which roles, scopes, or claims are required. Never trust client-provided roles or permissions; enforce scope server-side using verified credentials and authoritative server data.

## Build, Test, and Development Commands

Use stable Rust, rustfmt, Clippy, Just, Trunk, and the `wasm32-unknown-unknown` target.

- `cargo build --locked -p crono-server -p crono-worker -p crono-cli`: build native applications.
- `cargo fmt --all -- --check`: check formatting.
- `just clippy`: default/all-feature native linting plus WASM.
- `just test`: default/all-feature workspace tests.
- In `apps/web`, run `trunk serve` or `trunk build --release`.

## Coding Style & Strict Coding Rules

Use Rust 2024 and rustfmt's four-space indentation; Just recipes use two spaces. Use `snake_case` for modules/functions, `UpperCamelCase` for types, and `SCREAMING_SNAKE_CASE` for constants.

Propagate errors with `?`; handle missing data and unavailable dependencies. Avoid panic-prone shortcuts: use checked access and guard division by zero. Never hold locks across `.await`; bound concurrency, queues, and buffers. Define daemon CLI options/defaults in commands, validate them in dispatch, and keep business logic in action/application modules. Client command definitions may identify configuration inputs, but environment/default resolution and URL validation belong in dedicated configuration modules independent of clap.

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
