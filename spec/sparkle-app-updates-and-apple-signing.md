# Sparkle App Updates for `something_bg`

## Status

- Phase: Specification
- Scope: macOS app runtime, bundle packaging, and GitHub release workflow
- Target bootstrap release: `v1.11.0`
- Reference implementation reviewed: Flight Recall at the local checkout on July 18, 2026
- Last updated: July 18, 2026

This specification adds Flight Recall-style Sparkle 2 updates to the macOS menu-bar app. It does not authorize implementation, credential changes, or a release.

## Summary

Bundle Sparkle 2 in `Something in the Background.app`, use Sparkle for both update discovery and installation, and publish a Developer ID-signed, notarized, Sparkle-signed update ZIP plus `appcast.xml` in each GitHub Release.

Automatic checks must remain quiet. When Sparkle discovers a newer version in the background, the status menu's normal **Check for Updates...** item changes to **Update Available...**. Sparkle's standard interactive update window opens only after the user chooses that item. This is the menu-bar equivalent of Flight Recall's quiet automatic check and blue Update pill.

The first Sparkle-enabled release remains an Apple Silicon ZIP and uses the existing protected GitHub Actions signing/notarization path. Universal binaries, DMGs, and a new release orchestrator are outside this feature.

## Flight Recall Reference Behavior

The design intentionally copies the following behavior from Flight Recall and adapts only the presentation and hosting layers:

| Concern | Flight Recall behavior | `something_bg` adaptation |
| --- | --- | --- |
| Runtime | Dynamically loads bundled `Sparkle.framework` and retains one `SPUStandardUpdaterController` | Same, implemented inside `app-macos` with its existing `objc2` stack |
| Manual check | App menu calls `checkForUpdates:` | Status menu item calls `checkForUpdates:` |
| Automatic check | Starts Sparkle on launch and calls `checkForUpdateInformation` | Same |
| Background UX | Delegate suppresses scheduled install UI and sets in-memory update-available state | Same state changes the status menu item to **Update Available...** |
| Check schedule | Bundle enables automatic checks with a 3,600-second interval | Same initial interval |
| Feed contract | `SUFeedURL`, `SUPublicEDKey`, appcast item, signed ZIP, and release notes | Same Sparkle contract, hosted in GitHub Releases |
| Development fallback | Missing framework logs a clear error instead of crashing | Same; source builds remain usable without Sparkle |
| Local testing | Older installed bundle updates from a localhost appcast and signed fake-newer ZIP | Same, using a repository script and a simple local HTTP server |
| Release security | App is Developer ID signed, notarized, stapled, then archived and Sparkle-signed | Extend the existing GitHub Actions signing/notarization job in that order |
| Release orchestration | Deterministic local `rust-script` controller deploys GitHub and a public site | Keep `something_bg`'s existing tag-triggered GitHub Actions workflow |

Reference files reviewed:

- `/Users/ofera/Developer/1000Ants/flight_recall/crates/fltrcl-desktop/src/desktop/sparkle_update.rs`
- `/Users/ofera/Developer/1000Ants/flight_recall/scripts/bundle-macos-desktop.sh`
- `/Users/ofera/Developer/1000Ants/flight_recall/scripts/prepare-local-sparkle-update.sh`
- `/Users/ofera/Developer/1000Ants/flight_recall/scripts/prepare-sparkle-release.sh`
- `/Users/ofera/Developer/1000Ants/flight_recall/scripts/publish-local-release.sh`
- `/Users/ofera/Developer/1000Ants/flight_recall/scripts/release.rs`
- `/Users/ofera/Developer/1000Ants/flight_recall/site/public/appcast.xml`

## Goals

- Let a macOS user discover, download, install, and relaunch into a newer version without manually replacing the app bundle.
- Provide an explicit **Check for Updates...** status-menu action backed by Sparkle's standard UI.
- Check quietly at launch and on Sparkle's schedule without opening an unsolicited update window.
- Surface a discovered background update inside the existing status menu.
- Preserve tunnel and scheduled-task cleanup when Sparkle terminates the app for installation.
- Publish only Developer ID-signed, notarized, stapled, and Sparkle EdDSA-signed updates.
- Keep source-built apps usable when Sparkle is not present.
- Keep Linux and Windows code, artifacts, and release behavior unchanged.
- Fail the macOS release before publication when framework, signing, notarization, appcast, version, or archive validation fails.

