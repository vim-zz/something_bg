---
name: build-install
description: Build the macOS app bundle with cargo bundle --release and install it to /Applications. Use when the user wants to build, bundle, or install the app.
disable-model-invocation: true
user-invocable: true
allowed-tools: Bash
---

Build the macOS release bundle and install it to /Applications by running these steps in sequence from the repository root:

1. Kill any running instance: `pkill -x "Something in the Background" || true`
2. Run `./scripts/bundle-macos.sh` — compiles and packages the app into `target/release/bundle/osx/Something in the Background.app`
3. `cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/` — installs the bundle

Stream the output so the user can see build progress. If `bundle-macos.sh` fails, stop and report the error without attempting the install step.
