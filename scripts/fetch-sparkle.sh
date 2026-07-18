#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SPARKLE_VERSION="2.9.4"
SPARKLE_ARCHIVE_SHA256="ce89daf967db1e1893ed3ebd67575ed82d3902563e3191ca92aaec9164fbdef9"
SPARKLE_ARCHIVE_URL="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
DESTINATION="${1:-$ROOT_DIR/target/sparkle/$SPARKLE_VERSION}"

if [[ -d "$DESTINATION/Sparkle.framework" && -x "$DESTINATION/bin/sign_update" ]]; then
    echo "$DESTINATION"
    exit 0
fi

for command_name in curl shasum tar; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Required command not found: $command_name" >&2
        exit 1
    fi
done

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/something-bg-sparkle.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT
archive_path="$temporary_dir/Sparkle-${SPARKLE_VERSION}.tar.xz"
extract_path="$temporary_dir/extracted"

curl --fail --location --silent --show-error "$SPARKLE_ARCHIVE_URL" --output "$archive_path"
actual_sha256="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
if [[ "$actual_sha256" != "$SPARKLE_ARCHIVE_SHA256" ]]; then
    echo "Sparkle archive checksum mismatch." >&2
    echo "Expected: $SPARKLE_ARCHIVE_SHA256" >&2
    echo "Actual:   $actual_sha256" >&2
    exit 1
fi

mkdir -p "$extract_path" "$DESTINATION"
tar -xJf "$archive_path" -C "$extract_path"
rm -rf "$DESTINATION/Sparkle.framework" "$DESTINATION/bin"
/usr/bin/ditto "$extract_path/Sparkle.framework" "$DESTINATION/Sparkle.framework"
mkdir -p "$DESTINATION/bin"
/usr/bin/ditto "$extract_path/bin/sign_update" "$DESTINATION/bin/sign_update"
/usr/bin/ditto "$extract_path/bin/generate_keys" "$DESTINATION/bin/generate_keys"

echo "$DESTINATION"
