# Repository Guidelines

## Project Structure & Module Organization
- Core entry is `src/main.rs`; feature logic splits into `app.rs` (lifecycle), `menu.rs` (status item UI), `tunnel.rs` (process control), `config.rs` (TOML parsing), and `logger.rs` (macOS logging).
- Assets live in `resources/` plus the top-level `menubar.webp`; place new icons under `resources/images/` and update bundle metadata in `Cargo.toml` when shipping them.
- Keep docs and licensing at the repository root beside `Cargo.toml`; follow this layout for future guides.

## Build, Test, and Development Commands
- `cargo check` — sanity-compile edits without generating binaries.
- `cargo fmt` && `cargo clippy --all-targets --all-features` — enforce formatting and lint expectations before committing.
- `cargo test` — run the full Rust test suite; write new cases inline with `#[cfg(test)]` blocks near the code they cover.
- `cargo run` — launch the debug app for manual menu/tunnel smoke tests.
- `cargo bundle --release` — create the signed macOS bundle in `target/release/bundle/osx/` for distribution.

## Coding Style & Naming Conventions
- Rely on `rustfmt` defaults (4-space indents, trailing commas on wrapping lists); avoid manual formatting tweaks.
- Use snake_case for modules/functions, CamelCase for types, SCREAMING_SNAKE_CASE for constants, and kebab-case for cargo binaries or feature flags.
- Favor `impl` blocks that encapsulate stateful behavior; keep platform-specific Objective-C bridges confined to dedicated modules.
- Emit logs through `log`/`oslog` with concise context fields so Console filtering remains effective.

## Testing Guidelines
- Prefer deterministic tests that validate command construction and state transitions rather than spawning real tunnels.
- Cover edge cases around config loading (`config.rs`) and menu refreshes (`menu.rs`) when modifying those flows.
- Run `cargo test` before every push; document any macOS quirks or manual verification steps in the PR.

## Commit & Pull Request Guidelines
- Match existing history: short, imperative subjects under 72 chars (e.g., “fix workflow release”, “README update”) with optional detail in the body.
- Tie work to issues using `Fixes #123` or reference notes, and flag breaking config changes prominently.
- PRs should summarize behavior changes, attach screenshots for menu updates, list manual smoke checks, and confirm fmt/clippy/test status; squash local fixups before requesting review.

## Configuration & Security Tips
- Do not commit personal `~/.config/something_bg/config.toml`; share sanitized snippets instead.
- Keep secrets and API tokens out of the repo and app bundle; instruct users to supply them via environment variables or secure storage.
