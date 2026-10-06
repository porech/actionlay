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
cleanup() {
  if mount | grep -Fq " on $MOUNT "; then hdiutil detach "$MOUNT" -quiet || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT
mkdir -p "$OUTPUT" "$WORK/stage" "$MOUNT"
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
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
plutil -lint "$APP/Contents/Info.plist"
# No Developer ID credentials are needed. This signs each universal slice but
# does not claim Apple notarization or avoid Gatekeeper's unknown-publisher prompt.
codesign --force --sign - "$APP/Contents/MacOS/actionlay-telemetry"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
ln -s /Applications "$WORK/stage/Applications"
cp "$ROOT/LICENSE" "$WORK/stage/License.txt"
hdiutil create -quiet -volname "ActionLay" -fs HFS+ -format UDRW -srcfolder "$WORK/stage" "$WORK/writable.dmg"
hdiutil attach -quiet -nobrowse -mountpoint "$MOUNT" "$WORK/writable.dmg"
cp "$ROOT/assets/icons/dmg.icns" "$MOUNT/.VolumeIcon.icns"
SetFile -a C "$MOUNT"
hdiutil detach -quiet "$MOUNT"
hdiutil convert -quiet "$WORK/writable.dmg" -format UDZO -imagekey zlib-level=9 -o "$OUTPUT/actionlay-macos-universal.dmg"
hdiutil verify -quiet "$OUTPUT/actionlay-macos-universal.dmg"
echo "$OUTPUT/actionlay-macos-universal.dmg"
