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
truffle image highlight assets/images --thickness 2

# Snap messy or off-grid pixels back to a crisp pixel-art grid
truffle image snap assets/images --recursive

# Generate a grass integration overlay for one sprite
truffle image terrain assets/images/house.png
```

## Configuration

Truffle uses a single `truffle.toml` configuration file in your project root.
There is no `[codegen]` section: Truffle always generates nested tables with
extensions kept plus TypeScript declarations, the only shape the sync pipeline
understands. There is no `[truffle]` section either: those options live at the
top level next to `creator` and `inputs`.

### Example `truffle.toml`

```toml
# Top-level keys first: anything after a [table] header belongs to that
# table, so keep these above [creator]/[inputs]/[font].

# Everything below is optional. Shown with defaults.
# Prefer TRUFFLE_API_KEY (.env included) over committing `api_key`.
# api_key = "your-open-cloud-key"
assets_input = "src/shared/data/assets/assets.luau"
assets_output = "src/shared/data/assets/assets.luau"
dts_output = "src/shared/data/assets/assets.d.ts"
images_folder = "assets/images"

auto_highlight = false
highlight_thickness = 1
highlight_force = false

atlas = false
atlas_size = 1024
atlas_padding = 4
atlas_exclude = []
scratch_dir = ".truffle"

[creator]
type = "user"
id = 9670971

[inputs.assets]
path = "assets/images/**/*"
output_path = "src/shared/data/assets"

