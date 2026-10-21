# Bob Agent Instructions

## Non-negotiables

- Surgical edits only: every changed line must trace to the request. No drive-by refactors, no reformatting untouched code, no deleting pre-existing dead code (mention it instead).
- Fix root causes. Never silence a warning, weaken a check, or patch a symptom to make a gate pass.
- Never report completion while any `just` gate below fails. Report the failure verbatim and stop.
- Stop and ask when a request is ambiguous, conflicts with existing conventions, or requires credentials, publishing, or destructive operations.
- When a correction reveals a missing rule, add one specific rule here ("always X for Y", never "be careful with Y"); delete rules that never catch real errors.

## Scope

- Bob is a minimal hexagonal AI agent framework: domain core in `crates/bob-core`, orchestration in `crates/bob-runtime`, concrete integrations in `crates/bob-adapters`, channel and streaming types in `crates/bob-chat`, the skill system in `crates/bob-skills`, and the CLI in `bin/bob-cli`.
- Hexagonal purity is a hard rule: `bob-core` has zero internal dependencies, and `bob-runtime` must never import `bob-adapters`. `bob-runtime` talks to the outside world only through `bob-core` ports.
- The crate dependency direction is `bob-core` <- `bob-chat` / `bob-runtime` / `bob-adapters` / `bob-skills` <- `bob-cli`. A new dependency that points the other way is a bug, not a style issue.
- `agent.toml` in the repository root is the reference configuration. It documents the runtime, store, policy, approval, MCP and skills sections; keep it in sync when a setting is added or renamed.
- English only in code, comments, docs, and commit messages.

## Execution

- Parallelize aggressively: dispatch subagents for independent research, exploration, and implementation.
- NEVER run two `cargo` commands at once; `target/` is file-locked. Finish edits first, then run cargo.
- NEVER use `git worktree`. Edit the current branch in place.
- TDD for behavior changes: failing test first in the affected crate, smallest change to pass, then run `just test`. Keep `proptest` properties in the normal test loop.

## Verification loop

After each feature or bug fix, in order; any failure means the task is not done:

```bash
just format
just lint
just test
```

`just lint` is the gate: `typos`, `rumdl check`, `taplo fmt --check` for TOML, `cargo +nightly fmt --check`, `cargo +nightly clippy -D warnings` and `cargo machete` for unused dependencies. This repository has no CI workflow that runs `just`, so `just setup` must install every tool the gate needs.

## Rust framework rules

- Never hand-type dependency versions. Use `cargo add <crate> -p <crate>`. Shared crates live in the root `[workspace.dependencies]` with numeric versions and no features, and are referenced as `dep = { workspace = true }`.
- Sub-crates inherit `version`, `edition`, `license`, `repository`, `homepage`, `authors`, `description`, `keywords` and `categories` with `workspace = true`.
- `.rustfmt.toml` sets `unstable_features = true`, so formatting requires nightly: use `cargo +nightly fmt` and `cargo +nightly clippy`. The release workflow installs stable only, so nightly must be present locally.
- `.taplo.toml` is the TOML contract: `taplo fmt` in `format`, `taplo fmt --check` in `lint`. Do not reorder the `package` table.
- Workspace lints deny `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro` and `allow_attributes`. A bare `#[allow(...)]` on a rustc lint is a compile error; use `#[expect(lint, reason = "...")]`.
- `print_stdout` and `print_stderr` are denied in library code. The CLI writes through `tracing-subscriber` instead of `println!`.
- `missing_docs` and `missing_debug_implementations` are warnings. Public items in `bob-core` and `bob-runtime` are documented and derive `Debug`.

## Dependency choices

- Errors: `thiserror` in every library crate, `eyre` in `bin/bob-cli`. Never `anyhow`.
- Logging: `tracing` plus `tracing-subscriber` with `env-filter` in the CLI. Never `log`.
- Ports are async traits from `async-trait`; the framework is Tokio-based. Use `flume` for sync MPMC channels and `scc` for concurrent maps. Never `std::sync::mpsc`, never `dashmap`.
- Middleware is composed with `tower::Service` and `tower` middleware (`timeout`, `limit`, `util`). Do not introduce a bespoke middleware chain.
- LLM access goes through `liter-llm` with the `native-http` feature. MCP access goes through `rmcp`. Both live behind `bob-adapters` features (`llm-liter`, `mcp-rmcp`).
- Observability is feature-gated: `observe-tracing` for plain tracing and `observe-otel` for the OpenTelemetry event sink with `opentelemetry-otlp`.
- Tool schemas are validated with `jsonschema`; keep schema validation in `bob-runtime`, not in adapters.
- Observability is feature-gated: `observe-tracing` for plain tracing and `observe-otel` for the OpenTelemetry event sink with `opentelemetry-otlp`. Storage is feature-gated too, with `store-memory` for the in-process stores.

## Code conventions

- The agent loop is a native `enum` plus `match` state machine in `bob-runtime`. Do not pull in an external FSM crate.
- The turn loop is deterministic and bounded: `max_steps` and `turn_timeout_ms` come from the config. A change that can make a turn unbounded is a bug.
- Compile-time safety is a feature, not decoration: keep the `TypeState` builder in `bob-runtime/src/typed_builder.rs` and the state types in `bob-runtime/src/typestate.rs` intact, and keep the typed tool adapter's `Input`/`Output` associated types in `bob-core/src/typed_tool.rs`.
- Skills are `SKILL.md` documents parsed by `bob-skills` with YAML frontmatter. Add skill syntax in `crates/bob-skills/src/parsing/`, never in the runtime.
- Tool policies, approval modes and access control live in `bob-core` and are enforced by `bob-adapters`; a new tool must have an explicit policy and approval default.
- Unit tests are colocated in `#[cfg(test)]` modules; integration tests live in each crate's `tests/` directory. `bob-core` uses `proptest` for invariants.
- Feature-gated code must compile with the feature off. Run `cargo check --all-targets --all-features` and also `cargo check` without default features before proposing a change.