## Non-goals

- A custom downloader, installer, release JSON poller, or skip-version database.
- Automatic installation without a user accepting Sparkle's standard update flow.
- A custom release-notes window.
- Mac App Store distribution or App Sandbox adoption.
- DMG distribution.
- Intel or universal2 packaging; the existing arm64 release target remains unchanged.
- Delta updates in the first two Sparkle-enabled releases.
- Replacing the existing GitHub Actions release workflow with Flight Recall's local release controller.
- Changing Linux or Windows update behavior.

## Current State

- Workspace version is `1.10.1`; tag `v1.10.1` is the current signed release.
- `app-macos` has no updater module or Sparkle dependency.
- `app-macos/src/menu.rs` ends with **About**, a separator, and **Quit Something in the Background**.
- `app-macos/src/main.rs` installs one termination observer; its handler calls `App::cleanup_tunnels()`, which also shuts down the scheduler.
- `scripts/bundle-macos.sh` delegates directly to `cargo bundle` and does not embed a framework or write Sparkle keys.
- `.github/workflows/ci.yml` builds an arm64 app, signs and notarizes it in a protected `release` environment, and publishes `something_bg-macos-arm64.zip`.
- `scripts/sign-and-notarize-macos.sh` signs only the outer app today. Sparkle will add nested executable code that must also be signed and verified.
- Existing installed versions cannot update themselves because they do not contain Sparkle. The bootstrap release must be installed manually.

## User Stories and Acceptance Scenarios

### Story 1: User checks and installs an update (P1)

As a macOS user, I can choose **Check for Updates...**, review Sparkle's standard update UI, install an available version, and return to the relaunched app.

1. Given a bundled app with a reachable valid appcast, when the user chooses **Check for Updates...**, then Sparkle presents its normal checking result.
2. Given a newer valid release, when the user accepts it, then Sparkle downloads the archive, validates it, replaces the installed app, and relaunches the menu-bar app.
3. Given no newer release, when the user checks manually, then Sparkle reports that the app is current.
4. Given an unreachable or invalid feed, when the user checks manually, then Sparkle reports the failure without terminating the app.

### Story 2: Automatic discovery is quiet (P1)

As a user, I can learn that an update exists without an update window interrupting startup or background work.

1. Given a newer appcast item, when the launch-time information check completes, then no Sparkle install window opens automatically.
2. Given a newer appcast item, when the status menu is next opened, then **Update Available...** appears where **Check for Updates...** normally appears.
3. Given the user selects **Update Available...**, then Sparkle begins a user-initiated check and may show its standard install UI.
4. Given no update or a failed automatic check, then the menu remains **Check for Updates...** and no false availability state is shown.
5. Given Sparkle later performs a scheduled check, then it follows the same quiet behavior.

### Story 3: Development builds degrade safely (P2)

As a contributor, I can build and run the app from source without installing Sparkle or release credentials.

1. Given `cargo run -p something_bg` or a bundle without `Sparkle.framework`, when updater initialization runs, then the app logs an actionable unavailability message and continues running.
2. Given Sparkle did not initialize, when the status menu opens, then **Check for Updates...** is disabled.
3. Given configuration reload rebuilds the menu, then the app does not create a second updater controller or scheduled checker.

### Story 4: Maintainer publishes a secure update (P1)

As a maintainer, I can push a version tag and receive one release whose macOS app and Sparkle metadata have passed all signing and update-integrity gates.

1. Given valid protected release credentials and a tag matching the Cargo version, when the release workflow runs, then it embeds Sparkle, signs nested code and the app, notarizes and staples the app, creates the ZIP, signs the ZIP for Sparkle, and publishes a matching appcast.
2. Given a required credential, framework, key, architecture slice, signature, notarization result, or appcast field is missing or invalid, then the release remains unpublished.
3. Given Linux and Windows release jobs run for the same tag, then they require no Sparkle or Apple secrets and keep their existing artifact formats.

