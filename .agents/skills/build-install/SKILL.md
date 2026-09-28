---
name: build-install
description: Build and run the macOS app locally by default. Install to /Applications and restart the installed app only when the user explicitly approves that installation. Use when the user wants to build, bundle, demo, or install the app.
disable-model-invocation: true
user-invocable: true
allowed-tools: Bash
---

Build and run locally by default. A request to build, run, demo, test, commit, push, or publish a release does not authorize replacing or modifying the installed app. Do not quit the installed app, move its bundle, or replace even part of its contents unless the user explicitly approves that installation. Follow the installed-app protection rules in `AGENTS.md`.

## Default: local build or demo

1. Build with `./scripts/bundle-macos.sh` and run the resulting development bundle directly from `target/release/bundle/osx/Something in the Background.app` when a launch was requested.
2. Keep development and installed instances separate. Before restarting a preview, identify it by its exact executable path; never stop the installed app as part of local testing. Build a replacement successfully before stopping the previous preview, using a separate output path if needed to avoid overwriting a running bundle.
3. Verify the development process and requested preview state, and report the development bundle path. Do not copy anything into `/Applications` or claim a local preview is an installed update.

## Installation: only after explicit approval

Proceed with these steps only when the user has explicitly approved installing the local build over the installed app. If installation has not been approved, complete the local build/demo above; do not ask for installation permission merely to perform a demo.

Follow these steps in sequence from the repository root:

1. Record the expected version from `Cargo.toml` and identify the running app's PID, executable/bundle path, and running version when available. Distinguish installed and development copies; the executable is `something_bg`, which differs from the displayed app name. Do not use a broad process-name kill.
2. Run `./scripts/bundle-macos.sh` — compiles and packages the app into `target/release/bundle/osx/Something in the Background.app`. Build successfully before stopping the existing app.
3. Gracefully quit the exact running instance being replaced through its native Quit action or application termination API, allowing tunnel and scheduler cleanup. Wait for its recorded PID to exit before replacing files. If it does not exit, report the failure rather than silently force-killing it or copying over a running bundle.
4. Replace `/Applications/Something in the Background.app` with the newly built bundle. Preserve a recoverable copy of the previous bundle until installation succeeds; avoid merging into a stale bundle. If installation fails, report the error and do not launch a development copy as if installation succeeded.
5. Launch the installed bundle explicitly: `/usr/bin/open --env HOME=/Users/ofera "/Applications/Something in the Background.app"`. Use the user's real HOME so the restarted app retains its existing configuration.
6. Verify that the old PID is gone, a new process runs from the installed bundle, and no stale duplicate instance remains. Open the running app's About window and confirm its version matches the expected version. Reading only the on-disk `Info.plist` is insufficient to verify the running version. If UI verification is unavailable, report which process checks passed and that the running version remains unconfirmed.

Stream the output so the user can see build progress. If `bundle-macos.sh` fails, stop and report the error without stopping the app or attempting installation. Report installation, restart, and running-version verification separately; a successful copy alone is not a completed update.

For updates installed through the app's native Sparkle window, let Sparkle complete installation and relaunch before applying the process and running-version checks above. Do not launch a competing copy during Sparkle's update. Publishing a release alone does not require updating the local installation.
