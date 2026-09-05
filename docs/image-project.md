# Image projection maps

`truffle image project` turns a painted PNG into an output PNG using one JSON map:

```sh
truffle image project paint.png --map garment.json --output garment.png
```

These are the only two inputs. The map embeds the output canvas, source coordinates,
silhouette and shading. It never loads other files or executes scripts. Pack front,
back, soles, sleeves, or any other surfaces into the same source image. Truffle has
no garment, pose, animation, or Roblox-specific assumptions.

The map is a reusable pixel stencil: it stores where each visible output pixel
gets its paint, rather than recomputing geometry for every design. Perspective,
limb ownership, folds, occlusion and atlas gutters can be resolved once when
authoring the map, then reused across a collection of artwork.

## Command behavior

| Argument | Behavior |
| --- | --- |
| `SOURCE_PNG` | One PNG containing all painted surfaces. |
| `--map MAP_JSON` | Self-contained version 1 projection map. |
| `-o, --output OUTPUT_PNG` | Defaults to `<source-stem>-projected.png` beside the source. Creates parent directories. |
| `--force` | Replace a different existing output. Both input files are protected, including symlinks and hard links. |
| `--dry-run` | Parse, validate, decode and render without writing files or directories. |

Repeating a command with identical inputs succeeds without rewriting an identical
output, even without `--force`. The output bytes and modification time stay the
same. Changed outputs require `--force`. Encoding finishes before output is
replaced through a temporary file in the destination directory. Reproduction
requires neither the previous output nor a project config, cache, editor or base
atlas. PNG bytes are deterministic for a given Truffle build; different encoder
versions may compress identical pixels differently.

## Format, version 1

The machine-readable contract is [projection.schema.json](../schemas/projection.schema.json).
Coordinates start at the top-left, with x increasing right and y increasing down.
All coordinates are exact integers; sampling does not interpolate or clamp.

```json
{
  "version": 1,
  "source_size": [32, 16],
  "output_size": [191, 363],
  "palette": [
    [255, 255, 255, 255],
    [192, 192, 192, 255],
    [255, 240, 220, 128]
  ],
  "rows": [
    {"at": [11, 18], "pixels": [[4, 0, 0], [5, 0, 1], [20, 0, 2]]},
    {"at": [11, 19], "pixels": [[4, 1, 0], [5, 1, 1]]}
  ]
}
```

| Field | Meaning |
| --- | --- |
| `version` | Must be `1`. Unknown versions and fields are errors. |
| `source_size` | Required source PNG width and height. Must match exactly. |
| `output_size` | Output PNG width and height, independent of source dimensions. |
| `palette` | RGBA multipliers, each channel from 0 through 255. Defaults to one opaque white entry when omitted. |
| `rows` | Horizontal runs of explicitly mapped output pixels. An empty list produces a transparent canvas. |
| `rows[].at` | `[output_x, output_y]` of the first pixel in this run. |
| `rows[].pixels[]` | `[source_x, source_y, palette_index]`. All indices are zero-based. Each entry advances output x by one. |

The first run above paints output `(11,18)` from source `(4,0)`, `(12,18)`
from `(5,0)` with a gray shadow, and `(13,18)` from `(20,0)` with a warm tint
and partial coverage. Multiple runs can share an output row, separated by gaps.
Runs may appear in any order, but their destinations must not overlap. Several
destinations may sample the same source pixel. Reversing source coordinates in a
run mirrors the paint; changing their spacing creates pixel-exact compression.

Each output channel is `floor((paint * multiplier + 127) / 255)`, including alpha.
This is straight RGBA multiplication with nearest integer rounding, applied to
8-bit channel values without a linear-light conversion. RGB shading and alpha
coverage are independent. White paint reproduces the map's embedded shading.
Transparent paint removes coverage. Fully transparent output pixels are stored
as `[0,0,0,0]`, and unmapped pixels remain transparent.

Dimensions must be positive and each canvas is limited to 67,108,864 pixels.
Coordinates support the full declared dimensions, including values above 255.
Out-of-range source coordinates, palette indices and output runs fail validation.
The source must use the declared resolution; enlarged painting requires a map
whose source dimensions and coordinates address that resolution.

## Authoring maps

1. Choose one source layout and an output canvas. Name and explain the painted
   regions in the garment's documentation; the renderer only needs coordinates.
2. For each visible output pixel, choose a source coordinate and an RGBA
   multiplier. A single-pixel run is enough for a hand-authored mapping.
3. Deduplicate multipliers into `palette`, and optionally group adjacent output
   pixels into runs to keep the JSON compact. No external shading PNG is needed.
4. Check with `--dry-run`, then project white paint and a colored coordinate grid.
   White checks silhouette and shading; the grid reveals seams and wrong surfaces.

The final JSON is the mapping authority. A fitting tool can export this format,
but is not a dependency for rendering. Clothing variants with different
silhouettes use different maps. Pants and shoes can choose their own layouts,
dimensions, sampling and shading through this same contract.
