$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
$Version = ((Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value)
$Build = Join-Path $Root "target\release-package-win"
$Pdfium = Join-Path $Build "pdfium"
$OutDir = Join-Path $Build "kvikk-pdf"
Remove-Item -Recurse -Force $Build -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Pdfium, $OutDir, (Join-Path $OutDir "tessdata") | Out-Null

$PdfiumArchive = Join-Path $Build "pdfium.tgz"
Invoke-WebRequest "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz" -OutFile $PdfiumArchive
tar.exe -xf $PdfiumArchive -C $Pdfium
$env:PDFIUM_LIBRARY_PATH = Join-Path $Pdfium "bin\pdfium.dll"

cargo build --release
Copy-Item "target\release\kvikk.exe" $OutDir
Copy-Item (Join-Path $Pdfium "bin\pdfium.dll") $OutDir

# vcpkg dynamic runtime DLLs. GitHub Actions sets VCPKG_ROOT.
$TripletBin = Join-Path $env:VCPKG_ROOT "installed\x64-windows\bin"
if (Test-Path $TripletBin) {
  Copy-Item (Join-Path $TripletBin "*.dll") $OutDir -ErrorAction SilentlyContinue
}

# Tesseract language data can live beside the executable.
$Share = $env:KVIKK_TESSDATA
if (-not $Share) {
  $Share = Join-Path $env:VCPKG_ROOT "installed\x64-windows\share\tessdata"
}
if (-not (Test-Path $Share)) {
  $Share = Join-Path $env:VCPKG_ROOT "installed\x64-windows\share\tesseract\tessdata"
}
foreach ($lang in @("eng", "nor")) {
  $file = Join-Path $Share "$lang.traineddata"
  if (-not (Test-Path $file)) { throw "Missing OCR language data: $file" }
  Copy-Item $file (Join-Path $OutDir "tessdata")
}

$Zip = Join-Path $Root "target\kvikk-pdf-$Version-windows-x64.zip"
Remove-Item $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$OutDir\*" -DestinationPath $Zip
Write-Output $Zip
