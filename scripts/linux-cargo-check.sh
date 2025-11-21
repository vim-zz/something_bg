#!/usr/bin/env bash
set -euo pipefail

IMAGE_TAG=something-bg-linux-dev
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Build the image if missing
if ! docker image inspect "$IMAGE_TAG" >/dev/null 2>&1; then
  docker build -f "$REPO_ROOT/scripts/Dockerfile.linux-dev" -t "$IMAGE_TAG" "$REPO_ROOT"
fi

# Run cargo check inside the container
docker run --rm \
  -v "$REPO_ROOT":/workspace \
  -w /workspace \
  "$IMAGE_TAG" \
  cargo check -p something_bg_linux "$@"
