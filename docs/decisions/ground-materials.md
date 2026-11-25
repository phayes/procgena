# Ground materials: Ecotope-owned, driven by terrain measures and labels

Status: **planned with the user on 2026-11-24; GM0 built the same day**
(see "As built"). The choices under "Decisions" are the user's. Anything
still marked "proposed, not decided" is a recommendation waiting for the
user (see "Open questions").

Written from ARCHITECTURE.md ("3. Ecotope", "4. Biome and Terrain",
"Loading by chunk", "Masks and placement", "Seeds", "Extensibility",
"Asset alignment" rule 13), ROADMAP.md (G4, G6, C3a, C3, B1, W1, S1, R3,
IX, "For the user"), [terrain/plan.md](../../terrain/plan.md) ("Classification
and labels", "Bevy design", Phases 5, 8 and 10),
[terrain/features_plan.md](../../terrain/features_plan.md) and its
[review](../../terrain/features_plan_review.md), [D5](D5.md),
`procgena_biome/src/ecotope*`, `plant/crates/procgena_plant_bevy/src/far.rs`,
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
  (decided). Beyond a Scatter's place range, and at coarse
  chunk levels, its influence comes from its density field, not its
  copies.
- **Measures before labels** (decided: split G4). G4a adds water
  distance, road distance, height above water and curvature to
  `Terrain::query`; ground rules use them first. G4b adds `Landform` and
  `Flags`, and rules gain label conditions then. Before G4a, rules use
  slope and height from `Ground` and the water level from `query`.
  G4b is built (2026-11-25): `Terrain::annotate` and
  `query(p, SampleDetail::Labels)` give `Labels { landform, flags }`
  (see terrain/plan.md, "Labels as built"); `GroundSite` doesn't carry
  them yet, which is GM3's step.
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
  impostor (S1's TODO, already decided). Where plant's far colour
  function lives, so the terrain crate needn't depend on plant, is open
  with the plant-align lane.
- **Scale (D5):** texture tiling, chunk texels, and distances to roads and
  water never scale. A rule's height is either absolute, scaling with
  the World's `vertical`, or relative to the region's relief, which never
  scales (decided: an enum with both). Noise patch sizes stay open with D5 question 5.

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
- **Ties:** in a merged stack, a Biome layer and an Ecotope layer with the
  same `order` put the Ecotope's layer on top (proposed).
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
- **One direction** (decided): ground layers never feed a
  Scatter's density or mask. The same `Condition` types are offered to
  Scatter density rules, so "mud within 3 m of water" and "no grass within
  3 m of water" read the same numbers without a loop.
- **Ordering:** a chunk's ground weights are built after the Ecotope's
  placement for that chunk (plan, place, then draw), plus the largest
  influence reach as a margin.
- **Query cost:** `ground_at` with copy influence must place the Scatters
  around the point, so it is an expensive query in the sense of "Queries"
  (cached per chunk like `resolve`). The cheap form, `ground_at` with
  density-field influence, needs no placement, answers anywhere, and is
  what a server with no camera uses by default.
- **Edits** (proposed, not decided): influence reads copies after edits,
  so a felled tree's litter goes with it. Ground layers in edits (a camp's packed earth on a stamp
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
  as G6 colours today, plus a road surface from `road_tags` and a riverbed
  under the water level. Bare terrain has roads and rivers, so it should
  look like it.
- **Terrain features with meshes** (rock outcrops, later bridges) carry
  their own material from the catalogue, named in the feature's options
  (`cliff_rock` by default), and their footprint gives the ground under
  and around them a "surface hint" (features plan, "Landform": surface
  material hints) that a Biome base layer can read as a condition once
  G4b exposes it. The outcrop mesh is drawn with the terrain material's
  triplanar path at full weight of its one material.
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
- **Seams at one level:** neighbouring chunks of one level evaluate the
  same pure function at the same edge points, so their edge texels are
  equal whatever each one's palette is, as long as neither chunk hit the
  8-material cap (below).
- **Seams across a level ring: open, for GM1.** The coarser chunk
  interpolates across two of the finer chunk's edge texels, so colours can
  step slightly there. G6's `Transitions` are known to the coarser chunk
  (which takes the finer chunk's vertices), not to the finer one, and
  chunk samples don't depend on transitions; so the weight textures should
  not either. Candidates: accept the step (level changes are already
  dithered), or let the coarser chunk's shader read its finer
  neighbour's edge. GM1 looks at it in screenshots.
- **More than 8 materials in one chunk** (a triple junction of three
  Biomes, each with a base stack and an Ecotope): the evaluator keeps the
  8 with the most total weight over the chunk and gives each dropped
  material's weight to the kept material nearest in colour, deterministically,
  and counts it in stats. Two neighbours can drop different materials, so
  edges match only between chunks under the cap; `ground_at` itself is
  never capped. The cap is sized for that worst case: 3 base stacks of
  about 4 and 3 Ecotope stacks of about 3 rarely put more than 8 into one
  64 m chunk.
- **Limits:** weights are laid top-down, so on near-vertical cliffs a
  texel covers a tall strip of face (triplanar projection hides the
  stretching of the texture, not the coarseness of the weights), and
  overhang undersides and caves get no weights of their own. Volume
  patches (G6, Phase 6) need their own answer when they come.
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
- **Plant's far grass: open, with the plant-align lane.** The terrain
  material can't stack plant's `FarPlants` extension (both bind at 100),
  and importing `procgena_plant`'s `plant_far_color` into
  `procgena_terrain_bevy` would make the ground stack depend on the cover
  stack. So either the function moves to `procgena_core_bevy`, or the
  terrain material takes a shader hook that plant fills. Plant's own
  `FarPlantMaterial` stays for users without procgena's terrain.
- **Debug:** a view of each chunk's palette and a layer overlay, beside
  G6's level tints and chunk borders.

### Textures and memory

- **Shared loader:** `procgena_core_bevy` gains the one texture loader of
  rule 13 (TODO "Shared plumbing"), from stone's `TextureSize` and
  `TextureCompression`; ground is its first user that loads into arrays.
- **Budget arithmetic:** one material is three maps at 1 byte per texel
  (BC7, BC5) with mips (× 4/3): 1024² × 3 × 4/3 ≈ 4 MB; 2048² ≈ 16 MB;
  512² ≈ 1 MB; 128² ≈ 0.07 MB. The first proposal (1024 by default) was
  rejected by the user as far too high: defaults go well down, a 128 px
  version of every material is baked too, and full resolution is an
  opt-in level (decision 7). Exact sizes and the resident cap, with a
  validation warning when the Biomes in view name more, are set in GM2
  with the machines from ROADMAP item 18.
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
| **GM0** Engine-free model | `procgena_terrain::ground`: catalogue (colours), layers, conditions, compositing, site, `GroundPainter`, chunk weights on G6's `ChunkKey` (palette and cap), the default bare-terrain stack with roads and riverbeds. `procgena_biome`: Ecotope `ground` map in `.ecotope.yml`, Scatter influence from footprints, `Ecotope::ground_at`. Image dumps. | G6's engine-free chunks (merged into master 2026-11-24). Starts on the proposed answers to open questions 1 and 2 unless the user decides otherwise. | A top-down image of the pine forest floor shows litter under pines and dirt round boulders; tests show equal edges between same-level chunks under the cap, results independent of build order, file merge and `null` removal, and the default stack matching G6's colours. |
| **GM1** Colours drawn | `procgena_terrain_bevy` draws any painter's chunk weights in colour (replacing `TerrainSurface`'s fixed rules); the Terrain tab shows the default stack; the Ecotope tab draws its ground layers on its `HeightGrid` or flat patch as one weight texture over the patch, built by `procgena_biome_bevy` from `Ecotope::ground_at` and drawn with `procgena_terrain_bevy`'s material (`procgena_biome_bevy` gains that dependency); the level-ring seam is looked at; evaluation cost measured. | GM0 (G6 is merged). | The terrain viewer and Ecotope tab show ground colours, headless screenshots recorded, and the build cost per chunk is in the report. |
| **GM2** Textures | Shared texture loader in `procgena_core_bevy`, ground texture arrays, `terrain/textures.yml` as the catalogue's list, triplanar on steep ground, height blend, far colour, budgets and warnings. | GM1. | The pine forest floor and a cliff draw with real textures; memory matches the arithmetic; the far ground costs no texture samples. |
| **GM3** Measures, labels and the Biome base | Water and road distance, height above water and curvature conditions (G4a); `Landform` and `Flags` conditions (G4b). The Biome's base ground on empty Biomes (B0), so a Biome's roads and banks differ from bare terrain's. | GM1, G4a, then G4b for labels. | Mud follows river banks and gravel lines roads on a Terrain in an image dump; an empty Biome's base ground replaces the default stack; a cliff label picks cliff rock. |
| **GM4** Coverage and borders | The painter over Ecotope coverage; the interim border crossfade, then the Ecotope border agreement (B1 inside a Biome, W1 across Biomes). | GM3, B1, then W1. | A Biome shows its road and riverbed under two Ecotopes, with no seam at an Ecotope or Biome border. |
| **GM5** Far tints | Far grass (once plant sits in an Ecotope; where `plant_far_color` lives is settled with the plant-align lane); far forest tint (S1's TODO). | R3 (plant in the Ecotope), density-field masks, S1. | Grass and forest keep their colour past the last blade and the last impostor, with no seam. |
| **GM6** GPU evaluation | The rules in WGSL with CPU parity tests. | GM1's measurements calling for it. | Parity within one 8-bit step; chunk build time recorded. |

GM0 can start now, in `procgena_terrain` (a new `ground` module) and
`procgena_biome` (the Ecotope's ground). It builds on G6's `ChunkKey`,
which is on master since G6 merged (2026-11-24); this plan was written
on a branch from before the merge and read G6 from its branch.

## As built: GM0 (2026-11-24)

**Terrain crate** (`procgena_terrain::ground`, tests in
`tests/ground_layers.rs`, image dump `--example ground_dump`):

- `MaterialCatalogue` maps keys to `GroundMaterial { colour, roughness }`;
  `standard()` holds `grass`, `cliff_rock`, `snow`, `gravel_road`,
  `riverbed`, `bare_dirt`, `forest_soil`, `needle_litter`, `moss` and
  `mud`. A `MaterialId` is a material's index in key order. Texture
  fields wait for GM2; material presets and `.ground_material.yml`
  files are not built yet and no stage lists them.
- `GroundLayer { material, order, opacity, when, noise, under }`. A
  `Ramp` is `{ rise: [a, b], fall: [c, d] }`, both optional smoothsteps,
  rather than four numbers. Conditions built: `slope` (the sine of the
  angle, as G6's shader), `height` as the decided enum
  (`absolute`, divided by the World's `vertical` before the band, or
  `relief`, 0 to 1), `height_above_water` and `road_tags`. Noise is
  two octaves of the terrain's gradient noise with `wavelength`,
  `contrast` and `threshold`.
- `GroundSite { height, slope, relief_height, water, road_tags }`.
  **Changed from the plan:** "a missing measure counts as 1" would paint
  riverbed and road over every plain `Ground`, so a site over a plain
  `Ground` says "no water, no roads" (measured, none), and only truly
  unknown measures (today the relief height, which no source fills yet)
  count as 1. `GroundSource` reads sites from a `Terrain`, a
  `WorldTerrain` or any `Ground` (`OnGround`).
- `GroundStack` is a complete stack (its lowest layer must be opaque;
  no Scatter influences); `GroundOverlay` holds layers laid in among a
  stack's, Ecotope on top at equal orders, and layers below the stack's
  lowest are hidden. Compositing is "over" into a `GroundMix` of weights
  adding to 1. Duplicate orders, shared salts and unknown materials are
  errors.
- `GroundStack::bare(seed, vertical)` is the bare-terrain look:
  `soil`/grass 0, `riverbed` 610 (under water), `road_banks`/bare dirt 620
  (cut and embankment tags), `rock`/cliff rock 650, `road`/gravel 680,
  `snow` 900. Without water and roads it equals G6's colours to 1e-5 (the
  shader's patchy variation stays in the shader). Seed kind
  `terrain.child.ground`.
- `GroundPainter` (`catalogue`, `paint(p, site)`) and
  `ChunkWeights::build(painter, source, key, stage)`: 129 × 129 texels on
  the chunk's half-step lattice, a palette of up to 8 material ids, and
  weights in 255ths adding to 255 (largest remainder, ties to the lower
  id). Over the cap, the heaviest 8 stay and each dropped material goes
  to the nearest kept colour; `dropped()` counts them.

**Ecotope** (`procgena_biome`, tests in `tests/ecotope_ground.rs`, image
dump `--example ground_dump`):

- `Ecotope::ground` is a keyed map of `GroundLayer`s in `.ecotope.yml`:
  exact differences over a `base`, `null` removes a layer, and new layers
  are written without their default settings.
- `Footprints::influence(p, reach, falloff)`: `Reach::Metres(m)` reaches
  `m` beyond the footprint's edge, `Reach::Footprint(k)` to `k` times its
  radius; the falloff ramp reads 0 at the footprint's middle and 1 at the
  reach's edge. Footprints include the Scatter's clearance. Several
  influences on one layer take the strongest. Trees expose only their
  trunk, so the pine litter uses `metres: 3.0`, not a canopy multiple.
- `Ecotope::ground(base, placement)` gives an `EcotopeGround` painter,
  seeded from the placement's new `seed()` with kind
  `ecotope.child.ground`; `Ecotope::ground_at` paints one point.
  `validate` checks ground orders, salts and layer settings, but
  `place` checks only the Scatters, so a broken ground layer never stops
  placement. A layer following a Scatter the Ecotope lacks (one removed
  in the studio, or by `null` in a file) is simply absent. Only placed
  copies shape the ground; the far form from the density field, and
  coverage, are not built yet.
- The pine forest floor has `soil` (forest soil, 100), `moss` (320, noisy
  patches), `litter` (needle litter, 400, 3 m round pines) and `dirt`
  (bare dirt, 450, 0.6 m round boulders). A test checks that ground layers
  leave placement unchanged.

## As built: G4a measures on the site (2026-11-24)

- `Terrain::query(p, SampleDetail::Measures)` gives `Measures` (see
  terrain/plan.md, "Measures as built"). Water and road distances reach
  40 m (`MEASURE_REACH`) and height above water reaches 30 m
  (`WATER_HEIGHT_REACH`). Both are exactly equal across borders.
- `GroundSite` gains `water_distance`, `road_distance`, `curvature` and
  `nearby_water_height`, all filled by `from_query` when the sample has
  measures.
  - A plain `Ground` site reads both distances as 40 m (measured: none
    near) and curvature as unknown.
  - `height_above_water()` prefers the nearby water's height, so banks
    read how high they stand. It falls back to the covering water, or
    infinity.
- **`GroundSource for Terrain` still asks for `SampleDetail::Gradient`**,
  so chunk weights don't pay for measures yet. The
  `WaterDistance`/`RoadDistance`/`Curvature` conditions are GM3's, and GM3
  switches the source to `Measures` using GM1's per-chunk cost.
- **Cost per sample** (warm, `tests/bench.rs` `warm_sampling`, test
  profile, 48,000 points on the bench worlds). The machine was heavily
  loaded (load average about 190), so compare the rows with each other,
  not with other runs.

  | Query | p50 | p95 |
  |---|---|---|
  | height | 2.5 µs | 35 µs |
  | gradient (`sample`) | 8.9 µs | 218 µs |
  | measures | 17.9 µs | 331 µs |
  | measures, within 200 m of a border | 69 µs | 417 µs |

  Measures cost about twice the gradient (the gradient plus four
  heights for curvature; the distances are almost free). For 129² texels
  that is about 0.3 s per chunk at p50 on one thread under this load.
  Curvature from the chunk's own height grid would avoid the four extra
  heights if GM3 finds it too slow.

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
5. **The ground never feeds placement** (2026-11-24). Scatters shape the
   ground; ground layers never feed a Scatter's density or mask. Scatter
   density rules get the same condition types instead.
6. **Heights in rules: both forms, as an enum** (2026-11-24). A rule's
   height is either absolute (written at scale 1, multiplied by the
   World's `vertical`, like Terrain presets) or relative to the region's
   relief (0 valley floor, 1 highest peak, never scaled), whichever the
   author wants to say.
7. **Texture defaults go well down** (2026-11-24). 1024 px is far too high
   a default. Bake a small 128 px version of every material as well. The
   user's direction: **detail level 1 is the default for everything, with
   level 0 (full detail) optional**, so full-resolution textures are an
   opt-in. The exact sizes per level and the resident cap are set in GM2,
   with the machines from ROADMAP item 18.

## Open questions

1. **Exact texture sizes** per detail level and the number of resident
   materials (see decision 7), in GM2.
2. **Noise patch sizes under scale.** With D5 question 5 (Ecotope
   geometry), decided in C3 and B1.
3. **Order bands.** The numbers proposed above (soils, litter, terrain
   surfaces, weather) are a starting point, to settle with the first Biome.
4. **May an Ecotope switch off or replace a Biome base layer** (no snow
   under a dense canopy)? Not needed for GM0; settle in GM4.
5. **Texture distance and the 8-material cap.** Level 3 and 8 are
   starting points; GM1 and GM2 measure them on the triple-junction scene.
6. **The level-ring seam** in the weight textures (see "Chunk weight
   textures"): accept it or have the coarser chunk read its finer
   neighbour's edge. GM1.
7. **Where `plant_far_color` lives** (`procgena_core_bevy`, or a hook the
   terrain material offers): with the plant-align lane, before GM5.
8. **What fills the relief height** (`GroundSite::relief_height`, the
   `relief` form of decision 6), for GM3. G4a doesn't fill it: it isn't
   a feature-index measure, and a region's measured lowest and highest
   ground would need a scan of the region and would jump at every border,
   since neighbours have different ranges. The proposal: normalise
   analytically, `(height − E) / span`, where the span comes from the
   terrain's `Relief` layer spreads (or heights) and fades towards the
   agreed amplitude near borders, as the relief itself does through the
   convergence fade. Then it needs no scan and is continuous. **For the
   user:** near a border between, say, mountains and plains, the span
   blends, so the same metres read as "high" on the plains side and "low"
   on the mountain side only away from the border, and meet in between.
   Is that the behaviour wanted, or should relief height stay a
   per-region notion that is allowed to jump at borders (hidden by the
   Ecotope blend zone)? Also: is "1 at the highest peak" a theoretical
   peak (relief layers at full height) or a typical one (some spread
   above the mean), since real peaks rarely reach the theoretical top?

## What changes where

- **ROADMAP:** milestones GM0 to GM6; G4 split into G4a and G4b; graph
  nodes and waves; a "For the user" item for the open questions.
- **ARCHITECTURE:** a pointer in "3. Ecotope" (what it owns).
- **terrain/plan.md:** Phase 8's material and Phase 10's
  `terrain/textures.yml` follow this document. Not edited here, because
  G6 is editing that plan; to be noted when G6 merges.
