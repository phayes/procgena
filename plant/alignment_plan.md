# Plant alignment: a staged plan

Plant's own roadmap for joining the shared contract in
[ARCHITECTURE.md](../ARCHITECTURE.md) ("Asset alignment", rules 1–14, and
"Plant: the furthest from the rest"). The early stages let the Ecotope
place and draw grass soon. The full move-over (roadmap R3's plant part)
comes after them, in pieces. Written 2026-11-24 with the user; no code was
changed.

## Bottom line

- **Four stages, then the rest of R3.**
  1. **P1, split:** plant becomes `procgena_plant` (engine-free: settings,
     presets, files) plus `procgena_plant_bevy` (everything that draws),
     both under `plant/crates/`. Nothing drawn changes.
  2. **P2, plants in the early Ecotope:** the engine-free crate bakes a
     plant Scatter's ground and density over a `ScatterInput`, the Ecotope
     holds `ScatterKind::Plant`, and `procgena_biome_bevy` draws it as an
     ordinary `PlantScatter`. Grass gets a seed. Goldens stay identical.
  3. **P3, World cells and integer placement:** blades sit on tile cells
     fixed to the World, with D2's integer maths, so chunks and
     neighbouring fields agree. Every blade moves once: one golden bless.
  4. **P4, grass in the C3 Ecotope:** a new sibling of the `Scatter` trait
     for Scatters placed on the GPU, which plant implements. Needs the
     coordinator, because it changes `procgena_core`.
  - **Later:** production CPU path, export, promotion, content hash and
    slots, the shared Bevy pattern, shared LOD, slope-following blades,
    shared wind and edits, and streaming by chunk.
- **What the Ecotope can do after each stage:** nothing new after P1.
  After P2, grass under trees, boulders and logs, thinned by coverage, on
  any `Ground`, in the viewer and the studio's Ecotope tab. After P3, the
  same grass whatever the field's extent or split. After P4, grass as a
  first-class Scatter in the C3 Ecotope.
- **Grass has addresses but no copies.** A blade's address is its World
  tile cell plus its tile point (D3), defined in P3 and never
  materialised. Queries stay density-only (D2 decision 5), and edits stay
  area edits.
- **The GPU keeps placing blades** in every stage. The CPU mirror stays
  the oracle, and becomes bit-exact with the GPU in P3.

## Decisions (user, 2026-11-24)

1. **Typed options in the engine-free crate.** Plant's settings types
   (`PlantParams`, `ScatterPlant`, `PlantSegment`, `PlantBranches`,
   `PlantAccent`, `PlantVariation`, `GroundTint`), the preset catalogue
   and the YAML file format move out of Bevy, so the Ecotope can hold,
   validate and edit them like any other kind. Rejected: the Ecotope
   holding only a preset key plus untyped differences.
2. **House layout now.** The engine-free crate takes the name
   `procgena_plant`, the current crate becomes `procgena_plant_bevy`, and
   both live under `plant/crates/`, in one churn window.
3. **The split stays; Reflect comes from an optional feature.** The user
   asked whether each asset could instead be one crate with an optional
   `bevy` feature. What they were after was Reflect on every asset's
   options, not fewer crates. So the core/Bevy split stays (ARCHITECTURE
   rule "Engine-free core, thin Bevy crate"). Each engine-free core,
   plant's first, gets an optional `reflect` feature that derives Bevy's
   `Reflect` on its options through `bevy_reflect` alone. Its Bevy crate
   turns the feature on. Scenes, inspectors and plant's reflection
   round-trip tests keep working. `glam` 0.33 is the version
   `bevy_reflect` 0.20 uses, so glam fields reflect directly.
   `procgena_specimen`'s optional `bevy` feature is the precedent. Adding
   the same feature to tree, stone, deadwood, mushroom and clump is a
   follow-up outside this plan.
4. **Grass gets a sibling trait, not copies.** `Scatter::place` returns
   one `ScatterCopy` per copy, which does not scale to hundreds of blades
   per m². A second trait in `procgena_core` covers Scatters placed on the
   GPU. They bake density and ground over an area, state their reach and
   draw range, give their mask from density, and answer density queries.
   Their addresses are defined but never listed. The Ecotope holds either
   kind (P4).
5. **Seed: interim XOR, one bless.** In P2 a field's seed is
   `corner hash ^ scatter seed`, so at seed 0 every golden is unchanged.
   P3 drops the corner hash along with field-anchored cells, and blesses
   once.
6. **Rotated fields keep turning, blades don't.** In P3 a turned field
   keeps its turned outline, mask and heightmap, but its blades sit on
   World cells: chunks cover the field's bounding box, and a blade is
   kept only inside the turned outline. That is up to about twice the
   chunks at 45°. Rejected: two placement paths, or dropping rotation.
7. **Integer placement lands with World cells (P3).** One rewrite of the
   placement twins and one bless, not two.

## What the Ecotope needs from plant, at minimum

| Need | Where it comes from |
|------|--------------------|
| Settings it can hold, validate, save in `.ecotope.yml` and edit | P1: typed options with serde, `validate()` and files in the engine-free crate |
| A seed from the Ecotope (`scatter_seed(ecotope seed, key)`) | P2: the field seed mixes in the Scatter's seed |
| Heights from any `Ground`, with holes | P2: a height raster baked from `Ground::height` (non-finite is a hole, as today) |
| A slope limit | P2: `max_slope`, applied from `Ground::gradient` into the density raster |
| Coverage, and the footprints of the Scatters above | P2: a density raster from the Ecotope's mask, with the discs above stamped in |
| Its own mask for the Scatters below | Nothing until P4: grass leaves no footprints. In P4, `MaskSource::Density` |
| Blending at Ecotope and region borders | P2: each field thins by its own coverage (below); P3 makes neighbours' blades independent of extent |
| Drawing | P2: `procgena_biome_bevy` spawns a plain `PlantScatter` |

**Blending at borders.** Until the Ecotope border protocol exists, two
Ecotopes meet through their coverage. Where coverage falls from 1 to 0
across a blend zone, and the neighbour's rises from 0 to 1, each Ecotope's
plant Scatter thins by its own coverage. The two fields' blades are
independent (different seeds), so the total density follows the blend, and
the plant mix shifts across the zone. Across region borders this is the
same, since each region seeds its own Scatters. Before P3, a field's
blades depend on its corner, so two fields that overlap draw unrelated
patterns, which is what a blend wants anyway. What P3 adds is that one
Scatter cut into chunks, or into two abutting fields, draws exactly the
blades of the whole.

## Stages

### P1: split into `procgena_plant` and `procgena_plant_bevy`

- **Status (2026-11-24):** built.
  - `PlantScatter` holds `options: PlantScatterOptions`, and colours are
    `procgena_plant::Srgb` (a newtype, so inspectors and the web panel
    still see a colour).
  - Preset assets are the wrappers `PlantPresetAsset` and
    `PlantScatterPresetAsset`, and `PlantSettings::mesh_size` replaces
    `PlantParams::mesh_size`.
  - The studio's Plant Scatter documents hold `PlantScatterOptions`.
  - The 48 golden captures are byte-identical to the pre-split ones on the
    same machine.

- **Covers:** ARCHITECTURE "Plant alignment": the core/Bevy split, data
  half only.
  - The engine-free `procgena_plant` (`plant/crates/procgena_plant`, glam,
    serde, `procgena_core`) takes:
    - the settings types now in `look.rs`;
    - the Scatter's settings as a new `PlantScatterOptions`: everything in
      today's `PlantScatter` except `size`, `ground` and `density_map`;
    - `PlantPreset`, `PlantScatterPreset` and the preset catalogue
      (`presets/`);
    - the YAML reading, writing and checks of `files.rs`;
    - `validate()`, returning errors as every other core does. The Bevy
      side keeps clamping and warning at the render edge;
    - a plain sRGB colour type;
    - the optional `reflect` feature (decision 3).
  - `procgena_plant_bevy` (`plant/crates/procgena_plant_bevy`) keeps:
    - everything else: plugins, the `PlantScatter` component (size,
      ground, density map and a `PlantScatterOptions`), the asset loaders
      and embedded presets, GPU tables, shaders, `shader_model.rs`,
      `mirror.rs`, wind, edits, trampling, far material and stats;
    - `PlantParams::mesh_size`, which needs `PlantSettings` and
      `ShapeClass`, as a function or extension trait;
    - the prelude, re-exporting the moved types by name.
  - Callers move with it: the studio, `plant/web`, the examples,
    `tools/plant`, golden paths, the plant CI workflow and the skill file.
- **Needs:** nothing new. Touches the root `Cargo.toml` and `.github/`,
  which only one agent may edit at a time, so it runs in a churn window.
- **Ecotope after it:** nothing new to draw, but `procgena_biome` can now
  depend on plant's options without Bevy.
- **Done when:**
  - `procgena_plant` builds and tests with no Bevy in its tree (also with
    `--no-default-features`, which the full suite never builds).
  - Every plant test, GPU test and golden passes unchanged, with no
    re-bless.
  - The reflection round-trip tests pass through the `reflect` feature.
  - The studio and the web demo build.
- **Risks:**
  - Size. About 5,000 lines move: `look.rs` about 590, `presets/` about
    2,900, the core half of `files.rs`' 1,300.
  - `Color` appears in every preset constant, so the colour type must be
    usable in a `const`.
  - The `.plant.yml` and `.plant_scatter.yml` files must read the same,
    and `BLESS_PRESETS` must still rewrite them identically.
  - The plant skill's rules (Reflect on public types, prelude, `docs/api.md`
    rows) need rewording for two crates; a human edits the skill.
