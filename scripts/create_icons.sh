#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ICON_TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$ICON_TEMP_DIR"' EXIT
mkdir "$ICON_TEMP_DIR/AppIcon.iconset"
swift "$ROOT_DIR/scripts/create_app_icon.swift" "$ICON_TEMP_DIR/AppIcon.iconset"
iconutil -c icns "$ICON_TEMP_DIR/AppIcon.iconset" -o "$ROOT_DIR/resources/AppIcon.icns"
if [[ $# -gt 0 ]]; then
    cp "$ICON_TEMP_DIR/AppIcon.iconset/icon_512x512@2x.png" "$1"
fi
echo "Created resources/AppIcon.icns"
