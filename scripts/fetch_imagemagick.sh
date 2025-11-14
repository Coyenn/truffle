#!/bin/bash
set -euo pipefail

# Script to download ImageMagick binaries from GitHub releases (Linux only)
# Source: https://github.com/ImageMagick/ImageMagick/releases

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_DIR="$SCRIPT_DIR/../vendor/imagemagick"

# ImageMagick version
VERSION="7.1.2-8"
BASE_URL="https://github.com/ImageMagick/ImageMagick/releases/download/${VERSION}"

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

# Only support Linux
if [ "$OS" != "linux" ]; then
  echo "Error: This script only supports Linux"
  echo "ImageMagick is only bundled for Linux releases"
  exit 1
fi

# Detect Linux architecture
case "$ARCH" in
  x86_64)
    ARCH_SUFFIX="x86_64"
    ASSET_NAME="ImageMagick-a3b13d1-clang-x86_64.AppImage"
    ;;
  aarch64)
    ARCH_SUFFIX="aarch64"
    ASSET_NAME="ImageMagick-a3b13d1-clang-aarch64.AppImage"
    ;;
  *)
    echo "Unsupported Linux architecture: $ARCH"
    exit 1
    ;;
esac

PLATFORM_DIR="$VENDOR_DIR/linux-${ARCH_SUFFIX}"
mkdir -p "$PLATFORM_DIR"

URL="${BASE_URL}/${ASSET_NAME}"
TEMP_DIR=$(mktemp -d)
TEMP_FILE="${TEMP_DIR}/${ASSET_NAME}"

echo "Downloading ImageMagick ${VERSION} for linux-${ARCH_SUFFIX}..."
echo "URL: $URL"

# Download the file
HTTP_CODE=$(curl -L -o "$TEMP_FILE" -w "%{http_code}" -s "$URL")

if [ "$HTTP_CODE" != "200" ]; then
  echo "Error: Failed to download ImageMagick (HTTP $HTTP_CODE)"
  echo "Tried to download: $URL"
  echo ""
  echo "Please check: https://github.com/ImageMagick/ImageMagick/releases/tag/${VERSION}"
  echo "for available assets"
  rm -rf "$TEMP_DIR"
  exit 1
fi

echo "Downloaded ${ASSET_NAME}"

# Copy AppImage and make executable
cp "$TEMP_FILE" "$PLATFORM_DIR/magick"
chmod +x "$PLATFORM_DIR/magick"
echo "✅ ImageMagick AppImage installed to $PLATFORM_DIR/magick"

# Cleanup
rm -rf "$TEMP_DIR"

echo "Done! ImageMagick for linux-${ARCH_SUFFIX} is ready at $PLATFORM_DIR"
