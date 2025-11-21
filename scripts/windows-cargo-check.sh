#!/usr/bin/env bash
set -euo pipefail

IMAGE_TAG=something-bg-windows-xwin
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Build image if missing
if ! docker image inspect "$IMAGE_TAG" >/dev/null 2>&1; then
  docker build -f "$REPO_ROOT/scripts/Dockerfile.windows-xwin" -t "$IMAGE_TAG" "$REPO_ROOT"
fi

# Run cargo xwin check for the Windows app by default; allow extra cargo args.
docker run --rm \
  -v "$REPO_ROOT":/workspace \
  -w /workspace \
  "$IMAGE_TAG" \
  cargo xwin check -p something_bg_windows --target x86_64-pc-windows-msvc "$@"
