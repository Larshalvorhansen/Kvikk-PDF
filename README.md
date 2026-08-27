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
- Follow internal PDF links such as contents, references, and footnotes.
- Open `http://`, `https://`, and `mailto:` links in the system browser/mail client.
- Black fullscreen reading mode and PDF inversion.
- Time-based auto-scroll pacer with fine-grained speeds.
- Pinch zoom and manual zoom up to 2000%.
- Single-page and multi-page layouts up to 21 pages at once.
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
| `Shift+Space` | Previous page / page group |
| `+` / `-` | Zoom in / out |
| `O` | Reset zoom to 100% |
| Pinch / Ctrl-scroll | Zoom around pointer |
| `1` | Fit width |
| `2` | Fit height |
| `3` | 2 pages, 2×1 |
| `4` | 3 pages, 3×1 |
| `5` | 6 pages, 3×2 |
| `6` | 10 pages, 5×2 |
| `7` | 21 pages, 7×3 |
| `F` | Toggle fullscreen |
| `Ctrl/⌘ C` | Copy selected text |

## About

> This software is designed for rapid PDF navigation and flexible view customization. The goal is to make deep reading and skimming PDFs as frictionless as possible.
>
> Written in Rust by Lars Halvor, in dialogue with an LLM.
>
> **kvikk pdf is completely free and open source.**
>
> If you'd like to say thanks, the best thing you can do is visit my website or send me a message on any of my social platforms.
>
> **halvorhansen.no**

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
~/Applications/kvikk pdf.app
```

After that, launch it from Finder, Spotlight, Raycast, Alfred, or the Dock. You do not need to enter `nix-shell` every time.

## GitHub releases

The repository includes `.github/workflows/release.yml`.

Every tag matching `v*` builds release archives for macOS and Windows and attaches them to the corresponding GitHub Release.

For example:

```sh
git tag v0.3.0
git push origin v0.3.0
```

The workflow creates artifacts similar to:

```text
kvikk-pdf-0.3.0-macos-x86_64.zip
kvikk-pdf-0.3.0-windows-x64.zip
```

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
git tag v0.3.0
git push origin v0.3.0
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
