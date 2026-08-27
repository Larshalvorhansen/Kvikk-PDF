{ pkgs ? import <nixpkgs> {} }:

let
  lib = pkgs.lib;
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  isLinux = pkgs.stdenv.hostPlatform.isLinux;

  tesseractWithLanguages = pkgs.tesseract.override {
    enableLanguages = [ "eng" "nor" ];
  };

  commonPackages = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    pkg-config
    clang
    libclang

    pdfium-binaries
    tesseractWithLanguages
    leptonica
  ];

  # These are eframe/wgpu runtime/build dependencies on Linux only.
  # Pulling them on macOS causes Nix to evaluate Wayland/X11 packages that
  # are intentionally unsupported there.
  linuxPackages = with pkgs; [
    wayland
    libxkbcommon
    libGL
    vulkan-loader
    fontconfig
    freetype
    dbus
    openssl
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libxcb
  ];

  linuxRuntimeLibraries = with pkgs; [
    pdfium-binaries
    tesseractWithLanguages
    leptonica
    wayland
    libxkbcommon
    libGL
    vulkan-loader
    fontconfig
    freetype
    dbus
    openssl
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libxcb
  ];

  darwinRuntimeLibraries = [
    tesseractWithLanguages
    pkgs.leptonica
  ];

  pdfiumLibrary =
    if isDarwin then
      "${pkgs.pdfium-binaries}/lib/libpdfium.dylib"
    else
      "${pkgs.pdfium-binaries}/lib/libpdfium.so";
in
pkgs.mkShell {
  packages = commonPackages ++ lib.optionals isLinux linuxPackages;

  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
  TESSDATA_PREFIX = "${tesseractWithLanguages}/share/tessdata";

  # The Rust backend loads PDFium dynamically, so point it at the exact
  # platform-specific library instead of relying on loader search paths.
  PDFIUM_LIBRARY_PATH = pdfiumLibrary;

  # Linux and macOS use different dynamic loader variables. Most Nix-linked
  # libraries already carry store paths, but these make development-shell
  # execution robust for native OCR dependencies as well.
  LD_LIBRARY_PATH = lib.optionalString isLinux (
    lib.makeLibraryPath linuxRuntimeLibraries
  );

  DYLD_LIBRARY_PATH = lib.optionalString isDarwin (
    lib.makeLibraryPath darwinRuntimeLibraries
  );

  shellHook = ''
    echo "kvikk pdf native development shell"
    echo "  Platform:  ${pkgs.stdenv.hostPlatform.system}"
    echo "  Rust:      $(rustc --version)"
    echo "  Cargo:     $(cargo --version)"
    echo "  PDFium:    $PDFIUM_LIBRARY_PATH"
    echo "  Tesseract: eng + nor"
    echo
    echo "Run: cargo run --release"
    echo "Install macOS app: ./scripts/install-macos-app.sh"
  '';
}
