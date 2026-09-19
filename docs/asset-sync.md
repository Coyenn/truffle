# Asset sync for an evolving game

Truffle treats source PNGs as the artwork source of truth. Their current pixel
dimensions determine the generated metadata. Atlas state preserves placement;
the upload lockfile associates file content with Roblox asset IDs. Neither is a
second source of artwork dimensions.

## Normal art iteration

1. Edit or replace the PNG at its existing path. Resizing is supported.
2. Run `truffle sync --dry-run` to preview the operation without uploading or
   modifying the published catalog, atlas layout, or upload lockfile.
3. Run `truffle sync` to pack, upload changed textures, and publish matching Luau
   and TypeScript metadata.
4. Recompile the game and let its existing Studio sync deliver the new catalog.
   A running play session may retain previously required modules; restart play
   to verify the current catalog.

In Caramel, the normal command is `bun run assets:sync`. Truffle does not start,
restart, or manage the game's live Sloptor/Rojo process.

Do not enlarge a sprite to make its dimensions match old metadata. Do not edit
the generated asset modules by hand. Do not routinely delete packing state.

## What changes when artwork changes

| Change | Expected result |
| --- | --- |
| Pixels change, dimensions stay the same | Keep the rectangle; upload the changed atlas page and update its ID |
| An existing PNG grows or shrinks | Reallocate that sprite using its current dimensions; update its dimensions and rectangle |
| A new PNG is added | Allocate a free rectangle without overlapping any existing sprite |
| Sync is repeated without changes | Reuse content-addressed upload IDs and preserve valid placement |
| Old packing state contains overlapping rectangles | Preserve valid slots and repack conflicting slots automatically |

The uploader creates/reuses content-addressed assets. It does not depend on
overwriting an old Roblox texture ID and waiting for clients to invalidate it.

## Selective sync and shared atlas pages

For direct images, use a source glob:

```sh
truffle sync --skip-atlas --sync-only 'assets/images/interface/fonts/**/*.png'
```

Direct subset sync merges the selected entries into the existing catalog and
retains unrelated upload lock entries. Both single-file paths and ordinary
`*.png` patterns must resolve to the original asset hierarchy.

With atlas mode enabled, `--sync-only` cannot isolate the contents of a texture
page: changing one sprite affects the shared page used by other sprites. Truffle
explicitly reports a full atlas consistency sync. It checks the complete layout
and only uploads pages whose content changed; unrelated edits on other pages may
also be included. It must not publish new rectangles against a previously
uploaded texture that does not contain them.

## Files and recovery

| File | Purpose | Keep in version control? |
| --- | --- | --- |
| Source PNGs | Artwork and dimensions | Yes |
| `truffle.toml` | Inputs, creator, packing and generation settings | Yes; keep API keys in the environment |
| `truffle.lock.toml` | Content hashes mapped to uploaded asset IDs | Yes |
| `.truffle/truffle-atlases.toml` | Stable atlas placements | Yes |
| Generated Luau and TypeScript catalogs | Runtime IDs, native dimensions, and atlas rectangles | Follow the game's generated-file policy; Caramel tracks them |
| Other `.truffle/` files | Rebuildable texture and backend staging output | No |

On a handled sync failure, Truffle restores its snapshotted local catalogs,
packing state, and upload lockfile together, including files that were originally
empty or absent. Uploads already accepted by Roblox cannot be undone by a local
rollback. This is recovery from reported errors, not a claim that remote uploads
and multiple local file writes form a crash-proof distributed transaction.

Deleting `.truffle/truffle-atlases.toml` deliberately discards layout stability
and forces a new packing arrangement. Use that only when a full repack is wanted;
replacement and resize workflows should not require it.

## Diagnosing a missing or distorted image

Check the source PNG, then its generated `width`/`height`, atlas rectangle, and
texture ID. Inspect the matching rectangle in `.truffle/atlases/atlas_NNN.png`.
If those pixels are wrong locally, uploading again cannot fix the picture.

If the local packed pixels match the source, check that Studio has the same
catalog and that Roblox can load the referenced texture. A UI control that fits
art into a fixed card can keep the same display size even when native PNG
dimensions change; distinguish that layout behavior from incorrect atlas
dimensions or missing pixels.

The incremental packing regression tests cover additions, resizing, exact pixel
preservation (including alpha), full pages, and repair of overlapping saved
placements. Sync recovery tests cover catalogs, layout, and lockfiles together.
