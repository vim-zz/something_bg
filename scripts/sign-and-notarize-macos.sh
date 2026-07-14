#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <app-path> <output-zip>" >&2
    exit 2
fi

APP_PATH="$1"
OUTPUT_ZIP="$2"

required_variables=(
    APPLE_DEVELOPER_ID_CERTIFICATE_BASE64
    APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD
    APPLE_ID
    APPLE_APP_SPECIFIC_PASSWORD
    APPLE_TEAM_ID
)

for variable in "${required_variables[@]}"; do
    if [[ -z "${!variable:-}" ]]; then
        echo "::error::Missing required release secret: $variable" >&2
        exit 1
    fi
done

if [[ ! -d "$APP_PATH" ]]; then
    echo "::error::App bundle not found at $APP_PATH" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/macos-signing.XXXXXX")"
KEYCHAIN_PATH="$WORK_DIR/release-signing.keychain-db"
CERTIFICATE_PATH="$WORK_DIR/developer-id-application.p12"
NOTARY_ARCHIVE_PATH="$WORK_DIR/notarization.zip"
NOTARY_PROFILE="something-bg-ci-notary"
KEYCHAIN_PASSWORD="$(openssl rand -base64 32)"
ORIGINAL_KEYCHAINS=()
ORIGINAL_DEFAULT_KEYCHAIN="$(
    security default-keychain -d user 2>/dev/null \
        | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' \
        || true
)"

while IFS= read -r keychain; do
    if [[ -n "$keychain" ]]; then
        ORIGINAL_KEYCHAINS+=("$keychain")
    fi
done < <(
    security list-keychains -d user \
        | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' \
        || true
)

cleanup() {
    if [[ -n "$ORIGINAL_DEFAULT_KEYCHAIN" ]]; then
        security default-keychain \
            -d user \
            -s "$ORIGINAL_DEFAULT_KEYCHAIN" >/dev/null 2>&1 || true
    fi
    if [[ ${#ORIGINAL_KEYCHAINS[@]} -gt 0 ]]; then
        security list-keychains \
            -d user \
            -s "${ORIGINAL_KEYCHAINS[@]}" >/dev/null 2>&1 || true
    fi
    security delete-keychain "$KEYCHAIN_PATH" >/dev/null 2>&1 || true
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

printf '%s' "$APPLE_DEVELOPER_ID_CERTIFICATE_BASE64" | base64 --decode > "$CERTIFICATE_PATH"
chmod 600 "$CERTIFICATE_PATH"

security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security list-keychains \
    -d user \
    -s "$KEYCHAIN_PATH" "${ORIGINAL_KEYCHAINS[@]}"
security default-keychain -d user -s "$KEYCHAIN_PATH"
security import "$CERTIFICATE_PATH" \
    -k "$KEYCHAIN_PATH" \
    -f pkcs12 \
    -P "$APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD" \
    -T /usr/bin/codesign
security set-key-partition-list \
    -S apple-tool:,apple: \
    -s \
    -k "$KEYCHAIN_PASSWORD" \
    "$KEYCHAIN_PATH" >/dev/null

SIGNING_IDENTITY="$(
    security find-identity -v -p codesigning "$KEYCHAIN_PATH" \
        | awk '/Developer ID Application/ { print $2; exit }'
)"

if [[ -z "$SIGNING_IDENTITY" ]]; then
    echo "::error::The certificate does not contain a valid Developer ID Application identity" >&2
    security find-identity -v -p codesigning "$KEYCHAIN_PATH" >&2
    exit 1
fi

codesign \
    --force \
    --options runtime \
    --timestamp \
    --keychain "$KEYCHAIN_PATH" \
    --sign "$SIGNING_IDENTITY" \
    "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

ditto -c -k --keepParent "$APP_PATH" "$NOTARY_ARCHIVE_PATH"
xcrun notarytool store-credentials "$NOTARY_PROFILE" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --keychain "$KEYCHAIN_PATH"
xcrun notarytool submit "$NOTARY_ARCHIVE_PATH" \
    --keychain-profile "$NOTARY_PROFILE" \
    --keychain "$KEYCHAIN_PATH" \
    --wait \
    --timeout 30m

xcrun stapler staple "$APP_PATH"
xcrun stapler validate "$APP_PATH"
spctl --assess --type execute --verbose=2 "$APP_PATH"

rm -f "$OUTPUT_ZIP"
ditto -c -k --keepParent "$APP_PATH" "$OUTPUT_ZIP"
