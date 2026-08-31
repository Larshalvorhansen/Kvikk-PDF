# Changelog

## 0.6.3

- Added `⌘K` visual page-margin cropping. Kvikk analyzes page content bounds lazily with PDFium, caches the crop per page, and uses the cropped geometry for layout, rendering, text selection, and PDF-link hit-testing without modifying the PDF.
- Changed `P` from play/pause to a pacer-mode toggle: continuous scrolling remains the default, while page-turn mode advances one page or page group after a timed interval.
- Kept `K` as the dedicated play/pause key in both pacer modes.
- Reused `J`/`L` for slower/faster page-turn timing; the existing speed ladder maps inversely to seconds per page, with the default 15-level corresponding to about 60 seconds per page.
- Added an on-page countdown bar in page-turn mode showing time remaining before the next turn.

## 0.6.2

- Changed view modes `4`–`8` to fixed-row layouts with window-adaptive column counts: 2, 3, 4, 5, and 7 rows respectively. Mode `9` remains the dynamic whole-document overview.
- Changed `Shift+Space` so it first returns to the top of the current page/page group when that top has scrolled out of view; only when already at the top does it move to the previous page/group.
- Changed the new-tab shortcut from `⌘N` to `⌘T`.
- Added `⌘⇧Tab` and `⌘⇧T` to reopen the most recently closed PDF tab, preserving its reading state.
- Added 650, 800, 1000, and 1250 px/s pacer levels above the previous 550 px/s maximum.
- Reduced render pressure in very dense page grids by using thumbnail-sized, lower-priority render requests.
- Added an explicit packaging check that the DMG Applications item resolves to the real system `/Applications` directory.

## 0.6.1

- Added `⌘W` to close the current PDF tab and release its PDFium document.
- Added `⌘N` to create a new empty tab; opening a PDF fills that tab, while normal PDF opens continue to create tabs automatically.
- Replaced the fixed 160-page mode 9 with a dynamic whole-document overview. Kvikk chooses the row/column count from the PDF page count, page aspect ratio, and current window shape.
- Changed the default pacer speed to exactly 15 px/s and added 15 px/s to the speed ladder.
- Reduced overview rendering overhead by using smaller thumbnail render requests and avoiding unnecessary text extraction until search is active.
- Changed the local macOS installer default from `~/Applications` to the real system `/Applications` directory.
- Changed DMG packaging to include an explicit symlink to `/Applications`, so the drag target can never resolve to a Home Manager or per-user Applications directory.

## 0.6.0

- Fixed macOS Finder / **Open With** document handoff so Kvikk explicitly reports successful PDF opens instead of triggering Finder’s “could not be opened” warning.
- Added multiple PDF tabs. Opening any PDF creates and selects a new tab; each tab keeps its view mode, zoom, scroll position, search state, inversion, and extracted text.
- Added `⌘1`–`⌘8` tab switching and `⌘9` for the last tab.
- Changed `O` to **Open PDF** and `0` to reset zoom to 100%.
- Removed the page-count/status text shown at the right side of the main toolbar after a successful open.
- Kept multiple PDFium documents resident while discarding inactive page textures, making tab switching responsive without retaining every rendered bitmap.
- Added `LSSupportsOpeningDocumentsInPlace` to macOS bundles.

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
