#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${KVIKK_VERSION:-$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)}"
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) PDFIUM_ARCH=arm64 ;;
  x86_64) PDFIUM_ARCH=x64 ;;
  *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
esac

command -v cargo >/dev/null
command -v curl >/dev/null
command -v otool >/dev/null
command -v install_name_tool >/dev/null

if command -v brew >/dev/null 2>&1; then
  export PKG_CONFIG_PATH="$(brew --prefix tesseract)/lib/pkgconfig:$(brew --prefix leptonica)/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
fi

BUILD_DIR="$ROOT/target/release-package"
PDFIUM_DIR="$BUILD_DIR/pdfium"
APP="$BUILD_DIR/kvikk pdf.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"
rm -rf "$BUILD_DIR"
mkdir -p "$PDFIUM_DIR" "$MACOS" "$RESOURCES/tessdata" "$FRAMEWORKS"

curl -L --fail --retry 3 \
  "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-${PDFIUM_ARCH}.tgz" \
  -o "$BUILD_DIR/pdfium.tgz"
tar -xzf "$BUILD_DIR/pdfium.tgz" -C "$PDFIUM_DIR"

export PDFIUM_LIBRARY_PATH="$PDFIUM_DIR/lib/libpdfium.dylib"
export PDFIUM_DYNAMIC_LIB_PATH="$PDFIUM_DIR/lib"

cargo build --release
cp target/release/kvikk "$MACOS/kvikk"
cp "$PDFIUM_DIR/lib/libpdfium.dylib" "$MACOS/libpdfium.dylib"
chmod +x "$MACOS/kvikk"

# Copy OCR language data. Homebrew's prefix is preferred in release CI.
TESS_PREFIX="${TESSDATA_PREFIX:-}"
if [[ -z "$TESS_PREFIX" ]] && command -v brew >/dev/null 2>&1; then
  TESS_PREFIX="$(brew --prefix tesseract)/share/tessdata"
fi
for lang in eng nor; do
  if [[ -f "$TESS_PREFIX/$lang.traineddata" ]]; then
    cp "$TESS_PREFIX/$lang.traineddata" "$RESOURCES/tessdata/"
  else
    echo "Missing $lang.traineddata in $TESS_PREFIX" >&2
    exit 1
  fi
done

# AppKit's standard About panel automatically uses Credits.html from Resources.
cp assets/Credits.html "$RESOURCES/Credits.html"

# Bundle non-system dylibs used by kvikk (Tesseract/Leptonica and their Homebrew deps),
# then rewrite references to @executable_path/../Frameworks. PDFium is loaded at
# runtime from Contents/MacOS and therefore is handled separately above.
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

# Create a proper macOS icon from the supplied PNG.
if command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
  ICONSET="$BUILD_DIR/Kvikk.iconset"
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
  <key>CFBundleIconFile</key><string>Kvikk</string>
  <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Lars Halvor. MIT License.</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>PDF document</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key><array><string>com.adobe.pdf</string></array>
    <key>CFBundleTypeExtensions</key><array><string>pdf</string></array>
  </dict></array>
</dict></plist>
PLIST

codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

ZIP_OUT="$ROOT/target/kvikk-pdf-${VERSION}-macos-${ARCH}.zip"
DMG_OUT="$ROOT/target/kvikk-pdf-${VERSION}-macos-${ARCH}.dmg"
rm -f "$ZIP_OUT" "$DMG_OUT"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP_OUT"

# Create the familiar macOS installer window: app on the left, Applications
# shortcut on the right. create-dmg gives us the polished layout; hdiutil is a
# dependency-free fallback that still includes the /Applications alias.
DMG_STAGE="$BUILD_DIR/dmg-stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
ditto "$APP" "$DMG_STAGE/kvikk pdf.app"

if command -v create-dmg >/dev/null 2>&1; then
  set +e
  create-dmg \
    --volname "kvikk pdf" \
    --volicon "$RESOURCES/Kvikk.icns" \
    --window-pos 200 120 \
    --window-size 660 400 \
    --icon-size 128 \
    --icon "kvikk pdf.app" 175 190 \
    --hide-extension "kvikk pdf.app" \
    --app-drop-link 485 190 \
    "$DMG_OUT" "$DMG_STAGE"
  DMG_STATUS=$?
  set -e
else
  DMG_STATUS=1
fi

if [[ $DMG_STATUS -ne 0 || ! -f "$DMG_OUT" ]]; then
  rm -f "$DMG_OUT"
  ln -sfn /Applications "$DMG_STAGE/Applications"
  hdiutil create -volname "kvikk pdf" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_OUT" >/dev/null
fi

printf '%s\n%s\n' "$DMG_OUT" "$ZIP_OUT"
