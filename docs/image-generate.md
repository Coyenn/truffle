# Image generation prompts

`truffle image generate` turns a prompt Markdown file into PNGs via the
[Replicate API](https://replicate.com/docs), then chains each download through
the Pixel Snapper so the result is crisp pixel art:

```sh
truffle image generate prompts/slime.md --dry-run
truffle image generate prompts/slime.md --output assets/generated/slime.png
```

Each run keeps the raw download (`slime-1.png`) next to its snapped sibling
(`slime-1-snapped.png`) so a review loop can compare candidates before the
Aseprite cleanup pass. See the `generate-sprite` skill in Caramel for the full
prompt → generate → snap → cleanup workflow.

## Command behavior

| Argument | Behavior |
| --- | --- |
| `PROMPT_FILE` | Prompt Markdown file with YAML front matter (this format). |
| `-o, --output OUTPUT` | Raw output PNG (single output) or output directory (multiple outputs). Defaults to the file's `output`, else `<stem>-generated.png` beside the prompt. A single output into an existing directory writes `<prompt-stem>.png` there; multiple outputs expand `stem.png` into `stem-1.png`, `stem-2.png`, … |
| `--dry-run` | Parse the prompt, print the resolved model, full input JSON, and planned output paths without calling Replicate. Uses `num_outputs` from the input (default `1`) to plan paths. |
| `--force` | Overwrite existing raw and snapped outputs. Without it, the command fails before any API call when an output already exists. |
| `--no-snap` | Skip the Pixel Snapper chaining step (snapping can also be rerun later with `truffle image snap`). |
| `--replicate-token <TOKEN>` | API token override (otherwise `REPLICATE_API_TOKEN` / truffle.toml `replicate_token`). |

Repeating a generation is never deduplicated: every invocation creates a new
prediction, even with identical inputs.

## Format, version 1

The Markdown body is the image prompt. The `---` front matter pins the model
and every generation parameter:

```markdown
---
version: 1
replicate:
  model: "google/nano-banana-2"
  input:
    aspect_ratio: "1:1"
    resolution: "2K"
    google_search: false
    image_search: false
    output_format: "png"
  timeout_secs: 600
  poll_interval_secs: 2
snap:
  colors: 16
output: "assets/generated/slime.png"
---

A cozy pixel-art slime on a plain white background, game sprite. ...
```

| Field | Meaning |
| --- | --- |
| `version` | Must be `1` when present. Unknown versions are errors. |
| `replicate.model` | Required. `owner/name`, with an optional `:version` suffix. When the suffix and `replicate.version` are both set, that is an error: pick one. |
| `replicate.version` | Optional pinned model version hash for reproducibility. Unset means the model's current version. |
| `replicate.input` | Generic passthrough map sent verbatim to Replicate, so any model works without Truffle changes. Unknown models only need their documented input keys here. |
| `replicate.timeout_secs` | Seconds to poll before giving up (default `600`). |
| `replicate.poll_interval_secs` | Seconds between status polls (default `2`). |
| `snap.colors` | Palette colors quantized before snapping (default `16`). Scale with content diversity: `min(64, max(16, tiles × 2))` — singles stay at `16`, `16`-tile sheets use `32`, `32+`-tile sheets use `64`. |
| `snap.pixel_size` | Override the auto-detected pixel size. |
| `snap.palette` | Constrain the output to comma-separated 6-digit hex colors. |
| `snap.palette_png` | Constrain the output to the visible colors of a palette PNG. |
| `output` | Default raw output path (CLI `--output` wins). |

The body is sent as `replicate.input.prompt` unless that key is already set
explicitly, in which case the explicit value wins. When the body contains a
fenced code block, only the first fenced block is sent and the surrounding
Markdown is ignored, so prompt files can keep human instructions around the
runnable prompt. Either way, one source must provide a non-empty prompt.
Unknown front matter fields are errors, so typos fail fast instead of
silently changing a generation.

Each download prints its dimensions (`Generated: out.png (5504x3072)`), and
each snap prints its dimensions plus the implied pixel-block size
(`Snapped: out-snapped.png (505x291, ~10.9x10.6 px blocks)`). A `WARNING`
means the grid likely collapsed (tiny snapped output or enormous implied
blocks): inspect before any cleanup. Image models draw proportionally rather
than honoring absolute pixel counts, so these numbers are the scale gate —
see the sizing guidance in the `generate-sprite` skill.

## Authentication

`truffle image generate` resolves its token exactly like `truffle sync`
resolves its API key: `--replicate-token` flag, then the `REPLICATE_API_TOKEN`
environment variable (`.env` is loaded automatically), then the
`replicate_token` field in `truffle.toml`. Get a token from
[replicate.com/account/api-tokens](https://replicate.com/account/api-tokens).

## Authoring prompts

1. Copy an existing prompt from the prompts directory or write a new file with
   the front matter above. Keep the model and its full parameters in the file
   so anyone can reproduce the run.
2. Validate with `--dry-run`: it prints the exact input JSON Replicate would
   receive plus every path that would be written.
3. Generate with `num_outputs` greater than one while exploring, then narrow to
   one once the design is settled. Compare the `-snapped.png` siblings, not
   just the raw downloads.
4. Carry the best snapped candidate into Aseprite for background removal,
   cutout, trim, and cleanup before moving it under `assets/images/`.
