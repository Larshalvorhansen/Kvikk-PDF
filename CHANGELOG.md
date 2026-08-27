# Changelog

## 0.5.3

- Fix macOS Apple Event handler declaration for the `objc2 0.5.x` `declare_class!` macro.
- Finder/Open With integration remains based on `kAEOpenDocuments` without replacing Winit's application delegate.

## 0.5.2

- Fixed a macOS startup panic caused by replacing Winit 0.30.13's `NSApplicationDelegate`.
- Finder `Open With` now uses `NSAppleEventManager` for the standard open-documents Apple Event while leaving Winit's delegate intact.
- Cleaned unused-field warnings in the native viewer model and stale OCR event handling.

## 0.5.1

- Fixed macOS compilation with `objc2-foundation 0.2.2` by correctly containing the unsafe `NSURL::path()` call.
- Removed a macOS-only unused `KvikkApp` import warning.

## 0.5.0

- Added mode 8: 40-page 10×4 overview.
- Added mode 9: 160-page 20×8 overview.
- Added native macOS Finder / Launch Services “Open With” handling for PDFs.
- Added explicit `com.adobe.pdf` document registration to the macOS app bundle.
- Reduced the minimum thumbnail render width for very large overview grids.

## 0.4.0

- Made PDF links visibly clickable with hover cursor/destination feedback and immediate internal/external navigation.
- Added macOS native About credits via `Credits.html` and bundle copyright metadata.
- Added `.dmg` release packaging with drag-to-Applications installation.
- Added a Homebrew/Rust source-build path that does not require Nix.
- Reduced page-to-page spacing to 4 px in single-column modes and 2 px in multi-page grids.

## 0.3.0

- Renamed the application to **kvikk pdf** (`kvikk` executable).
- Added the kvikk application artwork and About dialog.
- Added internal PDF-link navigation and external web-link opening.
- Added public-repository metadata and MIT licensing cleanup.
- Added GitHub Actions release packaging for macOS and Windows.
- Preserved the keyboard-first reader, OCR, search, text selection, pacer, inversion, fullscreen, and multi-page layouts.
