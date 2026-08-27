#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer creates a macOS .app bundle and must be run on macOS." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

command -v cargo >/dev/null 2>&1 || { echo "cargo is unavailable; enter nix-shell first." >&2; exit 1; }
: "${PDFIUM_LIBRARY_PATH:?Enter the supplied nix-shell first.}"
: "${TESSDATA_PREFIX:?Enter the supplied nix-shell first.}"

cargo build --release

APP_PATH="${1:-$HOME/Applications/kvikk pdf.app}"
CONTENTS="$APP_PATH/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
rm -rf "$APP_PATH"
mkdir -p "$MACOS" "$RESOURCES"
cp target/release/kvikk "$MACOS/kvikk-bin"
chmod +x "$MACOS/kvikk-bin"

# Finder and Spotlight do not inherit a nix-shell. This tiny launcher records
# the Nix-store locations used by this local build.
cat > "$MACOS/kvikk" <<LAUNCHER
#!/bin/sh
export PDFIUM_LIBRARY_PATH='$PDFIUM_LIBRARY_PATH'
export TESSDATA_PREFIX='$TESSDATA_PREFIX'
export DYLD_LIBRARY_PATH='${DYLD_LIBRARY_PATH:-}'
exec "\$(dirname "\$0")/kvikk-bin" "\$@"
LAUNCHER
chmod +x "$MACOS/kvikk"

ICON_PLIST=""
if [[ -f assets/logo.png ]] && command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
  ICONSET="$(mktemp -d)/Kvikk.iconset"
  mkdir -p "$ICONSET"
  sips -z 16 16 assets/logo.png --out "$ICONSET/icon_16x16.png" >/dev/null
  sips -z 32 32 assets/logo.png --out "$ICONSET/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 assets/logo.png --out "$ICONSET/icon_32x32.png" >/dev/null
  sips -z 64 64 assets/logo.png --out "$ICONSET/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 assets/logo.png --out "$ICONSET/icon_128x128.png" >/dev/null
  sips -z 256 256 assets/logo.png --out "$ICONSET/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 assets/logo.png --out "$ICONSET/icon_256x256.png" >/dev/null
  sips -z 512 512 assets/logo.png --out "$ICONSET/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 assets/logo.png --out "$ICONSET/icon_512x512.png" >/dev/null
  sips -z 1024 1024 assets/logo.png --out "$ICONSET/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$ICONSET" -o "$RESOURCES/Kvikk.icns"
  ICON_PLIST=$'    <key>CFBundleIconFile</key>\n    <string>Kvikk</string>'
fi

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleName</key><string>kvikk pdf</string>
    <key>CFBundleDisplayName</key><string>kvikk pdf</string>
    <key>CFBundleExecutable</key><string>kvikk</string>
    <key>CFBundleIdentifier</key><string>no.halvorhansen.kvikk</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.3.0</string>
    <key>CFBundleVersion</key><string>0.3.0</string>
    <key>NSHighResolutionCapable</key><true/>
$ICON_PLIST
</dict></plist>
PLIST

touch "$APP_PATH"
codesign --force --deep --sign - "$APP_PATH" >/dev/null 2>&1 || true
printf '\nInstalled: %s\n' "$APP_PATH"
echo "Launch kvikk pdf from Finder, Spotlight, or the Dock."
open -R "$APP_PATH" || true
