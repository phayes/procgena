# Ground materials: Ecotope-owned, driven by terrain measures and labels

Status: **planned with the user on 2026-11-24.** The four choices under
"Decisions" are the user's. Everything marked "proposed, not decided"
is a recommendation still waiting for the user (see "Open questions").
No code was changed or run.

Written from ARCHITECTURE.md ("3. Ecotope", "4. Biome and Terrain",
"Loading by chunk", "Masks and placement", "Seeds", "Extensibility",
"Asset alignment" rule 13), ROADMAP.md (G4, G6, C3a, C3, B1, W1, S1, R3,
IX, "For the user"), [terrain/plan.md](../../terrain/plan.md) ("Classification
and labels", "Bevy design", Phases 5, 8 and 10),
[terrain/features_plan.md](../../terrain/features_plan.md) and its
[review](../../terrain/features_plan_review.md), [D5](D5.md),
`procgena_biome/src/ecotope*`, `plant/src/far.rs`,
`plant/docs/terrain.md`, and the G6 branch (`claude/terrain-g6-0c9714`:
`procgena_terrain::chunk`, `procgena_terrain_bevy`'s `material.rs`,
`terrain_fragment.wesl`, `lib.rs`, `stream.rs`), which had not merged.

## Bottom line

- **Three tiers own the ground's look** (decided).
  1. **The terrain crates** hold the World-wide **material catalogue**
     (keys, colours, texture sets, tiling) and a **default look for bare
     terrain**: today's slope, height and snow colours, rewritten as
     ordinary ground layers. The terrain viewer, the Terrain tab and empty
     Biomes use it.
  2. **Each Biome** has a **base ground**: soil, road surface, riverbed,
     cliff rock, beach, snow. So a desert road and a forest road differ.
  3. **Each Ecotope** adds **ground layers** on top (needle litter, moss,
     mud, bare dirt), weighted by its coverage, including layers shaped by
     its own Scatters (litter under trees, bare ground around boulders).

  This replaces terrain/plan.md's label-chosen texture sets (Phases 8 and
  10) and its `terrain/textures.yml` becomes the catalogue's texture list.
- **A layer is a material plus a rule.** The rule multiplies factors over
  a small **site** (slope, height, height above water, distance to water
  and roads, later labels), noise, and Scatter influence. Layers stack in
  `order` on one World-wide scale, like Scatter `priority`.
- **The CPU evaluator is canonical; textures are a cache** (decided:
  CPU weight textures). `ground_at(p)` is a pure function of position,
  for queries ("what is underfoot") and servers with no camera. For
  drawing, it fills a **weight texture per terrain chunk** on the shared
  grid: up to 8 materials per chunk, 129 × 129 texels at every level,
  texels on the chunk's vertices, so neighbouring chunks agree at their
  edge. Far chunks store a premixed colour instead. Textures are drawing
  only: they may be provisional (roads not routed yet) and coarse.
- **Scatters shape the ground; the ground never feeds placement**
  (proposed, not decided). Beyond a Scatter's place range, and at coarse
  chunk levels, its influence comes from its density field, not its
  copies.
- **Measures before labels** (decided: split G4). G4a adds water
  distance, road distance, height above water and curvature to
  `Terrain::query`; ground rules use them first. G4b adds `Landform` and
  `Flags`, and rules gain label conditions then. Before G4a, rules use
  slope and height from `Ground` and the water level from `query`.
- **Colours first, textures after** (decided). First the engine-free
  model, evaluator and image dumps (GM0, needs nothing). Then flat colours
  in the terrain viewer and the Ecotope tab once G6 merges (GM1). Then real
  texture sets (albedo, normal, packed roughness), the shared texture
  loader, triplanar on steep ground and a far colour (GM2).
- **Borders ride on Ecotope coverage**, not a protocol of their own. The
  ground of a blend zone is the coverage-weighted mix of each side's
  result. It works whether Ecotopes overlap by weight or one wins (open
  question 2 in ARCHITECTURE.md stays open).
