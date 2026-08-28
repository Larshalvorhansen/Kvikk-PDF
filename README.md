<p align="center">
  <img src="assets/logo.png" width="180" alt="kvikk pdf logo">
</p>

# kvikk pdf

**kvikk pdf** is a fast, keyboard-first native PDF reader for deep reading, skimming, and rapid navigation.

It is written in Rust and built around PDFium, egui/eframe, wgpu, and Tesseract OCR. The reader keeps the interface deliberately small while providing unusually fast keyboard navigation, pacing, flexible page layouts, search, selectable text, OCR fallback, inversion, and high-resolution rendering.

## Highlights

- Native Rust desktop application. No browser or webview.
- Fast virtualized PDF rendering with a bounded bitmap cache.
- Search embedded PDF text and OCR scanned/image-only pages.
- Select and copy native PDF text.
- Click PDF links directly. Internal links jump to their destination; web and mail links open in the system app.
- Link hover uses a hand cursor and shows the destination before you click.
- Black fullscreen reading mode and PDF inversion.
- Time-based auto-scroll pacer with fine-grained speeds and a 15 px/s default.
- Pinch zoom and manual zoom up to 2000%.
- Single-page and multi-page layouts, plus a dynamic whole-document overview.
- Multiple PDF tabs with preserved reading position, keyboard switching, new-tab, and close-tab shortcuts.
- PDFs stay local on your machine.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `Ctrl/⌘ F` or `/` | Search inside the PDF |
| `Enter` / `Shift+Enter` | Next / previous search result |
| `g45` | Go to page 45 |
| Hold `?` | Show keyboard commands |
| `S` | Show / hide toolbar |
| `I` | Invert PDF |
| `P` or `K` | Play / pause pacer |
| `J` | Slower pacer |
| `L` | Faster pacer |
| `Space` | Next page / page group |
| `Shift+Space` | Snap to the top of the current page/group; if already there, go to the previous page/group |
| `+` / `-` | Zoom in / out |
| `O` or `⌘O` | Open a PDF |
| `⌘T` | New empty tab |
| `⌘W` | Close current tab |
| `⌘⇧Tab` or `⌘⇧T` | Reopen the previously closed tab |
| `0` | Reset zoom to 100% |
| Pinch / Ctrl-scroll | Zoom around pointer |
| `1` | Fit width |
| `2` | Fit height |
| `3` | 2 pages, 2×1 |
| `4` | 2 rows of pages; columns adapt to the window |
| `5` | 3 rows of pages; columns adapt to the window |
| `6` | 4 rows of pages; columns adapt to the window |
| `7` | 5 rows of pages; columns adapt to the window |
| `8` | 7 rows of pages; columns adapt to the window |
| `9` | Dynamic overview: fit all or nearly all pages on screen |
| `⌘1`–`⌘8` | Switch to tabs 1–8 |
| `⌘9` | Switch to the last tab |
| `F` | Toggle fullscreen |
| `Ctrl/⌘ C` | Copy selected text |

Modes `4`–`8` keep a fixed row count but calculate their column count from the current window shape and the PDF’s average page aspect ratio. Resizing the window therefore changes how many pages fit in each navigation group without changing the requested number of rows. Mode `9` remains a fully dynamic whole-document overview. For example, a ten-page portrait PDF will typically use a compact layout such as 5×2, while a roughly 300-page PDF may use around 24–26 columns depending on the window aspect ratio.

On macOS, `⌘Tab`/`⌘⇧Tab` are normally reserved by the operating system for application switching, so Kvikk also supports the conventional `⌘⇧T` shortcut for reopening the most recently closed tab.

## About

The in-app About window contains the text below. Packaged macOS builds also include the same credits in AppKit's native **About kvikk pdf** panel.

> This software is designed for rapid PDF navigation and flexible view customization. The goal is to make deep reading and skimming PDFs as frictionless as possible.
>
> Written in Rust by Lars Halvor, in dialogue with an LLM.
>
> **kvikk pdf is completely free and open source.**
>
> If you'd like to say thanks, the best thing you can do is visit my website or send me a message on any of my social platforms.
>
> **halvorhansen.no**

## Install kvikk pdf (no Nix required)

### macOS

