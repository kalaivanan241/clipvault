#!/usr/bin/env bash
# Builds a double-clickable ClipVault.app (menu-bar style: no Dock icon).
# Usage: ./scripts/bundle-macos.sh        then: open ClipVault.app
#
# Options (environment variables):
#   SHOW_DOCK=1          keep a Dock icon
#   BIN=path/to/binary   bundle this binary instead of running `cargo build --release`
#                        (CI passes a universal Apple Silicon + Intel binary)
#   VERSION=1.2.3        version shown in Finder (default: the one in Cargo.toml)
#   SIGN_IDENTITY="Developer ID Application: …"
#                        sign for distribution (hardened runtime, needed for
#                        notarization); default "-" signs ad hoc
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${BIN:-}" ]; then
  cargo build --release
  BIN=target/release/clipvault
fi
VERSION=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
SIGN_IDENTITY=${SIGN_IDENTITY:--}

APP=ClipVault.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/ClipVault"

UIELEMENT=true
[ "${SHOW_DOCK:-0}" = "1" ] && UIELEMENT=false

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>ClipVault</string>
  <key>CFBundleDisplayName</key><string>ClipVault</string>
  <key>CFBundleIdentifier</key><string>local.clipvault</string>
  <key>CFBundleExecutable</key><string>ClipVault</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSUIElement</key><$UIELEMENT/>
</dict></plist>
PLIST

if [ "$SIGN_IDENTITY" = "-" ]; then
  # Ad-hoc sign so macOS keeps permissions/launch-at-login stable.
  codesign --force --deep -s - "$APP" || true
else
  codesign --force --deep --options runtime --timestamp -s "$SIGN_IDENTITY" "$APP"
fi
echo "Built $APP $VERSION — run: open $APP"
