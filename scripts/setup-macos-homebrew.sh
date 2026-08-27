#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This helper is for macOS." >&2
  exit 1
fi

if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew is required for this no-Nix source build: https://brew.sh" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  cat >&2 <<'EOF'
Rust is not installed. Install it with rustup:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
Then open a new terminal and run this script again.
EOF
  exit 1
fi

brew install tesseract leptonica pkg-config create-dmg

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
./scripts/package-macos-release.sh

echo
echo "No Nix was used. Open the generated .dmg in target/ and drag kvikk pdf to Applications."
