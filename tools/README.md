# procgena tools

Command-line tools shared by the projects in this workspace. They are one
[uv](https://docs.astral.sh/uv/) project; uv installs Python and the
dependencies on first run. Run them from the workspace root:

```bash
uv run --project tools textures --help
```

## textures

Downloads, prepares and publishes the textures of `stone`, `tree`, `clump`,
`mushroom` and `deadwood`.
Each project lists its textures in `<project>/textures.yml`: surface sets (rock,
bark, snow…) and cards (transparent leaf or stalk images). The finished files
live in a public bucket, `gs://ze_textures/<project>/`, and are listed with
SHA-256 checksums in `<project>/assets/textures/assets_manifest.json`.

### Getting the textures

Most people only need the finished textures, to run the viewers, examples and
texture tests:

```bash
uv run --project tools textures download tree
```

This saves the 512 and 1k sizes in both GPU formats into
`<project>/assets/textures/`. Add `--sizes 512,1k,2k` for the large size,
`--formats bc7` for one format, or `--images` for the JPEG/PNG images that
model export (glTF, OBJ) uses. Files that already match the manifest are kept.

### Making textures

To add or change textures, edit the project's `textures.yml`, then:

| Step | Command | What it does |
|------|---------|--------------|
| 1 | `textures fetch <project> [ID…]` | Downloads each surface set's 2k source images from its website into `<project>/prior_art/textures/<id>/`, with an `info.json` holding the title, authors and licence. |
| 2 | `textures convert <project> [ID…]` | Writes the KTX2 GPU textures into `<project>/assets/textures/`. `--only surfaces`, `cards` or `clumps` limits the work. |
| 3 | `textures manifest <project>` | Writes `assets_manifest.json` from the KTX2 files and prints the upload commands. |
| 4 | `textures images <project>` | Writes the JPEG/PNG export images and `images/images_manifest.json`, and prints the upload command. |

`prior_art/` is ignored by Git. Cards and hand-downloaded (`manual`) sets read
files you place there yourself; the comments in each `textures.yml` say where
each comes from.

Every surface set comes in three sizes (512, 1k and 2k pixels on the long side),
unless the project's `textures.yml` lists its own `sizes` (from 128, 256, 512,
1k and 2k; `terrain` uses 128, 512 and 1k). Every texture comes in two
formats, both with mipmaps and Zstandard compression:

- `bc7` — read directly by GPUs that support BC7, which is most desktops.
- `uastc` — Basis Universal; the loader converts it for the GPU it runs on (in
  Bevy, enable the `basis-universal` feature).

A project's `layout` decides how surface maps are stored:

- `orh` (stone, clump): `color`, `normal`, and `orh`, which packs ambient
  occlusion (red), roughness (green) and height (blue) into one image. Export
  images use `orm`: occlusion, roughness and metalness, as glTF expects.
- `roughness` (tree bark): `color`, `normal`, and `roughness` on its own.

Load colour as sRGB and every other map as linear data. Normal maps use the
OpenGL (+Y) convention.

`convert`, `images` and `average-colors` need ImageMagick 7 (`magick`);
`convert` also needs KTX-Software 4.4+ (`ktx`) and ctt-cli 0.6
(`cargo install ctt-cli --version 0.6.1 --locked`); `manifest` needs `ktx`.

### Other commands

- `textures average-colors <project>` prints each surface set's average colour
  as Rust table rows, for tables such as procgena_tree's `BARK_AVERAGE_COLORS`.
  Run it after changing bark textures and paste the output into the table.
- `textures key-card INPUT PREFIX` cuts reddish stalks (such as moss
  sporophytes) out of a photo on a light background, writing a card image and
  a normal map for a `generated` card.

### Card recolouring

A card's `recolor` setting regrades its colour image to green. Each pixel's
brightness picks a colour on the `dark` → `mid` → `light` ramp. Greyish
pixels, such as twigs, keep a muted version of their own colour instead:
`twig_level` is an ImageMagick `-level` range on inverted saturation (higher
values pick only greyer pixels) and `twig_modulate` the twig colour's
`-modulate` brightness,saturation percentages.

## plant

Scripts for procgena_plant only. Run them from the workspace root; each file's
opening comment explains its options.

| Script | What it does |
|--------|--------------|
| `python tools/plant/lint.py` | Runs clippy with warnings as errors for each feature set of procgena_plant and procgena_plant_bevy, and for the web demo. |
| `python tools/plant/bench_ab.py` | Paired A/B benchmark runs of the `bench` example. |
| `python tools/plant/bench_240.py` | The 240 Hz frame-time gate, on a machine listed in `tools/plant/reference-machines.json`. |
| `python tools/plant/web_demo.py` | Builds, serves and smoke-tests the hosted web demo (`plant/web/`). |
| `uv run --project tools python tools/plant/record_walk.py` | Records the `walk` example as the README's animated film; uv supplies Pillow. |
