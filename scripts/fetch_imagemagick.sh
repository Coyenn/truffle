#!/bin/bash
set -euo pipefail

# Script to download and extract ImageMagick static binaries for Linux and macOS
# ImageMagick static builds are available from: https://imagemagick.org/script/download.php

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_DIR="$SCRIPT_DIR/../vendor/imagemagick"

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux)
    PLATFORM="linux"
    if [[ "$ARCH" == "x86_64" ]]; then
      ARCH_SUFFIX="x86_64"
    elif [[ "$ARCH" == "aarch64" ]]; then
      ARCH_SUFFIX="aarch64"
    else
      echo "Unsupported Linux architecture: $ARCH"
      exit 1
    fi
    ;;
  darwin)
    PLATFORM="macos"
    if [[ "$ARCH" == "x86_64" ]]; then
      ARCH_SUFFIX="x86_64"
    elif [[ "$ARCH" == "arm64" ]]; then
      ARCH_SUFFIX="arm64"
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

# ImageMagick static build URLs
# Note: These are example URLs - you'll need to update with actual static build URLs
# For now, we'll use the official ImageMagick static builds
# Linux x86_64: https://imagemagick.org/archive/binaries/ImageMagick-x86_64-pc-linux-gnu.tar.gz
# macOS x86_64: https://imagemagick.org/archive/binaries/ImageMagick-x86_64-apple-darwin.tar.gz
# macOS arm64: https://imagemagick.org/archive/binaries/ImageMagick-arm64-apple-darwin.tar.gz

VERSION="7.1.1-15"
BASE_URL="https://imagemagick.org/archive/binaries"

case "$PLATFORM-$ARCH_SUFFIX" in
  linux-x86_64)
    TARBALL="ImageMagick-${VERSION}-x86_64-pc-linux-gnu.tar.gz"
    ;;
  linux-aarch64)
    TARBALL="ImageMagick-${VERSION}-aarch64-pc-linux-gnu.tar.gz"
    ;;
  macos-x86_64)
    TARBALL="ImageMagick-${VERSION}-x86_64-apple-darwin.tar.gz"
    ;;
  macos-arm64)
    TARBALL="ImageMagick-${VERSION}-arm64-apple-darwin.tar.gz"
    ;;
  *)
    echo "Unsupported platform: $PLATFORM-$ARCH_SUFFIX"
    exit 1
    ;;
esac

URL="${BASE_URL}/${TARBALL}"
TEMP_DIR=$(mktemp -d)
TEMP_TARBALL="${TEMP_DIR}/${TARBALL}"

echo "Downloading ImageMagick for $PLATFORM-$ARCH_SUFFIX..."
curl -L -o "$TEMP_TARBALL" "$URL" || {
  echo "Failed to download ImageMagick. You may need to manually download from:"
  echo "  https://imagemagick.org/script/download.php"
  echo "  Or use a different version/URL"
  exit 1
}

echo "Extracting ImageMagick..."
cd "$TEMP_DIR"
tar -xzf "$TEMP_TARBALL"

# Find the magick binary in the extracted directory
EXTRACTED_DIR=$(find . -maxdepth 1 -type d -name "ImageMagick-*" | head -1)
if [ -z "$EXTRACTED_DIR" ]; then
  echo "Could not find extracted ImageMagick directory"
  exit 1
fi

# Copy the magick binary
if [ -f "${EXTRACTED_DIR}/bin/magick" ]; then
  cp "${EXTRACTED_DIR}/bin/magick" "$PLATFORM_DIR/magick"
  chmod +x "$PLATFORM_DIR/magick"
  echo "✅ ImageMagick binary installed to $PLATFORM_DIR/magick"
else
  echo "Could not find magick binary in extracted archive"
  exit 1
fi

# Copy any required libraries (for static builds, there may be none)
if [ -d "${EXTRACTED_DIR}/lib" ]; then
  cp -r "${EXTRACTED_DIR}/lib"/* "$PLATFORM_DIR/" 2>/dev/null || true
fi

# Cleanup
rm -rf "$TEMP_DIR"

echo "Done! ImageMagick for $PLATFORM-$ARCH_SUFFIX is ready at $PLATFORM_DIR"

