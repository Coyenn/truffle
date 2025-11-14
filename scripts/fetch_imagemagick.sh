#!/bin/bash
set -euo pipefail

# Script to download ImageMagick binaries from GitHub releases
# Source: https://github.com/ImageMagick/ImageMagick/releases

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_DIR="$SCRIPT_DIR/../vendor/imagemagick"

# ImageMagick version
VERSION="7.1.2-8"
BASE_URL="https://github.com/ImageMagick/ImageMagick/releases/download/${VERSION}"

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

# Fetch available assets from GitHub API to find the correct filename
echo "Fetching available assets from GitHub releases..."
ASSETS_JSON=$(curl -s "https://api.github.com/repos/ImageMagick/ImageMagick/releases/tags/${VERSION}")

if [ -z "$ASSETS_JSON" ] || echo "$ASSETS_JSON" | grep -q '"message": "Not Found"'; then
  echo "Warning: Could not fetch asset list from GitHub API"
  echo "Will try hardcoded asset names..."
  ASSET_NAME=""
else
  ASSETS_LIST=$(echo "$ASSETS_JSON" | grep -o '"name": "[^"]*"' | sed 's/"name": "\(.*\)"/\1/')
fi

case "$OS" in
  linux)
    PLATFORM="linux"
    if [[ "$ARCH" == "x86_64" ]]; then
      ARCH_SUFFIX="x86_64"
      # Try to find AppImage asset matching x86_64
      if [ -n "$ASSETS_LIST" ]; then
        ASSET_NAME=$(echo "$ASSETS_LIST" | grep -i "x86_64.*AppImage" | head -1)
      fi
      # Fallback to known format
      if [ -z "$ASSET_NAME" ]; then
        ASSET_NAME="ImageMagick-a3b13d1-clang-x86_64.AppImage"
      fi
    elif [[ "$ARCH" == "aarch64" ]]; then
      ARCH_SUFFIX="aarch64"
      if [ -n "$ASSETS_LIST" ]; then
        ASSET_NAME=$(echo "$ASSETS_LIST" | grep -i "aarch64.*AppImage\|arm64.*AppImage" | head -1)
      fi
      if [ -z "$ASSET_NAME" ]; then
        ASSET_NAME="ImageMagick-a3b13d1-clang-aarch64.AppImage"
      fi
    else
      echo "Unsupported Linux architecture: $ARCH"
      exit 1
    fi
    ;;
  darwin)
    PLATFORM="macos"
    if [[ "$ARCH" == "x86_64" ]]; then
      ARCH_SUFFIX="x86_64"
      if [ -n "$ASSETS_LIST" ]; then
        ASSET_NAME=$(echo "$ASSETS_LIST" | grep -i "x86_64.*pkg\|darwin.*pkg\|macos.*pkg" | head -1)
      fi
      if [ -z "$ASSET_NAME" ]; then
        ASSET_NAME="ImageMagick-a3b13d1-clang-x86_64.pkg"
      fi
    elif [[ "$ARCH" == "arm64" ]]; then
      ARCH_SUFFIX="arm64"
      if [ -n "$ASSETS_LIST" ]; then
        ASSET_NAME=$(echo "$ASSETS_LIST" | grep -i "arm64.*pkg\|aarch64.*pkg" | head -1)
      fi
      if [ -z "$ASSET_NAME" ]; then
        ASSET_NAME="ImageMagick-a3b13d1-clang-arm64.pkg"
      fi
    else
      echo "Unsupported macOS architecture: $ARCH"
      exit 1
    fi
    ;;
  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

PLATFORM_DIR="$VENDOR_DIR/$PLATFORM-$ARCH_SUFFIX"
mkdir -p "$PLATFORM_DIR"

URL="${BASE_URL}/${ASSET_NAME}"
TEMP_DIR=$(mktemp -d)
TEMP_FILE="${TEMP_DIR}/${ASSET_NAME}"

echo "Downloading ImageMagick ${VERSION} for $PLATFORM-$ARCH_SUFFIX..."
echo "URL: $URL"

# Download the file
HTTP_CODE=$(curl -L -o "$TEMP_FILE" -w "%{http_code}" -s "$URL")

if [ "$HTTP_CODE" != "200" ]; then
  echo "Error: Failed to download ImageMagick (HTTP $HTTP_CODE)"
  echo "Tried to download: $URL"
  echo ""
  echo "Available assets for version ${VERSION}:"
  if [ -n "$ASSETS_LIST" ]; then
    echo "$ASSETS_LIST" | head -20
  else
    echo "Could not fetch asset list. Please check manually:"
  fi
  echo ""
  echo "Please check: https://github.com/ImageMagick/ImageMagick/releases/tag/${VERSION}"
  echo "and update the script with the correct ASSET_NAME"
  rm -rf "$TEMP_DIR"
  exit 1
