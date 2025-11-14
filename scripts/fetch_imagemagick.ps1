# Script to download and extract ImageMagick static binaries for Windows
# ImageMagick static builds are available from: https://imagemagick.org/script/download.php

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$VendorDir = Join-Path (Split-Path -Parent $ScriptDir) "vendor\imagemagick"

# Detect architecture
$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -eq "AMD64") {
    $ArchSuffix = "x86_64"
} elseif ($Arch -eq "ARM64") {
    $ArchSuffix = "arm64"
} else {
    Write-Host "Unsupported Windows architecture: $Arch"
    exit 1
}

$PlatformDir = Join-Path $VendorDir "windows-$ArchSuffix"
New-Item -ItemType Directory -Force -Path $PlatformDir | Out-Null

# ImageMagick static build URL
$Version = "7.1.1-15"
$BaseUrl = "https://imagemagick.org/archive/binaries"

$Tarball = "ImageMagick-${Version}-${ArchSuffix}-pc-windows.zip"
$Url = "${BaseUrl}/${Tarball}"

$TempDir = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }
$TempZip = Join-Path $TempDir $Tarball

Write-Host "Downloading ImageMagick for Windows-$ArchSuffix..."
try {
    Invoke-WebRequest -Uri $Url -OutFile $TempZip -UseBasicParsing
} catch {
    Write-Host "Failed to download ImageMagick. You may need to manually download from:"
    Write-Host "  https://imagemagick.org/script/download.php"
    Write-Host "  Or use a different version/URL"
    exit 1
}

Write-Host "Extracting ImageMagick..."
Expand-Archive -Path $TempZip -DestinationPath $TempDir -Force

# Find the magick binary in the extracted directory
$ExtractedDir = Get-ChildItem -Path $TempDir -Directory -Filter "ImageMagick-*" | Select-Object -First 1
if (-not $ExtractedDir) {
    Write-Host "Could not find extracted ImageMagick directory"
    exit 1
}

# Copy the magick binary
$MagickExe = Join-Path $ExtractedDir.FullName "bin\magick.exe"
if (Test-Path $MagickExe) {
    Copy-Item $MagickExe "$PlatformDir\magick.exe"
    Write-Host "✅ ImageMagick binary installed to $PlatformDir\magick.exe"
} else {
    Write-Host "Could not find magick.exe binary in extracted archive"
    exit 1
}

# Copy any required DLLs
$BinDir = Join-Path $ExtractedDir.FullName "bin"
if (Test-Path $BinDir) {
    Get-ChildItem -Path $BinDir -Filter "*.dll" | ForEach-Object {
        Copy-Item $_.FullName $PlatformDir
    }
}

# Cleanup
Remove-Item -Recurse -Force $TempDir

Write-Host "Done! ImageMagick for Windows-$ArchSuffix is ready at $PlatformDir"

