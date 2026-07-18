#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -f "$ROOT_DIR/.env" ]]; then
    set -a
    # shellcheck disable=SC1091
    source "$ROOT_DIR/.env"
    set +a
fi

if [[ $# -ne 3 ]]; then
    echo "Usage: $0 <app-bundle> <update-zip> <appcast-output>" >&2
    exit 2
fi

APP_BUNDLE="$1"
UPDATE_ZIP="$2"
APPCAST_OUTPUT="$3"
PLIST_PATH="$APP_BUNDLE/Contents/Info.plist"
SIGN_UPDATE="${SOMETHING_BG_SPARKLE_SIGN_UPDATE:-}"
PRIVATE_KEY="${SOMETHING_BG_SPARKLE_PRIVATE_KEY:-}"
KEY_ACCOUNT="${SOMETHING_BG_SPARKLE_KEY_ACCOUNT:-com.vim-zz.something-bg}"
RELEASE_TAG="${SOMETHING_BG_RELEASE_TAG:-}"
REPOSITORY="${SOMETHING_BG_REPOSITORY:-vim-zz/something_bg}"
FEED_URL="${SOMETHING_BG_SPARKLE_FEED_URL:-https://github.com/$REPOSITORY/releases/latest/download/appcast.xml}"
DOWNLOAD_BASE_URL="${SOMETHING_BG_SPARKLE_DOWNLOAD_BASE_URL:-}"
RELEASE_URL_OVERRIDE="${SOMETHING_BG_SPARKLE_RELEASE_URL:-}"

for path in "$APP_BUNDLE" "$UPDATE_ZIP" "$PLIST_PATH"; do
    if [[ ! -e "$path" ]]; then
        echo "Required release input not found: $path" >&2
        exit 1
    fi
done
if [[ -z "$SIGN_UPDATE" || ! -x "$SIGN_UPDATE" ]]; then
    echo "SOMETHING_BG_SPARKLE_SIGN_UPDATE must name Sparkle's executable sign_update tool." >&2
    exit 1
fi
sign_file() {
    local file_path="$1"
    if [[ -n "$PRIVATE_KEY" ]]; then
        printf '%s' "$PRIVATE_KEY" | "$SIGN_UPDATE" --ed-key-file - "$file_path"
    else
        "$SIGN_UPDATE" --account "$KEY_ACCOUNT" "$file_path"
    fi
}

verify_file() {
    local file_path="$1"
    if [[ -n "$PRIVATE_KEY" ]]; then
        printf '%s' "$PRIVATE_KEY" \
            | "$SIGN_UPDATE" --verify --ed-key-file - "$file_path"
    else
        "$SIGN_UPDATE" --verify --account "$KEY_ACCOUNT" "$file_path"
    fi
}

short_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$PLIST_PATH")"
bundle_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$PLIST_PATH")"
if [[ -z "$RELEASE_TAG" ]]; then
    RELEASE_TAG="v$short_version"
fi
if [[ "$RELEASE_TAG" != "v$short_version" ]]; then
    echo "Release tag $RELEASE_TAG does not match bundle version $short_version." >&2
    exit 1
fi

signature_output="$(sign_file "$UPDATE_ZIP")"
archive_signature="$(printf '%s\n' "$signature_output" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p' | head -n 1)"
archive_length="$(stat -f%z "$UPDATE_ZIP")"
if [[ -z "$archive_signature" ]]; then
    echo "Sparkle sign_update did not return an archive EdDSA signature." >&2
    exit 1
fi

xml_escape() {
    printf '%s' "$1" \
        | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
}

archive_name="$(basename "$UPDATE_ZIP")"
release_url="${RELEASE_URL_OVERRIDE:-https://github.com/$REPOSITORY/releases/tag/$RELEASE_TAG}"
if [[ -z "$DOWNLOAD_BASE_URL" ]]; then
    DOWNLOAD_BASE_URL="https://github.com/$REPOSITORY/releases/download/$RELEASE_TAG"
fi
download_url="${DOWNLOAD_BASE_URL%/}/$archive_name"
pub_date="$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')"
mkdir -p "$(dirname "$APPCAST_OUTPUT")"

cat > "$APPCAST_OUTPUT" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"
  xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle"
  xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>Something in the Background Updates</title>
    <link>$(xml_escape "$FEED_URL")</link>
    <description>Something in the Background macOS updates.</description>
    <language>en</language>
    <item>
      <title>Something in the Background $(xml_escape "$short_version")</title>
      <pubDate>$pub_date</pubDate>
      <link>$(xml_escape "$release_url")</link>
      <description>This update is signed and notarized. See the linked GitHub Release for full release notes.</description>
      <sparkle:version>$(xml_escape "$bundle_version")</sparkle:version>
      <sparkle:shortVersionString>$(xml_escape "$short_version")</sparkle:shortVersionString>
      <enclosure
        url="$(xml_escape "$download_url")"
        sparkle:edSignature="$(xml_escape "$archive_signature")"
        length="$archive_length"
        type="application/octet-stream" />
    </item>
  </channel>
</rss>
EOF

# SURequireSignedFeed is enabled in release bundles, so sign the completed XML
# after all mutable fields have been written.
sign_file "$APPCAST_OUTPUT" >/dev/null

xmllint --noout "$APPCAST_OUTPUT"
if ! grep -q 'sparkle-signatures:' "$APPCAST_OUTPUT"; then
    echo "Signed appcast does not contain Sparkle's embedded signature block." >&2
    exit 1
fi
verify_file "$APPCAST_OUTPUT"

shasum -a 256 "$UPDATE_ZIP" > "$UPDATE_ZIP.sha256"
echo "Sparkle appcast ready: $APPCAST_OUTPUT"
