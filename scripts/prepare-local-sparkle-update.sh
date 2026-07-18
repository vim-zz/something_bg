#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -f "$ROOT_DIR/.env" ]]; then
    set -a
    # shellcheck disable=SC1091
    source "$ROOT_DIR/.env"
    set +a
fi
FIXTURE_DIR="$ROOT_DIR/target/sparkle-dev"
SPARKLE_ROOT="${SOMETHING_BG_SPARKLE_ROOT:-$ROOT_DIR/target/sparkle/2.9.4}"
SPARKLE_FRAMEWORK="${SOMETHING_BG_SPARKLE_FRAMEWORK:-$SPARKLE_ROOT/Sparkle.framework}"
SIGN_UPDATE="${SOMETHING_BG_SPARKLE_SIGN_UPDATE:-$SPARKLE_ROOT/bin/sign_update}"
PUBLIC_ED_KEY="${SOMETHING_BG_SPARKLE_PUBLIC_ED_KEY:-}"
UPDATE_SHORT_VERSION="${SOMETHING_BG_LOCAL_UPDATE_VERSION:-9999.99.0}"
UPDATE_BUILD_VERSION="${SOMETHING_BG_LOCAL_UPDATE_BUILD_VERSION:-99999999.0.0}"
LOCAL_PORT="${SOMETHING_BG_LOCAL_UPDATE_PORT:-18080}"
LOCAL_BASE_URL="http://127.0.0.1:$LOCAL_PORT"

if [[ ! -d "$SPARKLE_FRAMEWORK" ]]; then
    echo "Sparkle.framework not found: $SPARKLE_FRAMEWORK" >&2
    echo "Run scripts/fetch-sparkle.sh or set SOMETHING_BG_SPARKLE_ROOT." >&2
    exit 1
fi
if [[ ! -x "$SIGN_UPDATE" ]]; then
    echo "Sparkle sign_update not found: $SIGN_UPDATE" >&2
    exit 1
fi
if [[ -z "$PUBLIC_ED_KEY" ]]; then
    echo "SOMETHING_BG_SPARKLE_PUBLIC_ED_KEY is required." >&2
    exit 1
fi

SOMETHING_BG_SPARKLE_FRAMEWORK="$SPARKLE_FRAMEWORK" \
SOMETHING_BG_SPARKLE_APPCAST_URL="$LOCAL_BASE_URL/appcast.xml" \
SOMETHING_BG_SPARKLE_PUBLIC_ED_KEY="$PUBLIC_ED_KEY" \
SOMETHING_BG_SPARKLE_ALLOW_INSECURE_UPDATES=1 \
SOMETHING_BG_REQUIRE_SPARKLE=1 \
    "$ROOT_DIR/scripts/bundle-macos.sh"

source_app="$ROOT_DIR/target/release/bundle/osx/Something in the Background.app"
seed_app="$FIXTURE_DIR/seed/Something in the Background.app"
update_app="$FIXTURE_DIR/update/Something in the Background.app"
archive_path="$FIXTURE_DIR/something_bg-macos-local-update.zip"
appcast_path="$FIXTURE_DIR/appcast.xml"

rm -rf "$FIXTURE_DIR"
mkdir -p "$(dirname "$seed_app")" "$(dirname "$update_app")"
/usr/bin/ditto "$source_app" "$seed_app"
/usr/bin/ditto "$source_app" "$update_app"

update_plist="$update_app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $UPDATE_SHORT_VERSION" "$update_plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $UPDATE_BUILD_VERSION" "$update_plist"
marker_path="$update_app/Contents/Resources/local-sparkle-update-marker.txt"
printf 'Something in the Background local Sparkle update %s\n' "$UPDATE_SHORT_VERSION" > "$marker_path"

/usr/bin/ditto -c -k --keepParent "$update_app" "$archive_path"

SOMETHING_BG_RELEASE_TAG="v$UPDATE_SHORT_VERSION" \
SOMETHING_BG_SPARKLE_SIGN_UPDATE="$SIGN_UPDATE" \
SOMETHING_BG_SPARKLE_PRIVATE_KEY="${SOMETHING_BG_SPARKLE_PRIVATE_KEY:-}" \
SOMETHING_BG_SPARKLE_FEED_URL="$LOCAL_BASE_URL/appcast.xml" \
SOMETHING_BG_SPARKLE_DOWNLOAD_BASE_URL="$LOCAL_BASE_URL" \
SOMETHING_BG_SPARKLE_RELEASE_URL="$LOCAL_BASE_URL/release-notes.html" \
    "$ROOT_DIR/scripts/prepare-sparkle-appcast.sh" \
        "$update_app" \
        "$archive_path" \
        "$appcast_path"

cat > "$FIXTURE_DIR/release-notes.html" <<EOF
<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>Local Sparkle update</title></head>
  <body><h1>Local Sparkle update $UPDATE_SHORT_VERSION</h1><p>Replacement fixture generated from the current checkout.</p></body>
</html>
EOF

cat <<EOF
Local Sparkle fixture ready at:
  $FIXTURE_DIR

Serve it without launching an app:
  python3 -m http.server $LOCAL_PORT --bind 127.0.0.1 --directory "$FIXTURE_DIR"

Install the seed bundle manually in /Applications, then launch it and choose
Check for Updates... from the status menu. After accepting the update, verify:
  /Applications/Something in the Background.app/Contents/Resources/local-sparkle-update-marker.txt

This script prepared files only. It did not install or launch the application.
EOF
