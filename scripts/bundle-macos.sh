#!/usr/bin/env bash
set -euo pipefail

# Bundle the macOS app and, when available, embed/configure Sparkle. Ordinary
# source builds remain valid without the framework; release builds set
# SOMETHING_BG_REQUIRE_SPARKLE=1 to fail closed.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -f "$ROOT_DIR/.env" ]]; then
    set -a
    # shellcheck disable=SC1091
    source "$ROOT_DIR/.env"
    set +a
fi
DEFAULT_SPARKLE_ROOT="$ROOT_DIR/target/sparkle/2.9.4"
SPARKLE_FRAMEWORK="${SOMETHING_BG_SPARKLE_FRAMEWORK:-$DEFAULT_SPARKLE_ROOT/Sparkle.framework}"
SPARKLE_APPCAST_URL="${SOMETHING_BG_SPARKLE_APPCAST_URL:-https://github.com/vim-zz/something_bg/releases/latest/download/appcast.xml}"
SPARKLE_PUBLIC_ED_KEY="${SOMETHING_BG_SPARKLE_PUBLIC_ED_KEY:-}"
REQUIRE_SPARKLE="${SOMETHING_BG_REQUIRE_SPARKLE:-0}"
ALLOW_INSECURE_UPDATES="${SOMETHING_BG_SPARKLE_ALLOW_INSECURE_UPDATES:-0}"

set_plist_string() {
    local plist_path="$1"
    local key="$2"
    local value="$3"
    if /usr/libexec/PlistBuddy -c "Print :$key" "$plist_path" >/dev/null 2>&1; then
        /usr/libexec/PlistBuddy -c "Set :$key $value" "$plist_path"
    else
        /usr/libexec/PlistBuddy -c "Add :$key string $value" "$plist_path"
    fi
}

set_plist_bool() {
    local plist_path="$1"
    local key="$2"
    local value="$3"
    if /usr/libexec/PlistBuddy -c "Print :$key" "$plist_path" >/dev/null 2>&1; then
        /usr/libexec/PlistBuddy -c "Set :$key $value" "$plist_path"
    else
        /usr/libexec/PlistBuddy -c "Add :$key bool $value" "$plist_path"
    fi
}

set_plist_integer() {
    local plist_path="$1"
    local key="$2"
    local value="$3"
    if /usr/libexec/PlistBuddy -c "Print :$key" "$plist_path" >/dev/null 2>&1; then
        /usr/libexec/PlistBuddy -c "Set :$key $value" "$plist_path"
    else
        /usr/libexec/PlistBuddy -c "Add :$key integer $value" "$plist_path"
    fi
}

bundle_target=""
expect_target_value=0
for argument in "$@"; do
    if [[ "$expect_target_value" == "1" ]]; then
        bundle_target="$argument"
        expect_target_value=0
        continue
    fi
    case "$argument" in
        --target)
            expect_target_value=1
            ;;
        --target=*)
            bundle_target="${argument#--target=}"
            ;;
    esac
done
if [[ "$expect_target_value" == "1" ]]; then
    echo "--target requires a value" >&2
    exit 2
fi

cd "$ROOT_DIR/app-macos"
CARGO_TARGET_DIR="$ROOT_DIR/target" cargo bundle --release "$@"

target_prefix="$ROOT_DIR/target"
if [[ -n "$bundle_target" ]]; then
    target_prefix="$target_prefix/$bundle_target"
fi

app_bundle="$target_prefix/release/bundle/osx/Something in the Background.app"
plist_path="$app_bundle/Contents/Info.plist"
frameworks_dir="$app_bundle/Contents/Frameworks"
bundled_framework="$frameworks_dir/Sparkle.framework"

if [[ ! -d "$app_bundle" || ! -f "$plist_path" ]]; then
    echo "Bundled application was not found at: $app_bundle" >&2
    exit 1
fi

if [[ ! -d "$SPARKLE_FRAMEWORK" ]]; then
    if [[ "$REQUIRE_SPARKLE" == "1" ]]; then
        echo "Required Sparkle.framework was not found at: $SPARKLE_FRAMEWORK" >&2
        echo "Run scripts/fetch-sparkle.sh or set SOMETHING_BG_SPARKLE_FRAMEWORK." >&2
        exit 1
    fi
    echo "Sparkle.framework not found; leaving this development bundle without updates."
    echo "Expected: $SPARKLE_FRAMEWORK"
    exit 0
fi

if [[ -z "$SPARKLE_PUBLIC_ED_KEY" ]]; then
    if [[ "$REQUIRE_SPARKLE" == "1" ]]; then
        echo "SOMETHING_BG_SPARKLE_PUBLIC_ED_KEY is required for a release bundle." >&2
        exit 1
    fi
    echo "Sparkle public EdDSA key is unset; leaving this development bundle without updates."
    exit 0
fi

if [[ "$ALLOW_INSECURE_UPDATES" == "1" ]]; then
    case "$SPARKLE_APPCAST_URL" in
        http://127.0.0.1:*|http://localhost:*)
            ;;
        *)
            echo "Insecure Sparkle updates are allowed only for loopback appcast URLs." >&2
            exit 1
            ;;
    esac
fi

mkdir -p "$frameworks_dir"
rm -rf "$bundled_framework"
/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$bundled_framework"

set_plist_string "$plist_path" "SUFeedURL" "$SPARKLE_APPCAST_URL"
set_plist_string "$plist_path" "SUPublicEDKey" "$SPARKLE_PUBLIC_ED_KEY"
set_plist_bool "$plist_path" "SUEnableAutomaticChecks" "true"
set_plist_integer "$plist_path" "SUScheduledCheckInterval" "3600"
set_plist_bool "$plist_path" "SUAutomaticallyUpdate" "false"
set_plist_bool "$plist_path" "SUVerifyUpdateBeforeExtraction" "true"
set_plist_bool "$plist_path" "SURequireSignedFeed" "true"

if [[ "$ALLOW_INSECURE_UPDATES" == "1" ]]; then
    set_plist_bool "$plist_path" "SUAllowsInsecureUpdates" "true"
else
    /usr/libexec/PlistBuddy -c "Delete :SUAllowsInsecureUpdates" "$plist_path" >/dev/null 2>&1 || true
fi

echo "Bundled Sparkle.framework from: $SPARKLE_FRAMEWORK"
echo "Sparkle appcast: $SPARKLE_APPCAST_URL"
echo "Bundle ready: $app_bundle"