- **Far grass and far forest are two more distance-gated tints** in the
  same chunk textures: plant's far ground colour beyond the grass radius
  (after plant joins the Ecotope, R3), and the forest tint beyond the last
  impostor (S1's TODO, already decided). The terrain shader calls plant's
  `plant_far_color` rather than stacking a second material extension.
- **Scale (D5):** texture tiling, chunk texels, and distances to roads and
  water never scale. Heights in rules scale with the World's `vertical`
  (proposed, not decided). Noise patch sizes stay open with D5 question 5.

## The design

### Materials and the catalogue

A **ground material** is World-wide data, named by a frozen key
(`needle_litter`, `moss`, `forest_soil`, `gravel_road`, `riverbed`,
`cliff_rock`, `mud`, `snow`). Two Ecotopes that use `moss` share one
texture slot, and blending `moss` into `moss` across a border is the
identity.

| Field | Meaning |
|-------|---------|
| `key` | Frozen key, as for Scatters (`ChildKey`). |
| `colour` | Linear RGB. The whole look in the colour stage; the far colour later. |
| `roughness` | Used when there's no texture. |
| `texture` | Optional texture set id in the catalogue's texture list (GM2). |
| `tile` | Metres per texture repeat. Never scales with the World. |
| `projection` | `top` (world XZ) or `triplanar` (rock on steep ground). |
| `blend_height` | Optional: use the height map to sharpen transitions (GM2). |

The catalogue lives in the engine-free terrain crate
(`procgena_terrain::ground`), with presets and `.ground_material.yml`
files in the F3 pattern. Its textures are listed in
`terrain/textures.yml` and fetched with the existing
`uv run --project tools textures` tool. The terrain crate holds it because
it sits below the Biome: `procgena_terrain_bevy` can't see Biome or
Ecotope types (`procgena_biome` imports `procgena_terrain`), and it must
draw bare terrain on its own.

### Layers and rules

```text
GroundLayer {
    material: key,            // in the catalogue
    order: u32,               // one World-wide scale, like Scatter priority
    opacity: 0..1,
    when: [Condition],        // all multiplied together
    noise: Option<Noise>,     // wavelength, contrast, threshold; seeded per layer
    under: [ScatterInfluence] // Ecotope layers only
}
Condition = Slope(ramp) | Height(ramp) | HeightAboveWater(ramp)
          | WaterDistance(ramp) | RoadDistance(ramp) | RoadTag(set)
          | Curvature(ramp)                  // after G4a
          | Landform(set) | Flags(any, none) // after G4b
ramp = soft band: [in_start, in_full, out_full, out_end]
```

- A layer's weight at a point is `opacity × Π conditions × noise ×
  influence`. Conditions are named after G4's `Measures` and `Flags`, so
  labels slot in by adding variants; nothing else moves.
- **Layers are a map by frozen key** with an `order` number, as Scatters
  are, so a `.ecotope.yml` differences file changes or adds one layer
  without restating the others, and `null` removes one. Two layers of one
  stack with the same `order` are a validation error.
- **Order bands** (proposed): 0–299 soils and bare ground, 300–599 litter
  and moss, 600–899 terrain surfaces that sit on everything (road, riverbed,
  cliff rock, beach), 900 and up weather (snow). So the Biome's road stays
  above an Ecotope's needles, while the Ecotope's moss covers the Biome's
  soil.
- **Compositing** is "over", bottom to top: each layer takes its weight
  `w` of what remains, `result = result × (1 − w) + material × w`, kept
  as per-material weights that sum to 1. The bottom layer of every stack
  is opaque (the Biome's soil, or the bare-terrain default), so there are
  no holes.
- **The site** is what a rule reads:

  ```text
  GroundSite { height, slope, water_level: Option, height_above_water: Option,
               water_distance: Option, road_distance: Option, road_tags,
               curvature: Option, labels: Option<(Landform, Flags)> }
  ```

  The Ecotope stays Terrain-unaware: it gets a site, filled by whoever
  holds the ground. Over a `Ground` alone (the Ecotope tab) only height and
  slope are set; the Biome fills the rest from `Terrain::query`. A
  condition whose measure is missing counts as 1 (no effect), and the
  Ecotope tab says which conditions it couldn't evaluate.
- **Seeds:** a layer's noise is seeded `child_seed(parent seed,
  "ecotope.child.ground", key)` for Ecotope layers and `"biome.child.ground"`
  for Biome base layers, under the chain World → region's Biome → Ecotope
  (see "Seeds"). Same-type neighbours differ by region seed, and the blend
  zone hides the change, as it does for Scatters.

### Scatter influence

An Ecotope layer can follow its own Scatters:

```text
ScatterInfluence { scatter: key, reach: Reach, falloff: ramp, invert: bool }
Reach = Metres(f32) | Footprint(f32)   // multiple of the copy's footprint radius
```

- "Needle litter within 1.5 × the pine's canopy radius" and "bare dirt
  ring 0.5 m around boulders" are influences.
- **Near, from copies:** the placed copies' exact footprints (C2's
  `Footprints`), so litter sits under the tree that is drawn there.
- **Far, from the density field:** beyond the Scatter's place range, or
  at chunk levels coarser than the influence's reach, the influence is the
  Scatter's density field times its coverage, not its copies. Canopy shade
  from the density field (`MaskSource::Density`) is still a TODO; until it
  exists the far influence is the Scatter's density option times coverage.
  The look may change slightly between the two; that is drawing, not
  placement.
- **One direction** (proposed, not decided): ground layers never feed a
  Scatter's density or mask. The same `Condition` types are offered to
  Scatter density rules, so "mud within 3 m of water" and "no grass within
  3 m of water" read the same numbers without a loop.
- **Ordering:** a chunk's ground weights are built after the Ecotope's
  placement for that chunk (plan, place, then draw), plus the largest
  influence reach as a margin.
- **Edits:** influence reads copies after edits, so a felled tree's litter
  goes with it. Ground layers in edits (a camp's packed earth on a stamp
  or reserve) are later work for X1.

### Who composes what

At a point `p` with the Biome's base stack `B` and Ecotopes `i` with
coverage `c_i` (with `Σ c_i ≤ 1`):

```text
ground(p) = Σ c_i · over(B ∪ E_i)  +  (1 − Σ c_i) · over(B)
```

`B ∪ E_i` is the Biome's and the Ecotope's layers merged by `order`.

- **Gaps between Ecotopes** show the Biome's base ground.
- **Overlap:** if Ecotopes split ground by weight, the sum mixes them; if
  one wins, its `c` is 1. Both answers to open question 2 work unchanged.
- **Across a Biome border**, the Ecotope border agreement already fades
  each side's Ecotope coverage; the Biome base stacks fade by the same
  agreed weight. No second protocol. Before the protocol exists (B1, W1),
  the interim is a crossfade over a fixed width at the region border, with
  noise, from each point's distance to the border.
- **Bare terrain** (no Biome, or an empty Biome) uses the terrain's
  default stack: grass-coloured soil, rock on steep ground, snow up high,
  as G6 colours today.
- The Biome implements an engine-free `GroundPainter` trait from
  `procgena_terrain::ground` (sites in, chunk weights out); the default
  stack is the terrain's own painter. So `procgena_terrain_bevy` draws any
  painter without knowing Biomes.

### Chunk weight textures

- **Grid:** the shared chunk grid (64 m at level 0, doubling per level).
  Each chunk gets 129 × 129 texels, one on every half cell of its 64-cell
  mesh, the outer ones exactly on the chunk's edge. Texel size: 0.5 m at
  level 0, 1 m at level 1, 2 m at level 2, and so on.
- **Near chunks (texture levels):** a per-chunk palette of up to 8
  materials and two RGBA8 weight maps (8 bytes per texel, about 133 KB per
  chunk). Hardware filtering works, since weights are dense. The shader
  samples the 8 weights and textures only those above a small threshold,
  at most the 4 strongest per pixel.
- **Seams:** neighbouring chunks evaluate the same pure function at the
  same edge points, so their edge texels are equal whatever each one's
  palette is. Across a level ring the coarser chunk interpolates over two
  of the finer chunk's texels; the finer chunk's edge row is snapped to that
  interpolation when its side has a coarser neighbour (the transitions the
  mesh already knows), as the mesh does with its vertices.
- **More than 8 materials in one chunk** (a triple junction of three
  Biomes, each with a base stack and an Ecotope): the evaluator keeps the
  8 with the most total weight over the chunk and gives each dropped
  material's weight to the kept material nearest in colour, deterministically,
  and counts it in stats. The cap is sized for that worst case: 3 base
  stacks of about 4 and 3 Ecotope stacks of about 3 rarely put more than 8
  into one 64 m chunk.
- **Far chunks** (levels at or beyond the texture distance, proposed:
  level 3, 512 m chunks): an RGBA8 premixed colour and roughness, plus
  the far tints below. No palette, so no cap.
- **Far tints** (every level where they apply): two RGBA8 maps, the far
  grass tint (colour and amount, from the plant Scatters' far colours and
  densities) and the far forest tint (canopy colour and amount, from the
  tree Scatters' coverage). The shader gates each by distance: grass beyond
  the grass radius, forest beyond the last impostor.
- **Cost:** evaluation runs in the chunk build task, on the CPU with rayon,
  after the chunk's placement. GM1 measures it; a GPU port of the same
  rules (integer-free, plain arithmetic, like D2's kernels) is GM6, only if
  needed.
- **Memory (arithmetic, not measured):** a view of about 30 chunks at each
  of levels 0 to 2 (90 near chunks × 133 KB ≈ 12 MB) and about 60 far
  chunks (2 tint maps + colour, 3 × 66 KB ≈ 200 KB each, ≈ 12 MB) is about
  24 MB of weight textures. The chunk counts are a guess to replace with
  G6's real selection counts.

### The terrain material (`procgena_terrain_bevy`)

- **One material per chunk** with the chunk's weight textures and palette
  indices. Today G6 has one material per level; about 150 materials in a
  view is fine for Bevy. A bindless texture array of chunk maps is a later
  option.
- **Colour stage (GM1):** albedo = Σ weight × material colour, with the
  small two-scale variation G6 already has; roughness the same way.
- **Texture stage (GM2):**
  - Material textures in three texture arrays (albedo BC7, normal BC5,
    packed occlusion, roughness and height BC7), one slice per resident
    material.
  - `top` projection samples world XZ at the material's `tile`; `triplanar`
    samples three planes only where the slope passes the layer's threshold
    (branch on slope), so flat ground pays for one sample per material.
    The triplanar code is clump's and stone's, moved into
    `procgena_core_bevy` as a shared WESL module.
  - Height-blended transitions where a material gives `blend_height`.
  - Beyond the texture distance each material is its mean colour (taken
    from its smallest mip at load), and far chunks are premixed, so far
    ground costs no texture samples.
- **Plant's far grass:** the terrain shader imports
  `procgena_plant::shaders::far::plant_far_color` and feeds it the far
  grass tint, instead of stacking `FarPlants` (both bind at 100). Plant's
  own `FarPlantMaterial` stays for users without procgena's terrain.
- **Debug:** a view of each chunk's palette and a layer overlay, beside
  G6's level tints and chunk borders.

### Textures and memory

- **Shared loader:** `procgena_core_bevy` gains the one texture loader of
  rule 13 (TODO "Shared plumbing"), from stone's `TextureSize` and
  `TextureCompression`; ground is its first user that loads into arrays.
- **Budget arithmetic:** one material is three maps at 1 byte per texel
  (BC7, BC5) with mips (× 4/3): 1024² × 3 × 4/3 ≈ 4 MB; 2048² ≈ 16 MB;
  512² ≈ 1 MB. Proposed, not decided: 1024 by default, 2048 at High and
  Ultra, at most 24 resident materials (≈ 96 MB at 1K, ≈ 384 MB at 2K),
  with a validation warning when the Biomes in view name more. Which
  machines set the budget is ROADMAP item 18.
- Texture sets are CC0 only, as in every other `textures.yml`.

### Scale (D5)

| Length | Scales? |
|--------|---------|
| Texture `tile`, chunk texels, the chunk grid | Never |
| Distances to roads and water, road tags | Never (widths are fixed) |
| Slope thresholds | Not a length; unchanged |
| Heights in rules (a snow line) | Proposed: written at scale 1, × `vertical` |
| Noise wavelengths | Open, with D5 question 5 |
| Scatter influence reach | Never (object scale) |

The chunk cost is the same in every scale class, since texels are fixed
metres.

## Stages

| Stage | Covers | Needs | Done when |
|-------|--------|-------|-----------|
| **GM0** Engine-free model | `procgena_terrain::ground`: catalogue (colours), layers, conditions, compositing, site, `GroundPainter`, chunk weights (palette, cap, edge snapping), the default bare-terrain stack. `procgena_biome`: Ecotope `ground` map in `.ecotope.yml`, Scatter influence from footprints, `Ecotope::ground_at`. Image dumps. | Nothing (C3a is built). | A top-down image of the pine forest floor shows litter under pines and dirt round boulders; tests show chunk edges equal from both sides, results independent of build order, file merge and `null` removal, and the default stack matching G6's colours. |
| **GM1** Colours drawn | `procgena_terrain_bevy` draws any painter's chunk weights in colour (replacing `TerrainSurface`'s fixed rules); the Ecotope tab draws its ground layers; the Terrain tab shows the default stack; evaluation cost measured. | GM0, G6 merged. | The terrain viewer and Ecotope tab show ground colours, headless screenshots recorded, and the build cost per chunk is in the report. |
| **GM2** Textures | Shared texture loader in `procgena_core_bevy`, ground texture arrays, `terrain/textures.yml` as the catalogue's list, triplanar on steep ground, height blend, far colour, budgets and warnings. | GM1. | The pine forest floor and a cliff draw with real textures; memory matches the arithmetic; the far ground costs no texture samples. |
| **GM3** Measures and labels | Water and road distance, height above water and curvature conditions (G4a); `Landform` and `Flags` conditions (G4b). | G4a, then G4b. | Mud follows river banks and gravel lines roads in an image dump; a cliff label picks cliff rock. |
| **GM4** Biome base and borders | Biome base ground in `.biome.yml`; the painter over Ecotope coverage; the interim border crossfade, then the Ecotope border agreement (B1 inside a Biome, W1 across Biomes). | B1, then W1. | A Biome shows its road and riverbed under two Ecotopes, with no seam at an Ecotope or Biome border. |
| **GM5** Far tints | Far grass via `plant_far_color` (once plant sits in an Ecotope); far forest tint (S1's TODO). | R3 (plant in the Ecotope), density-field masks, S1. | Grass and forest keep their colour past the last blade and the last impostor, with no seam. |
| **GM6** GPU evaluation | The rules in WGSL with CPU parity tests. | GM1's measurements calling for it. | Parity within one 8-bit step; chunk build time recorded. |

GM0 can start now, in `procgena_terrain` (a new `ground` module) and
`procgena_biome` (the Ecotope's ground), neither of which a running lane
owns. It does not touch `procgena_terrain_bevy`, which G6 still owns.

## Decisions

The user, 2026-11-24:

1. **Ownership: three tiers.** The terrain crates hold the material
   catalogue and the bare-terrain look; each Biome has a base ground (soil,
   road, riverbed, cliff rock, snow); each Ecotope adds layers on top,
   weighted by coverage. This supersedes terrain/plan.md Phases 8 and 10's
   label-chosen texture sets.
2. **First stage: the model, then colours.** The engine-free model,
   evaluator and image dumps first; colours in the terrain viewer and
   Ecotope tab once G6 merges; real textures with the shared loader after.
3. **Evaluation: CPU weight textures.** The CPU evaluator fills a weight
   texture per chunk (up to 8 materials, 129 × 129 texels); far chunks
   store a premixed colour; a GPU port only if it's too slow.
4. **Split G4.** G4a, Measures on `Terrain::query`, lands first; G4b,
   `Landform` and `Flags`, later.

## Open questions

Proposed answers are recommendations, not decisions. The user hasn't seen
the first three yet; the question that asked them was interrupted.

1. **Can ground feed placement?** Proposed: no. Scatters shape the ground;
   the ground never feeds a Scatter. Scatter density rules get the same
   condition types instead. The alternative lets a Scatter read the
   ground computed from layers and Scatters above it, which ties ground
   evaluation into the placement order and chunk margins.
2. **Heights under World scale.** Proposed: heights in rules are written
   at scale 1 and multiplied by `vertical`, like Terrain presets. The
   alternative is heights relative to the region's relief (0 valley floor,
   1 highest peak), which never need scaling but move with every Terrain.
3. **Texture budget.** Proposed: 1024 px by default, 2048 at High and
   Ultra, at most 24 resident materials (≈ 96 MB and ≈ 384 MB). Waits on
   which machines count (ROADMAP item 18).
4. **Noise patch sizes under scale.** With D5 question 5 (Ecotope
   geometry), decided in C3 and B1.
5. **Order bands.** The numbers proposed above (soils, litter, terrain
   surfaces, weather) are a starting point, to settle with the first Biome.
6. **May an Ecotope switch off or replace a Biome base layer** (no snow
   under a dense canopy)? Not needed for GM0; settle in GM4.
7. **Texture distance and the 8-material cap.** Level 3 and 8 are
   starting points; GM1 and GM2 measure them on the triple-junction scene.

## What changes where

- **ROADMAP:** milestones GM0 to GM6; G4 split into G4a and G4b; graph
  nodes and waves; a "For the user" item for the open questions.
- **ARCHITECTURE:** a pointer in "3. Ecotope" (what it owns).
- **terrain/plan.md:** Phase 8's material and Phase 10's
  `terrain/textures.yml` follow this document. Not edited here, because
  G6 is editing that plan; to be noted when G6 merges.
