#!/usr/bin/env bash
# Builds a double-clickable ClipVault.app (menu-bar style: no Dock icon).
# Usage: ./scripts/bundle-macos.sh        then: open ClipVault.app
# Set SHOW_DOCK=1 to keep a Dock icon.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
APP=ClipVault.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/clipvault "$APP/Contents/MacOS/ClipVault"

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
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSUIElement</key><$UIELEMENT/>
</dict></plist>
PLIST

# Ad-hoc sign so macOS keeps permissions/launch-at-login stable.
codesign --force --deep -s - "$APP" || true
echo "Built $APP — run: open $APP"