- **Twins and goldens:** no WESL change; `gpu_layout.rs` reads the moved
  types unchanged. No bless.
- **Files:** `plant/Cargo.toml` (moves), `src/lib.rs`, `look.rs`,
  `api.rs`, `files.rs`, `presets/*`, `settings.rs` (only where it names
  moved types), `docs/api.md`, `docs/presets.md`, `CHANGELOG.md`, the
  skill, `plant/web`, `procgena/` (plant and Plant Scatter tabs), root
  `Cargo.toml`, `.github/workflows/`.

### P2: plants in the early Ecotope

- **Covers:**
  - In `procgena_plant`:
    - `max_slope` on `PlantScatterOptions` (none by default, so presets
      don't change), in the file format;
    - a bake over a `ScatterInput` at a cell size (0.5 m by default) that
      returns a height raster (core's `HeightGrid` where it fits) and an
      R8G8 density raster with the green channel at 1;
    - in the bake, density is the input's mask at each texel centre, 0
      where the slope from `Ground::gradient` passes `max_slope` and where
      the height is not finite, and 0 on every texel a footprint disc
      from `input.exclude` touches.
  - The seed: `FieldGpu.placement.x` becomes
    `corner hash ^ seed`, where the seed is the Scatter's (from the
    Ecotope) or the field's `default_seed` (on its own). Today that line
    is `scatters.rs:1264`, CPU only.
  - In `procgena_biome`:
    - `ScatterKind::Plant(PlantScatterOptions)`;
    - its adapter in `ecotope/scatters.rs`, which bakes and leaves no
      footprints;
    - the `plant` kind in `.ecotope.yml`;
    - a plant Scatter in `pine_forest_floor` (grass or bracken in the
      100–300 band).
  - In `procgena_biome_bevy`:
    - spawning a `PlantScatter` over the area from the bake, with
      `PlantGround::Heightmap` and the density map as images and the
      Ecotope's seed;
    - redrawing it when its settings, seed or footprints above change, as
      for other kinds.
  - In the studio's Ecotope tab, the plant kind with the Plant Scatter
    tab's controls.
- **Needs:** P1. Lands on C3a's adapter, which C3 later replaces (no one
  owns `ecotope/scatters.rs` today).
