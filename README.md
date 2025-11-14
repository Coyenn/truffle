# Truffle

A Rust CLI tool for managing assets and generating image highlight.

## Installation

### From Release Tarballs

Download the appropriate release archive for your platform from the releases page. Extract the archive and add the `bin` directory to your PATH.

### From Source

```bash
cargo build --release
```

The binary will be available at `target/release/truffle` (or `target/release/truffle.exe` on Windows).

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
- No external dependencies – image processing is handled entirely in-process via Rust crates.

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

### Image Processing

Truffle previously depended on an external ImageMagick binary for highlight generation. The pipeline now uses pure-Rust libraries, so the CLI ships without extra binaries or runtime dependencies. All highlight variants are produced with the built-in processor, ensuring consistent results across platforms.
