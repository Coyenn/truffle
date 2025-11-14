# Script to download ImageMagick binaries from GitHub releases
# Source: https://github.com/ImageMagick/ImageMagick/releases

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$VendorDir = Join-Path (Split-Path -Parent $ScriptDir) "vendor\imagemagick"

# ImageMagick version
$Version = "7.1.2-8"
$BaseUrl = "https://github.com/ImageMagick/ImageMagick/releases/download/$Version"

# Fetch available assets from GitHub API
Write-Host "Fetching available assets from GitHub releases..."
try {
    $AssetsResponse = Invoke-RestMethod -Uri "https://api.github.com/repos/ImageMagick/ImageMagick/releases/tags/$Version" -UseBasicParsing
    $AssetsList = $AssetsResponse.assets | ForEach-Object { $_.name }
} catch {
    Write-Host "Warning: Could not fetch asset list from GitHub API"
    $AssetsList = @()
}

# Detect architecture
$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -eq "AMD64") {
    $ArchSuffix = "x86_64"
    # Try to find Windows asset matching x64
    if ($AssetsList) {
        $AssetName = $AssetsList | Where-Object { $_ -match "x64.*7z|windows.*x64|win.*x64" } | Select-Object -First 1
    }
    # Fallback - need to check actual Windows asset name format
    if (-not $AssetName) {
        $AssetName = "ImageMagick-a3b13d1-clang-x86_64.7z"
    }
} elseif ($Arch -eq "ARM64") {
    $ArchSuffix = "arm64"
    if ($AssetsList) {
        $AssetName = $AssetsList | Where-Object { $_ -match "arm64.*7z|aarch64.*7z" } | Select-Object -First 1
    }
    if (-not $AssetName) {
        $AssetName = "ImageMagick-a3b13d1-clang-arm64.7z"
    }
} else {
    Write-Host "Unsupported Windows architecture: $Arch"
    exit 1
}

$PlatformDir = Join-Path $VendorDir "windows-$ArchSuffix"
New-Item -ItemType Directory -Force -Path $PlatformDir | Out-Null

$Url = "${BaseUrl}/${AssetName}"
$TempDir = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }
$TempFile = Join-Path $TempDir $AssetName

Write-Host "Downloading ImageMagick $Version for Windows-$ArchSuffix..."
Write-Host "URL: $Url"

try {
    Invoke-WebRequest -Uri $Url -OutFile $TempFile -UseBasicParsing
} catch {
    Write-Host "Error: Failed to download ImageMagick"
    Write-Host "Tried to download: $Url"
    Write-Host ""
    Write-Host "Available assets for version $Version:"
    if ($AssetsList) {
        $AssetsList | Select-Object -First 20 | ForEach-Object { Write-Host "  $_" }
    } else {
        Write-Host "  Could not fetch asset list. Please check manually:"
    }
    Write-Host ""
    Write-Host "Please check: https://github.com/ImageMagick/ImageMagick/releases/tag/$Version"
    Write-Host "and update the script with the correct AssetName"
    exit 1
}

Write-Host "Downloaded $AssetName"
Write-Host "Extracting archive..."

# Extract .7z file
$ExtractDir = Join-Path $TempDir "extracted"
New-Item -ItemType Directory -Force -Path $ExtractDir | Out-Null

# Try 7-Zip if available
$sevenZip = $null
if (Get-Command 7z -ErrorAction SilentlyContinue) {
    $sevenZip = "7z"
} elseif (Get-Command 7za -ErrorAction SilentlyContinue) {
    $sevenZip = "7za"
} elseif (Test-Path "C:\Program Files\7-Zip\7z.exe") {
    $sevenZip = "C:\Program Files\7-Zip\7z.exe"
}

if ($sevenZip) {
    & $sevenZip x $TempFile "-o$ExtractDir" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: Failed to extract .7z file"
        exit 1
    }
} else {
    Write-Host "Error: 7z or 7za not available. Cannot extract .7z file"
    Write-Host "Please install 7-Zip or use WSL with 7z available"
    exit 1
}

# Find magick.exe
$MagickBinary = Get-ChildItem -Path $ExtractDir -Recurse -Filter "magick.exe" | Select-Object -First 1

if ($MagickBinary -and (Test-Path $MagickBinary.FullName)) {
    Copy-Item $MagickBinary.FullName "$PlatformDir\magick.exe"
    Write-Host "✅ ImageMagick binary installed to $PlatformDir\magick.exe"
    
    # Copy any DLLs from the same directory
    $MagickDir = Split-Path $MagickBinary.FullName
    Get-ChildItem -Path $MagickDir -Filter "*.dll" | Copy-Item -Destination $PlatformDir
} else {
    Write-Host "Error: Could not find magick.exe in extracted archive"
    Write-Host "Extracted contents:"
    Get-ChildItem -Path $ExtractDir -Recurse | Select-Object -First 20
    exit 1
}

# Cleanup
Remove-Item -Recurse -Force $TempDir

Write-Host "Done! ImageMagick for Windows-$ArchSuffix is ready at $PlatformDir"
