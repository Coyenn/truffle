#!/bin/bash
set -euo pipefail

# Script to package Truffle CLI with bundled ImageMagick for release
# Usage: ./scripts/package_release.sh [target-triple]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Default target is the current platform
TARGET="${1:-$(rustc -vV | sed -n 's|host: ||p')}"

echo "Packaging Truffle CLI for target: $TARGET"

# Detect platform from target triple
PLATFORM=""
ARCH_SUFFIX=""
BINARY_NAME="truffle"

case "$TARGET" in
  *linux*x86_64*)
    PLATFORM="linux"
    ARCH_SUFFIX="x86_64"
    ;;
  *linux*aarch64*)
    PLATFORM="linux"
    ARCH_SUFFIX="aarch64"
    ;;
  *apple-darwin*x86_64*)
    PLATFORM="macos"
    ARCH_SUFFIX="x86_64"
    ;;
  *apple-darwin*arm64*)
    PLATFORM="macos"
    ARCH_SUFFIX="arm64"
    ;;
  *windows*gnu*x86_64*|*windows*msvc*x86_64*)
    PLATFORM="windows"
    ARCH_SUFFIX="x86_64"
    BINARY_NAME="truffle.exe"
    ;;
  *windows*gnu*aarch64*|*windows*msvc*aarch64*)
    PLATFORM="windows"
    ARCH_SUFFIX="arm64"
    BINARY_NAME="truffle.exe"
    ;;
  *)
    echo "Unsupported target: $TARGET"
    exit 1
    ;;
esac

PLATFORM_DIR="${PLATFORM}-${ARCH_SUFFIX}"
echo "Detected platform: $PLATFORM_DIR"

# Build the Rust binary
echo "Building Rust binary..."
cargo build --release --target "$TARGET"

# Ensure ImageMagick binaries are fetched
echo "Ensuring ImageMagick binaries are available..."
if [ "$PLATFORM" = "windows" ]; then
    echo "Please run scripts/fetch_imagemagick.ps1 on Windows to fetch ImageMagick binaries"
    echo "For now, assuming binaries are already present..."
else
    bash "$SCRIPT_DIR/fetch_imagemagick.sh"
fi

# Create release directory
RELEASE_DIR="release/truffle-${PLATFORM_DIR}"
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR/bin"
mkdir -p "$RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}"

# Copy binary
BINARY_SRC="target/${TARGET}/release/${BINARY_NAME}"
if [ ! -f "$BINARY_SRC" ]; then
    echo "Error: Binary not found at $BINARY_SRC"
    exit 1
fi
cp "$BINARY_SRC" "$RELEASE_DIR/bin/${BINARY_NAME}"

# Make binary executable (for Unix-like systems)
if [ "$PLATFORM" != "windows" ]; then
    chmod +x "$RELEASE_DIR/bin/${BINARY_NAME}"
fi

# Copy ImageMagick binary
IMAGEMAGICK_SRC="vendor/imagemagick/${PLATFORM_DIR}"
if [ ! -d "$IMAGEMAGICK_SRC" ]; then
    echo "Error: ImageMagick binaries not found at $IMAGEMAGICK_SRC"
    echo "Please run scripts/fetch_imagemagick.sh first"
    exit 1
fi

if [ "$PLATFORM" = "windows" ]; then
    cp "$IMAGEMAGICK_SRC/magick.exe" "$RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}/"
    # Copy any DLLs
    if ls "$IMAGEMAGICK_SRC"/*.dll 1> /dev/null 2>&1; then
        cp "$IMAGEMAGICK_SRC"/*.dll "$RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}/"
    fi
else
    cp "$IMAGEMAGICK_SRC/magick" "$RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}/"
    chmod +x "$RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}/magick"
fi

# Copy README
cp README.md "$RELEASE_DIR/"

# Create archive
ARCHIVE_NAME="truffle-${PLATFORM_DIR}.tar.gz"
if [ "$PLATFORM" = "windows" ]; then
    ARCHIVE_NAME="truffle-${PLATFORM_DIR}.zip"
    cd release
    zip -r "$ARCHIVE_NAME" "truffle-${PLATFORM_DIR}"
    cd ..
else
    cd release
    tar -czf "$ARCHIVE_NAME" "truffle-${PLATFORM_DIR}"
    cd ..
fi

echo "✅ Release package created: release/$ARCHIVE_NAME"
echo "   Binary: $RELEASE_DIR/bin/${BINARY_NAME}"
echo "   ImageMagick: $RELEASE_DIR/vendor/imagemagick/${PLATFORM_DIR}/"

