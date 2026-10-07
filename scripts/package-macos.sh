#!/usr/bin/env bash
# Merge native Intel/ARM builds and package an ad-hoc-signed app in a DMG.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARM="${1:?ARM binary directory required}"
INTEL="${2:?Intel binary directory required}"
OUTPUT="${3:-$ROOT/dist}"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
BUILD="${GITHUB_RUN_NUMBER:-1}"
WORK="$(mktemp -d /tmp/actionlay-dmg.XXXXXX)"
MOUNT="$WORK/mounted"
VERIFY_MOUNT="$WORK/verified"
cleanup() {
  for point in "$MOUNT" "$VERIFY_MOUNT"; do
    hdiutil detach -quiet "$point" >/dev/null 2>&1 || true
  done
  rm -rf "$WORK"
}
trap cleanup EXIT
mkdir -p "$OUTPUT" "$WORK/stage" "$MOUNT" "$VERIFY_MOUNT"
APP="$WORK/stage/ActionLay.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
for binary in actionlay actionlay-telemetry; do
  lipo -create "$ARM/$binary" "$INTEL/$binary" -output "$APP/Contents/MacOS/$binary"
  lipo "$APP/Contents/MacOS/$binary" -verify_arch arm64
  lipo "$APP/Contents/MacOS/$binary" -verify_arch x86_64
  chmod 755 "$APP/Contents/MacOS/$binary"
done
cp "$ROOT/assets/icons/actionlay.icns" "$APP/Contents/Resources/ActionLay.icns"
python3 "$ROOT/scripts/package-release.py" --notices "$APP/Contents/Resources/Licenses"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>actionlay</string>
<key>CFBundleName</key><string>ActionLay</string>
<key>CFBundleDisplayName</key><string>ActionLay</string>
<key>CFBundleIdentifier</key><string>org.ActionLay.ActionLay</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$BUILD</string>
<key>CFBundleIconFile</key><string>ActionLay.icns</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>CFBundleDocumentTypes</key><array><dict>
<key>CFBundleTypeName</key><string>Action camera video</string>
<key>CFBundleTypeRole</key><string>Viewer</string>
<key>LSHandlerRank</key><string>Alternate</string>
<key>LSItemContentTypes</key><array>
<string>public.mpeg-4</string><string>com.apple.quicktime-movie</string>
<string>org.actionlay.lrv-video</string><string>org.actionlay.insv-video</string>
</array></dict></array>
<key>UTImportedTypeDeclarations</key><array>
<dict><key>UTTypeIdentifier</key><string>org.actionlay.lrv-video</string>
<key>UTTypeDescription</key><string>Action camera low-resolution video</string>
<key>UTTypeConformsTo</key><array><string>public.mpeg-4</string></array>
<key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>lrv</string></array></dict></dict>
<dict><key>UTTypeIdentifier</key><string>org.actionlay.insv-video</string>
<key>UTTypeDescription</key><string>Insta360 video</string>
<key>UTTypeConformsTo</key><array><string>public.mpeg-4</string></array>
<key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>insv</string></array></dict></dict>
</array>
</dict></plist>
PLIST
plutil -lint "$APP/Contents/Info.plist"
# No Developer ID credentials are needed. This signs each universal slice but
# does not claim Apple notarization or avoid Gatekeeper's unknown-publisher prompt.
codesign --force --sign - "$APP/Contents/MacOS/actionlay-telemetry"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
ln -s /Applications "$WORK/stage/Applications"
mkdir -p "$WORK/stage/.background"
cp "$ROOT/assets/dmg/background.png" "$WORK/stage/.background/background.png"
hdiutil create -quiet -volname "ActionLay" -fs HFS+ -format UDRW -srcfolder "$WORK/stage" "$WORK/writable.dmg"
hdiutil attach -quiet -nobrowse -mountpoint "$MOUNT" "$WORK/writable.dmg"
cp "$ROOT/assets/icons/dmg.icns" "$MOUNT/.VolumeIcon.icns"
SetFile -a C "$MOUNT"
# Use an isolated build-tool environment; the application has no Python dependency.
python3 -m venv "$WORK/dmg-tools"
"$WORK/dmg-tools/bin/pip" install --quiet -r "$ROOT/scripts/dmg-requirements.txt"
"$WORK/dmg-tools/bin/python" "$ROOT/scripts/configure-dmg.py" "$MOUNT"
hdiutil detach -quiet "$MOUNT"
hdiutil convert -quiet "$WORK/writable.dmg" -format UDZO -imagekey zlib-level=9 -o "$OUTPUT/actionlay-$VERSION-macos-universal.dmg"
hdiutil verify -quiet "$OUTPUT/actionlay-$VERSION-macos-universal.dmg"
# Validate the actual distributed image, mounted at a different path. A valid
# staging alias alone does not prove Finder can resolve it on the user's Mac.
hdiutil attach -quiet -readonly -nobrowse -mountpoint "$VERIFY_MOUNT" "$OUTPUT/actionlay-$VERSION-macos-universal.dmg"
"$WORK/dmg-tools/bin/python" "$ROOT/scripts/verify-dmg-layout.py" "$VERIFY_MOUNT" "$WORK/background.alias"
swift "$ROOT/scripts/verify-dmg-background.swift" "$WORK/background.alias" "$VERIFY_MOUNT/.background/background.png"
hdiutil detach -quiet "$VERIFY_MOUNT"
echo "$OUTPUT/actionlay-$VERSION-macos-universal.dmg"
