# Truffle

![CI](https://github.com/Coyenn/truffle/actions/workflows/ci.yml/badge.svg)

> A fast Rust CLI for managing 2D Roblox game assets. Turning Asphalt metadata into Luau + TypeScript catalogs enriched with per-image width/height and highlight variants.

Truffle acts as the connective between your art pipeline and the runtime you ship to. It bundles [Asphalt](https://github.com/jackTabsCode/asphalt) to sync assets to Roblox, augments each PNG with 2D-friendly properties (dimensions, highlight IDs), emits fresh Luau + TypeScript modules, and regenerates the outline variants you showcase in-game.

## Quick Links

- [Releases](https://github.com/Coyenn/truffle/releases) – download prebuilt binaries.
- [Issues](https://github.com/Coyenn/truffle/issues) – file bugs, propose features, or ask questions.
- [Actions](https://github.com/Coyenn/truffle/actions) – view the latest CI runs.

## Installation

### Prebuilt binaries

1. Grab the latest archive for your platform from the [releases page](https://github.com/Coyenn/truffle/releases).
2. Unzip it and place the `truffle` binary somewhere on your `PATH`.

### Using Cargo

```bash
cargo install --path .
```

### From Source

```bash
cargo build --release
```

The optimized binary will be available in `target/release/truffle` (or `truffle.exe` on Windows). Add that directory to your `PATH` or copy it into your toolchain.

## Quick Start

1. Create a `truffle.toml` configuration file (see Configuration below)
2. Set `TRUFFLE_API_KEY` environment variable with your Roblox API key
3. Run `truffle sync` to sync assets and generate augmented modules

```bash
# Sync assets and generate Luau + TypeScript modules
truffle sync

# Generate highlight variants for every PNG in a folder
truffle highlight assets/images --thickness 2
```

## Configuration

Truffle uses a `truffle.toml` configuration file that extends Asphalt's configuration. This file should be placed in your project root.

### Example `truffle.toml`

```toml
# All Asphalt configuration options are supported
[creator]
type = "user"
id = 9670971

[codegen]
typescript = true
style = "flat"

[inputs.assets]
path = "assets/**/*"
output_path = "src/shared"

# Truffle-specific options
[truffle]
# Automatically generate highlight variants
auto_highlight = true
# Default highlight thickness (used when auto_highlight is true)
highlight_thickness = 2
# Force regenerate highlights even if they exist
highlight_force = false
```

### Configuration Options

#### Asphalt Options

All options from [Asphalt's configuration](https://github.com/jackTabsCode/asphalt?tab=readme-ov-file#configuration) are supported:
- `creator`: Roblox creator (user or group) to upload assets under
- `codegen`: Code generation options (TypeScript, style, etc.)
- `inputs`: Asset input configurations (paths, output directories, etc.)

#### Truffle Options

- `auto_highlight` (default: `false`): Automatically generate highlight variants after syncing assets
- `highlight_thickness` (default: `1`): Outline thickness in pixels for auto-generated highlights
- `highlight_force` (default: `false`): Force regenerate highlights even if they already exist

## Commands

### `truffle sync`

Syncs assets to Roblox using the bundled Asphalt, then augments the Luau asset module with PNG metadata and highlight variant IDs. Finally, it emits a strongly-typed `.d.ts` file so TypeScript projects can statically reason about the same asset set.

| Option | Description | Default |
| --- | --- | --- |
| `--assets-input <PATH>` | Existing Luau asset registry to read | `src/shared/data/assets/assets.luau` |
| `--assets-output <PATH>` | Location to write the augmented module | `src/shared/data/assets/assets.luau` |
| `--dts-output <PATH>` | Path for generated TypeScript definitions | `src/shared/data/assets/assets.d.ts` |
| `--images-folder <PATH>` | Root folder that contains PNG sources | `assets/images` |
| `--api-key <KEY>` | API key override (otherwise `.env`/env var) | `TRUFFLE_API_KEY` |

Requirements:

- `truffle.toml` configuration file in the project root
- `TRUFFLE_API_KEY` environment variable set (or provided via `--api-key`)

### `truffle highlight`

Creates `*-highlight.png` siblings for every PNG you point it at.

| Argument / Option | Description |
| --- | --- |
| `<INPUT_PATH>` | File or directory containing PNGs. Directories are scanned recursively. |
| `--dry-run` | Log what would happen without touching files. |
| `--force` | Overwrite existing highlight variants. |
| `--thickness <N>` | Outline thickness in pixels (default `1`). |

Example flows:

```bash
# Preview which assets would change
truffle highlight assets/images --dry-run

# Force-regenerate with thicker outlines
truffle highlight assets/images --force --thickness 3

# Target a single file
truffle highlight assets/images/character/base.png
```

The command tracks successes, skips, and failures so you can quickly spot assets that need manual attention.

## Development

```bash
# Run the CLI locally
cargo run -- sync
cargo run -- highlight assets/images

# Check format + lints
cargo fmt
cargo clippy -- -D warnings

# Run tests (including highlight algorithms)
cargo test
```

CI runs fmt, clippy, and tests on every push or pull request. Tagged releases additionally build and upload platform-specific archives.

## Authentication

Set the `TRUFFLE_API_KEY` environment variable with your Roblox Open Cloud API key. You can get one from the [Creator Dashboard](https://create.roblox.com/credentials).

The following permissions are required:
- `asset:read`
- `asset:write`

Make sure that your API key is under the Creator (user or group) that you've defined in `truffle.toml`.