## Runtime Design

### Module and ownership

Add `app-macos/src/updater.rs`. The module owns one updater runtime for the process lifetime and exposes a small Rust API:

```rust
pub fn start_automatic_checks() -> Result<(), String>;
pub fn check_for_updates() -> Result<(), String>;
pub fn is_available() -> bool;
pub fn automatic_update_available() -> bool;
```

The exact storage type is an implementation detail, but it must retain the controller and delegate for the entire app lifetime and prevent duplicate initialization. `OnceLock`, main-thread-owned retained Objective-C objects, or a narrowly synchronized singleton are acceptable.

The module must not be added to `something_bg_core`. Updating is macOS shell behavior.

### Framework loading and controller creation

- Load `Contents/Frameworks/Sparkle.framework/Sparkle` dynamically when `SPUStandardUpdaterController` is not already registered.
- Resolve Sparkle classes only after `NSApplication::sharedApplication` is created and while running on the main thread.
- Create one `SPUStandardUpdaterController` with the updater started.
- Use Sparkle's standard user driver for manual update UI.
- Return an actionable error if the bundle, framework binary, class, delegate, controller, or updater cannot be initialized.
- Never crash merely because a development build lacks Sparkle.

The bridge should use the repository's current `objc2` dependencies. Adding the older `objc` crate solely to copy Flight Recall's implementation is not required.

### Delegate behavior

The Objective-C delegate must implement the Sparkle callbacks needed to distinguish automatic discovery from user-initiated presentation:

- A valid update found by an information/scheduled check sets an in-memory atomic update-available signal.
- A no-update result clears the signal.
- Scheduled update presentation is declined so Sparkle does not open an install window by itself.
- Gentle scheduled reminders are supported if required by the selected Sparkle API contract.
- A user-initiated `checkForUpdates:` retains Sparkle's normal visible behavior.

Sparkle remains authoritative for version comparison, skipped versions, archive validation, download, replacement, and relaunch. The app must not duplicate that state.

### Startup integration

In `app-macos/src/main.rs`:

1. Create `NSApplication` and set accessory activation policy.
2. Create the handler, app state, and status item as today.
3. Store `GLOBAL_APP` and install the existing notification and termination observers.
4. Call `updater::start_automatic_checks()` once before `app.run()`.
5. Log initialization/check failures and continue startup.

Starting the updater must not block the main thread on a network request.

### Menu integration

In `app-macos/src/menu.rs`:

- Add one updater menu item after **About** and before the separator preceding Quit.
- Add a stable menu-item tag and a `checkForUpdates:` handler selector.
- The handler calls `updater::check_for_updates()` and logs a failure rather than panicking.
- `menuNeedsUpdate:` refreshes the update item along with scheduled-task and configuration state:
  - unavailable runtime: title **Check for Updates...**, disabled;
  - available, no discovered update: title **Check for Updates...**, enabled;
  - available update: title **Update Available...**, enabled.
- Configuration reload may rebuild the menu but must reuse the singleton updater.
- Update state is in-memory for the current process. Sparkle owns persistent preferences such as skipped versions and last-check time.

The menu-bar icon itself must not change for update availability because its empty/filled state already communicates tunnel activity.

### Termination and relaunch

- Preserve `NSApplicationWillTerminateNotification` as the single cleanup path for both Quit and Sparkle-driven termination.
- Confirm `App::cleanup_tunnels()` stops active tunnels and shuts down the scheduler before the process exits.
- Do not add a Sparkle-specific fast-exit path that bypasses cleanup.
- A successful update must relaunch as an accessory/menu-bar application with exactly one status item and no stale previous process.

## Bundle and Feed Contract

### Sparkle framework