- **Ecotope after it:** draws grass on any `Ground`. The grass is thinned
  by coverage, absent from the discs and capsules of the trees, boulders
  and logs above it, absent on slopes past `max_slope`, and seeded from
  the Ecotope's seed. Changing the seed moves the blades.
- **Done when:**
  - The pine forest floor draws its trees, boulders, logs, mushrooms and
    grass on rolling ground, in the viewer (headless screenshot) and in
    the studio tab.
  - A test shows that no density texel under an upper footprint is above
    0, and that the slope limit and holes hold.
  - Seed 0 leaves every golden identical, and another seed changes the
    blades (a mirror test).
- **Risks:**
  - Footprints are only as sharp as the texel. The bake zeroes any texel
    a disc touches, so no blade roots inside a trunk. In exchange, a bare
    ring up to one texel wide (0.5 m, and softened by plant's bilinear
    mask read) can show around thin trunks. D2 measured the same
    one-texel softness. Finer texels cost four times the memory per
    halving.
  - One field per plant Scatter per Ecotope area; large areas hit the
    4096-texel limit at 0.5 m past 2 km.
  - `MAX_ENTRIES` (256 entries over all fields, `scatters.rs:47`) is fine
    for one field per Ecotope, but not for streaming (later).
  - The corner part of the seed is interim, and is documented as such.
