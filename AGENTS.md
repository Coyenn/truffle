# agents.md

This repository is a Rust workspace (`Cargo.toml` lists `crates/*`). It ships the `truffle` CLI and vendors `asphalt` as a workspace member.

## Quick Commands

### Recommended dev environment (Nix)

- Enter dev shell: `nix develop`
- Run a command inside the shell without entering it:
  - `nix develop -c cargo test --all`
  - `nix develop -c cargo fmt -- --check`

`flake.nix` provides `rustfmt` + `clippy`; if you see missing `cargo-fmt`/`rustfmt`, you are likely using a different toolchain.

### Build

- Debug build (workspace): `cargo build`
- Release build (workspace): `cargo build --release`
- Build the CLI package only: `cargo build -p truffle`
- Release build the CLI package: `cargo build --release -p truffle`

### Run locally

- Run Truffle:
  - `cargo run -p truffle -- --help`
  - `cargo run -p truffle -- sync`
  - `cargo run -p truffle -- highlight assets/images --dry-run`

### Tests

- Run all tests (matches CI): `cargo test --all`
- Run tests for one package:
  - `cargo test -p truffle`
  - `cargo test -p asphalt`

Single test guidance (Rust/Cargo):

- By test name (substring match): `cargo test <name>`
- Exact match: `cargo test <name> -- --exact`
- Show test output: `cargo test <name> -- --nocapture`
- List tests: `cargo test -- --list`

Single integration test file (e.g. `crates/asphalt/tests/sync.rs`):

- `cargo test -p asphalt --test sync`
- One test within that file:
  - `cargo test -p asphalt --test sync <name>`
  - Add `-- --exact` / `-- --nocapture` as needed.

### Lint / Format

- Format (write changes): `cargo fmt`
- Format (check only): `cargo fmt -- --check`
- Clippy (fail on warnings): `cargo clippy --all-targets --all-features -- -D warnings`

Note: CI currently runs `cargo test --all` only; fmt/clippy are still expected locally.

## Code Style (Rust)

### Formatting

- Use `cargo fmt` (repo has `rustfmt.toml`).
- Don’t hand-format; let rustfmt win.
- Keep diffs minimal; avoid drive-by reformatting unrelated code.

### Imports

- Prefer explicit imports over globs (`use foo::*`) unless the module is a deliberate prelude.
- Group imports in the conventional rustfmt order (std / external crates / workspace crates / crate-local).
- Don’t alias types/functions unless it materially improves clarity or avoids a conflict.

### Naming

- Types/traits: `PascalCase`.
- Functions/vars/modules: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- CLI args: follow existing `clap` patterns and naming in `crates/truffle/src`.

### Types and API shape

- Prefer small, typed structs/enums over “stringly typed” flags and maps.
- Prefer `Option<T>` over sentinel values.
- Prefer `&str`/`&Path` parameters for read-only inputs; return owned types when the caller should own.
- Keep public APIs narrow; avoid `pub` unless needed across crates.

### Error handling

- This codebase uses `anyhow` widely; prefer `anyhow::Result<T>` for fallible functions.
- Add context at boundaries:
  - `use anyhow::Context;`
  - `some_fallible().with_context(|| format!("..."))?;`
- Use `anyhow::bail!` for early exits with a message.
- Avoid `unwrap()`/`expect()` in production paths; use them only in tests or truly impossible states.

### Async + blocking work

- Tokio is used (`tokio` with `full`). Keep async boundaries consistent.
- Don’t block the async runtime with CPU-heavy or filesystem-heavy loops; follow existing patterns like `tokio::task::spawn_blocking` when necessary.

### Logging

- `asphalt` uses `log` + `env_logger`; prefer `log::{debug, info, warn, error}` macros.
- Avoid `println!` for diagnostics in library-ish code; use structured logging where available.

### Tests

- Prefer small, deterministic tests.
- Integration tests live under `crates/*/tests/` (example: `crates/asphalt/tests/sync.rs`).
- When adding snapshots (if any), follow existing `insta` usage in `asphalt`.

## Repo-specific Notes

- Workspace packages have mixed editions (`2021` and `2024`). Don’t “upgrade” editions unless explicitly requested.
- The release workflow builds `truffle` for multiple targets; keep `truffle`’s CLI behavior stable and backwards compatible.