- Pin one tested Sparkle 2 release and its SHA-256 checksum in repository-controlled release tooling. The implementation PR must record the selected version; do not fetch `latest`.
- Fetch only the official Sparkle release archive or use a pre-provisioned verified framework path.
- Copy `Sparkle.framework` into `Contents/Frameworks` while preserving symlinks, permissions, and extended attributes.
- Keep framework acquisition out of ordinary Cargo compilation. `cargo check`, core tests, and source runs must not require network access or Sparkle.
- `scripts/bundle-macos.sh` must accept explicit feed URL, public key, framework path, and local-insecure-test overrides through `SOMETHING_BG_*` environment variables.

### Required Info.plist keys

The packaged release must contain:

| Key | Production value |
| --- | --- |
| `SUFeedURL` | `https://github.com/vim-zz/something_bg/releases/latest/download/appcast.xml` |
| `SUPublicEDKey` | Repository/release configuration containing only the Sparkle public EdDSA key |
| `SUEnableAutomaticChecks` | `true` |
| `SUScheduledCheckInterval` | `3600` |
| `SUAutomaticallyUpdate` | `false` |
| `SUVerifyUpdateBeforeExtraction` | `true` |
| `SURequireSignedFeed` | `true` |

`SUAllowsInsecureUpdates=true` is permitted only in a locally generated test bundle whose feed is loopback HTTP. It must never be present in a production artifact.

Do not enable anonymous system profiling.

### Version identity

For every release:

- Git tag must equal `v` plus `[workspace.package].version`.
- `CFBundleShortVersionString` must equal the workspace version.
- `CFBundleVersion` must be a valid monotonically increasing Sparkle build version. For ordinary stable SemVer releases, the same numeric dotted version may be used.
- The appcast item's `sparkle:shortVersionString` and `sparkle:version` must equal the corresponding bundle fields.
- The appcast enclosure and GitHub Release must identify the same tag/version.
- Prerelease versions are rejected until a numeric build-version mapping is specified.

### Appcast item

Each published `appcast.xml` must be EdDSA signed and contain one current release item with:

- human-readable title and publication date;
- signed, embedded release notes content plus a link to the matching GitHub Release page;
- versioned enclosure URL, not a mutable `latest` URL;
- `sparkle:version`;
- `sparkle:shortVersionString`;
- EdDSA signature produced by Sparkle's pinned signing tool;
- exact archive byte length;
- appropriate archive MIME type.

Example shape, with placeholders:

```xml
<item>
  <title>Something in the Background 1.11.0</title>
  <pubDate>...</pubDate>
  <sparkle:releaseNotesLink>https://github.com/vim-zz/something_bg/releases/tag/v1.11.0</sparkle:releaseNotesLink>
  <enclosure
    url="https://github.com/vim-zz/something_bg/releases/download/v1.11.0/something_bg-macos-arm64.zip"
    sparkle:version="1.11.0"
    sparkle:shortVersionString="1.11.0"
    sparkle:edSignature="..."
    length="..."
    type="application/octet-stream" />
</item>
```

The stable feed is the `appcast.xml` asset attached to the latest published GitHub Release. The enclosure URL is immutable for that version.

## Signing, Notarization, and Publication

### Existing security boundary

Retain the current split in `.github/workflows/ci.yml`:

- lint/test and unsigned build jobs receive no release credentials;
- the protected `release` environment owns Apple and Sparkle signing secrets;
- Linux and Windows jobs receive neither Apple nor Sparkle secrets;
- reusable actions remain pinned to immutable commits.

Add a Sparkle private-key secret to the protected release environment. Only the public key is embedded in the app. Never commit, echo, archive, or upload the private key.

### Required macOS release order