For normal users, download the `.dmg` from a GitHub Release, open it, and drag **kvikk pdf** onto the **Applications** shortcut. That is the recommended installation method; Nix and Rust are not required.

The release DMG contains the application, PDFium, OCR libraries, and English/Norwegian OCR data. Its **Applications** item is an explicit link to the system `/Applications` directory, not `~/Applications` or a Home Manager folder.

To build from source on macOS without Nix, install Rust and Homebrew, then run:

```sh
./scripts/setup-macos-homebrew.sh
```

The script installs the native Homebrew build dependencies and produces a `.dmg` and `.zip` under `target/`.

### Windows

Windows users can download the `windows-x64.zip` release, extract it, and run `kvikk.exe`. Everything needed at runtime is shipped in that release directory. Nix is not involved.

For a Windows source build, install Rust and vcpkg, then install `tesseract:x64-windows` and `leptonica:x64-windows`; `scripts/package-windows-release.ps1` performs the final packaging.

## Development with Nix

On macOS or Linux:

```sh
nix-shell
cargo run --release
```

Open a PDF directly:

```sh
cargo run --release -- /path/to/document.pdf
```

### Install as a normal macOS app

From the Nix shell:

```sh
./scripts/install-macos-app.sh
```

This builds and installs:

```text
/Applications/kvikk pdf.app
```

After that, launch it from Finder, Spotlight, Raycast, Alfred, or the Dock. You do not need to enter `nix-shell` every time.

## GitHub releases

The repository includes `.github/workflows/release.yml`.

Every tag matching `v*` builds release archives for macOS and Windows and attaches them to the corresponding GitHub Release.

For example:

```sh
git tag v0.6.2
git push origin v0.6.2
```

The workflow creates artifacts similar to:

```text
kvikk-pdf-0.6.2-macos-arm64.dmg
kvikk-pdf-0.6.2-macos-arm64.zip
kvikk-pdf-0.6.2-windows-x64.zip
```

The macOS DMG uses the standard drag-to-Applications layout.

The exact macOS architecture follows the GitHub-hosted runner used by the workflow. The local Nix installer builds natively for your Mac, including Apple Silicon.

### Windows release dependencies

The Windows workflow uses vcpkg to build Tesseract and Leptonica. This is the setup recommended by the Rust `leptess` bindings. Runtime DLLs, PDFium, and OCR data are copied beside `kvikk.exe` in the release archive.

### macOS release dependencies

The macOS workflow installs Tesseract/Leptonica with Homebrew and bundles their non-system dynamic libraries into the `.app`. PDFium and `eng`/`nor` OCR data are included in the app bundle.

The CI-produced macOS app is ad-hoc signed. A future public distribution can add an Apple Developer ID certificate and notarization without changing the application architecture.

## Creating the public GitHub repository

With the GitHub CLI installed and authenticated:

```sh
git init
git add .
git commit -m "Initial release of kvikk pdf"
git branch -M main
gh repo create kvikk-pdf --public --source=. --remote=origin --push
```

Then create the first downloadable release:

```sh
git tag v0.6.2
git push origin v0.6.2
```

GitHub Actions will do the delightfully repetitive packaging work from there.

## Architecture

- **egui / eframe / wgpu**: native UI and GPU composition.
- **PDFium**: PDF parsing, rendering, text geometry, destinations, and links.
- **Tesseract + Leptonica**: OCR fallback for scanned/image-only pages.
- **Crossbeam channels**: separate PDF and OCR workers so expensive work stays off the UI thread.

PDF pages are virtualized. Only visible and nearby pages are rendered at high resolution; stale render jobs are discarded during rapid zooming/layout changes.

## License

MIT. See [LICENSE](LICENSE).

## Open PDFs from Finder on macOS

The packaged macOS app registers itself as a PDF viewer. After copying **kvikk pdf.app** to Applications, you can:

1. Right-click any PDF in Finder.
2. Choose **Open With → kvikk pdf**.
3. To make Kvikk the default, choose **Get Info → Open with → kvikk pdf → Change All…**.

You can also test the association from Terminal with:

```bash
open -a "kvikk pdf" /path/to/file.pdf
```

Kvikk registers as a PDF viewer and reports successful document opens back to Finder. Opening a PDF from Finder creates a new tab whether Kvikk is launching or already running.
