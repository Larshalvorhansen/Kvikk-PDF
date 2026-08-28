#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer creates a macOS .app bundle and must be run on macOS." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${KVIKK_VERSION:-$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)}"

command -v cargo >/dev/null 2>&1 || { echo "cargo is unavailable. Install Rust with rustup, or enter nix-shell." >&2; exit 1; }
command -v otool >/dev/null 2>&1 || { echo "otool is unavailable; install the macOS command-line tools." >&2; exit 1; }
command -v install_name_tool >/dev/null 2>&1 || { echo "install_name_tool is unavailable; install the macOS command-line tools." >&2; exit 1; }
: "${PDFIUM_LIBRARY_PATH:?Set PDFIUM_LIBRARY_PATH, or enter the supplied nix-shell. For a no-Nix build use scripts/package-macos-release.sh.}"
: "${TESSDATA_PREFIX:?Set TESSDATA_PREFIX, or enter the supplied nix-shell. For a no-Nix build use scripts/package-macos-release.sh.}"

cargo build --release

APP_PATH="${1:-/Applications/kvikk pdf.app}"
APP_PARENT="$(dirname "$APP_PATH")"
if [[ ! -d "$APP_PARENT" ]]; then
  mkdir -p "$APP_PARENT"
fi
if [[ ! -w "$APP_PARENT" ]]; then
  echo "Cannot write to $APP_PARENT. Re-run with appropriate macOS permissions or pass a writable .app destination." >&2
  exit 1
fi

CONTENTS="$APP_PATH/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"
rm -rf "$APP_PATH"
mkdir -p "$MACOS" "$RESOURCES/tessdata" "$FRAMEWORKS"
cp assets/Credits.html "$RESOURCES/Credits.html"

# Use the actual Mach-O executable as CFBundleExecutable. The older local
# installer used a shell launcher to recreate the Nix environment, which made
# Launch Services document delivery unnecessarily fragile. Instead, make the
# local .app self-contained just like a release build.
cp target/release/kvikk "$MACOS/kvikk"
chmod +x "$MACOS/kvikk"
cp -L "$PDFIUM_LIBRARY_PATH" "$MACOS/libpdfium.dylib"

for lang in eng nor; do
  if [[ ! -f "$TESSDATA_PREFIX/$lang.traineddata" ]]; then
    echo "Missing OCR language data: $TESSDATA_PREFIX/$lang.traineddata" >&2
    exit 1
  fi
  cp "$TESSDATA_PREFIX/$lang.traineddata" "$RESOURCES/tessdata/"
done

# Bundle non-system dylibs (Tesseract, Leptonica, and their dependencies) and
# rewrite their install names so the app no longer relies on Nix store paths.
copy_deps() {
  local binary="$1"
  while IFS= read -r dep; do
    [[ "$dep" == /System/* || "$dep" == /usr/lib/* || "$dep" == @* ]] && continue
    [[ -f "$dep" ]] || continue
    local base="$(basename "$dep")"
    local dest="$FRAMEWORKS/$base"
    if [[ ! -f "$dest" ]]; then
      cp -L "$dep" "$dest"
      chmod u+w "$dest"
      copy_deps "$dest"
    fi
    install_name_tool -change "$dep" "@executable_path/../Frameworks/$base" "$binary" || true
  done < <(otool -L "$binary" | tail -n +2 | awk '{print $1}')
}
copy_deps "$MACOS/kvikk"
for dylib in "$FRAMEWORKS"/*.dylib; do
  [[ -e "$dylib" ]] || continue
  install_name_tool -id "@executable_path/../Frameworks/$(basename "$dylib")" "$dylib" || true
done

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
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Lars Halvor. MIT License.</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSSupportsOpeningDocumentsInPlace</key><true/>
    <key>CFBundleDocumentTypes</key><array><dict>
      <key>CFBundleTypeName</key><string>PDF document</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key><array><string>com.adobe.pdf</string></array>
      <key>CFBundleTypeExtensions</key><array><string>pdf</string></array>
    </dict></array>
$ICON_PLIST
</dict></plist>
PLIST

touch "$APP_PATH"
codesign --force --deep --sign - "$APP_PATH" >/dev/null 2>&1 || true

# Register the finished bundle with Launch Services so Finder can offer Kvikk
# under Open With immediately after a local install.
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [[ -x "$LSREGISTER" ]]; then
  "$LSREGISTER" -f "$APP_PATH" >/dev/null 2>&1 || true
fi
printf '\nInstalled: %s\n' "$APP_PATH"
echo "Launch kvikk pdf from Finder, Spotlight, or the Dock."
open -R "$APP_PATH" || true