1. Validate tag and Cargo version equality.
2. Fetch the pinned Sparkle artifact and verify its checksum.
3. Build the arm64 app and embed the framework and production Info.plist keys.
4. Import the Developer ID identity into the ephemeral CI keychain.
5. Sign Sparkle's nested executable components in the order required by the pinned Sparkle distribution.
6. Sign the outer app with hardened runtime and secure timestamp.
7. Verify every nested signature and the outer app under strict verification.
8. Submit the app archive to Apple notarization, wait for acceptance, staple the app, and validate the ticket.
9. Run Gatekeeper assessment against the stapled app.
10. Create `something_bg-macos-arm64.zip` with `ditto --keepParent`.
11. Sign that exact final ZIP with Sparkle's signing tool and capture signature, size, and checksum.
12. Generate and validate `appcast.xml` from those exact values.
13. Create or update a draft GitHub Release and upload Linux, Windows, macOS ZIP, and appcast assets.
14. Verify draft asset names and sizes, then publish the release once all jobs succeed.
15. Clean up the temporary certificate, Sparkle key material, and ephemeral keychain in an always-run cleanup step.

The workflow must never publish `appcast.xml` before its enclosure is present in the same release. Publication must be repeatable for the same tag without creating duplicate releases or contradictory assets.

### Signing implementation change

`scripts/sign-and-notarize-macos.sh` currently signs only the outer app. It must be extended or split so nested Sparkle code is signed explicitly before the outer bundle. Release verification may use `codesign --deep`, but signing must not rely on a single outer-bundle command to discover all nested code.

The existing Apple certificate, hardened runtime, ephemeral keychain, notarization, stapling, `spctl`, and cleanup behavior must be preserved.

## Local End-to-End Test Fixture

Add `scripts/prepare-local-sparkle-update.sh`, modeled on Flight Recall's fixture. It must:

1. Require an explicit local Sparkle framework, `sign_update` tool, public key, and access to the matching private key.
2. Build a seed bundle using the real current version and a loopback appcast URL.
3. Build a second bundle with a clearly newer test version such as `9999.99.0` without changing tracked source files.
4. Add a harmless marker to the update bundle so replacement can be proven.
5. Archive and EdDSA-sign the update bundle.
6. Write local release notes and `appcast.xml` under an ignored `target/sparkle-dev/` directory.
7. Print commands to serve that directory on loopback, install the seed app in `/Applications`, launch it, and verify the marker after update.

The full replacement test cannot be considered covered by `cargo run`; it requires a bundled app with `Sparkle.framework`, an HTTP-served appcast, a signed update archive, and an installed app location Sparkle can replace.

Local test bundles may be ad hoc or locally Developer ID signed as appropriate, but production signing/notarization claims apply only to release artifacts.

## Functional Requirements

- **FR-001**: The macOS status menu MUST contain one Sparkle-backed manual update action.
- **FR-002**: The app MUST initialize at most one updater controller on the main thread.
- **FR-003**: Automatic checks MUST start on launch and use a 3,600-second Sparkle schedule in production bundles.
- **FR-004**: Automatic discovery MUST NOT present an install window without a subsequent user action.
- **FR-005**: A discovered update MUST change the update menu item to **Update Available...**.
- **FR-006**: Manual checks MUST use Sparkle's standard UI and installer.
- **FR-007**: Sparkle MUST remain authoritative for version comparison, skipped versions, validation, installation, and relaunch.
- **FR-008**: Missing or failed Sparkle initialization MUST NOT crash or prevent normal app startup.
- **FR-009**: Source builds MUST NOT require Sparkle, signing secrets, or network framework acquisition.
- **FR-010**: Configuration reload MUST NOT create an additional updater controller.
- **FR-011**: Sparkle-driven termination MUST use the existing tunnel and scheduler cleanup path.
- **FR-012**: Production app bundles MUST contain the configured feed URL, public key, automatic-check setting, and interval.
- **FR-013**: Production updates MUST be Developer ID signed, notarized, stapled, and Sparkle EdDSA signed before publication.
- **FR-014**: The release workflow MUST validate tag, Cargo, bundle, and appcast version consistency.
- **FR-015**: The stable appcast MUST reference an immutable versioned archive with matching signature and byte length.
- **FR-016**: The release MUST remain unpublished if any required macOS validation fails.
- **FR-017**: Apple and Sparkle private credentials MUST remain limited to the protected release environment and ephemeral storage.
- **FR-018**: Linux and Windows behavior and artifacts MUST remain unchanged.
- **FR-019**: The repository MUST provide a localhost end-to-end replacement fixture.
- **FR-020**: README and release-signing documentation MUST describe bootstrap installation, manual checks, quiet automatic discovery, local testing, and release prerequisites.

