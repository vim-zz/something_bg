# Repository Guidelines

## Project Overview
- Something in the Background is a macOS menu bar application written in Rust that manages SSH tunnels, Kubernetes port forwarding, and other long-running background processes through a simple menu UI.
- App runs with `NSApplicationActivationPolicyAccessory` (menu bar only); tunnels are cleaned up on termination, and status icons reflect active tunnel state.
- PATH is extended to include Homebrew locations for command execution.

## Project Structure & Module Organization
- Core entry is `src/main.rs`; feature logic splits into `app.rs` (lifecycle), `menu.rs` (status item UI), `tunnel.rs` (process control), `config.rs` (TOML parsing), and `logger.rs` (macOS logging).
- Global app state uses `OnceLock<App>`; `StatusItemWrapper` keeps Cocoa objects thread-safe; Objective-C interactions stay within dedicated bridge helpers.
- Assets live in `resources/` plus the top-level `menubar.webp`; place new icons under `resources/images/` and update bundle metadata in `Cargo.toml` when shipping them.
- Keep docs and licensing at the repository root beside `Cargo.toml`; follow this layout for future guides.

## Build, Test, and Development Commands
- `cargo check` — sanity-compile edits without generating binaries.
- `cargo fmt` && `cargo clippy --all-targets --all-features` — enforce formatting and lint expectations before committing.
- `cargo test` — run the full Rust test suite; write new cases inline with `#[cfg(test)]` blocks near the code they cover.
- `cargo run` — launch the debug app for manual menu/tunnel smoke tests.
- `cargo build --release` — build optimized binaries.
- `cargo bundle --release` — create the signed macOS bundle at `target/release/bundle/osx/Something in the Background.app` for distribution.
- `cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/` — install the bundle locally for manual verification.

## Versioning & Releases
- When work warrants a new build (user-visible changes, dependency updates, or release packaging), bump the crate version in `Cargo.toml` and update related artifacts (`CHANGELOG.md`/`RELEASE_NOTES.md`) proactively without waiting for a prompt.
- Follow SemVer, keep release notes concise, and ensure bundle metadata stays in sync with the version.

## Coding Style & Naming Conventions
- Rely on `rustfmt` defaults (4-space indents, trailing commas on wrapping lists); avoid manual formatting tweaks.
- Use snake_case for modules/functions, CamelCase for types, SCREAMING_SNAKE_CASE for constants, and kebab-case for cargo binaries or feature flags.
- Favor `impl` blocks that encapsulate stateful behavior; keep platform-specific Objective-C bridges confined to dedicated modules.
- Emit logs through `log`/`oslog` with concise context fields so Console filtering remains effective.

## Architecture & Design Patterns
- **Core components:** `main.rs` (Cocoa setup, initialization, run loop), `app.rs` (main state with `TunnelManager` and status item wrapper), `config.rs` (TOML config load/manage), `menu.rs` (NSStatusItem + NSMenu creation and icon handling), `tunnel.rs` (SSH/port-forward lifecycle), `logger.rs` (oslog configuration).
- **Key patterns:** thread-safe global state via `OnceLock<App>`, thread-safe Cocoa wrappers, Objective-C bridge class for menu callbacks, async threads for long-running tunnel commands.
- **Dependencies:** cocoa/objc2, core-foundation, log/oslog, libc, toml, serde; patched `objc` fork for compatibility.

## Tunnel Configuration
- Config file lives at `~/.config/something_bg/config.toml`; created with defaults on first run and reloaded for menu construction.
- Each tunnel defines `name`, `command`, `args`, `kill_command`, `kill_args`, optional `separator_after`, optional `group_header`, and optional `group_icon` (SF Symbol name such as `sf:cylinder.fill`).
- Supports any command-line tool that can be started/stopped; config uses `#[serde(default)]` on optional fields for backward compatibility.
- Default seed tunnels: `example-ssh`, `k8s-example`, and `colima`.

## Configuration Management
- If config loading fails, fall back to hardcoded defaults.
- `path` config entry controls PATH used for child processes; defaults include Homebrew locations.
- Escape TOML special characters (e.g., backslashes) with `\\` in configuration strings.

## Menu Organization & Icon System
- Group headers created via `create_header_item()` render disabled menu items with optional SF Symbol icons.
- Separators use `NSMenuItem::separatorItem()` and are driven by `separator_after` in config.
- Icon loader understands `sf:` prefixes and falls back gracefully; PNG icons in `resources/images/` (`peacock_folded_16x16.png` inactive, `peacock_open_16x16.png` active). Falls back to Unicode (☷/☰) if images fail to load.

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

## Release Notes
- In the Release Notes, don't include Upgrade Instructions nor Future Plans.