- **Twins and goldens:** none. The seed already reaches the shader, and
  `mirror::candidate` already takes it. No bless.
- **Files:** `crates/procgena_plant` (options, file, the new bake module),
  `crates/procgena_plant_bevy/src/scatters.rs`, `docs/terrain.md`,
  `docs/scatters.md`, `CHANGELOG.md`;
  `biome/crates/procgena_biome/src/ecotope/{scatters,file,presets}.rs`,
  `procgena_biome_bevy/src/{lib,placing}.rs`, `Cargo.toml` of both;
  `procgena/src/ecotope/`.

### P3: World cells and integer placement

- **Covers:**
  - ARCHITECTURE "World-anchored lattices" and "World-stable ids" for
    plant, and D2's integer contract for grass's candidate source.
  - Chunks sit on a 16 m grid fixed to the World, so they nest in the
    64 m terrain chunk.
  - Tile cells are World cells. A cell's hash comes from its World cell
    coordinates and the Scatter's seed, with no field corner and no chunk
    index.
  - The tile cell size depends only on the Scatter's density option, as
    `ChunkLayout::for_density` with the fixed 16 m chunk.
  - Positions are fixed point (1/256 m, relative to the chunk), the tile
    is quantised to 16 bits per axis (as D2's spike ran it), and ranks are
    compared as integers against the 8-bit mask.
  - Turned fields keep their outline and maps, with blades on World cells
    (decision 6).
  - The address is defined: `CopyAddress { cell: World tile cell, index:
    tile point }`, written down and tested through the mirror, but not
    built as API.
- **Needs:** P2 (the seed path). Independent of C2's code, but follows
  its contract.
- **Ecotope after it:** the same blades whatever the area's extent or how
  it is cut, and the same blades in a field turned or moved by a whole
  cell. Ready for chunked placement.
- **Done when:**
  - A mirror test shows that one field and the same area as two or four
    abutting fields give the same blades.
  - The GPU readback (`cargo test -- --ignored gpu`) matches the mirror
    bit for bit.
  - Changing only coverage or a mask never moves a surviving blade.
  - The goldens are re-blessed after the user accepts the before and
    after crops.
- **Risks:**
  - Every blade moves, which is the one look change of this plan.
  - The density option still sets the cell size, so changing a Scatter's
    density moves every address (D3 accepts that, and saved addresses
    carry the cell size).
  - Turned fields pay for up to about twice the chunks.
  - Integer rank tests may cost GPU time: measure with
    `tools/plant/bench_ab.py` against the keep bar the user sets.
- **Twins and goldens:** `mirror::candidate` and `keep_fraction` with
  `generate.wesl`'s `evaluate`; `candidates_needed` with the cull pass;
  the tile in `shader_model.rs` with its WESL copy; `FieldGpu`'s
  placement words with `common.wesl`'s `Field` and `Chunk`; the chunk
  table build in `scatters.rs`. One bless.
- **Files:** `crates/procgena_plant_bevy/src/{shader_model,mirror,scatters,gpu_layout}.rs`,
  `shaders/{common,generate}.wesl`, `render/` (chunk ids), `trample.rs`
  and `debug.rs` (chunk lookups), `docs/design.md`, `docs/performance.md`
  (the bench pass), `CHANGELOG.md`, `tests/golden/`.

### P4: grass in the C3 Ecotope

- **Covers:**
  - The sibling trait in `procgena_core::scatter` (decision 4). It bakes
    density and ground over an area or chunk, gives `reach` (S = 0, J =
    the tuft pull, F = 0), `draw_range` (the plant radius), mask source
    `Density`, a density query at a point, and the address scheme.
  - Plant implements it in `procgena_plant`.
  - The C3 Ecotope holds `dyn Scatter` or the new kind, and runs both in
    one priority order.
- **Needs:** P3, C2, C3, and the coordinator's agreement, since a
  `procgena_core` change rebuilds the workspace.
- **Ecotope after it:** plant is a Scatter like the others in the full
  Ecotope; the C3a plant adapter goes. The C3 warning about a dense Scatter
  above a long-range one sees plant's draw range.
- **Done when:** C3's "pine forest floor" done-when holds with grass
  through the trait, and the trait has conformance checks: chunks match
  the area, and density queries match the bake.
- **Risks:** the trait's shape is new design. A plant Scatter above
  another, rare in practice, gives the ones below no footprints, only its
  density mask.
- **Twins and goldens:** none expected.
- **Files:** `core/crates/procgena_core/src/scatter/`,
  `crates/procgena_plant`, `biome/crates/procgena_biome/src/ecotope*`,
  `procgena_biome_bevy`.

### Later: the rest of R3 for plant

Each waits for a need, in roughly this order:

- **Streaming by chunk:** a field per World chunk, or one table spanning
  chunks; lift or rethink `MAX_ENTRIES`. With S1.
- **Content hash and slots:** `ContentHash` for the options and a `PoolId`
  per mix entry, if the sibling trait or impostor-free far forms need
  them.
- **Production CPU path:** promote `mirror.rs` to a real CPU placement and
  blade geometry producing a tagged `Specimen` (rule 1, rule 8).
- **Export:** OBJ and glTF of a patch from the CPU path (rule 11).
- **Queries and promotion:** density-only queries in the core first; per
  blade only if a game asks (rule 7, R4).
- **Slope-following blades:** slope as a texture from the tile's gradient
  grid (ARCHITECTURE "Ground carries slope").
- **The shared Bevy pattern:** `XParams`, `XGenerating`, `XFinished` and
  `XPart`, where they fit an asset built on the GPU (rule 12).
- **Shared LOD and quality:** `Quality` in place of `PlantQuality`, and the
  far form named in the shared `Lod` (rule 2).
- **Shared wind, edits and interaction:** rules 9 and 14.

## Open questions

1. **Bake cell size.** 0.5 m matches C2's footprint masks. Should a plant
   Scatter choose it, for example 0.25 m under dense trunks?
2. **The sibling trait's name and exact methods** (P4), settled with the
   coordinator when C3 starts.
3. **An explicit tile cell option.** Should the tile cell size be its own
   frozen setting, so that changing density no longer moves addresses? Or
   should it stay derived from density, as D3 accepts?
4. **Reflect feature for the other cores** (decision 3's follow-up): when,
   and whether the feature is named `reflect` or `bevy`.
