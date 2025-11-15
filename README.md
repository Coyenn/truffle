# Truffle

![CI](https://github.com/Coyenn/truffle/actions/workflows/ci.yml/badge.svg)

> A fast Rust CLI for managing 2D Roblox game assets. Turning Asphalt metadata into Luau + TypeScript catalogs enriched with per-image width/height and highlight variants.

Truffle acts as the connective between your art pipeline and the runtime you ship to. It ingests the metadata produced by [Asphalt](https://github.com/jackTabsCode/asphalt), augments each PNG with 2D-friendly properties (dimensions, highlight IDs), emits fresh Luau + TypeScript modules, and regenerates the outline variants you showcase in-game.

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

```bash
# 1. Refresh Luau + TypeScript assets after an Asphalt sync
truffle sync \
  --assets-input src/shared/data/assets/assets.luau \
  --dts-output src/shared/data/assets/assets.d.ts \
  --images-folder assets/images

# 2. Generate highlight variants for every PNG in a folder
truffle highlight assets/images --thickness 2
```

Set `ASPHALT_API_KEY` (or pass `--asphalt-api-key`) so Truffle can invoke `asphalt sync` on your behalf.

## Commands

### `truffle sync`

Pulls the latest data from your Asphalt backend, then augments the Luau asset module with PNG metadata and highlight variant IDs. Finally, it emits a strongly-typed `.d.ts` file so TypeScript projects can statically reason about the same asset set.

| Option | Description | Default |
| --- | --- | --- |
| `--assets-input <PATH>` | Existing Luau asset registry to read | `src/shared/data/assets/assets.luau` |
| `--assets-output <PATH>` | Location to write the augmented module | `src/shared/data/assets/assets.luau` |
| `--dts-output <PATH>` | Path for generated TypeScript definitions | `src/shared/data/assets/assets.d.ts` |
| `--images-folder <PATH>` | Root folder that contains PNG sources | `assets/images` |
| `--asphalt-api-key <KEY>` | API key override (otherwise `.env`/env var) | `ASPHALT_API_KEY` |

Requirements:

- `asphalt` CLI installed and accessible on your `PATH`.
- `ASPHALT_API_KEY` exported or provided via `--asphalt-api-key`.

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
