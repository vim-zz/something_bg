Project plan: prepare core/platform split (mac-first)

Goal
- Reshape the current macOS-only app into a workspace with a platform-independent core crate plus a macOS shell crate, without yet adding Linux/Windows support.

Milestones
- Workspace scaffold: convert to a Cargo workspace; keep existing bundle metadata intact while introducing `core/` (lib) and `app-macos/` (bin) members.
- Core extraction: move platform-agnostic modules (`config`, `tunnel`, `scheduler`, `wake_detector`, shared state) into `core/`; expose a small API for driving menus/status and tunnel lifecycle.
- Platform interfaces: define traits for `TrayUi`, `Notifier`, `LoggerSink`, `AppPaths`, and `ProcessSpawner` in `core`; keep implementations macOS-only at this stage.
- macOS adapter: re-host current Cocoa/menu/oslog code inside `app-macos/`, implementing the new traits and wiring the event loop to the core state machine.
- Logging and config path cleanup: route logging through the traited sink (oslog impl now, fallback logger ready later); use `directories` crate for config/cache paths with macOS defaults preserved.
    - ✅ Config/scheduler now use injected `AppPaths`; macOS provides `MacPaths` (currently keeps `~/.config/something_bg`).
- Tests and linting: relocate existing tests to `core`; add unit tests for config validation and command construction; ensure `cargo fmt`, `cargo clippy --all-targets --all-features`, and `cargo test -p core -p app-macos` stay green.
- Tooling/CI: update scripts and CI to build/check the workspace members; keep `cargo bundle --release -p app-macos` as the packaging path.
- Docs and migration notes: update README to point to the new workspace layout and add a brief migration note explaining how to run/build after the split.

Out of scope now
- Implementing Linux or Windows trays, installers, or notification backends.
- Packaging changes beyond keeping the existing macOS bundle working.

Acceptance criteria for this phase
- The app still bundles and functions on macOS using the new workspace layout.
- Core logic compiles without macOS-specific bindings and is testable in isolation.
- Menu/tunnel behavior matches current functionality with no user-visible regressions.
