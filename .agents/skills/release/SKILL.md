---
name: release
description: Prepare and publish a versioned Something in the Background release, including approved release notes, SemVer updates, validation, GitHub commits and tags, CI monitoring, signed artifacts, and final release verification. Use when Codex is asked to cut, prepare, build, tag, deploy, publish, or push a new release for this repository.
---

# Release

Run this workflow from the repository root. This project publishes releases from
`.github/workflows/ci.yml` when an annotated `v*` tag is pushed.

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
- Treat the approved release text as the source of truth for both the changelog
  and the GitHub Release body. Do not replace it with GitHub-generated notes.

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

Report the commit, tag, workflow result, release URL, exact approved bullets,
and asset verification. Do not claim success while the release remains a draft
or any expected asset/check is missing.
