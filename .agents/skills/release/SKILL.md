---
name: release
description: Prepare and publish a versioned Something in the Background release, including approved release notes, SemVer updates, validation, GitHub commits and tags, CI monitoring, signed artifacts, and final release verification. Use when Codex is asked to cut, prepare, build, tag, deploy, publish, or push a new release for this repository.
---

# Release

Run this workflow from the repository root. This project publishes releases from
`.github/workflows/ci.yml` when an annotated `v*` tag is pushed.

## Write user-facing version updates

Treat the release text as the app's “What's new” update for everyday users,
including the copy shown in the updater and on the release page. Use familiar
product language; this is not a developer changelog or a record of the work.
Describe what is new for the user: observable changes, new capabilities, or
problems they will no longer encounter. Explain the result, not the design
principles, implementation instructions, development process, or internal
technical choices used to achieve it. Use descriptive statements about the
product's changed behavior, not instructions telling users what to do. Prefer
“The menu bar tooltip now displays whether any connections are active” over
“Hover over the menu bar icon to check connection status” or “Make connection
status clear and accessible.” Each bullet must identify a concrete, verified
change; omit generic quality claims and do not invent benefits or pad the list
to meet the bullet count. Mention an interaction only when needed to explain
the changed behavior, and phrase it descriptively rather than as a tutorial.

## Mandatory release-note approval gate

On every release request, perform only read-only inspection first. Before any
prepare, tag, build, deploy, or publish action, propose the exact release text
and wait for explicit user approval of its bullets.

1. Inspect the current version, recent commits, tags, worktree status, and the
   complete relevant diff. Infer the SemVer bump from the actual changes; do not
   change files or run commands that can write build metadata yet.
2. Draft a release-text proposal containing exactly 2–5 concise, user-facing
   Markdown bullets. Use the format below; the heading and date are context, but
   the bullets are the exact text requiring approval:

   ```text
   Proposed release text — vX.Y.Z

   ## vX.Y.Z

   - Approved bullet one.
   - Approved bullet two.
   ```

3. Ask explicitly: “Approve these exact 2–5 release-note bullets for vX.Y.Z?”
   Treat approval as applying only to the exact displayed bullets. If the user
   changes, adds, or removes a bullet, show the revised 2–5-bullet proposal and
   ask again.
4. Do not modify `Cargo.toml` or `RELEASE_NOTES.md`, run `cargo fmt`, `cargo
   check`, `cargo test`, `cargo clippy`, build or bundle an artifact, commit,
   create or push a tag, push a branch, invoke a release/deploy command, or
   publish a GitHub release before that exact approval. A general “go ahead” does
   not replace approval of the displayed bullets.

Keep release notes concise. Do not add Upgrade Instructions or Future Plans,
and do not invent filler bullets to reach two. If the changes do not support at
least two meaningful bullets, ask whether to delay the release or approve a
truthful maintenance/packaging bullet.

## Release procedure after approval

Use the approved version and bullets verbatim from this point forward.

### 1. Verify the release target

- Confirm the branch is `main` and preserve unrelated worktree changes. Never
  stage the untracked files or edits belonging to another task.
- Authenticate GitHub operations with the real user home for this repository:
  `HOME=/Users/ofera /opt/homebrew/bin/gh auth status`.
- Fetch the target branch and verify that `HEAD` is based on the latest
  `origin/main`. If the branch is behind, diverged, or has new remote commits,
  stop and resolve that state before preparing the release; never force-push or
  reset destructively.
- Confirm that the approved tag does not already exist locally or on `origin`.
- Re-read `.github/workflows/ci.yml` and `.github/RELEASE_SIGNING.md` if the
  workflow or signing requirements have changed since the last release.

For Git commands that need identity, SSH, or keychain credentials, set
`HOME=/Users/ofera` on that command only; do not change the process-wide home.

### 2. Update version and changelog

- Bump `version` in the root `Cargo.toml` under `[workspace.package]` to the
  approved SemVer. The platform crates inherit this value; do not duplicate it
  in their manifests.
