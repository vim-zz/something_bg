#!/usr/bin/env bash
set -euo pipefail

# Bundle the macOS app with the correct manifest and target dir so bundle metadata
# (icon, identifier, resources) are applied.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

cd "$ROOT_DIR/app-macos"
CARGO_TARGET_DIR="$ROOT_DIR/target" cargo bundle --release