## Verification Gates

### Automated

- `cargo fmt --all -- --check`
- `cargo clippy -p something_bg_core --all-targets --all-features -- -D warnings`
- `cargo test -p something_bg_core`
- `cargo check -p something_bg`
- Focused updater/menu tests for unavailable, available/no-update, and update-available states
- Tag/version and bundle/appcast version equality
- Pinned Sparkle archive checksum
- `lipo -archs` reports `arm64` for the current release target
- Required Info.plist values are present; insecure-update allowance is absent
- Every nested and outer code signature validates
- Hardened runtime and secure timestamp are present
- Notarization is accepted and stapled
- Gatekeeper accepts the app
- Sparkle signature validation succeeds for the final ZIP
- Appcast is valid XML and includes the exact archive size and versioned URL
- Draft GitHub Release contains all expected assets before publication

### Manual

- A browser-downloaded bootstrap ZIP launches without quarantine workarounds.
- **Check for Updates...** shows the correct Sparkle response for current, newer, and unreachable feeds.
- A launch/background check against a newer feed does not open a window.
- Opening the menu after background discovery shows **Update Available...**.
- Clicking **Update Available...** opens Sparkle's normal interactive flow.
- An installed `v1.11.0` bundle updates to a test `v1.11.1`, terminates tunnels/scheduler, replaces the app, and relaunches exactly once.
- Config, command history, and other user data remain unchanged.
- Config reload does not duplicate the updater or menu action.
- A bundle without Sparkle runs normally with the update action disabled.
- Existing tunnel, command, notification, schedule, wake, About, and Quit behavior still works.

## Rollout

### `v1.11.0`: bootstrap

- Add the runtime, menu item, framework bundle metadata, signed appcast generation, local fixture, and release gates.
- Existing `v1.10.1` users install this release manually because their app has no updater.
- Confirm the release is signed, notarized, stapled, and contains the correct public key and feed URL.

### `v1.11.1`: update-chain proof

- Publish a small, low-risk change through the same pipeline.
- Validate quiet discovery, menu indication, interactive acceptance, download, verification, cleanup, replacement, and relaunch from `v1.11.0`.
- Do not rotate the Apple certificate or Sparkle key during this proof release.

### Later

- Consider delta updates only after at least one full update chain has succeeded in production.
- Consider user-facing update preferences only if Sparkle's standard preferences and menu behavior prove insufficient.

## Risks and Mitigations

- **Bootstrap gap**: old apps cannot self-update. Document the one-time manual `v1.11.0` installation.
- **Nested signing failure**: sign Sparkle components explicitly and verify every code object before notarization.
- **Broken feed publication**: upload and verify the immutable enclosure before publishing the release/appcast.
- **Key loss or compromise**: keep protected backups and follow Sparkle's key-rotation bridge procedure; never rotate Apple and Sparkle keys in the same release.
- **Accessory-app UI focus**: verify Sparkle's standard window appears in front when opened from the menu-bar-only app.
- **Cleanup race**: retain the existing application termination notification and test active tunnels during replacement.
- **False update indicator**: clear the in-memory signal on no-update and never set it on check failure.
- **CI secret exposure**: restrict credentials to the protected environment and delete temporary key material in unconditional cleanup.

## Definition of Done

- A production bundle contains Sparkle and the required secure feed metadata.
- Manual checks use Sparkle's standard UI.
- Automatic checks are quiet and update the status-menu label when a newer version exists.
- A signed older app installs and relaunches a signed newer app from the GitHub-hosted appcast.
- Active tunnels and scheduled work are cleaned up before replacement.
- Missing Sparkle does not crash a development build.
- The release artifact passes Developer ID, hardened runtime, notarization, stapling, Gatekeeper, Sparkle signature, appcast, and version checks.
- The first full update-chain test from `v1.11.0` to `v1.11.1` succeeds.
- Linux and Windows release behavior remains unchanged.