- Run the normal Cargo metadata/check step after the edit so `Cargo.lock` is
  updated if workspace package versions are recorded there. Do not hand-edit
  dependency resolution entries.
- Prepend a new `## vX.Y.Z` section to `RELEASE_NOTES.md`, include the release
  date and a short category heading if useful, and copy the approved bullets
  verbatim. Keep the new section to exactly 2–5 release-note bullets.
- Treat the approved release text as the source of truth for the changelog,
  GitHub Release body, and release notes shown in Sparkle's update window.
  Preserve the approved wording and order across all three; only formatting
  may differ. Do not replace the changes with GitHub-generated notes or generic
  signing/notarization text.

### In-app release notes

Sparkle displays the release item's notes from `appcast.xml`, not the GitHub
Release body. Updating GitHub notes after publication does not update the app's
update window.

- Inspect `scripts/prepare-sparkle-appcast.sh` before tagging. It must extract
  the approved bullets from the exact `## vX.Y.Z` section in `RELEASE_NOTES.md`
  and embed them in that release item's `<description>`. If it still emits the
  hardcoded "This update is signed and notarized" placeholder, fix the generator
  as part of release preparation; a link to GitHub alone is not sufficient.
- Render the bullets as an HTML list inside CDATA, with appropriate escaping
  and Markdown formatting converted to HTML. Preserve the visible wording;
  do not show literal Markdown markers, raw HTML, unrelated version history,
  or technical packaging boilerplate. See Sparkle's
  [embedded release notes documentation](https://sparkle-project.org/documentation/publishing/#embedded-release-notes).
- Fail release preparation if the matching section or approved bullets are
  missing. Validate the generated notes against the approved list before
  signing. Finalize the notes before signing the appcast; later edits require
  regenerating and re-signing the feed, never patching signed XML in place.

### 3. Validate and review

Run the repository checks appropriate to the host, at minimum:

```bash
cargo fmt --all -- --check
cargo clippy -p something_bg_core --all-targets --all-features -- -D warnings
cargo test -p something_bg_core
cargo check -p something_bg
```

Also run the relevant Linux or Windows check when the release changes
cross-platform code. Before committing:

- Run `git diff --check`.
- Inspect `git status -sb` and the complete relevant diff, including the staged
  diff after staging.
- Verify the root Cargo version, lockfile workspace versions, tag version, and
  new release-notes heading all agree.
- Count the bullets in the new release section and confirm they are exactly the
  approved 2–5 bullets; remove any generated or unapproved text.
- Validate the appcast generator's output for the selected release: its rendered
  list must contain those same approved bullets. When changing the generator,
  check missing-version handling and HTML/XML escaping as well as a normal
  release. Do not treat correct GitHub notes as proof of correct in-app notes.

Stage only intentional release files, normally `Cargo.toml`, `Cargo.lock` when
Cargo changed it, and `RELEASE_NOTES.md`. Commit with a short imperative
subject such as `release vX.Y.Z`. Do not include unrelated files. Review the
staged diff before committing:

```bash
git add Cargo.toml RELEASE_NOTES.md
git add Cargo.lock  # only when Cargo changed the tracked lockfile
git diff --cached --check
git diff --cached
HOME=/Users/ofera git commit -m "release vX.Y.Z"
```

### 4. Push the release commit and annotated tag

Push the release commit to `main` first. Then create and push the annotated tag;
the tag is the deployment trigger:

```bash
HOME=/Users/ofera git push origin main
HOME=/Users/ofera git tag -a "vX.Y.Z" -m "Release vX.Y.Z"
HOME=/Users/ofera git push origin "vX.Y.Z"
```

Verify that the remote tag points to the release commit and that the pushed tag
matches the root workspace version. Do not push a tag before the release commit
is on `origin/main`.

### 5. Monitor GitHub Actions

Pushing `vX.Y.Z` starts `ci.yml`. Identify the run for the pushed tag and commit
SHA with the GitHub CLI, then watch it to completion with `--exit-status`:

```bash
HOME=/Users/ofera /opt/homebrew/bin/gh run list --workflow ci.yml --limit 10
HOME=/Users/ofera /opt/homebrew/bin/gh run watch <run-id> --exit-status
```

Select the run whose tag/ref and head SHA match the tag just pushed; do not
watch an older run.
The expected sequence is:

1. `lint-test`
2. Linux and Windows release assets
3. unsigned Apple-Silicon macOS bundle with Sparkle
4. protected macOS signing, notarization, stapling, archive signing, and
   signed appcast generation
5. release asset verification and publication

If a job fails, inspect the failed logs and stop. Do not create another version
tag or force-push to work around a failure. Rerun only a transient failed job;
make a new release commit/tag when the source or release metadata must change.

### 6. Verify the published release

After the workflow completes, verify the GitHub Release for the exact tag:

- `isDraft` is `false`, the tag and title are correct, and the release is not a
  prerelease unless explicitly requested.
- The final body contains the approved release text and exactly the approved
  2–5 bullets. The current workflow uses `--generate-notes` when it first
  creates a release, so explicitly replace generated notes with the approved
  body using `gh release edit --notes-file` if necessary, then re-read and
  verify the body.
- All expected assets are present: `appcast.xml`,
  `something_bg-macos-arm64.zip`, its `.sha256` file,
  `something_bg-linux-x86_64-unknown-linux-gnu.tar.gz`, and
  `something_bg-windows-x86_64-pc-windows-msvc.zip`.
- The macOS artifact is signed, notarized, stapled, and Sparkle metadata points
  to the matching immutable release tag; rely on the protected workflow checks
  and report any missing or mismatched asset.
- Download the published `appcast.xml` and check the release item's description
  against the approved bullets after decoding its HTML. Also verify the feed
  served at the app's configured `SUFeedURL` points to this release and contains
  the same notes. Checking the version, signature, and archive URL alone is
  insufficient: reject placeholder-only or stale release text.
- Verify the notes as users see them in Sparkle's update window when UI access
  is available. If UI verification is unavailable, report that limitation and
  verify the downloaded, decoded notes instead; do not claim visual verification.

Use the CLI for the final inspection and, if needed, the exact approved body:

```bash
HOME=/Users/ofera /opt/homebrew/bin/gh release view "vX.Y.Z" \
  --json url,isDraft,isPrerelease,tagName,name,body,assets
HOME=/Users/ofera /opt/homebrew/bin/gh release edit "vX.Y.Z" \
  --title "vX.Y.Z" --notes-file <approved-release-body.md>
```

The body file must contain only the approved release heading and its exact 2–5
bullets. Re-read the release after editing and verify that no generated notes
or extra bullets remain.

### 7. Verify restart when installing an update locally

When the requested release work includes installing or updating the local app,
finish with the new version running. Publishing a release alone does not require
changing the user's local installation.

- Record the running instance's PID, executable/bundle path, and version before
  updating. Distinguish the installed app from development copies; the displayed
  app name and bundle identifier alone may match multiple builds.
- For an in-app update, let Sparkle finish its installation and relaunch flow.
  Do not manually launch a competing copy while replacement is in progress or
  treat download completion as installation completion.
- For a local bundle replacement, build successfully first, gracefully quit the
  exact instance being updated so tunnel and scheduler cleanup can run, and wait
  for its PID to exit before replacing the bundle. Launch the updated bundle
  explicitly with the user's real HOME and existing configuration afterward.
- Verify the old PID is gone, a new process runs from the intended bundle, and
  the running app's About window reports the expected version. Reading only the
  bundle's on-disk `Info.plist` does not prove that the running process updated.
  Check that the update did not leave an extra stale instance running.
- If replacement, relaunch, or running-version verification fails, report that
  exact limitation; do not claim the local app is updated based on a successful
  copy or published release alone.

Report the commit, tag, workflow result, release URL, exact approved bullets,
asset verification, and in-app release-note verification. When a local update
was requested, also report restart and running-version verification. Do not claim
success while the release remains a draft or any expected asset/check is missing.
