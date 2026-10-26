#!/usr/bin/env bash
# Plan M6 stage 5: the macOS arm64 preview, an ad-hoc signed app in a zip. It is not notarized;
# the DMG, signing and notarization are M7's. The app finds ONNX Runtime next to its executable
# (crates/cloakframe-app/src/tools.rs). Its name and identifier differ from 1.12.1's so that both
# can sit in Applications while the preview is tried.
#
# The preview takes photos only. The FFmpeg builds in manifests/native.toml are GPL and their
# corresponding source cannot be had (the macOS one fetches x264's master when it is built), so
# FFmpeg waits for M7's own build (plan 10.1).
#
# Usage: macos_preview.sh <version> <cloakframe> <libonnxruntime.dylib> <out>
# Writes <out>/CloakFrame-Preview-<version>-macOS-arm64.zip and prints its path.
set -euo pipefail

version=$1
exe=$2
ort=$3
out=$4
root=$(cd "$(dirname "$0")/../.." && pwd)

# CFBundleShortVersionString takes numbers only: 2.0.0-alpha.0 is 2.0.0 there.
short=${version%%-*}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/CloakFrame Preview.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$exe" "$app/Contents/MacOS/cloakframe"
cp "$ort" "$app/Contents/MacOS/libonnxruntime.dylib"
cp "$root/assets/icons/CloakFrame.icns" "$app/Contents/Resources/CloakFrame.icns"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.txt" "$app/Contents/Resources/"

cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>CloakFrame Preview</string>
    <key>CFBundleExecutable</key>
    <string>cloakframe</string>
    <key>CFBundleIconFile</key>
    <string>CloakFrame</string>
    <key>CFBundleIdentifier</key>
    <string>com.cloakframe.app.preview</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleLocalizations</key>
    <array>
        <string>en</string>
        <string>ko</string>
        <string>ja</string>
    </array>
    <key>CFBundleName</key>
    <string>CloakFrame Preview</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$short</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.photography</string>
    <key>LSMinimumSystemVersion</key>
    <string>15.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist" >/dev/null

# Inside out, as codesign wants: the library first, then the app, which seals it.
codesign --force --sign - --timestamp=none "$app/Contents/MacOS/libonnxruntime.dylib"
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --deep "$app"

mkdir -p "$out"
zip="$(cd "$out" && pwd)/CloakFrame-Preview-$version-macOS-arm64.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "$zip"