# Optional shared defaults for `truffle font`. CLI flags win over these.
[font]
padding = 5
px = 121
line_height = 95
```

### Configuration Options

- `creator`: Roblox creator (user or group) to upload assets under
- `inputs`: Asset input configurations (paths, output directories, etc.)
- `api_key` (default: unset): Open Cloud API key fallback. Precedence is
  `--api-key` flag, then `TRUFFLE_API_KEY` (`.env` is loaded automatically),
  then this field.
- `assets_input` / `assets_output` / `dts_output` / `images_folder`: sync
  input/output paths (defaults shown above). Each has a matching CLI flag
  that overrides it for one invocation.
- `auto_highlight` (default: `false`): Automatically generate highlight variants after syncing assets
- `highlight_thickness` (default: `1`): Outline thickness in pixels for auto-generated highlights
- `highlight_force` (default: `false`): Force regenerate highlights even if they already exist
- `atlas` (default: `false`): Pack sprites into atlas textures before upload
- `atlas_size` (default: `1024`): Square atlas texture size (power of two)
- `atlas_padding` (default: `4`): Padding in pixels around each sprite in the atlas
- `atlas_exclude` (default: `[]`): Image keys excluded from atlas packing (synced individually)
- `scratch_dir` (default: `.truffle`): Scratch directory for intermediate files
- `[font]`: Optional `truffle font` preset (`padding`, `charset`,
  `charset_file`, `px`, `line_height`, `max_atlas_size`, `kerning_gap`,
  `luau`, `dts`, `runtime_out`, `outline`, `outline_png`, `no_antialias`).
  Every field is optional; CLI flags override the preset, which overrides
  the built-in defaults. Input/output paths stay per-invocation arguments.

### Atlas packing

When `atlas = true`, sprites are packed with a **MaxRects** layout (Best
Short-Side Fit) for high density. Packing is also **incremental**: previously
placed sprites keep their page and rect, and only new/changed sprites are
packed into leftover free space or fresh pages. Because Roblox uploads are
content-addressed, this means adding one sprite only reuploads the atlas page
it actually lands on instead of regenerating every atlas.

Packing state is persisted to `.truffle/truffle-atlases.toml` (inside the
scratch directory). It is intended to be tracked in version control — add a
`.truffle/*` + `!.truffle/truffle-atlases.toml` gitignore exception — so
results are stable across machines and CI. Delete it to force a fresh full
repack.

## Commands

### `truffle sync`

Syncs assets to Roblox using the bundled Asphalt, then augments the Luau asset module with PNG metadata and highlight variant IDs. Finally, it emits a strongly-typed `.d.ts` file so TypeScript projects can statically reason about the same asset set.

| Option | Description | Default |
| --- | --- | --- |
| `--assets-input <PATH>` | Existing Luau asset registry to read | truffle.toml `assets_input` |
| `--assets-output <PATH>` | Location to write the augmented module | truffle.toml `assets_output` |
| `--dts-output <PATH>` | Path for generated TypeScript definitions | truffle.toml `dts_output` |
| `--images-folder <PATH>` | Root folder that contains PNG sources | truffle.toml `images_folder` |
| `--api-key <KEY>` | API key override (otherwise `TRUFFLE_API_KEY` / truffle.toml `api_key`) | `TRUFFLE_API_KEY` env |

Requirements:

- `truffle.toml` configuration file in the project root
- An API key from `--api-key`, `TRUFFLE_API_KEY` (`.env` is loaded
  automatically), or truffle.toml `api_key`

### `truffle image highlight`

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
truffle image highlight assets/images --dry-run

# Force-regenerate with thicker outlines
truffle image highlight assets/images --force --thickness 3

# Target a single file
truffle image highlight assets/images/character/base.png
```

The command tracks successes, skips, and failures so you can quickly spot assets that need manual attention.

### `truffle image project`

Generate a PNG from one painted source and one self-contained JSON coordinate map.
The map includes the output dimensions, silhouette, and per-pixel RGBA shading;
reuse it for many designs without an editor, base atlas, or separate shade image.

```bash
truffle image project paint.png --map shirt.json --output shirt.png
truffle image project paint.png --map shirt.json --output shirt.png --force
```

Identical reruns leave output bytes and modification times untouched. `--dry-run`
validates both inputs without writing. See the [format and authoring guide](docs/image-project.md)
and [JSON Schema](schemas/projection.schema.json) for maps that can address shirts,
pants, shoes, animations, or arbitrary pixel art.

### `truffle image snap`

Snaps messy, blurry, or off-grid pixels (e.g. AI-generated sprites) back to a
crisp pixel-art grid, using the open-source
[Sprite Fusion Pixel Snapper](https://github.com/Hugo-Dz/spritefusion-pixel-snapper)
(MIT). Accepts PNG/JPEG input and always writes PNG output; without `--output`,
each input gets a `<stem>-snapped.png` sibling.

| Argument / Option | Description |
| --- | --- |
| `<INPUT_PATH>` | File or directory containing PNG/JPEGs. Directories are scanned non-recursively unless `-r` is passed. |
| `-o`, `--output <PATH>` | Output PNG file (single input) or output directory (directory input). |
| `--colors <N>` | Number of palette colors quantized before snapping (default `16`). |
| `--pixel-size <PIXELS>` | Override the auto-detected pixel size. |
| `--palette <HEX,...>` | Constrain the output to comma-separated 6-digit hex colors. |
| `--palette-png <PNG>` | Constrain the output to the visible colors of a palette PNG. |
| `--dry-run` | Log what would happen without touching files. |
| `--force` | Overwrite existing snapped outputs. |
| `-r`, `--recursive` | Recursively process directories. |

Example flows:

```bash
# Snap one sprite (writes sprite-snapped.png beside it)
truffle image snap assets/images/slime.png

# Snap a folder recursively into an output directory
truffle image snap assets/images --recursive --output assets/snapped

# Snap with a fixed palette and explicit pixel size
truffle image snap assets/images --palette 0d2b45,ffecd6 --pixel-size 8
```

### `truffle image terrain`

Creates transparent `*-grass.png` overlays for integrating sprite bases into grass.

| Argument / Option | Description |
| --- | --- |
| `<INPUT_PATH>` | File or directory containing PNGs. |
| `--dry-run` | Log what would happen without touching files. |
| `--force` | Overwrite existing grass overlays. |
| `-r`, `--recursive` | Recursively process directories. |
| `--grass-sample <PNG>` | Use visible pixels from a grass sample image as grass colors. |

Example flows:

```bash
# Generate a grass overlay
truffle image terrain assets/images/house.png

# Generate a grass overlay using a sample tile
truffle image terrain assets/images/house.png --grass-sample assets/images/grass.png

# Process a folder recursively
truffle image terrain assets/images --recursive
```

## Development

```bash
# Run the CLI locally
cargo run -- sync
cargo run -- image highlight assets/images
cargo run -- image terrain exterior.png

# Check format + lints
cargo fmt
cargo clippy -- -D warnings

# Run tests (including highlight algorithms)
cargo test
```

CI runs fmt, clippy, and tests on every push or pull request. Tagged releases additionally build and upload platform-specific archives.

## Authentication

Set the `TRUFFLE_API_KEY` environment variable with your Roblox Open Cloud API key (`.env` files are loaded automatically), pass `--api-key`, or set `api_key` in `truffle.toml`. You can get a key from the [Creator Dashboard](https://create.roblox.com/credentials).

The following permissions are required:
- `asset:read`
- `asset:write`

Make sure that your API key is under the Creator (user or group) that you've defined in `truffle.toml`.