fi

echo "Downloaded ${ASSET_NAME}"

# Handle different file types
if [[ "$ASSET_NAME" == *.AppImage ]]; then
  # Linux AppImage - just copy and make executable
  cp "$TEMP_FILE" "$PLATFORM_DIR/magick"
  chmod +x "$PLATFORM_DIR/magick"
  echo "✅ ImageMagick AppImage installed to $PLATFORM_DIR/magick"
  
elif [[ "$ASSET_NAME" == *.pkg ]]; then
  # macOS .pkg - extract using pkgutil
  echo "Extracting macOS .pkg file..."
  
  # Create a temporary extraction directory
  EXTRACT_DIR="${TEMP_DIR}/extracted"
  mkdir -p "$EXTRACT_DIR"
  
  # Extract the .pkg payload
  if command -v pkgutil >/dev/null 2>&1; then
    # List contents first to find the payload
    pkgutil --expand "$TEMP_FILE" "$EXTRACT_DIR/pkg" >/dev/null 2>&1 || {
      echo "Error: Failed to extract .pkg file"
      rm -rf "$TEMP_DIR"
      exit 1
    }
    
    # Find and extract the payload
    PAYLOAD=$(find "$EXTRACT_DIR/pkg" -name "Payload" -o -name "*.pkg" | head -1)
    if [ -n "$PAYLOAD" ]; then
      cd "$EXTRACT_DIR"
      if command -v cpio >/dev/null 2>&1; then
        cat "$PAYLOAD" | cpio -i 2>/dev/null || true
      fi
    fi
    
    # Find the magick binary
    MAGICK_BINARY=$(find "$EXTRACT_DIR" -name "magick" -type f | head -1)
    
    if [ -n "$MAGICK_BINARY" ] && [ -f "$MAGICK_BINARY" ]; then
      cp "$MAGICK_BINARY" "$PLATFORM_DIR/magick"
      chmod +x "$PLATFORM_DIR/magick"
      echo "✅ ImageMagick binary installed to $PLATFORM_DIR/magick"
    else
      echo "Error: Could not find magick binary in extracted .pkg"
      echo "Extracted contents:"
      find "$EXTRACT_DIR" -type f | head -20
      rm -rf "$TEMP_DIR"
      exit 1
    fi
  else
    echo "Error: pkgutil not available. Cannot extract .pkg file"
    echo "On macOS, pkgutil should be available by default"
    rm -rf "$TEMP_DIR"
    exit 1
  fi
  
elif [[ "$ASSET_NAME" == *.7z ]] || [[ "$ASSET_NAME" == *.zip ]]; then
  # Windows archive - extract and find magick.exe
  echo "Extracting archive..."
  
  if [[ "$ASSET_NAME" == *.7z ]]; then
    if command -v 7z >/dev/null 2>&1; then
      7z x "$TEMP_FILE" -o"$TEMP_DIR/extracted" >/dev/null 2>&1
    elif command -v 7za >/dev/null 2>&1; then
      7za x "$TEMP_FILE" -o"$TEMP_DIR/extracted" >/dev/null 2>&1
    else
      echo "Error: 7z or 7za not available. Cannot extract .7z file"
      rm -rf "$TEMP_DIR"
      exit 1
    fi
  else
    unzip -q "$TEMP_FILE" -d "$TEMP_DIR/extracted" 2>/dev/null || {
      echo "Error: Failed to extract zip file"
      rm -rf "$TEMP_DIR"
      exit 1
    }
  fi
  
  # Find magick.exe
  MAGICK_BINARY=$(find "$TEMP_DIR/extracted" -name "magick.exe" -type f | head -1)
  
  if [ -n "$MAGICK_BINARY" ] && [ -f "$MAGICK_BINARY" ]; then
    cp "$MAGICK_BINARY" "$PLATFORM_DIR/magick.exe"
    echo "✅ ImageMagick binary installed to $PLATFORM_DIR/magick.exe"
    
    # Copy any DLLs from the same directory
    MAGICK_DIR=$(dirname "$MAGICK_BINARY")
    if [ -d "$MAGICK_DIR" ]; then
      find "$MAGICK_DIR" -maxdepth 1 -name "*.dll" -exec cp {} "$PLATFORM_DIR/" \; 2>/dev/null || true
    fi
  else
    echo "Error: Could not find magick.exe in extracted archive"
    rm -rf "$TEMP_DIR"
    exit 1
  fi
else
  echo "Error: Unknown file format: $ASSET_NAME"
  rm -rf "$TEMP_DIR"
  exit 1
fi

# Cleanup
rm -rf "$TEMP_DIR"

echo "Done! ImageMagick for $PLATFORM-$ARCH_SUFFIX is ready at $PLATFORM_DIR"
