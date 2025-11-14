# Truffle

A Rust CLI tool for managing assets and generating image highlight.

## Installation

### From Release Tarballs

Download the appropriate release archive for your platform from the releases page. Extract the archive and add the `bin` directory to your PATH.

**Note**: Release tarballs include a bundled ImageMagick binary, so no additional dependencies are required.

### From Source

```bash
cargo build --release
```

The binary will be available at `target/release/truffle` (or `target/release/truffle.exe` on Windows).

**Note**: When building from source, ImageMagick must be installed separately:
- **Linux**: `sudo apt-get install imagemagick` (or equivalent for your distribution)
- **macOS**: `brew install imagemagick`
- **Windows**: Download from [ImageMagick website](https://imagemagick.org/script/download.php)
- **Nix**: ImageMagick is automatically available in the development shell

## Commands

### sync

Sync assets and augment metadata with image dimensions.

```bash
truffle sync [OPTIONS]
```

**Options:**
- `--assets-input <PATH>` - Path to the Luau assets module file (default: `src/shared/data/assets/assets.luau`)
- `--assets-output <PATH>` - Path to write the augmented Luau assets module (default: `src/shared/data/assets/assets.luau`)
- `--dts-output <PATH>` - Path to write the TypeScript declaration file (default: `src/shared/data/assets/assets.d.ts`)
- `--images-folder <PATH>` - Path to the raw assets images folder (default: `assets/images`)
- `--asphalt-api-key <KEY>` - ASPHALT_API_KEY (or read from environment/.env file)

**Requirements:**
- `asphalt` command must be available
- `ASPHALT_API_KEY` environment variable must be set (or provided via `--asphalt-api-key`)

**Description:**
Runs `asphalt sync` to sync assets, then augments the asset metadata with PNG image dimensions and highlight variant IDs. The command reads the assets file (supports both Luau and JSON formats), processes PNG files to extract dimensions, and writes updated Luau and TypeScript declaration files.

### highlight

Generate highlight variants of PNG images with white outlines.

```bash
truffle highlight <INPUT_PATH> [OPTIONS]
```

**Arguments:**
- `<INPUT_PATH>` - Input path (file or directory)

**Options:**
- `--dry-run` - Preview what would be generated without creating files
- `--force` - Overwrite existing highlight variants
- `--thickness <N>` - Outline thickness in pixels (default: 1)

**Requirements:**
- `magick` (ImageMagick) command must be available
  - **Bundled**: Official release tarballs include a bundled ImageMagick binary, so no separate installation is required
  - **Override**: Set `TRUFFLE_MAGICK` environment variable to use a custom ImageMagick installation
  - **Development**: When building from source, ImageMagick must be installed system-wide or via Nix

**Examples:**
```bash
# Process a single file
truffle highlight assets/images/character/base.png

# Process a directory recursively
truffle highlight assets/images/character/

# Dry run to preview changes
truffle highlight assets/images/ --dry-run

# Force overwrite existing highlight
truffle highlight assets/images/ --force

# Custom thickness
truffle highlight assets/images/ --thickness 2
```

**Description:**
Processes PNG files to generate highlight variants with white outlines. When given a directory, recursively finds all PNG files (excluding existing `-highlight.png` files) and generates highlight variants for each.

## Development

### Building

```bash
cargo build
```

### Running

```bash
cargo run -- sync
cargo run -- highlight assets/images/
```

### Testing

```bash
cargo test
```

### Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

## Bundled Dependencies

### ImageMagick

Truffle CLI release tarballs include a bundled ImageMagick binary to eliminate the need for users to install it separately. The bundled version is used automatically when available.

**Bundled ImageMagick Details:**
- **Version**: 7.1.1-15 (static build)
- **Source**: Official ImageMagick static builds from https://imagemagick.org/script/download.php
- **License**: Apache License 2.0 (see https://imagemagick.org/script/license.php)
- **Location**: `vendor/imagemagick/<platform>/` relative to the executable

**Overriding the Bundled Binary:**
You can override the bundled ImageMagick by setting the `TRUFFLE_MAGICK` environment variable:
```bash
export TRUFFLE_MAGICK=/usr/local/bin/magick
truffle highlight assets/images/
```

**Detection Order:**
1. `TRUFFLE_MAGICK` environment variable (if set)
2. Bundled binary relative to executable
3. System `magick` command (fallback)

**Note**: Release archives are larger due to the bundled ImageMagick binary (~10-20MB additional size per platform).
