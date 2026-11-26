# procgena_terrain: plan

A procedural terrain generator for Rust and Bevy. One generator makes one
terrain type (plains, hills, mountains, badlands, coast, and so on) inside a
region it is given. The region can be bounded or infinite. The terrain is
built in chunks, labelled (underwater, road, cliff, bluff, beach and so on),
and joins any other terrain generator on its border without a seam: both
meet in height and slope along the border line. Past the border each
terrain's own ground differs from its neighbour's, so anything that
reads ground around a point near a border (curvature, shape labels) can
differ a little from side to side.

## Bottom line

- **Read this first: "volumetric" with a heightmap interface.** The request
  asked for a "2d heightmap / mesh". The answer to the follow-up question
  was "volumetric from day one". This plan reads both together:
  - The terrain is a **signed distance field (SDF)**. Its base is the
    heightfield `y − h(x, z)`. Volumetric modifiers sit on top of it:
    overhanging cliffs, undercut bluffs, arches and caves.
  - The crate still gives a pure **top-surface `height(x, z)`** and a 2D
    **`annotate(x, z)`**. `procgena_plant` (`with_ground_from_fn`,
    `with_density_from_fn`), the future world generator and anything else
    that wants a heightmap gets one.
  - Meshes come from the SDF, so overhangs and caves appear in the mesh.

  **Decided 2026-11-23 (user):** a heightfield mesh by default, with
  volume meshing only in bounded patches for caves and overhangs (see
  [features_plan_review.md](features_plan_review.md), "Decisions"). The
  SDF stays the shape of the land; only the default mesher changes.
- **Terrain features.** Rivers, roads, ponds, and later cliffs, caves,
  lakes and general landforms (mesas, terraces, outcrops) share one
  lifecycle: plan, check bounds, evaluate, draw, query. Each keeps its own
  planner. See [features_plan.md](features_plan.md) for the design and
  [features_plan_review.md](features_plan_review.md) for what is adopted
  and how its steps map onto the phases below.
- **Two new crates**, in the stone/tree split:
  - `terrain/crates/procgena_terrain`: engine-free core. It holds the
    options, presets, field, the seam protocol, features, classification,
    the chunk mesher and `procgena_specimen::Specimen` output. It depends on
    `glam`, `procgena_specimen`, `serde`, `serde_json`, and optionally
    `rayon`. **No new dependencies.**
  - `terrain/crates/procgena_terrain_bevy`: chunk streaming around cameras,
    async builds, LOD, a label-driven triplanar material, a debug label
    overlay, and a viewer. No water material: we say where water goes,
    and the user brings their own water.
- **Every point is a pure function.** Height, density and labels at a
  world position depend only on four things:
  1. the generator's options;
  2. its region;
  3. its **seam agreements** with its neighbours;
  4. small, deterministic **per-cell feature caches**: river, lake and road
     graphs on a 1 km macro-cell lattice.

  Because of this, a chunk generated alone is identical to the same chunk
  generated in any order. Neighbouring chunks share their border samples bit
  for bit.
- **The seam protocol: an "edge contract", not overlap blending.** This is
  the original part of the design, and it is not proven yet.
  - Before generating, each generator publishes an `EdgeOffer` per border
    edge. The offer depends **only on itself**: its natural height and
    cross-slope along the edge, plus stiffness, margin, edge-style
    preferences and road/river ports.
  - A pure, symmetric `negotiate(edge, offer_a, offer_b)` gives an
    `EdgeAgreement`. The result is the same bit for bit when the two
    offers are swapped. It holds:
    - the agreed height profile and cross-slope;
    - the edge style (blend, escarpment, shore);
    - the agreed crossings (road and river ports);
    - a seam seed.
  - Region **corners** (vertices where 3+ regions meet) get a
    `CornerAgreement` from all incident offers. The function does not depend
    on their order. Edge profiles are pinned to it at their ends.
  - Each generator then **conforms** its own terrain to the agreement within
    its own margin. It never evaluates its neighbour's field. Both sides
    meet the same curve with the same slope, so the seam is continuous in
    height and slope (C1).
  - The same protocol runs **inside** a large or infinite region between its
    1 km macro cells. That is how internal rivers and roads cross cell
    borders without global passes.
- **Rivers and roads cross region borders through negotiated ports.** None
  of the prior art does this (the plate-local paper avoids it on purpose).
  This section is original and is spiked first.
  - Ports and flow direction are decided **inside `negotiate`** from the
    agreed profile. They are not taken from one side's request.
  - Every generator must accept any inbound road or river. It has a
    guaranteed fallback: a river ends in a lake, marsh or sink, and a road
    ends at a clearing.
  - Outbound offers never depend on inbound flow, so nothing chains across
    the world. The visible cost is that **river flow and width do not grow
    across region borders** (see open questions).
- **Erosion is analytic only**: derivative-damped fBm plus a gully filter
  built from analytic gradients. There is no simulation and no
  priority-flood erosion. Rivers, lakes and roads are **designed first, and
  the terrain conforms to them**. They are not found by simulation.
  Routing them is a cached per-cell pass (coarse grid, deterministic, and
  bounded per cell), not a per-point formula.
- **Two kinds of labels.**
  - **Provenance labels**, from what made the point: a road carve, a river,
    a lake, the sea, a cliff feature, a cave, an overhang, a terrace riser.
  - **Morphology labels**, from the shape: slope, curvature, local relief,
    height above water, normal direction.

  Classification gives each surface point one primary `Landform`, a
  `Flags` bitset and continuous `Measures`. Meshes carry per-vertex label
  weights. They are also split into `procgena_specimen` `Part`s by primary
  label. A separate
  water `Part` comes with each chunk: the water surface's geometry,
  tagged water, with no material of ours. Vector features (road polylines,
  river polylines, lake polygons, cliff lines) are exported as data.
- **Meshing**: Transvoxel-style marching cubes on an integer world lattice.
  Only the band around the surface is meshed. Transition cells join chunks
  of different LOD. Normals come from the field gradient. Chunk-local f32
  meshes have an integer chunk origin, and noise is evaluated on split
  integer-plus-fraction coordinates, so the terrain stays exact far from the
  origin. The licence of the Transvoxel tables must be checked before
  porting (see Licences).
- **Phase 1 is a 2D spike of the seam protocol.** It uses heightfield only:
  two presets, a triple junction and one river crossing. Volume, meshing
  and Bevy come only after the protocol holds.

### Data flow

```
World generator (TBD; a reference "stitcher" ships in this crate for tests and examples)
  │ region graph: polygons, edges with neighbour ids, corners with incident regions
  ▼
Phase A, offers      Terrain::offer(options, region, edge) -> EdgeOffer     (self-only)
                     Terrain::corner_offer(options, region, corner) -> CornerOffer
Phase B, agreements  negotiate(edge, a, b) -> EdgeAgreement                 (pure, symmetric)
                     negotiate_corner(&[CornerOffer]) -> CornerAgreement     (order-free)
Phase C, generator   Terrain::new(options, region, agreements) -> Terrain
  │
  ├─ height(xz), annotate(xz), density(xyz), features(rect)   pure queries
  ├─ tile(key) -> HeightTile          2D grids: height, water level, labels
  └─ chunk(key, lod, transitions) -> TerrainChunk -> procgena_specimen::Specimen
```

Inside one `Terrain`, the field is built in stages. Each stage is pure:

```
base relief  ->  landforms  ->  analytic erosion  ->  conform to edges
   ->  water features (rivers, lakes, coast)  ->  roads and clearings
   ->  volumetric modifiers (masked out in the margin)  ->  SDF
classify(point) reads the stages' provenance plus morphology
```

---

## Prior art

Shallow clones in `terrain/prior_art/` (about 310 MB, untracked). **It is
not in `.gitignore` yet**; adding it is a Phase 0 coordinator edit.
infinigen was copied from `mushroom/prior_art/`.

| Source | Commit / date | Licence | Use |
|---|---|---|---|
| [SahilK-027/MapForge](https://github.com/SahilK-027/MapForge) | `34de8e1` (2026-11-20) | MIT | **May port with attribution.** Terraced highland lift with noise-varied cliff sharpness; `\|fBm\|` ramp corridors; river carve gated by how low the ground is; C2 beach shelf. |
| [PHOENIXNFS/Game-Heightmap-Generator](https://github.com/PHOENIXNFS/Game-Heightmap-Generator) | `f9c1f70` (2026-11-03) | MIT | **May port with attribution.** Ridged noise with rotated, softened octaves; mountain stack (mask → spine → crest → dominance); constant-slope C2 coast ramp. Skip its erosion and its per-map percentile masks. |
| [Chin-Wee/heightmap-level-generator](https://github.com/Chin-Wee/heightmap-level-generator) | `884335c` (2026-10-19) | MIT | **May port with attribution.** World-space pure `rawTerrain`; amplitudes calibrated so parameters mean metres and percent; road carve (clamp to level ± tolerance plus an integrated bank shoulder); tanh clearing; 16-neighbour Dijkstra road cost with grade penalty and reuse discount; `limitGrade` envelopes; parabolic river bed; water-level field; splat priority scheme. Skip its pipe erosion (analytic only). |
| [princeton-vl/infinigen](https://github.com/princeton-vl/infinigen) | `53033dd` (2026-11-10) | BSD-3, partly GPL | **BSD parts may be ported with attribution**: pure point-evaluated SDF elements, tag thresholds, the beach band, the landtile Voronoi blend with a fixed central band, warped-rock and Voronoi-rock SDFs. `FastNoiseLite.h` and IQ's `smooth_bool_ops.h` are MIT. **Ideas only, no copying**: everything under `infinigen_gpl` (hash, Voronoi, Blender noise nodes, surface kernels) and the ANT landscape tiles. SoilMachine erosion has an unknown licence, and we don't simulate anyway. |
| [Crouzet, "Seamless and Collaborative Generation of Infinite Worlds"](https://christophercrouzet.com/blog/cg/infinite-world) | 2022 | blog | **Ideas.** Seam data computed from every region meeting at a junction (not only pairs); a safe interior plus a declared edge band with per-feature rules; padding between regions. |
| [Davis, "Plate-Local River Generation" (arXiv 2610.00301)](https://arxiv.org/abs/2610.00301) + [repo](https://github.com/Davismk3/plate-local-river-generation) | 2026 | CC BY 4.0 (paper and code) | **Ideas plus citation**, not a software licence. Coarse grid per cell; sources traced downhill from highest to lowest; dead-end paths discarded whole; affine monotone height remap; lazy per-cell cache. It never crosses borders, which is the part we must add. The numbers from its section 3.5 onward came to us as a model summary; re-measure our own. |
| [Johansen, erosion filter](https://blog.runevision.com/) (via [80.lv](https://80.lv/articles/fast-terrain-erosion-filter-that-emulates-erosion-without-simulation)) and [`bevy_erosion_filter`](https://docs.rs/crate/bevy_erosion_filter/0.1.0) | 2026 | MPL-2.0 (blog code and crate, read 2026-11-23); the earlier note that a core function derives from CC BY-NC-SA code is unverified (the Shadertoy pages couldn't be fetched) | **Ideas only.** Gullies steered by analytic gradients, every point evaluated alone. We write our own from the description. |
| [IQ, "More noise" (fBm derivatives)](https://iquilezles.org/articles/morenoise) and [fBm](https://iquilezles.org/articles/fbm) | — | articles (the derivative-noise shader is MIT) | **Basis for our erosion look**: derivative-damped fBm with analytic derivatives. |
| Minecraft 1.18 noise router ([wiki](https://minecraft.wiki/w/Tutorial%3ACustom_world_generation)) | — | ideas | **Ideas.** Splines over a few macro noises (continentalness, erosion, peaks and valleys) choose terrain character. We use the same idea for landform masks inside one generator. |
| Transvoxel ([`transvoxel`](https://docs.rs/crate/transvoxel/1.0.0) crate MIT/Apache, [`transvoxel-data`](https://docs.rs/transvoxel-data) MIT) | — | **Lengyel's own terms for the tables not confirmed** | Check the official repository's licence before porting the tables. Fallback: surface nets with our own LOD stitching. |

### What we take from each

- **Calibration (HLG).** Divide noise amplitudes by measured statistics, so
  `hill_height: 12.0` means hills about 12 m high and `mountain_coverage:
  0.4` means about 40% of the area. HLG's constants belong to its noise, so
  **we measure our own**: a test samples our noise and writes the quantile
  table, and the table is checked in.
- **Landforms (MapForge, GHG, infinigen).**
  - Terraced lift for mesas and plateaus, with cliff sharpness varied by a
    noise mask. The same mechanism gives bluffs.
  - The mountain mask uses `1 − exp(−2u²)`, so summits are not flat-topped.
  - Ridged noise with rotated, softened octaves.
  - A max of several masked fBm groups (infinigen MultiMountains).
  - The C2 coast ramp.
- **Roads (HLG).** Use the carve and the routing cost model. The routing runs
  per macro cell on a coarse grid, with ports as fixed endpoints.
- **Rivers (plate-local, HLG).** Trace from sources downhill on a coarse grid
  per macro cell, apply the monotone remap, then carve a parabolic bed with
  banks. Bank steepness decides when a cut bank becomes a **bluff**.
- **Tags (infinigen).** Per-vertex weights, converted to per-face tags by
  thresholds. Placement queries use domain expressions such as
  `land = terrain, -underwater, -cave, -beach`.

---

## Workspace patterns to reuse

- **Options:** stone style. snake_case serde; `from_json` deep-merges over
  `Default`; `to_json`; `validate() -> Result<(), OptionsError>` rejects bad
  values and never clamps; `#[non_exhaustive]` errors; `MAX_*` constants.
- **Presets:** a `TerrainPreset` enum with `ALL`, `name()` and `options()`,
  written in Rust.
- **RNG and noise:** clump's counter-based `pcg3d` hash (`hash.rs`) and
  stone's noise style. No `rand`. New: gradient noise with analytic
  derivatives, Worley/Voronoi (our own; infinigen's is GPL), and 1D noise
  along an edge.
- **Object model:** `procgena_specimen::{Specimen, Level, Part, Tag,
  TagSet, TagFilter, SurfaceMesh}`.
- **Bevy crate shape:** `TerrainPlugin`, `TerrainParams`, a `.terrain.json`
  loader, `TerrainGenerating` / `TerrainFinished`, `AsyncComputeTaskPool`,
  headless `MinimalPlugins` tests, `examples/viewer.rs`.
- **Materials:** clump's triplanar texturing, weighted by label.
- **Plant integration:** `PlantScatter::with_ground_from_fn(|p| terrain.height(p))`
  and `with_density_from_fn(|p| …annotate(p)…)` work, given a `Vec2` to
  `DVec2` conversion (plus the field's world offset) in the closure.
- **Units:** metres, Y up. The terrain plane is XZ, as in plant.
- **Marching-cube tables:** stone has them in `stone/.../mesh/tables.rs`.
  Moving them into `procgena_specimen` so both crates can use them would be a
  refactor; this plan does not require it.

---

## Core design (`procgena_terrain`)

### Coordinates and determinism

- **World positions** in the API are `DVec2` / `DVec3` (f64 metres).
  Internally a position becomes an integer lattice index (`i64`) plus an
  f32 fraction. Noise lattice cells come from the integer part, so the
  terrain stays exact at any distance. Mesh vertices are f32 relative to
  their chunk's integer origin.
- **Lattice sampling:** a chunk at LOD `l` samples world lattice points
  `i · 2^l · cell` (`cell` 1 m by default). It computes them from integers,
  never by adding floats from different origins. A border sample shared by
  two chunks is therefore the same input in both, and gives the same output.
- **No hidden state:** per-cell caches are built from pure inputs. Building
  them in another order, or on another thread, gives bit-identical caches.
- **Normals** come from the field gradient: analytic where the stage has
  derivatives, central differences of the field otherwise. They never come
  from the chunk mesh, so lighting has no seams.

### Regions

```rust
pub struct Region {
    pub id: RegionId,                    // u64, chosen by the world generator
    pub boundary: Boundary,              // Unbounded | Polygon { outer, holes } (f64)
    pub edges: Vec<RegionEdge>,          // one per boundary run shared with one neighbour
    pub corners: Vec<RegionCorner>,      // vertices where 3+ regions meet
}
pub struct RegionEdge { pub id: EdgeId, pub polyline: Vec<DVec2>, pub neighbor: Option<RegionId>,
                        pub start: CornerId, pub end: CornerId }
pub struct RegionCorner { pub id: CornerId, pub position: DVec2, pub incident: Vec<RegionId> }
```

- `EdgeId` and `CornerId` are canonical hashes of the sorted region ids and
  vertex positions. Both sides compute the same ids.
- An edge with no neighbour (the world's end, or a neighbour not decided
  yet) gets a **default agreement** from its own offer alone. When the
  neighbour arrives later, chunks within the margin of that edge must be
  rebuilt. The Bevy crate tracks this.
- **A requirement for the future world generator:** it must supply corner
  incidence (which regions meet at each vertex), not only edge neighbours.

### Options

```rust
pub struct TerrainOptions {
    pub seed: u32,
    pub base: BaseOptions,          // macro/meso relief: plains, hills, mountains; amplitudes in metres, coverage in %
    pub landforms: LandformOptions, // terraces/mesas, escarpments (cliff lines), plateaus, ramps, dunes
    pub erosion: ErosionOptions,    // analytic: derivative damping, gully filter strength/scale/slope response
    pub water: WaterOptions,        // sea (uses WorldContext::sea_level), lakes, rivers
    pub roads: RoadOptions,         // density, max grade, width, bank angle, clearings
    pub volume: VolumeOptions,      // overhangs, undercuts, arches, caves, rock outcrops
    pub edges: EdgeOptions,         // stiffness, margin (m), allowed edge styles, port preferences
    pub classify: ClassifyOptions,  // thresholds: cliff/bluff slope, plain relief, beach band, snow line…
}
pub struct WorldContext { pub seed: u64, pub sea_level: f32 }  // see open question on sea level
```

### Field stages

1. **Base relief.** A few macro noises (continentalness, relief and
   ridges, in the Minecraft-router sense) run through per-preset splines.
   They choose between flat, rolling, hilly and mountainous character
   inside one terrain type. Then come fBm, ridged and billow octaves, with
   domain warp. Everything is calibrated to metres.
2. **Landforms.**
   - Terraced lift gives mesas and plateaus. Riser sharpness comes from a
     noise mask, and riser cells are labelled as escarpment provenance.
   - Escarpments run along noise isolines, with a sharp step across the
     line (HLG cliff band).
   - Ramp corridors keep plateaus reachable.
   - Dunes come from warped Worley noise.
3. **Analytic erosion.** Derivative-damped fBm (IQ) for the overall worn
   look. A gully filter steered by the analytic gradient carves along slopes.
   Both are pure per point. The filter also outputs a "gully" measure, used
   for wetness and scree labels.
4. **Conformance to edges** (see the seam protocol).
5. **Water features.**
   - Coast: a C2 ramp against `sea_level`.
   - Lakes: a flat surface at the pour level, with the bed carved to a
     shore profile. (Superseded by ROADMAP milestone LK, which holds the
     lake design: fill and spill from a hollow, bounded by area, rise and
     the border clearance.)
   - Rivers: a parabolic bed plus a bank shoulder, at monotone levels
     (to become valleys shaped by cut depth and river type, at
     stream-power profiles: Phase 2.8).
   - A steep or undercut outer bank gives a bluff.
   - Output: a water-level field, which is NaN where there is no water.
6. **Roads.** Clamp to the road level ± tolerance, plus a bank shoulder
   (HLG), and tanh clearings for road ends and settlement sites. Roads stay
   farther than their influence distance from region and cell edges, except
   at agreed ports. Water and roads are carved **after** conformance, so
   both sides of a port carve the same thing with the same parameters.
7. **Volumetric modifiers**, combined with smooth min/max (IQ, MIT):
   - **Overhangs and undercuts**: on escarpment and bluff faces, push the
     face outward along its horizontal normal near the top and inward near
     the base, with 3D noise.
   - **Arches**: a capsule subtracted through a thin ridge or riser.
   - **Caves**: worm tunnels (cell-seeded capsule chains with noise) and
     "spaghetti" tunnels (`|n1| < t ∧ |n2| < t`). They are kept below a
     minimum overburden, and their entrances are placed only where they meet
     a riser or bluff face.
   - **Rock outcrops**: Voronoi-cell solids near the surface (the infinigen
     idea). Stone scatter is out of scope for now.
   - All modifiers fade to zero within the edge margin and away from roads,
     so seams are heightfield only.
   - The modifiers have a known maximum vertical reach. That gives each
     column a cheap vertical band for meshing and for the top-surface
     search.

**SDF.** `sdf = (y − h) / sqrt(1 + |∇h|²)` near the surface (infinigen's
normalisation), then combined with the volumetric modifiers.

**Top surface.** `height(x, z)` starts from `h`. Where modifiers can reach,
it refines with a short downward sphere-trace inside the column band. Under
an overhang it returns the **highest** surface, as plant expects.

### The seam protocol

#### Alternatives considered

| Approach | How | Why not alone |
|---|---|---|
| A. Overlap blend (partition of unity) | Evaluate both generators beyond their borders and blend them by distance weights. | Each generator has to evaluate its neighbours' fields: two or three generators per sample near borders. Cliffs and plateaus turn to mush. Features ghost and average. Volumetric elements double. No generator gets a say. |
| B. **Edge contract (recommended)** | Agree on a boundary curve, a cross-slope and crossings. Each side conforms within its own margin. | It is new, so it is spiked first. |
| C. World-owned macro field plus per-region relief | The world generator owns base elevation, and generators add relief. | **Adopted for the low-frequency part (2026-11-22):** `WorldTerrain` owns a world elevation field E; regions negotiate only their relief on top of it (B). See `terrain/world_plan.md`. |

Crouzet's article supports B's corner rule. Seam data that depends on the
full set of regions at a junction, instead of only pairs, lets per-edge
curves meet at the corners.

#### Offers (self-only)

```rust
pub struct EdgeOffer {
    pub natural: Vec<EdgeSample>,   // height and cross-slope sampled every `EDGE_STEP` (8 m) along the edge,
                                    // from stages 1–3 of this generator alone
    pub stiffness: f32,             // how strongly it keeps its own shape (mountains high, marsh low)
    pub margin: f32,                // metres it may deform inward to meet the agreement
    pub roughness: f32,             // amplitude of the shared seam noise it tolerates
    pub styles: EdgeStyleSet,       // which styles it can build: Blend, Escarpment{up/down}, Shore
    pub river_out: Vec<PortWish>,   // its own rivers that want to leave here (own drainage only)
    pub road_out: Vec<PortWish>,    // its own roads that want to leave here
    pub lake: Option<LakeWish>,     // superseded: lakes stay inside their region for now (ROADMAP LK)
}
pub struct CornerOffer { pub region: RegionId, pub height: f32, pub gradient: Vec2, pub stiffness: f32 }
pub struct PortWish { pub s: f32,              // arc-length position along the edge
                      pub level: f32,          // river or road level where it enters this side's margin
                      pub width: f32 }
```

An offer is computed from the generator's own options, its region and the
edge geometry. It never depends on another offer or agreement, so nothing
chains.

#### Negotiation (pure, symmetric, order-free)

`negotiate(edge, a, b)` first sorts `(a, b)` by `RegionId`, so swapping the
arguments gives the same bits. Then:

1. **Profile.** Each sample is
   `h(s) = (k_a·h_a(s) + k_b·h_b(s)) / (k_a + k_b)`, where `k` is the
   stiffness. The cross-slope is averaged the same way, in a shared normal
   direction. The ends are pinned to the two `CornerAgreement` heights over
   a short taper. Seam noise is added: 1D noise along the arc length,
   seeded by the `EdgeId`, with amplitude `min(roughness_a, roughness_b)`.
2. **Style.**
   - Blend is the default.
   - Escarpment needs one side to offer it and its heights to differ by
     more than a threshold. The face belongs to the side that offered it,
     inside its own margin. The agreed profile is the *other* side's
     natural level, so that side barely moves.
   - Shore applies when the profile drops below the sea level.
3. **River ports.** A port exists **only where at least one side wishes
   one**. Its position and level come from the **agreed** profile:
   - Each `river_out` wish snaps to the nearest local minimum of the agreed
     profile within a window. Minima are snap targets, never ports on their
     own.
   - The port level is the agreed height minus the channel depth.
   - **Uphill check:** the wish carries the river's level where it enters
     the owner's margin. If that level is not above the port level by a
     minimum drop, the wish is dropped, and its owner ends that river
     internally. The receiving side's agreed cross-slope must also fall
     away from the edge at the port, or the wish is dropped as well.
   - Ports closer together than the minimum spacing are merged, with ties
     broken by region order. (As built: 195 m, or 130 m between two
     rivers from the same side.)
   - **Border valleys:** where both sides slope down towards the edge,
     nothing can cross or run along it. Both sides end their rivers in
     lakes or sinks inside their margins.
4. **Road ports.** The union of both sides' wishes, snapped to the
   low-grade points of the agreed profile and merged by spacing. The road
   level is the agreed height.
5. **Lakes across an edge** (superseded: under ROADMAP milestone LK,
   lakes stay inside their region's border clearance for now; lakes
   shared across a border are a later step). Both sides' wishes must overlap. The level is
   the lower of the two wishes, and both sides flatten to it. Along the
   shared stretch, the agreed profile is lowered below the lake level (to
   the lake-bed shore profile), so the seam leaves no ridge through the
   water. Otherwise the lake stays on one side, kept inside that side's
   margin.

`negotiate_corner(&[CornerOffer])` sorts by region id and takes the
stiffness-weighted mean.

#### Conformance (per generator)

> **As built in Phase 1** (the text below is the original design; these
> changes came from the spike images):
>
> - **Smoothed profiles.** Natural offers are smoothed along the edge
>   (Gaussian, σ = 48 m) before mixing. Without this, the profile's fine
>   detail was pushed straight out from the border as streaks.
> - **Shift, don't blend.** Across the margin the terrain is *shifted* by
>   `B_agreed − B_own` (its own smoothed profile, carried out the same way),
>   instead of blended towards `B_agreed`. The terrain keeps its own detail.
> - **Bounded cross-slope.** It is carried as `slope · R·tanh(d/R)`
>   (R = 32 m), not `slope · d`, which ran away over a 500 m margin.
> - **Shared detail at the border.** In a 48 m strip next to the border,
>   own detail gives way to the agreed profile plus **shared detail noise**.
>   That noise depends only on world position, with an agreed amplitude
>   per sample (the smaller roughness) that tapers to 0 at corners. It
>   replaces the 1D seam noise.
> - **Soft distance.** The blend uses a soft minimum distance
>   (`(Σ d_j⁻⁸)^(−1/8)`), not `d_min`, which creased along bisectors at
>   bends and corners. Side effect: where many segments are about equally
>   near (densely sampled borders), the effective margin shrinks a little.
> - **Not built yet:** thin-region margin shrinking, escarpment edges.

Edges are polylines. Every **straight segment** `j` of every edge gives
its own term, so the closest-point parameter never jumps on the inside of
a bend. For point `p`, let `s_j` be its projection onto segment `j`,
clamped to the segment, and `d_j` its distance from it. The target is
`t_j(p) = profile(s_j) + slope(s_j) · d_j`.

1. **Boundary value.** `B(p)` is the Shepard-weighted mean of the `t_j`
   over the region's segments, with `w_j = 1 / d_j^k` and `k ≥ 2`. (With
   `k > 1` the cross-slope survives at the edge.)
   - There is no hard "nearby" cutoff, because a segment entering the set
     would make `B` jump. Use all of the region's segments, with a spatial
     index for speed, or weights with smooth compact support (such as
     `(1 − d/R)^4_+ / d^k`) that fall to zero with zero slope.
   - On an edge, `B` is **exactly** that edge's target, all the way into a
     corner, because both edges' profiles are pinned to the same corner
     values.
   - Both regions on an edge compute the same `B` on that edge.
2. **Blend.** `h = lerp(B(p), h_own(p), smootherstep(d_min / margin))`.
   The blend factor has zero first derivative at `d = 0`, and `B` carries
   the agreed cross-slope, so the seam is C1 **along edges**.
   - **At corners** the incident cross-slopes need not fit one plane. The
     `CornerAgreement` therefore also holds an agreed **gradient**
     (stiffness-weighted, order-free). Each profile end and its
     cross-slope taper onto that height and gradient, which makes
     junctions C1 too.
   - If that taper proves fragile in the spike, the fallback claim is C1
     along edges and C0 at junctions.
3. ~~**Escarpment edges.**~~ **Dropped as an edge style (2026-11-22).**
   Cliffs at a border would be an artefact of the seam. Escarpments become
   a feature the user places and controls (with the volumetric work), not
   a way of joining regions.
4. **Thin regions.** Where the region is narrower than two margins, the
   margin shrinks to `0.45 ×` the local width, found from the distance to
   the far boundary. If the agreements cannot be met smoothly in that
   width, the result is a steep but continuous ramp. That case gets the
   `transition` flag, and the generator reports a warning count.

Labels in the band follow the agreed edge style. Volumetric modifiers are 0
in the band, and so are internal roads (except at ports).

#### Phase 2 as built (2026-11-22)

- **Planning is per region for now** (not macro cells yet). A 25 m grid
  over the region's polygon is used for planning, then thrown away.
  Unbounded regions get no rivers or roads until Phase 4.
- **The region's own rivers:**
  - Sources are the highest cells outside the margin.
  - Rivers run downhill on the terrain's own relief to within
    `FEATURE_CLEARANCE` (65 m) of the border.
  - A river stuck in a hollow escapes by a small local flood search over
    the lowest rim. It may cut through rims up to
    `4 m + 3 % × (hill + mountain height)`; deeper hollows end it in a pond.
  - Rivers that reach a shared border's zone make a wish.
- **Crossing negotiation (`ports.rs`):**
  - A river wish snaps to a low point of the agreed profile.
  - It is kept only if it runs downhill to the crossing level (agreed
    ground minus 1 m) and the receiving side's ground falls away.
  - A road crossing is placed only where each side can get from the
    crossing down (or up) to its own ground within about 1 km of road at its
    grade. Offers carry `road_grade` for this check.
  - Crossings stay 195 m from corners (not a full corner-blend width) and
    195 m apart.
- **At build time:**
  - Own rivers are re-fitted to the *bent* terrain. Water stays below the
    ground. A river without a crossing stops (in a pond) where the bent
    ground rises above it by more than the cut-through limit. Without
    this, rivers carved 450 m walls into the steep border ramps.
  - Rivers flowing in from a neighbour are traced on the bent terrain from
    the crossing. They end in a pond or join one of the region's rivers.
  - Each crossing carves a straight 130 m piece across the border, carved
    identically by both sides.
  - Roads link crossings to a hub near the middle of the region, or to an
    existing road. The route is found on the grid with a grade-penalised
    Dijkstra search, then levelled within the grade from both ends.
- **Carving:**
  - River and pond banks rise and steepen until they stop affecting the
    ground 40 m out.
  - Roads cut and fill to within 0.25 m of their level, with banks that
    also stop 40 m out.
  - So no feature affects ground more than 60 m from its centre line,
    which is less than the 65 m clearance.
- **Later changes in Phase 2** (after the advisor's review):
  - **Sloped crossing pieces.** Each crossing records the agreed cross-slope
    (`Port::slope`). Its piece follows the same curve the bent ground
    approaches, `level + slope · R·tanh(s/R)`, sampled in 13 steps (an odd
    number, so the border falls mid-step where the piece is smooth). A flat
    piece cut 10 m into one bank and floated above the other.
  - **Stricter crossings.** River crossings need a cross-slope of at most
    0.25, and a receiving side whose bend from the agreed height down to its
    own ground is gentle (at most 0.25 of its margin). Road crossings need a
    cross-slope within both grades and at most 500 m of ramp each side.
  - **Bends.** Crossings stay 195 m from an edge's ends. Each piece's ends
    must also be at least `FEATURE_CLEARANCE` from the edge's other
    segments, so carving near a bend never reaches another part of the
    border.
  - **Water below ground.** After smoothing, every river point's water
    level is capped by the actual uncarved ground.
  - **Smooth carving.** Rivers and ponds meet the ground with a smooth
    minimum (0.5 m blend), and roads with a smooth clamp (0.2 m), so carved
    ground is continuous in slope wherever the uncarved ground is.
  - **New API:** `Terrain::uncarved_height`.
- **Measured** (release, M-series Mac, one thread; before the 16-direction
  routing, inland samples and dip checks, which add some cost):
  - 4 km test worlds build in 36–45 ms;
  - two 10 × 10 km regions build in 556 ms (rivers are traced twice per
    region: offers, then build);
  - sampling near a border costs about 0.9 µs per height.
- **Observed:**
  - The region's own rivers often end in basins, because noise relief has
    closed hollows.
  - Cross-border rivers occur only when a region's own drainage happens to
    reach a gently lower neighbour: 6 seeds in 300 for the hills-to-plains
    test world, after the checks below. WorldTerrain's drainage tilt is
    meant to fix this.
  - Further crossing checks (added after measuring 80 seeds): each side's
    own ground 65 m inland must stay within 4 m of its border profile
    carried inward (offers carry `inner` samples), and a river wish
    carries the lowest ground on its way to the crossing (`ground`), which
    must be above the crossing, allowing for the sender being bent down.
    With them, worst water above ground fell from 3.8 m to 0.6 m and worst
    road cut from 15 m to 7.7 m; crossings became about three times rarer.
  - Plains–mountains borders get no road or river crossing (the ramp is
    too steep).
  - Roads crossing rivers dam them (no bridges yet).
  - Mountain rivers cut gorges of 25–50 m.
  - Roads cut or fill up to about 8 m.

#### Inside a region: the same protocol on macro cells

A bounded region is tiled by 1 km macro cells clipped to its polygon. An
unbounded region is tiled by an infinite lattice. Between cells:

- the height needs no negotiation, because one generator owns both cells;
- **river and road ports are negotiated with the same `negotiate`**, using
  the generator's own coarse base field as the profile.

Within a region, rivers follow the drainage of that coarse base field:

1. Each cell drains to its lowest neighbour.
2. Flow accumulation is limited to a window of K cells (K = 8), so it stays
   bounded.
3. Sinks become lakes, up to a maximum size. A larger sink is forced to
   spill along a carved channel. (Superseded by ROADMAP milestone LK: a
   lake that reaches its limit before finding an outlet stays a terminal
   lake with no outflow.)

Paths are traced on a coarse grid per cell. Dead ends are discarded whole,
and levels are remapped to be monotone (from the plate-local paper). Caches
are built lazily per cell. Segments go into a uniform-grid spatial index.
The plate-local paper scans segments linearly per sample. Its reported
sampling cost with rivers was several times higher than without, but that
figure came to us from an unverified summary and was not measured for the
scan alone. Measure our own.

### Classification and labels

```rust
pub enum Landform { Plain, Hill, Mountain, Plateau, Valley, Ridge, Peak, Cliff, Bluff, Escarpment,
                    Scree, Beach, Shore, Riverbed, Lakebed, Seabed, Road, Clearing, CaveFloor,
                    CaveWall, Overhang, Dune }
bitflags-like Flags: UNDERWATER, SEA, LAKE, RIVER, ROAD, CAVE, OVERHANG, CLIFF_TOP, CLIFF_BASE,
                     WET, SNOW, TRANSITION, PORT
pub struct Measures { slope: f32, curvature: f32, relief: f32, height_above_water: f32,
                      water_depth: f32, road_distance: f32, water_distance: f32, gully: f32 }
pub struct Annotation { landform: Landform, flags: Flags, measures: Measures, weights: LabelWeights }
```

- **Provenance first.** A road carve gives `Road`, a riverbed `Riverbed`, an
  escarpment riser `Escarpment` or `Cliff`, a cave `CaveFloor` / `CaveWall`.
- **Morphology otherwise**, with thresholds in `ClassifyOptions`:
  - **Cliff**: steeper than 55°, or a downward-facing normal (`Overhang`).
  - **Bluff**: 30–55°, within a distance of water or a valley floor, with a
    convex top.
  - **Scree**: 30–40° below a cliff, with gully weight.
  - **Plain / Hill / Mountain**: by local relief, the max minus min over
    a radius of about 150 m. It is estimated with a fixed ring of samples
    (pure; the cost is a few extra field samples).
  - **Ridge, Valley, Peak**: by curvature at a scale.
  - **Beach / Shore**: by a band around the water level, distorted by
    noise (infinigen).
- `annotate(x, z)` labels the top surface. `annotate_at(xyz)` labels any
  surface point, for overhang undersides and caves.
- **Measures as built (G4a, 2026-11-24).** `Terrain::query(p,
  SampleDetail::Measures)` gives `Measures { slope, curvature,
  height_above_water, water_distance, road_distance }` (`#[non_exhaustive]`;
  `relief`, `water_depth` and `gully` above are not built).
  - `slope` is the sine of the angle, from G2's gradient, as ground rules
    read it.
  - `curvature` is the carved ground's height minus the mean of four
    heights `CURVATURE_STEP` (4 m) away, over a quarter of the step
    squared: positive convex, negative concave, per metre. Near borders
    the stencil reads each terrain's own ground beyond the border; the two
    sides differ by at most 0.0044 /m on the test worlds (slope by 0.0018).
  - Water and road distance run from the water's or road surface's edge,
    from the feature index, **capped at `MEASURE_REACH` = 40 m**
    (`BANK_REACH`). The cap keeps the measures per region and exactly
    equal on both sides of a border: a terrain's own features stay
    `FEATURE_CLEARANCE` (65 m) from its borders, so their edges are at
    least 45 m off, and crossing pieces are carved by both sides. The
    index already lists every piece within `BANK_REACH` of its edge, so
    no second index is needed.
  - `height_above_water` is the height above the nearest water's surface,
    mixed by inverse square distance with every river piece or pond
    within 10 m of as near (`WATER_MIX`), and `None` beyond
    `WATER_HEIGHT_REACH` (30 m = the reach less the mix band). That way
    every surface that counts lies within the index's 40 m, so the
    measure stays continuous and equal across borders. Mixing per piece,
    not per river, keeps it continuous where a river comes back near
    itself; the cost is that on steep rivers a point's surface can be a
    few centimetres off its own (99.5% of water points within 25 cm on
    the test worlds). Where waters at different levels meet, the mixed
    surface can lie below the drawn one (the carve draws the higher).
  - **Follow-up (World level):** a longer reach needs the neighbours'
    features near the border, so it belongs to `WorldTerrain` (which can
    read neighbouring regions), not to `Terrain`.
- **Labels as built (G4b, 2026-11-25).** `Terrain::annotate(p)` and
  `query(p, SampleDetail::Labels)` give `Labels { landform, flags }`
  (`src/label.rs`). A smaller set than the sketch above, from what the
  terrain has today:
  - `Landform`: Plain, Slope, Cliff, Ridge, Peak, Valley, Basin,
    Riverbed, Pondbed, Road, Riverbank, Shore, each with `name()` and
    `colour()`. `Flags`: UNDERWATER, RIVER, POND, ROAD, ROAD_BANK, WET.
  - **Order:** water covering the point (riverbed or pondbed, by the
    nearest water's kind, a pond winning ties), the road surface, cliffs
    (`CLIFF_SLOPE`, 55°, or a road bank tagged cliff or retaining wall),
    banks and shores (within `BANK_WIDTH` 8 m of the water's edge and
    `BANK_HEIGHT` 2 m of its surface), shape, slope (`STEEP_SLOPE`, 20°),
    plain.
  - **Shape** is read from the principal bends (eigenvalues of the
    height's second differences) on a 3 × 3 stencil: a peak or basin
    bends past the threshold both ways, a ridge or valley one way. Step
    and threshold are `SHAPE_STEP` (12 m) and `SHAPE_CURVATURE`
    (0.012 /m) at scale 1, times `horizontal` and `vertical /
    horizontal²` at the World's `WorldScale`. The user picked 24 m and
    0.006 /m on the medium worlds (scale 2 × 2) from three candidates.
  - **The scale reaches the terrain:** `WorldScale` moved here from
    `procgena_world`; `WorldTerrainOptions::scale` (default 1),
    `Terrain::with_scale`, `StitchedWorld::with_scale`. Every terrain of
    a world has the same one, so labels don't jump at borders.
  - **Across borders:** labels from water and roads, and the flags, use
    the 40 m measures and agree exactly. Slope and shape read each
    terrain's own ground past the border, which matches the neighbour's
    only on the border line (in height and slope), so shape labels can
    differ in a thin strip there. On the medium test worlds they differ
    at 2.9% of border points; the two sides' bends differ by 7% of the
    threshold at p50, 80% at p99 and 153% at worst. The strip grows with
    the horizontal scale, as the stencil does. Accepted by the user
    (2026-11-25); `tests/labels.rs` bounds it.
  - **Longer reach:** Hill, Mountain and Plateau by local relief (the
    150 m ring above) need the neighbours' ground near borders, so they
    belong to `WorldTerrain`. Bluff, Scree and Beach wait for those and
    for lakes. No label needs relief height (ROADMAP item 31).
  - **Cost** (warm, per sample, `tests/bench.rs` `warm_sampling`, load
    average about 244, so read the rows against each other): height
    2.3 µs p50, gradient 8.5 µs, measures 16.7 µs, **labels 33.5 µs**
    (p95 589 µs); labels within 200 m of a border 114 µs (measures there
    60 µs). Labels cost about twice the measures: eight more heights.
  - Not built: `annotate_at` for caves and overhangs, label weights, and
    mesh parts by label (Phase 7).
- **Mesh output.**
  - Each vertex carries up to four `(label, weight)` pairs.
  - Triangles are grouped into `Part`s by primary landform. Each part is
    tagged `terrain` plus its label tags, and `underwater` where the flag is
    set.
  - Water is its own `Part`, tagged `water` plus `sea`, `lake` or `river`,
    and `closed: false`.
- **Tags.** Generic tags (`TERRAIN`, `WATER`, `ROAD`, `CLIFF`, `BEACH`,
  `UNDERWATER`, `CAVE`) are added to `procgena_specimen::tags`. That file is a
  coordinator edit. The rest live in `procgena_terrain::tags` (open
  question).

### Chunks and meshing

```rust
pub struct ChunkKey { pub x: i64, pub z: i64, pub lod: u8 }   // footprint 64 m · 2^lod, 64 cells a side
pub struct Transitions(u8);                                   // which of the 4 sides border a finer chunk
pub struct TerrainChunk { pub origin: DVec3, pub object: Object, pub water: Option<Part>,
                          pub labels: LabelBuffers, pub bounds: (f32, f32) }
pub struct HeightTile { pub height: Vec<f32>, pub water: Vec<f32>, pub landform: Vec<Landform>,
                        pub flags: Vec<Flags>, pub res: u32 }
```

- **Vertical band.** A coarse 8 m pass over the chunk finds each column's
  `[h_min − reach_down, h_max + reach_up]`. Only cells in that band are
  sampled, and the band is chunked vertically when it is tall (cliffs).
- **Mesher.** Marching cubes on the lattice. In Transvoxel the **coarser**
  chunk owns the transition cells, on the sides where it faces a finer
  neighbour (the sides in `Transitions`). Confirm this against Lengyel's
  paper when implementing. If the table licence fails, use
  surface nets with skirts as the fallback.
- **Batch sampling.** Stage evaluation is laid out for a whole chunk (struct
  of arrays) and parallel across chunks with rayon. A GPU path is a later
  option (stone has the WESL infrastructure).
- **Grids for plant and heightmap users.** `tile(key)` returns the 2D grids
  without building a mesh.

**As built (G6, 2026-11-24): heightfield chunks** (`src/chunk.rs`), following
the 2026-11-23 decision (heightfield by default; no marching cubes or
Transvoxel yet, so the band and the licence question wait for volume
patches).

- `ChunkKey { x, z, level }`: 64 m · 2^level, `CHUNK_CELLS` = 64 cells a
  side, vertices on the world lattice from integers, so chunks that share
  a point read the same height bit for bit.
- `ChunkSamples::sample(source, key, stage)` reads the chunk once: the
  vertex lattice, a one-sample ring for normals, and three half-step rows
  along every side. `ChunkSamples::mesh(transitions, skirt)` meshes it for
  any `Transitions` without reading again.
- `Transitions` holds four side bits and four corner bits for a finer
  neighbour. As planned, the **coarser** chunk owns the transition: along
  such a side it adds the finer chunk's half-step vertices and fans its
  edge cells to them, so there are no T-junctions. Normals are lattice
  differences; along a transition side, and at a corner any finer chunk
  touches, they are taken at the finer step, so both sides shade a shared
  vertex alike.
- `ChunkSelection::around(lod, factor, viewers, previous)` is the
  restricted quadtree: levels from a `procgena_core::Lod` (level `i` is
  chunk level `i`, its cull distance the view distance), split by distance
  with each level's hysteresis against the last selection, and balanced so
  neighbours across a side or a corner are at most one level apart.
- **Feature tiers** (features_plan.md §9, "Feature tiers"; superseded as
  the direction on 2026-11-26 by canonical TIN at every chunk level with
  a highest level per feature type, [terrain_lod.md](terrain_lod.md);
  this describes what is built today). Chunks are
  read at a `FeatureTier` (`Rough`, `Fine`); only fine is canonical. Each
  `FeatureKind` declares its tiers (`tiers`, `first_tier`) and what it
  reads (`reads`; a unit test checks none is drawn before what it reads):
  rivers and ponds at every tier, roads fine only. `Terrain::with_tier`
  plans each kind at its first tier; `WorldTerrain::terrain_tier` caches
  every tier on its own and builds each from the one before (fine is
  bit-identical to before). A different method per tier has no user yet,
  so it isn't built; rough roads would be the first.
- **Skirts between tiers.** Each terrain records how far each tier moved
  the ground (`Terrain::tier_difference`): measured along every road
  when roads are planned, on the centreline, both edges and 4 m beyond,
  times 1.1 plus 1 m. `ChunkSource::tier_difference` gives it for an
  area. In Bevy, wherever two drawn chunks of different tiers touch, both
  get skirts at least that deep, kept per chunk across swaps. A test
  checks every shared side a road crosses: the gap is never more than the
  bound. Labels (G4) will add per-vertex weights and parts to
  `ChunkMesh`; the struct marks the place.
- Water: each chunk's `WaterMesh` covers every triangle with water at a
  corner, with dry corners at the wet corners' mean level so it runs into
  the banks.
- **Benchmark (2026-11-24, provisional):** release, `--test bench chunks`,
  load average 160 to 180 while other lanes built; rerun on a quiet
  machine. Warm read of one chunk (6,000 samples, rayon): p50 31 ms at
  level 0, 112 ms at level 3, 60 ms at level 6. Meshing: about 0.4 ms at
  every level, split sides or not, so a neighbour changing level costs
  almost nothing. Cold first chunk of a fresh world (builds the regions
  it touches and their neighbours' plans): canonical p50 18 s (p95 61 s),
  provisional p50 7 s at level 0 and 9 s at level 5: the road-free stage
  cuts a cold start by about 2.5×.
- Not built: the vertical band, volume patches, exports (Phase 9),
  geometric-error LOD (distance only), and `tile(key)` beyond
  `HeightTile`.

### Exports

`export-obj` and `export-gltf` features (house pattern). A chunk or a
rectangle of chunks becomes one node per LOD. Label weights go out as vertex
colours, and the options go into `asset.extras`. `HeightTile` can export as
16-bit PNG or EXR-free raw `f32`.

---

## Bevy design (`procgena_terrain_bevy`)

- `TerrainPlugin { texture_root: Option<String>, view_distance, lod_distances }`.
- `TerrainWorld` resource or component: a set of `Terrain` generators with a
  region lookup (the reference stitcher), or a single `Terrain` for one
  region.
- **Streaming.**
  - Chunks around each `TerrainViewer` camera, by LOD ring.
  - Builds run on the `AsyncComputeTaskPool`.
  - Neighbouring LODs differ by at most one, so `Transitions` are known.
  - When a ring changes, the old chunk is despawned only once its
    replacement is ready.
- **Material.** An `ExtendedMaterial<StandardMaterial, TerrainExtension>`:
  triplanar texture sets chosen by label weights (grass, rock, sand, road,
  mud, snow), with clump's triplanar code reused. `texture_root: None`
  gives vertex colours only.
- **Water.** Not ours to render. The crates give where water goes (the
  water-level field, river and pond data, and each chunk's water `Part`,
  tagged water); the user brings their own water material and rendering.
  The debug overlay may tint water areas so they can be seen.
- **Debug.** A label overlay (colour by landform or flag), seam-band and
  margin visualisation, and chunk borders.
- **Out of scope for now:** hooking terrain tags up to clump, mushroom or
  stone scatter (`Specimen` on chunk entities). This plan is just
  terrain.
- `examples/viewer.rs`: presets; "two terrains" and "triple junction" scenes;
  a fly camera.
- **Studio**: a "Terrain" tab in a later phase (stone's 10 touch points).

---

## Presets (v1)

| Preset | Character | Explicit features |
|---|---|---|
| Plains | flat, low relief, meanders | rivers, lakes, roads |
| Rolling hills | 10–40 m relief | streams, roads, small lakes |
| Mountains | ridged, 300–1500 m, eroded gullies | cliffs, scree, alpine lakes, passes for road ports |
| Alpine | steep peaks, snow line | cliffs with overhangs, cirque lakes |
| Badlands / mesa | terraced lift, canyons | escarpments, arches, bluffs |
| Highland plateau | high flat top with an escarpment rim | escarpment edges, ramps |
| River valley | broad valley, wide river | bluffs on the outer bends, terraces |
| Lakeland | many basins | lakes spanning cell edges |
| Coast | beach, dunes, sea cliffs | coast ramp, sea caves |
| Karst | sinkholes, towers | caves, sinks as river terminals |
| Dunes | warped Worley dunes | no roads by default |

---

## Tests

Pinned to the hard goals first:

- **Chunk independence:** a chunk built alone is bit-identical to the same
  chunk built after its neighbours, in a random order, on many threads.
- **Chunk borders:** the shared border samples of adjacent chunks are
  bitwise equal; mesh border vertices coincide; at an LOD transition, the
  mesh is watertight across the join.
- **Symmetry:** `negotiate(e, a, b) == negotiate(e, b, a)` bit for bit;
  `negotiate_corner` gives the same result for any order.
- **Seams:** for every pair of presets across one edge, height is
  continuous (the max jump along the edge is below 1 mm) and the gradient
  is continuous within tolerance. Labels are consistent across the edge.
- **Whole margin band:** on a bent, non-convex region polygon, height
  jumps between neighbouring samples anywhere in the band stay below a
  bound set by the slope, not only on the edge itself.
- **Triple junction:** three presets at one corner, continuous in height
  and gradient at and near the corner.
- **Rivers across a port:** the level is monotone downhill along the whole
  path through both regions, and the carved channel is continuous at the
  port. An inbound river into a region with no outlet ends in its fallback.
- **Roads across a port:** continuous and within the maximum grade.
- **Far from the origin:** the same tests at `x = 10^7 m`.
- **Calibration:** the noise quantile table matches a fresh measurement.
- **Options:** defaults validate, JSON round-trips, partial `from_json`
  merges, each invalid range is rejected.
- **Classification:** synthetic fields (a ramp, a step, a bowl, a ridge)
  get the expected landforms.
- **Plant contract:** `height()` is the top surface under an overhang.
- **Bevy:** a headless streaming test (chunks appear and LOD rings swap
  without gaps), the material compiles, and each chunk's `Part`s carry
  their label tags.

---

### Relief convergence (Phase 2.5)

The settings that make two terrains different converge as they approach
their shared edge, so the seam has less to hide. It generalises the relief
taper (mountains fading their own peaks to foothills): the taper is the
same idea for one value.

- **What converges is a summary, not the settings themselves.** Each
  generator states its relief amplitude (RMS height of its relief, from
  the calibration constants; possibly RMS slope later). The two sides
  agree one target per edge. Each side then scales its own layers to hit
  it, fading its roughest layer first (ridges, then hills). Converging the
  raw settings would need every generator to share one settings layout,
  which breaks "joins with any other generator".
- **Only amplitude-like values may vary across space:** layer heights,
  erosion strength and the mountain mask threshold. Wavelength may not: a
  wavelength that changes with position stretches the noise by an amount
  that grows with distance from the origin. Octave counts are whole
  numbers and cannot fade.
- **Shared world noise (fixes a Phase 1 design flaw).** Today `relief.rs`
  seeds every layer from the region's own seed. That was right for the
  spike, which had to prove the seam could join two unrelated surfaces,
  but wrong as the default:
  - two rolling-hills neighbours get different hills and a negotiated seam
    where there should be no seam at all;
  - even with converged settings the two sides still disagree point by
    point, so convergence only matches statistics;
  - the mismatch it adds at borders makes the steep ramps that reject
    river crossings (an inference, to be measured).

  **Change:** each layer's seed comes from the **world seed plus the
  layer's role** (swells, hills, mountain mask, ridges, detail), not the
  region seed. Every region that uses the standard layers then samples the
  same infinite noise and differs only in how it weights each layer.
  - With convergence the weights are equal at the edge, so heights already
    match before negotiation and the seam's shift falls to about zero.
    Mountain masks and ridges continue across borders instead of stopping
    at them.
  - `TerrainOptions::seed` becomes the world seed (in WorldTerrain, passed
    in by it). Regions sharing a world seed are continuous; there is no
    per-region reroll.
  - **Cost:** a region's terrain depends on where it is, not only on its
    own settings, and one region cannot be rerolled alone without a seam.
    If that is wanted later, a region may add its own seeded variation
    layer that fades out near its edges through the same convergence fade.
  - **What stays:** the seam protocol, as the guarantee and for generators
    that don't use the shared layers (a different algorithm is always
    unrelated); crossing ports for rivers and roads; seam tests that pair
    unrelated generators on purpose. Those fixtures give each region a
    different world seed explicitly, so the suites keep testing the hard
    case alongside the shared-noise case.
  - River and road planning is unchanged: it plans on whatever ground the
    region has.
- **Rules:**
  - The target is computed from options alone, before offers, by a pure
    symmetric rule, and is order-free at corners. `natural` samples come
    from the converged relief.
  - The fade inside a region uses the same Shepard weights per edge and
    soft-min distance as conformance (a hard distance gave bisector
    creases before).
  - The fade width scales with the amplitude change, so the relief never
    changes faster than a set gradient (about 1 km for mountains to
    plains). Relief deep inside a region is untouched, which keeps the
    calibration tests valid and sets a minimum region size.
  - `Relief::gradient` includes the fade's own gradient term.
- **Agreement rule: a world option, `relief_meeting`** (decided
  2026-11-22: keep the choices open and experiment). The edge target is
  the **power mean** of the stated amplitudes, with `relief_meeting` as
  its exponent p: `((a^p + b^p) / 2)^(1/p)` (p = 0 means the geometric
  mean). One exponent covers every rule discussed:

  | p | Rule | Plains 10 m, mountains 150 m → target |
  |---|---|---|
  | −∞ (minimum) | Only the rougher side changes | 10 m |
  | **−1 (default)** | **Each side changes in proportion to its own roughness** (harmonic mean) | 18.75 m |
  | 0 | Each side changes by the same factor (geometric mean) | 38.7 m |
  | 1 | Meet in the middle (arithmetic mean) | 80 m |

  - **Why −1 is the default:** it is the "scale who changes by their
    roughness" rule. If each side moves a share of the gap equal to its
    share of the total roughness, the meeting point is exactly the
    harmonic mean. It adapts by itself: next to a big contrast the rough
    side does nearly all the changing (plains stay almost flat beside
    mountains), and between similar terrains both sides share it.
  - **Corners:** the power mean takes any number of amplitudes and doesn't
    depend on their order, so a three-region corner uses the same rule.
  - Accepted range: p ≤ 1, with negative infinity allowed. Above 1 the
    smoother side would end up rougher than the halfway point. An
    amplitude of 0 (perfectly flat) gives a target of 0 for any p ≤ 0.
  - It must be one value for the whole world, since both sides of an edge
    have to compute the same target. So it lives on `WorldOptions`, not
    on `TerrainOptions`. Until WorldTerrain exists (Phase 2.5),
    `StitchedWorld::build` takes it as an argument and each generator
    receives it with its neighbours' amplitudes.
- **Tests:**
  - Two neighbours with the same preset and world seed: the negotiated
    shift is below a small tolerance along the whole edge.
  - Neighbours with different presets and the same world seed: the shift
    is much smaller than with different world seeds (measured, recorded).
  - Relief deep inside a region matches the unconverged relief exactly.
  - The existing seam and feature suites pass on both fixture kinds
    (shared world seed, and unrelated seeds), at `relief_meeting` −∞, −1,
    0 and 1.
  - At −∞ the smoother side's relief is unchanged everywhere.
  - The target is the same for every order of the amplitudes, at edges
    and corners.
- **Exit:** re-run the border gap probe (239 m mean, plains–mountains) and
  the crossing count (6 in 300 seeds) with shared noise and convergence
  on, and record both against today's figures. Re-pick `RIVER_SEEDS`
  afterwards.
- **Results (2026-11-22, `tests/probes.rs`, run with `--ignored`):**
  - Border gap, plains–mountains, before: 241 m mean, 712 m largest
    (100 m of the mean is the presets' elevation difference).
  - After, shared seed: 100 m (−∞), 101 m (−1), 109 m (0), 194 m (1).
    Unrelated seeds: 102, 104, 122 and 229 m. At −1 the gap is the
    elevation difference alone, which E is to absorb.
  - Shared noise alone (no convergence) changed little: 182 m, within the
    seed-to-seed scatter. Plains and mountains use different wavelengths,
    so they share no noise point for point; the negotiated shift is about
    the same with shared or unrelated seeds (86 m against 89 m at −1).
    Only alike presets match exactly (`tests/convergence.rs`).
  - River crossings in 300 seeds, before: 6. After (with the knight-move
    breach fix), shared seed: 10, 13, 12 and 13 (at −∞, −1, 0, 1);
    unrelated seeds: 7, 9, 10 and 14. After crossings were judged against
    the bent ground (below): shared 10, 12, 12, 13; unrelated 7, 8, 9, 13.
    (These counts predate relief blending and Phases 2.6–2.8, so later
    counts can't be compared with them: on 2026-11-23, before WorldTerrain
    W1, the default world gave 24 of 300; see `world_plan.md`, "W1 as
    built".)
    Only 253 crosses in all eight, so `RIVER_SEEDS` is that one;
    `JOIN_SEEDS` (64 shared, 136 unrelated) cover rivers joining on bends.
  - The eight-configuration fixtures surfaced Phase 2 planner bugs, all
    fixed:
    - Knight and diagonal moves no longer carry a river over higher
      ground between cells (a 64 m cut).
    - A road refuses a join it can't reach within its grade after
      smoothing (0.0605 against 0.06).
    - A joining river ended at the trunk's traced point, which smoothing
      moves up to about 10 m off the trunk's line, so the two crossed or
      the joining river ended short (133 cases in 300 seeds × 8). It now
      ends exactly on the trunk's smoothed line, which gains a point
      there. It can end up to 0.14 m below the trunk's water there.
    - A crossing piece followed the agreed profile while the receiving
      side's ground, bending back to its own height, fell away under it
      (water 5 m above the ground). Negotiation now judges each piece
      end against the side's bent ground, so a side whose margin is
      narrower than a piece (65 m) gets almost no crossings.
  - Roads always prefer flatter ground, and now have two grades: `grade`,
    the steepest climbed comfortably (above it the route search penalises
    steps much more), and `max_grade`, the hard
    limit on road levels (presets: twice `grade`, at most 0.25). Both
    fade with the relief towards the steepest grades of the terrains
    meeting at each edge and corner (`EdgeTarget`, `BorderTarget`,
    `TerrainGenerator::road_grades`). Road negotiation also refuses a
    crossing whose road couldn't reach a side's unfaded ground across the
    fade plus `ROAD_RAMP`.
  - Still open: at meeting exponent 1 the uniform scale-up multiplies the
    plains' relief about 46× next to mountains (their 250 m hills become
    ±69 m, slopes 0.5–0.9). One road there still leaves the ground by
    13.6 m (the only road problem in 300 seeds × 8: triple junction,
    shared seeds, meeting 1). The crossing and the climb near it keep to
    the gentler side's preferred grade; judging them at the steeper
    side's hard limit let in two more floating roads.
    The fix planned: "Relief blending (Phase 2.6)" below.
  - The amplitude scale-up for the smoother side is uniform over its
    layers. The fade blends each layer's scale separately; blending one
    combined "level" made cracks where a layer is absent.

### Relief blending (Phase 2.6)

**Bottom line.** Near a border each terrain can also fade towards a
**border blend**: a weighted mix of the actual height fields of every
region meeting there, each evaluated with that region's own settings and
seed. A new world option, `relief_blend` (0 to 1), sets how much of each
strategy is used: 0 is Phase 2.5's scaling exactly, 1 is the blend alone.

- **What changes:**
  - **New world option, and one place for world options.**
    `StitchedWorld::build(specs, relief_meeting)` becomes
    `build(specs, &Convergence)`, where
    `Convergence { meeting: f32, blend: f32 }` has a `Default` (meeting −1,
    blend 0.5) and a `validate`. It is what
    `WorldOptions` will hold for this in WorldTerrain.
  - **`BorderTarget` gains blend weights** (region id → weight, summing to
    1) beside its amplitude and road grades. `EdgeTarget` keeps its
    middle/start/end shape.
  - **`with_edge_targets` gains the neighbours' terrains**: the relief
    settings and seed of every region named in the weights, plus `blend`.
    A generator can then evaluate its neighbours' height fields.
  - **`Relief` gains neighbour reliefs**, the blend weights per edge and
    the blend share; `relief_target` gains a sibling, `blend_weights`.
- **Why:** scaling a terrain's own layers up to a rougher neighbour's
  amplitude makes short wavelengths absurdly steep (plains next to
  mountains at meeting 1: hills ±69 m at 250 m, slopes 0.5–0.9). The blend
  brings in the neighbour's own layers at their own wavelengths, so
  foothills look like foothills. Each side evaluates the same fields with
  the same weights, so **at blend 1 both sides have exactly the same
  height and slope on the border, before negotiation, with any seeds**.
  Negotiation stays: it carries detail, rivers and roads, and joins
  generators that don't use this crate's layers.
- **Trade-offs:**
  - Two uncorrelated fields mixed half and half have about 0.71 times
    their relief, so the border between similar terrains with unrelated
    seeds is calmer than either side. Alike terrains with a shared seed
    are identical, and blending changes nothing. (Open question 10.)
  - Heights inside the fade band cost about (1 + number of neighbours)
    times as much. The interior is unchanged.
  - Faint ridges of a mountain neighbour show in the plains near the
    border. They should read as foothills; judge by eye in `seam_spike`.
  - This doesn't break the "no wavelength varies with position" rule:
    whole fields are mixed, never stretched.

#### How it works

- **Height near a border.** For a terrain with own height `H` (no fade)
  and today's faded height `S`:
  - **Border blend** at a point on edge segment k: `B_k = Σ αⱼ Hⱼ`, the
    weighted sum of the unfaded heights of the regions in that segment's
    weights. The sum runs in region-id order, so both sides get the same
    bits.
  - **Blended height:** `Hb = H + fade(d) · Σₖ wₖ (B_k − H) / Σₖ wₖ`, with
    the same Shepard weights `wₖ`, soft-min distance `d` and smootherstep
    fade as the relief fade. On an edge it is that edge's `B_k`; beyond
    the fade it is exactly `H`.
  - **Height:** `S` when `blend` is 0, `Hb` when it is 1, and
    `(1 − blend)·S + blend·Hb` between. The two ends are separate code
    branches, so blend 0 is bit for bit Phase 2.5 and its tests keep
    pinning it.
- **Weights.** For each edge and corner, from the incident regions' relief
  amplitudes `aᵢ` and the meeting exponent p:
  - p = −∞: all weight on the smoothest region (shared equally on a tie):
    only the rougher sides change, as now.
  - Otherwise `αᵢ ∝ aᵢ^q`, with q found by bisection in [−2, 1] so the
    blend's relief, `√(Σ αᵢ² aᵢ²)` for uncorrelated fields, equals
    `relief_target(a, p)`. If the target is out of reach (similar
    amplitudes, where mixing always lowers the relief), q is clamped to
    the nearer end. Checked numerically: plains 3.1 m and mountains
    282 m give the plains weight 0.98 (p = −1), 0.90 (0) and 0.49 (1),
    each hitting the target; 3.1/30/282 m hits it at every p too.
  - A region with zero amplitude: with p ≤ 0 the target is 0, so all
    weight goes to the zero-amplitude regions (shared equally).
  - Along an edge the weights change to each corner's over the same ramp
    as the amplitude (`along_edge`, per region), so every edge meeting at
    a corner asks for the same blend there. Corners include every region
    touching them, even ones that share only the corner.
- **Fade width.** At least the margin; at least today's width (amplitude
  change ÷ 0.25); and at least the gap between the blend's mean height
  and the terrain's own, ÷ `BLEND_RATE` = 0.25, the rate scaling already
  changes a mountain range's mean at. (0.1 was planned, but it gave a
  3.9 km fade for mountains next to plains, wider than the 2 km test
  region.) Mean height is `elevation` plus the mountain layer's mean
  (`height × coverage × CREST_MEAN`); this term usually decides next to
  mountains.
- **Neighbour reliefs** are built once per generator from the neighbours'
  `TerrainOptions` (base settings and seed). They are evaluated without
  their own fade: a fade belongs to one side only, and the other side
  can't know it.
- **What else reads the relief:** `natural_height`, offers, rivers and
  roads all use the blended height; `unfaded_height` stays the region's
  own `H`. `Relief::gradient` (central differences) needs no change.

#### Steps

1. `Convergence` options type, validation, and the `build` signature;
   update fixtures (`Config` gains `blend`), the example and the doctest.
   No behaviour change (blend 0).
2. `blend_weights(amplitudes, meeting)` with unit tests: the limits above,
   order-free results bit for bit, weights summing to 1, the targets hit
   for contrasting amplitudes, zero amplitudes.
3. Blend weights in `BorderTarget`, computed in `stitch::converge` for
   edges and corners; `along_edge` per region weight; validation (weights
   finite, not negative, summing to 1 within rounding, regions known).
4. Neighbour reliefs and the blend in `Relief` and `Fade` (one more value
   mixed through `Fade::blend`), the fade width rule, and the blend-share
   mix with exact ends.
5. Tests (below), then probes, then the 300-seed feature probe at every
   configuration.
6. Record results here; re-pick `RIVER_SEEDS` and `JOIN_SEEDS` if needed.

#### Tests

- At blend 1, both sides' `natural_height` are equal on every border
  point (bit for bit), for shared and unrelated seeds, at every meeting
  exponent; and their gradients match within rounding.
- The same at a triple junction's corner and along its three edges.
- The interior is bit-exact (unfaded) at blend 0, 0.5 and 1.
- Blend 0 is bit for bit Phase 2.5 (the existing convergence tests
  unchanged).
- At meeting −∞ the smoothest region is unchanged everywhere at blend 1.
- `border_gap` falls as blend rises (probe; recorded, not asserted beyond
  "at blend 1 the gap is 0 on the border").
- Suites: every configuration at the default blend, plus the seam and
  feature suites at blend 0 and 1 for the default meeting only (open
  question 11 if more are wanted). `CONFIGS` gains the blend.

#### Exit

- Border gap and negotiated shift at blend 0, 0.5 and 1, every meeting
  exponent, recorded here.
- The triple-junction road at meeting 1 (13.6 m off the ground at blend 0)
  re-measured at 0.5 and 1.
- River crossing counts in 300 seeds at the default blend.
- `seam_spike` images at blend 0 and 1 reviewed with the user.

#### Results (2026-11-22, built)

- **Border gap, plains–mountains** (shared / unrelated seeds):
  - Meeting −1: 101 / 104 m at blend 0, 50 / 52 m at 0.5, 0 / 0 m at 1.
  - At blend 0.5: 50 / 51 m (−∞), 55 / 61 m (0), 97 / 114 m (1).
  - At blend 1 both sides' own heights are equal bit for bit on the
    two-region border in every configuration, their slopes across it
    within 0.02, and the triple junction's within 1 mm.
  - Blend 0 is Phase 2.5's code path unchanged, so its gaps at the other
    exponents are those recorded under Phase 2.5's Results.
- **River crossings in 300 seeds:** meeting −1 gives 12 / 8 at blend 0,
  24 / 26 at 0.5 and 29 / 29 at 1. At 0.5, 21–33 for −∞ to 0, and 12–13
  for 1. `RIVER_SEEDS` is now 81 and 293, the only seeds crossing in all
  12 configurations.
- **Features, 300 seeds × 12 configurations** (blend 0.5 at every
  meeting exponent; blends 0 and 1 at meeting −1 only): no problems,
  except one road at blend 1. Fixtures checked separately at meeting 1:
  the triple-junction road still floats 13.6 m at blend 0 (shared seeds),
  and is gone at blends 0.5 and 1.
- **The blend-1 road** (plains–mountains world, shared seeds) climbs
  550 m into the mountains within about 1.2 km of the border and cuts
  17 m on rough patches 700–1150 m in, past the fade's ramp. It is not
  the fade: mountains next to identical mountains (no fade at all, seed
  11) give roads 36 m off the ground. The 25 m road planner can't follow
  full-strength mountain ground; blend 1 only puts a road there. Fixed
  by the road rework (Phase 2.7).
- **Coverage:** scaling alone (blend 0) is now tested only at meeting −1;
  −∞, 0 and 1 at blend 0 dropped out of the seam and feature suites.
- `BLEND_RATE` is 0.25, not the planned 0.1 (see "Fade width").
- Cost: the seam suite went from about 30 s to 61 s: 12 configurations
  instead of 8, and the neighbour heights inside the fade band (only
  there; the interior skips them).

### Road rework (Phase 2.7)

Built 2026-11-22; signed off by the user 2026-11-23. It started from the blend-1 mountain
road: a 4.7 km road climbing 556 m to the region's hub, cutting 17 m,
with zero-radius hairpins whose legs overlapped. The causes, measured:
the road's heights came from the nearest 25 m grid cell (up to 17.6 m
off the real ground) and a 7-point average (up to 23 m more); and the
25 m planning grid could not see ledges, knobs or saddles narrower than
a cell against a 6 m road.

**The test case: the blend-1 mountain road.** Keep coming back to this
road; it is the hardest road in the test worlds.

| | |
|---|---|
| World | `two_regions(DVec2::ZERO, preset(Plains, 62), preset(Mountains, 62), CONFIGS[10].convergence(), &Elevation::flat())` in `tests/common/mod.rs` (re-picked 2026-11-23, see below) |
| Convergence | shared seeds, relief meeting −1 (`DEFAULT_RELIEF_MEETING`), relief blend 1.0 |
| Regions | `RegionId(1)` Plains, seed 62, west; `RegionId(2)` Mountains, seed 62, east; border through (0, −1500), (150, −700), (−200, 0), (100, 800), (0, 1500); 4 km × 3 km in all |
| Road | region 2's routed road: from the border crossing near (72, −398), level 28.4 m, up to the hub, the `RoadEnd` point of interest at (900, 16), level 572.0 m (a 546 m climb); mountains' road grade 0.14, steepest 0.25; width 6 m |
| Window | x 0 to 1280, z −960 to 800 holds the whole climb |
| Re-picked | The string-key seed salts (2026-11-23) changed every world. The old world (seed 1, road from (93, −441) to (1060, 128) at 576.5 m) now ends at a low hub, 53.9 m at (636, 320). Seed 62 has the biggest climb of seeds 0–119 (546 m; next 98 at 543 m and 88 at 540 m). |
| Look at it | `cargo run --release -p procgena_terrain --example seam_spike -- <dir> 1.0` writes `two_terrains.ppm`; crop pixels 500–800 across and 350–650 down. The 3D viewer's data came from a temporary `scene_dump` probe in `tests/probes.rs` (2.5 m samples of that window with road tags). |

**What changed**

- [x] **Road levels from the real ground.** Each road point reads the
  bent terrain where it actually is, not the nearest cell.
- [x] **Cut-leaning level fit.** Road levels keep the deepest fill, and
  the deepest cut divided by 3, as small as the grades allow (a
  bisection on the gap, then a backward pass that keeps near a lightly
  smoothed ground). Roads cut deep sooner than they build high
  embankments.
- [x] **Cutting cost in the route search.** Steps over the steepest grade
  also pay for the cutting or filling they will need.
- [x] **Its own 8 m planning grid for roads** (rivers keep 25 m). Water
  cells are every road-grid cell a river passes through.
- [x] **Hairpins with a radius.** The search tracks the direction a route
  arrived by and never turns more than 45° per step: hairpins about
  10 m in radius. (On the 25 m grid a turn limit made routes steeper,
  so it only works at this scale.)
- [x] **Leg spacing.** Where two legs of one road come closer than three
  road widths (more than twice that apart along the road), the cells
  around the clash are blocked and the road is routed again, up to 8
  times; after that the last route is kept.
- [x] **Banks follow the terrain.** Each road point measures the natural
  ground's slope 20 m around it, averaged 20 m along the road; its banks
  start at twice that slope, between 0.6 (31°) and 20 (87°). Each bank
  is a flat face at that slope, steepening only near its 40 m reach so
  it always meets the ground. Crossing pieces keep 0.6, and each road
  eases in over its first 80 m, so both sides of a border carve the
  same ground. Banks are never gentler than before, so they never reach
  further.
- [x] **Road tags.** `Terrain::road_tags(p)` returns a `RoadTags` set:
  `road`; the bank's kind, `cut`, `embankment`, or `retaining_wall` for
  a fill of 65° or more; and its steepness, `bluff` (45° to 65°) or
  `cliff` (65° to 90°). Phase 5 turns these into labels.
- [x] **Road ends as points of interest.** `Terrain::points_of_interest()`
  lists the hub as a `PointOfInterest` of kind `RoadEnd`.
- [x] **Feature test:** roads may cut to any depth; embankments stay at
  most 10 m (decided 2026-11-22).

**Results**

- The blend-1 mountain road now winds up in rounded switchbacks with
  its legs apart. Every suite passes: features 7/7, seams 9/9,
  convergence 12/12, the doctest, and 66 unit tests. The feature-limits
  probe (the test worlds plus hills into plains with seeds 0 to 299, in
  all 12 configurations) finds no problems; it took 32 minutes, 12
  after the speed work. Rerun clean after dynamic width and the
  across-the-road level fit (2026-11-23).
- Cost: features went from 19 s to 66 s, seams from 61 s to 83 s and
  convergence from 3 s to 23 s, all from the road grid's heights and the
  direction-tracking search; the speed work below brought them back to
  38 s, 68 s and 10 s.

**Still to do**

- [x] **Dynamic road width** (2026-11-23). `FeatureLine` has a width per
  point (`widths`, replacing `width`); `RoadOptions::min_width` (default
  4 m, at most `width`) is the narrowest a road gets. Full width up to a
  3 m cut (at the highest ground across the road), narrowing smoothly to
  the minimum at 10 m, averaged over 20 m along the road, and eased in
  over the first 80 m so it leaves its crossing at the agreed width.
  Crossing pieces and rivers keep one width.
- [x] **Cut and fill measured across the road** (2026-11-23). The level fit
  takes the lowest and highest ground across each road point (its
  centre and both edges): fill from the lowest, cut from the highest. On
  a side slope the road sinks into the hillside. A separate "full bench"
  rule was not needed: steep fills become tagged retaining walls (decided
  earlier), and measuring at the edges already pushes the road into the
  slope.
- [ ] **Fill costs more than cut in the route search.** Deferred: the
  search sees only cell heights, not the levels the road will get, so
  it can't tell a step that will be cut from one that will be filled.
  The level fit already prefers cut 3:1.
- [ ] **Where the hub goes** (question 12): a low valley spot rather than a
  flat summit. Deferred by the user.
- [x] **Crossing a river must cost roads something** (user TODO,
  2026-11-23; built 2026-11-23, signed off). The search used to
  multiply a step onto a water cell by `RIVER_COST` (6), so roads crossed
  the same river twice to save a few metres, and diagonal and knight
  steps could hop a one-cell river for free. Now every grid move whose
  segment crosses a river line pays a bridge: `BRIDGE_COST` (300 m of
  level road) divided by the sine of its angle to the river (capped at
  5×), so a slanting bridge costs more. (A road never follows another
  over its bridge: the search stops at the first road it can join.) The
  per-cell water cost stays,
  so roads don't run along riverbeds. The road search now builds the
  water cells itself from the river lines (moved out of `plan.rs`).
  - In the feature test worlds: 89 road–river crossings before, 44
    after. Repeat crossings of one river: 15 before, 3 after, and those
    3 are one road crossing both arms of a river that loops back, with
    1.8 km of river between the crossings against 87 m of road.
  - Tests: `roads_cross_a_river_again_only_to_save_a_long_way_round`
    (features; a repeat crossing needs at least 600 m of river between,
    two bridges' worth), and unit tests
    `roads_go_round_a_river_bend_rather_than_cross_it_twice` and
    `roads_cross_a_river_in_their_way_once_and_near_square` (over 60°;
    a flat cost crossed at 52° by hopping the river with a knight step).
    All three fail with the bridge cost set to 0.
  - Preview v6: triple junction 5 bridges → 1 (its roads 536 m longer
    in all), two terrains at blend 1 2 → 1 (the road no longer crosses
    back); the other scenes are unchanged. Seams suite CPU time is the
    same with and without the bridge cost (231 s and 229 s).
  - `feature_limits` probe (300 seeds × 12 configurations): 5 problems,
    against 0 before. They are 2 rivers (seed 149 in four
    configurations, seed 4 in one) where a road now crosses within
    2.5 m of a confluence, and the road's level, about 1 m above the
    water, removes the water at the tributary's end, so the probe
    reports it as "ends nowhere". It isn't new: every road crossing
    removes its river's water, 89 of 89 before and 44 of 44 after
    (roads cut and fill through rivers until bridges exist). Fixed by
    Phase 4 bridges (the road stops cutting or filling over water). The
    probe is left unchanged.
  - Later: a bridge point of interest for the user to fill, and the
    bridge label of Phase 4.
- [ ] **Clearings at points of interest:** per ARCHITECTURE.md, a terrain
  height-stamp hook on a point of interest (user story 2), not a terrain
  option.
- [x] **Speed** (2026-11-23). Measured per region: road-grid heights
  80–170 ms, route search 0.15–3.5 s (worst in the mountains, where the
  leg-spacing re-route ran 9 searches and still gave up). Fixed:
  - the search is A*, heading for the nearest network cell with a
    distance that never overestimates (the last step onto the network at
    the road-sharing discount);
  - each leg-spacing retry blocks every crowded spot at once (the
    mountain road now takes 4 searches and ends uncrowded);
  - turn checks come from a 16 × 16 table instead of an angle per step;
  - with the `rayon` feature the stitcher builds its regions on several
    threads.
  Plains routes went from about 170 ms to 25 ms and mountain routes from
  2.2–3.5 s to 0.6–1.1 s. Suites: features 66 s → 38 s, seams 83 s →
  68 s, convergence 23 s → 10 s.
- [ ] **GPU implementation** (a follow-up, alongside the CPU version, which
  stays the reference): the embarrassingly parallel parts, which are
  every per-point evaluation (planning-grid heights, height tiles, later
  the SDF and meshing). The route search is a sequential priority-queue
  search and stays on the CPU.
- [ ] **Truly vertical faces** need the volumetric mesher (Phase 6); a
  height grid can only approximate them.

### River valleys and erosion (Phase 2.8, built in part)

Added 2026-11-22. Today a river cuts a parabolic bed with a fixed bank
shoulder (`channel()`), at levels that only ever step down. Two ideas
from the literature fix the two weak spots: valleys whose size doesn't
follow the cut, and river profiles with no natural shape. Both are
ideas from the papers (no code to port). Nothing below is settled: the
work lists are a first guess, to be rewritten after the spike.

**Step 1: spike.** Before any of the work below, a throwaway prototype
answers whether each idea earns its place, and how:

- [x] Valley profiles: try a cut-depth-scaled `channel()` (V and U shapes)
  on the river test worlds (`RIVER_SEEDS`, `JOIN_SEEDS`) and the
  two-terrain mountains; look at it in the 3D viewer and the
  `seam_spike` images.
- [x] Stream-power profiles: compute drainage area on one region's river
  grid and the steady-state profile along its longest river; compare
  with today's levels (shape, depth of cut, how tributaries meet).
- [x] Measure the cost of each (build time per region, suite times):
  drainage area 14–78 ms per region; valleys see "As built" below.
- [x] Write up what to keep, what to drop and what to change, rewrite the
  work lists below, and stop for review with the user.

**Spike results so far (2026-11-23).** From `src/spike_erosion.rs`
(test-only, ignored; deleted in G1, 2026-11-24, once its drainage and
cut-depth measurements moved to `src/drainage.rs`; the other spike probes
named below went with it, their results recorded here): one-region 4 km
worlds, seeds 1 to 6.

- **River cut below the natural ground today** (all river points):

  | Preset | Median | 90% | 99% | Max | Over 10 m | Over 20 m |
  |---|---|---|---|---|---|---|
  | Plains | 1.4 m | 2.7 m | 3.7 m | 4.0 m | 0% | 0% |
  | Rolling hills | 1.7 m | 4.9 m | 6.9 m | 8.2 m | 0% | 0% |
  | Mountains | 5.1 m | 20.8 m | 30.9 m | 62.5 m | 34% | 11% |

  Mountain rivers spend a third of their length in trenches over 10 m
  deep, cut with today's fixed bank: gorges. **Valleys scaled by cut depth
  are worth building.**
- **Mountain river profiles are badly shaped.** The longest rivers drop
  500–900 m in their first sixth, then run nearly level while the ground
  stays 20–40 m above them (upper-third slope 14 to 121 times the
  lower-third). Hills are milder (1.2 to 3.3 times).
- **Stream power as a drop-in for river levels doesn't work here.**
  - Drainage area along a traced river is not monotonic: often one cell
    (0.001 km²) mid-river, then a basin. Our tracing breaches ridges and
    doesn't follow the steepest-descent network, so area would have to
    be carried along each river (its inflow plus its tributaries).
  - Even so, the steady-state profile assumes land shaped by its rivers.
    Scaled to the same total drop, it floats up to 700 m above the
    mountains' noise ground. It needs the ground to conform to the river
    (idea 1, Génevaux), or belongs at world scale (WorldTerrain's
    elevation field).
  - **Stays analytic** (the user's rule, 2026-11-25): any version of
    stream-power levels, at region or World scale, must be closed-form on
    E or bounded work per region. No iterated flow, erosion passes or
    global solves.
- **Drainage area is cheap:** 14–78 ms per region on the 25 m river grid.
  It can size rivers (width, depth) and valleys and pick a river type,
  even without stream-power levels.
- **Proposal (reviewed by the user; see below):** build idea 1 first (valleys whose width
  grows with cut depth, shaped by river type, types chosen from slope and
  drainage area); smooth mountain river levels so they don't drop
  hundreds of metres at the source; leave stream-power levels for the
  world level. Not yet tried: the valley shapes themselves in the 3D
  viewer.

**Step 2: valleys scaled by cut depth (built 2026-11-23, review
pending).** Today's bank steepened to vertical within a fixed 40 m, so
any cut deeper than about 15 m became a near-vertical trench. Now:

- [x] Each river point has its own valley reach: 1.5 × its cut below the
  natural ground ÷ the wall slope (0.35, about 19°), between 40 m and
  300 m, averaged over 60 m along the river, and kept clear of the
  region's border (half the river's width plus 5 m), so the other side
  of a border never needs to carve it.
- [x] The valley wall is a flat 0.35 face from the water's edge,
  steepening only near its reach (the road banks' profile), so a 30 m
  cut opens about 86 m to each side instead of a gorge.
- [x] Crossing pieces keep the 40 m reach, so both sides carve the same
  ground at the border.
- [x] River levels are unchanged. Unit test `deep_cuts_open_into_valleys`;
  every suite passes, at the same speed as before (seams 71 s).
- [x] Valley shapes by river type (V, U, flat-floored), chosen from the
  river's slope and drainage area (Rosgen-style). Built from slope only
  (see "Vary the shape"); drainage area isn't used yet.
- [ ] Smoother mountain river profiles (no drop of hundreds of metres in
  the first stretch).
- [x] Look at the valleys in the 3D viewer and tune the wall slope and
  spare factor (the user's review below).

**Review of step 2 (user, 2026-11-23): it looks like a waterslide, not a
natural course.** The carve clamps the ground to one fixed wall profile,
so inside the valley all the terrain's detail is erased, and the same
symmetric cross-section is swept along a smooth river line. To fix:

- [x] **Keep the terrain on the walls** (built 2026-11-23, review
  pending): Génevaux's replace operator, changed so it only lowers. The
  ground is pulled towards the valley profile by a weight
  `(1 − (x/reach)²)²` (1 at the water's edge, 0 at the reach, flat at
  both ends): `height = h − w · max(h − profile, 0)`, the max rounded.
  The walls keep `(1 − w)` of the ground's own relief, more further out;
  the edge of the valley has no crease. Unit tests
  `deep_cuts_open_into_valleys` (rewritten: it asserted the flat 0.35
  wall the user rejected) and `valley_walls_keep_the_grounds_relief`.
- **Approved baseline (user, 2026-11-23: "looks great").** Viewer version
  6 (https://claude.ai/artifact/FiUubtQVBbd8eukkZTugHX), scenes dumped
  after the fade change and before any shape variation or gullies.
  Exactly:
  - Profile: bed parabola `depth` deep inside the water; outside, a
    straight wall from the water's edge at `RIVER_BANK` 0.35, no floor,
    the same on both sides and all along the river.
  - Pull: `w = (1 − (x/reach)²)²`, x measured from the water's edge;
    `height = h − w · smooth_max(h − profile, 0, RIVER_BLEND 0.5)`,
    taking the deepest lowering of all rivers and ponds.
  - Reach per river point: `VALLEY_SPARE 1.5 × cut ÷ 0.35`, clamped to
    `BANK_REACH`..`MAX_VALLEY_REACH` (300 m), averaged over
    `VALLEY_SMOOTHING` 60 m, kept `half width + VALLEY_CLEARANCE` (5 m)
    from the border. Crossing pieces and ponds: `BANK_REACH`.
  - River courses, levels and widths unchanged from Phase 2.7.
  Anything added on top (variation, gullies) is judged against this; if
  it looks worse, go back to these settings.
- [x] **Vary the shape** (built 2026-11-23, on top of the baseline; review
  in the separate "Varied Valleys Preview" viewer,
  https://claude.ai/artifact/A34crTYs6dkUce3qx1QYLo). Each river point
  has a `ValleyShape`: floor, wall and reach per side (`valley_shapes`
  in `plan.rs`).
  - River type: fall ≥ 4% (`STEEP_RIVER`) is a V with walls up to twice
    as steep and no floor; ≤ 1% (`GENTLE_RIVER`) gets a flat floor 3
    river widths wide each side (`FLOOR_WIDTHS`), corner rounded over
    8 m. Fall averaged over 100 m.
  - Wall slope per side = the ground's own steepness 20–60 m out on that
    side, clamped to 0.2–0.8.
  - Each side widens and narrows by ±35% over about 180 m
    (`VALLEY_WOBBLE`, `WOBBLE_LENGTH`), independently.
  - Bends (Peytavie type C): the inside is up to 40% wider and gentler,
    the outside narrower and steeper (`BEND_SHARE`, full at about 40 m
    radius).
  - Past a river line's end the two sides blend by angle, so their
    shapes meet without a step (found by `no_cracks_around_crossings`).
  - Crossing pieces and ponds keep the even 40 m shape.
- [x] **Side gullies feeding the rivers** (built 2026-11-23, idea 3 below,
  `src/gully.rs`): stripes aligned with the way to the nearest river,
  from jittered 40 m cells blended with Gaussian weights and
  renormalised, plus a half-size octave. Narrow grooves, broad spurs.
  Depth 15% of the wall's height above the water, at most 6 m; nothing on
  the floor or at the reach, strongest a third of the way up the wall;
  faded out within 10–40 m of roads. Only ever lowers.
  - No gullies on crossing pieces (`ValleyShape::gullies`): theirs are
    the only valleys that reach a border, and the road fade uses each
    region's own roads, so the two sides cut different gullies there
    (`every_preset_pair_joins_continuously` caught a slope mismatch).
  - Stripe weights fall to exactly zero at 1.2 cells, with pivots
    jittered at most 0.3 of a cell, so the 3 × 3 cells around a point
    hold every stripe that counts. A first version read a fixed block
    whose edge cells still carried weight, which left 0.17 m cracks
    along the 40 m cell lines (found by `no_cracks_around_a_triple_junction`).
- [x] **Check whether the waterslide stretches are the deep-cut ones**
  (the spike: a third of mountain river length cuts over 10 m). If they
  are, river placement is the root cause (see "What the papers say"
  below): rivers traced over noise cut trenches that a drainage-based
  layout wouldn't need.
  - **Measured (2026-11-23, `spike_what_makes_deep_cuts`; mountains,
    seeds 1 to 6).** 34% of river points cut more than 10 m. At those
    points the water is dead level: the median fall over 100 m is 0,
    against 0.09 over all points. They sit downstream: 502 in rivers'
    first thirds, 1356 in the middle, 1614 in the last.
  - **One cause, not two.** Half the deep-cut points have the ground
    rising downstream and half falling, but that split is what a single
    mechanism gives: water at one level passing a bump is deep-cut on
    both of its flanks. (A first reading here, "two causes about
    equally", was wrong.)
  - **The mechanism.** A river's level is the running minimum of the
    ground along it (`next_level`, `fit_to_ground`): it drops into every
    dip and stays there. The tracer escapes a hollow by cutting through
    its rim, up to `breach` metres above the water: 31.9 m for
    mountains (4 + 0.03 × (30 + 900)). So the trenches are the breach
    working as designed. No profile that never rises and never sits
    above the ground can be higher than the running minimum, so
    smoothing alone can't make the cuts shallower.
  - **Capping the cut floods dips instead** (`spike_capped_cut_profiles`,
    mountains, seeds 1 to 6; level = max(running minimum, highest ground
    downstream − freeboard − cap)). Counted along the river line only;
    real lakes would also spread sideways:

    | Cap | Points cut over 10 m | Lakes per region | Median length | Longest | Deepest |
    |---|---|---|---|---|---|
    | none (today) | 31% | 0 | — | — | — |
    | 20 m | 28% | 10 | 29 m | 213 m | 31.5 m |
    | 10 m | 10% | 22 | 42 m | 393 m | 41.5 m |
    | 5 m | 0% | 34 | 31 m | 631 m | 46.5 m |

  - **Options (for the user to choose):** leave it (the approved look
    already has these cuts; lakes at hollows are Phase 4), pull lakes
    forward (cap the cut and flood the dips: carve, water labels and
    float checks all change, and it needs its own preview), or lower
    `breach` (keeps "carving only lowers", but rivers end in ponds
    sooner, and that may undo crossings).
  - **Decided (user, 2026-11-23): leave it until lakes**, now milestone
    LK. LK's design does this: the breach shrinks to a few metres, and a
    river that can't cut through floods the hollow (ROADMAP, LK).

**As built (2026-11-23).**

- Built: valleys that keep the ground's relief (the approved baseline),
  varied shapes, gullies, and rivers stepping down at crossings (see
  "Rivers that stop short of a border").
- Review: the user looked at version 4 of the Varied Valleys Preview
  (shapes, gullies and the first stepped crossing) and said it "looks
  great" (2026-11-23). Version 5 adds the second mountain river crossing
  in two_terrains (the only scene that differs from version 4); the user
  approved it too (2026-11-23).
- Public API added: `INNER_DEPTHS`, `inner_depth`, `Port::downstream`,
  `Port::drop`; `EdgeOffer::inner` now holds 4 depths per sample.
- Tests changed in intent:
  - `deep_cuts_open_into_valleys` was rewritten, because it asserted the
    flat 0.35 wall the user rejected.
  - `no_crossing_whose_piece_floats_above_the_bent_ground` became
    `rivers_step_down_onto_low_ground_but_roads_dont_cross`: a river may
    now step down onto low ground.
  - The old guarantee, that a piece never floats above bent ground, is
    still checked by `piece_floats`, now at 4 depths on both sides (in
    `agree_ports`). Two tests exercise it: the reverse-direction refusal
    in that test, and `a_piece_steps_down_only_past_the_border_step`.
- New tests:
  - `gullied_valleys_have_no_steps_at_bends_joints_and_confluences`
    steps 1 cm at a time; the steepest is 1.2 cm per cm, against a limit
    of 3.
  - `gullies_cut_into_valley_walls_and_never_raise_them` fails if
    gullies are switched off by mistake (checked by breaking them on
    purpose).
- After the speed-ups, the scene dumps were bit-identical to those from
  before, and every suite and the probe passed.
- Then rivers from one side were allowed to cross 130 m apart (see
  "Rivers that stop short of a border"). Suites after that: lib 78,
  convergence 12, features 8, seams 9, doctest 1. The `feature_limits`
  probe reports 0 problems in all 12 configurations. Of the six scenes,
  only two_terrains changed.
- Cost (seams suite, CPU time):
  - Before the speed-ups: 414 s with gullies on, 213 s with them off, at
    a falling load average of about 20 to 15, so only a rough figure.
  - After, measured back to back at a load of about 4: 225 s on (90 s
    wall) against 203 s off (73 s wall), so gullies cost about 11%.
  - The speed-ups: stripes skip cells beyond their reach, and `apply`
    skips a gully that can't beat another river's lowering.

**3. Gully filter** (Johansen 2026, building on Fewes's and Clay John's
Shadertoys). It draws stripes of gullies along the slope using the exact
slope of the noise, evaluated point by point.

- **Licence (corrected 2026-11-23):** the blog's code and the
  `bevy_erosion_filter` crate are MPL-2.0. None of the sources we could
  read mentions CC BY-NC-SA; the Shadertoy pages (where that note came
  from: Shadertoy's default licence) couldn't be fetched. Unverified, so
  we still write our own from the description only.
- **Limitation:** its gullies don't join into a real drainage network.
- **Fix it gives:** tilting the gully direction toward the nearest river
  makes side gullies feed into the rivers. Cheap, and our own design. It
  breaks up smooth valley walls into spurs and gullies.
- **Cautions for our version:** fade to zero at the water, at the valley's
  reach and before the region's border (the neighbour doesn't know our
  rivers); keep it away from roads, whose levels were fitted to the
  ground before gullies; directions from "the nearest river" jump at
  confluences and inside bends, so look at those spots.

**What the papers say (read 2026-11-23).** We first worked from short
summaries; these notes are from the papers themselves.

- **Génevaux et al. 2013** (read in full). The whole terrain is built from
  the river network, not carved into existing terrain. Rivers grow
  upstream from river mouths on the coast, by a grammar using
  Horton–Strahler numbers; every new node is higher than its parent, by
  a slope map. Ridges sit on the Voronoi edges between river nodes, at
  `max(river heights) + λ·distance` with `λ ∈ [0, 0.25]` (a "terrain
  slope" map). Terrain primitives take their height from the nearest
  river point and the nearest ridge, plus noise whose amplitude grows
  with distance from the river and with the ridge's height above it
  (smooth valleys, rough mountains). Rivers are curves with a profile
  per Rosgen type, `h = river level + profile(signed distance)`, put in
  with a replace operator `h = (1 − w)·h_terrain + w·h_river`, weights
  `w = (1 − d²/r²)²`. River flow is `0.42·A^0.69` (A = drainage area,
  m², flow m³/s). Their limits: rivers can't adapt to large mountains
  with clear valleys; flat rivers can cut through high mountains when
  the slope maps vary sharply.
- **Peytavie et al. 2019** (read in full). Takes an existing heightfield.
  Rivers come from it: drainage area per cell (after filling hollows so
  every cell drains), and cells above an area threshold are the river
  network, smoothed into splines. Each stretch gets a Rosgen type from
  slope, flow and stream power (`A·S`): steep A/B are step-pools (basins
  and falls placed along the course, more on steeper stretches), C bends
  have an asymmetric cross-section (deep on the outside), D braids into
  channels. Cross-sections are unit-area templates scaled by `flow ÷
  velocity`. Scale: the riverbed, not the valley.
- **Galin et al. 2019 survey** (skimmed by topic, not read in full).
  Multi-fractal noise (higher octaves damped at low heights) gives
  smooth valley floors and rough peaks. Stream-power erosion
  (Cordonnier 2016) needs the whole grid.
- **Steer 2021, ESurf** (the Salève model; sections 1–5 read, section 6
  from a summary; CC BY 4.0). The steady-state river profile for `n = 1`
  is `z(l) = z(0) + U·∫ dl / (K·A^m)` from the outlet upstream, which
  needs the flow network and drainage area first. Hillslopes in that
  model are held at a critical angle (30° in its example).
- **Tzathas et al. 2024: abstract only.** Every copy we found was behind a
  bot check (Eurographics library, HAL). From the abstract: time becomes
  a slider for how far an input terrain ages, from light erosion to a
  fully formed range, solved on a grid with a multigrid iteration, plus
  landslides and hillslopes. Not per point.
- **Johansen 2026** (blog post and crate source read). Height plus its
  exact gradient goes in. Stripes are cosines across the downhill
  direction from jittered cell pivots, blended between cells; octaves
  follow the slope the previous octave left; a fade target (−1 valleys,
  +1 peaks) and a slope mask stop later octaves overwriting earlier
  ridges and creases. Fully per point. Its limits: gullies stop halfway
  down a slope, and height is a poor fade target in general.
- **What this means for us.** None of them is a recipe for valleys carved
  into noise terrain. Génevaux grows the terrain from the rivers, and
  Peytavie carves beds, not valleys. The replace operator, the compact
  weight, and noise damped near rivers carry over directly; that is
  what "keep the terrain on the walls" above builds. For river placement, both trace rivers from
  drainage (after filling hollows) rather than walking downhill and
  breaching rims as ours do. That fits the waterslides: our mountain
  rivers cut deep trenches where a drainage-based layout would follow
  existing valleys.

**Rivers that stop short of a border (found 2026-11-23).** In the
two-terrain world, two mountain rivers end in ponds just short of the
bend in the border, though the plains below have a pond waiting.

- **Cause:** both ask to cross, and every check passes but one. Their
  crossing piece is straight, carrying the border's own cross-slope
  inward, and the plains' ground 60 m in sits 6 m *below* the piece's end
  (the limit is 4 m), because the mountains-to-plains ramp is steeper
  than the piece. The negotiation refuses rather than letting the river
  float.
- **Second cause:** the two wishes are 150 m apart, and crossings had to
  be 195 m apart (`PORT_SPACING`), so even with the first fixed only one
  crossed. Fixed with `RIVER_SPACING` (see below).
- **Checked:** allowing that crossing, the river continues on the plains
  side, dropping from 69 m to 19.7 m, and joins the plains river right at
  its pond.
- **Fix:**
  - [x] (built 2026-11-23) The step down is part of the agreement, so both
    sides carve the identical piece. `Port` gains `downstream` (the side
    the river flows into) and `drop` (how far below the agreed slope the
    piece's downstream end sits: as far as the receiving side's bent
    ground has fallen). `Port::level_at` eases the drop in past the step
    that straddles the border, so the border level never changes. The
    negotiation refuses only when the receiving ground is more than 4 m
    *above* the piece (`piece_lands`); roads and the sending side keep
    the two-sided check.
  - [x] Each side now reports its ground at 4 depths (15, 30, 45 and 60 m;
    `EdgeOffer::inner` is `Vec<[f32; INNER_DEPTHS]>`, `inner_depth`), and
    a river crossing is refused if its water would sit more than 2 m
    above either side's bent ground anywhere along the piece
    (`piece_floats`). This closed an old gap: only the piece's far end
    was checked, and one newly allowed crossing floated 4.5 m over a dip
    30 m in on the sending side.
  - [x] Tests: `no_crossing_whose_piece_floats_above_the_bent_ground`
    became `rivers_step_down_onto_low_ground_but_roads_dont_cross`; new
    `a_piece_steps_down_only_past_the_border_step`.
  - Tried first and dropped: stepping down only in the receiving region's
    copy of the piece. The stepped segments are within carving reach of
    the border, so the two sides carved differently there (a 0.17 m
    crack).
  - [x] (built 2026-11-23, signed off) The second river crosses too.
    Two rivers flowing across an edge from the same side may now cross
    130 m apart (`RIVER_SPACING`, 2 × `FEATURE_CLEARANCE`). Every other
    pair of crossings keeps 195 m (`PORT_SPACING`). In two_terrains the
    second mountain river crosses at 2550 m: it steps down from 67.5 m to
    61.5 m on its piece and joins the first river's inbound stretch at
    21.9 m, about 60 m before that stretch meets the plains river by the
    pond (19.7 m). No other test world changed. Tests:
    `rivers_from_one_side_may_cross_closer_than_other_crossings`
    (fails with the old spacing) and
    `neighbouring_mountain_rivers_both_cross_and_join_below`.
  - Tried first and dropped: carrying a refused river along on its own
    side to join the river that won, by tracing on from its end. It
    can't work here, because the refused river is already lower than the
    winner upstream of its crossing (69.2 m against 72.1 m). Of the five
    refused rivers in the test worlds, none joined anything; the traces
    wandered up to 2.9 km and stopped in hollows.

**1. Valleys from the distance to the river** (Génevaux et al. 2013,
*Terrain Generation Using Procedural Models Based on Hydrology*;
Peytavie et al. 2019, *Procedural Riverscapes*).

- **How it works:** the rivers are designed first, and the ground near
  a river comes from its distance to the nearest river plus that river's
  level. Peytavie carves the riverbed from shaped pieces placed along
  the course, sized from the terrain and the river's type, using the
  Rosgen river classification: from slow meandering rivers to fast
  rapids.
- **Fit:** very close to our design. Rivers already come first, and
  every point is still computed on its own.
- **What it fixes:** the valley's width grows with how deep the cut is.
  A shallow cut gives a narrow channel; a deep cut gives a wide V- or
  U-shaped valley instead of a gorge.
- **Work:**
  - [x] `channel()` takes a valley profile whose width depends on the cut
    depth (the river level against the uncarved ground), not a fixed
    shoulder.
  - [x] (Simplified: steep V, middle, gentle flat floor, from slope
    only.) A river type per stretch (Rosgen-style: steep step-pool, riffle
    and run, meandering), chosen from the stretch's slope and the
    terrain, picks the bed and valley shape.
  - [x] (`gullied_valleys_have_no_steps_at_bends_joints_and_confluences`,
    `deep_cuts_open_into_valleys`, the seams suite.) Tests: no cracks where valleys of two rivers overlap or a
    tributary joins; valley width grows with cut depth; across a border
    both sides carve the same valley (crossing pieces keep agreed
    shapes, as road banks do).

**2. Analytic stream-power erosion** (Tzathas, Gailleton, Steer and
Cordonnier, Eurographics 2024, *Physically-based Analytical Erosion for
fast Terrain Generation*; the Salève model, ESurf 2021).

- **How it works:** the standard erosion law is solved with a formula,
  giving the eroded terrain at any chosen time without stepping through
  it. At steady state, the height along a flow path is an integral of
  `(U / (K·A^m))^(1/n)`: A is the area draining into each point, U the
  uplift rate, K the erosion strength, and m and n the law's exponents.
  The Salève model gives the same steady-state formula and adds that
  the drainage network itself still has to be computed numerically.
- **Fit:** we already trace rivers on a coarse grid per region, so
  drainage area can be computed per region, with no global pass.
- **What it fixes:** river profiles steep near the source and
  flattening downstream, instead of today's simple step-down. Tributary
  valleys get matching slopes for free.
- **Work:**
  - [ ] Drainage area A per cell on the river grid (flow accumulation
    along the traced flow directions).
  - [ ] River levels from the steady-state integral, still falling
    monotonically and still meeting the agreed level at every border
    crossing (a crossing fixes one point of the profile, as a road's
    crossing fixes its start level).
  - [ ] Options for U, K, m and n (presets: steep young mountains, worn
    old hills).
  - [ ] Tests: profiles concave (steep at the source, flatter downstream);
    a tributary meets its trunk at the trunk's level and slope; the
    existing river suite still passes.
- **Open:** whether drainage area should count water flowing in from
  neighbours (a river crossing in brings its upstream area with it as
  part of the agreement), and how this fits the world generator's
  drainage tilt (`world_plan.md`).

**As built in G1 (2026-11-24): measured, not wired in.** Any profile that
never rises and never floats sits at or below today's running minimum, so
stream-power levels can only deepen cuts, which the user decided to leave
until lakes, and the approved look keeps levels and widths unchanged. So
G1 builds the tools and measures, and changes no height (the bit-for-bit
dump of five test worlds is unchanged).

- [x] Drainage area per cell on a region's 25 m grid (steepest-descent
  accumulation) and carried along each river, never falling, with each
  tributary's area added to its trunk below the join (`src/drainage.rs`,
  test-only for now; tests
  `a_tilted_plane_drains_downhill_and_keeps_every_square_metre`,
  `areas_never_fall_along_a_river_and_add_up_at_a_join`).
- [x] The steady-state profile through a river's ends
  (`steady_profile`, concavity 0.45), as a measurement
  (`steady_profiles_are_concave_and_meet_their_ends`).
- [ ] River levels from it: **waiting (the user, 2026-11-24).** See the
  exit measurements: in mountains it would cut up to 176 m deeper than
  today. It needs the ground to conform to the river (Génevaux), or lakes
  (G3), or the world level's elevation field. The user's constraint for
  any version: keep it analytic and fast. Drainage and erosion easily
  grow into simulation (iterated flow, repeated erosion passes, global
  solves); this must stay closed-form or bounded per-region work.
- [ ] Options for U, K, m and n: not added, since nothing reads them yet.
- [ ] Smoother mountain river profiles: the same question as river levels
  (today's lower thirds are dead level, see below); left with it.
- [ ] Sizing rivers (width, depth) and picking river types from drainage
  area: changes the approved look, so it waits for a review. Costs about
  20 ms per region.

**Phase 2.8 exit measurements (2026-11-24).** One-region 4 km worlds,
seeds 1 to 6 per preset, `drainage::tests::measure_phase_2_8_exit`. The
machine was heavily loaded (load average about 60), so timings are upper
bounds.

| Preset | River points | Cut median | 90% | 99% | Max | Over 10 m | Over 20 m |
|---|---|---|---|---|---|---|---|
| Plains | 12 365 | 1.4 m | 3.6 m | 4.8 m | 5.1 m | 0% | 0% |
| Rolling hills | 856 | 1.5 m | 4.7 m | 6.4 m | 7.1 m | 0% | 0% |
| Mountains | 7 763 | 6.4 m | 27.3 m | 31.7 m | 54.4 m | 41% | 19% |

- **Extra cut if levels followed the steady profile** (kept under today's
  levels): plains median 0, 90% 1.6 m, max 5.1 m; rolling hills median
  1.4 m, 90% 6.0 m, max 12.4 m; mountains median 0, 90% 27.9 m, 99%
  115 m, max 176 m.
- **Concavity of the longest river** (upper-third slope over lower-third
  slope), today / steady: plains 1.7/2.3, 6.5/2.3, 1.3/1.7, 6.5/2.0,
  8.5/2.4, 3693/1.7; rolling hills 3.6/5.9, 4.6/7.4, 8.1/7.8, 3.0/3.1;
  mountains 114/3.2, 4.2·10⁵/3.1, 1.4·10⁶/5.7, 15.6/3.2, 5.9·10⁵/3.7,
  42/2.3. Mountain rivers' lower thirds are dead level today (the running
  minimum after a breach); hills are already about as concave as stream
  power would make them.
- **Drainage area cost:** 14–37 ms per region (cells and carrying along
  rivers).
- Valleys: built before G1 (see "Step 2" above); their measurements stand.

### Re-recorded after the string-key seed salts (2026-11-23)

Seed salts became named string keys (`hash::KEYS`, FNV-1a), and the
stated relief amplitude now counts erosion (see below). Both change
every world, so the fixtures were re-picked and the figures re-measured.
Earlier figures in this plan describe the worlds as they were then.

- **Stated relief amplitude counts erosion.** `layer_spreads` multiplies
  the hills' spread by `erosion_damping(erosion)`, a table measured by
  the ignored test `measure_erosion_damping` (fBm at roughness 0.5,
  6 octaves, 400 000 samples: 0.87 at erosion 0.25, 0.67 at 1, 0.56 at 2,
  0.30 at 10; octave count makes no difference, roughness 0.4–0.6 under
  1.5 %). Stated over measured amplitude, 40 seeds per preset: rolling
  hills 1.29 (1.28–1.32) → 1.00 (0.99–1.02); plains 1.01 → 1.00;
  mountains 1.10 → 1.10. The test `stated_relief_amplitude_matches_the_ground`
  now checks 10 seeds per preset within 15 % (was seed 3 alone within
  30 %, which rolling hills had only just passed).
- **Re-picked seeds** (old → new):
  - `RIVER_SEEDS` [81, 293] → [102, 173]: the only seeds in 0–299 with a
    river crossing in all 12 configurations (9 to 37 of 300 per
    configuration).
  - `JOIN_SEEDS` [64, 136] → [56, 65]: the sharpest bends where one
    plains river joins another, 83° with shared seeds and 87° with
    unrelated seeds (the probe `join_bend_seeds`, new).
  - `neighbouring_mountain_rivers_both_cross_and_join_below`: the fixed
    two-terrain world → plains and mountains with seed 30 (crossings
    140 m apart, both joining a river); the test now finds the closest
    same-side pair and checks it is under 3 × the feature clearance.
  - The mountain road test case: seed 1 → seed 62 (see the table under
    "Road rework").
  - Preview scenes: hills into plains 81 → 102 and 64 → 56; the flat
    and tilted pair 33 → 15 (no crossing flat, two on the tilt).
- **Border gap** (`border_gap`, plains–mountains, mean / largest):
  default world (shared seeds, meeting −1, blend 0.5) 50 / 54 m; at
  meeting −∞ 50 / 52 m, 0 60 / 73 m, 1 148 / 265 m; blend 0 100 / 107 m;
  blend 1 0 / 0 m. Unrelated seeds: 50 / 54, 50 / 57, 65 / 101,
  139 / 274, 100 / 114 and 0 / 0 m.
- **River crossings on a slope** (`river_crossings_on_a_slope`): flat
  24 → 20, tilted down towards the plains 65 → 64, up towards them
  6 → 4. Measured again on 2026-11-24 (master after G6, before and after
  the offers moved to the exact slope, the same both times): flat 19,
  down 63, up 4. Some later change between these records moved them
  (not the exact slope).
- **Bridges in the feature test worlds** (12 configurations): road–river
  crossings 44 → 33, all of them still removing the water; rivers
  crossed twice by one road 3 → 2, each with over 1.5 km of river between
  the crossings (none under 600 m).
- **`feature_limits`** (300 seeds × 12 configurations): 5 problems → 5,
  now one river (seed 60, in five configurations) instead of seeds 149
  and 4: the same cause, a road crossing 4.2 m from a confluence, its
  level about 0.9 m above the water. Fixed by Phase 4 bridges.

### Alignment with ARCHITECTURE.md

Reviewed 2026-11-23 against the workspace's `ARCHITECTURE.md`. Terrain is
level 4 there, beside the Biome; the stitcher grows into WorldTerrain.
What that asks of this crate, and where it stands:

- [x] **Terrain is only terrain.** No water material, no scatter hooks:
  the crate says where water and roads go, and labels the ground.
- [x] **Points of interest are where stamps go.** `points_of_interest()`
  gives them; a clearing is a height stamp (a Terrain "inputs" hook),
  kept out of the border margin.
- [x] **CPU first, GPU alongside.** The CPU path stays the reference; a
  GPU implementation of the per-point work is a later follow-up (see
  Phase 2.7).
- [x] **"Two terrains", not "two biomes".** A Biome is the level above
  that binds a Terrain to its Ecotopes.
- [x] **Slope as part of the ground** (G2, built 2026-11-24).
  `Terrain` implements `procgena_core::Ground` with a gradient
  (`Terrain::sample`), and `HeightTile::sample_with_gradients` fills a
  gradient grid in the same pass (opt-in since G3 step 2: it costs 3–5
  times the heights). Labels' `Measures::slope` will use the same
  gradient.
  - Analytic: relief (fBm through the noise's gradient; eroded hills
    through its second derivatives; ridged mountains, which take one
    side's slope on a sharp crest), the fade's scaling and blend, E, the
    shared border detail, and all carving (valley profile and pull,
    gullies, road cuts and fills, the smooth minimum and maximum).
  - By differences: the soft distance and mixed targets of the border's
    and the fade's pulls, 0.5 m either side in f64 (`pull_gradients`). The
    pulls' closed forms move with the point in ways not worth deriving.
  - Heights were bit-identical to before. The border offers then still
    read the old central-difference `Relief::gradient`, which averages
    across the ridges' sharp crests where a point slope jumps (from 1.97 to
    −1.0 across one measured crest).
  - **Offers on the exact slope** (decided by the user and built
    2026-11-24). Edge and corner offers now read `Relief::slope`, and the
    central-difference `Relief::gradient` is gone. On the offer samples of
    four worlds (hills into plains, three plains–mountains seeds) the two
    slopes differ by 7e-5 at p50 and 0.014 at most: the fade has already
    flattened the mountains' crests by the time they reach the border.
    Across the test worlds (two terrains straight and warped, the triple
    junction, hills into plains with `RIVER_SEEDS` and `JOIN_SEEDS`) the
    agreed profiles moved by 0.5 mm on average and 3.3 cm at most in
    height, and 0.0013 at most in slope; no port appeared or vanished.
    Every seam, crack, feature and world test passed unchanged, no
    expected value moved, and no seed needed re-picking (`RIVER_SEEDS`
    still cross in every configuration, `JOIN_SEEDS` are still the
    sharpest joins, seed 217's two mountain rivers still cross). The
    border gap, river-crossing, join-bend and feature-limit probes print
    the same figures before and after (see "Re-recorded after the
    string-key seed salts" for the slope-crossing figures, which had
    drifted before this change).
  - Cost (cold region bench, old and new binaries run back to back, load
    about 25–30): offers p50 30 → 31 ms, p95 68 → 80 ms (cold ÷ warm
    0.7 → 0.8); a whole region plan 631 → 644 ms at p50, within the
    noise. A busier pair (load about 45) showed the same picture.
  - Precision: mountain heights carry up to about 5e-3 m of f32 rounding,
    so tests compare the slope with differences over 4 m spans
    (`tests/ground.rs`, `relief::tests::slopes_match_the_heights`).
  - Cost: a sample near a border costs several pull evaluations (5 for the
    bend, 5 for each fade blend); G3 step 2's batched path is the place to
    make that cheaper.
- [ ] **Labels for Biome rules.** `annotate` gives the primary `Landform`,
  `Flags` (WET, SNOW, ROAD …) and `Measures` (slope, height above water,
  distance to water or road). Today's `RoadTags` (road, cut, embankment,
  retaining wall, bluff, cliff) become provenance labels and `Flags`
  there, with the tag names living in `procgena_specimen::tags`. Distance to
  road and to water can come from the features' spatial index.
- [ ] **Seeds from outside.** `TerrainOptions::seed` becomes
  `default_seed`, used only when a terrain is built alone; inside a
  World the seed is a hash of the parent seed and the region's id.
- [ ] **Hooks** (Rust only, pure, `Send + Sync`, keyed): height stamps
  outside the margin as inputs, extra labels as outputs; and for
  WorldTerrain, extra seam constraints (a road that must cross a given
  border).
- [ ] **Content hash** of the options (floats by their bits, as tree's
  `tree_id`), for caching.

## Phases

| Phase | Work | Depends on |
|---|---|---|
| 0 | **Coordinator only:** add both crates to the root `Cargo.toml`; add `terrain/prior_art/` to `.gitignore` and the `_typos.toml` excludes; add `CLAUDE.md` package rows and a README row; add the generic tags to `procgena_specimen/src/tag.rs`. | — |
| 1 ✅ | **Seam spike (2D heightfield only).** Done 2026-11-22: three presets (plains, rolling hills, mountains); continuity in height and slope tested for every pair, at a triple junction and at 10⁷ m. Signed off by the user 2026-11-23. Hash, noise with derivatives, calibration, a minimal base relief for two presets, `Region` types, offers, `negotiate`, `negotiate_corner`, conformance, the reference stitcher, and an image dump of two terrains and a triple junction. Includes the symmetry, seam and triple-junction tests. | 0 |
| 2 ✅ | River and road ports across one edge in the spike (monotone check, fallback ending). Done 2026-11-22, see "Phase 2 as built". Signed off 2026-11-23. | 1 |
| 2.5 ✅ | **Shared world noise and relief convergence** (built 2026-11-22; exit results recorded under "Relief convergence (Phase 2.5)") (replaces the relief taper; see "Relief convergence (Phase 2.5)" above): layer seeds come from the world seed and the layer's role, not the region seed; near each edge both sides fade their relief toward one agreed relief amplitude, so same-family neighbours match before negotiation. Exit: the border gap probe (239 m mean, plains–mountains) and the crossing count (6 in 300 seeds) re-run with both on. Done already: 16-direction river and road routing, 3 smoothing passes, rivers merge on meeting and never double back. | 2 |
| 2.6 ✅ | **Relief blending** (built 2026-11-22, signed off 2026-11-23) (see "Relief blending (Phase 2.6)" above): near borders each terrain also fades towards a weighted mix of its neighbours' actual height fields; the world option `relief_blend` (0 to 1) sets how much of that against Phase 2.5's scaling. Exit: border gap, the triple-junction road and crossing counts measured at blend 0, 0.5 and 1. | 2.5 |
| 2.7 ✅ | **Road rework** (built 2026-11-22, signed off 2026-11-23; see "Road rework (Phase 2.7)" above): road levels from the real ground with a cut-leaning fit, an 8 m road grid, hairpins with a radius and spaced legs, banks that steepen with the terrain into flat faces, road tags (cut, embankment, retaining wall, bluff, cliff) and road-end points of interest; cut and fill measured across the road; roads narrow through deep cuts (`min_width`); sped up with an A* search and parallel region builds. Still to do: hub placement (deferred); a GPU implementation is a later follow-up. | 2.6 |
| 2.8 ✅ | **River valleys and erosion** (built 2026-11-23 and 2026-11-24 (G1); exit measurements recorded under "As built in G1"). Built: valleys from the distance to the river that keep the ground's relief, wider for deeper cuts, shaped by river type (from slope); gullies; rivers stepping down at crossings; rivers from one side crossing closer together; drainage area per region and along rivers, and the steady-state stream-power profile, as measurements. Deep cuts come from rivers breaching hollow rims, and wait for lakes (ROADMAP LK). For the user: whether river levels, widths and types should follow drainage area (stream-power levels would cut up to 176 m deeper in mountains). | 2.7 |
| W | **WorldTerrain** (the big follow-up): world elevation field E (its slope is the drainage tilt, its zero the sea level), region graph and region edges, biome choice. See `terrain/world_plan.md`. | 2.6 |
| 3 | Full options and presets as data, base relief, landforms, analytic erosion | 1 |
| 4 | Drainage, rivers, roads, **bridges** (where a road crosses a river it stops cutting or filling and carries a bridge label), with the spatial index. Lakes at hollows are ROADMAP milestone LK. | 2, 3 |
| 5 | Classification, `annotate`, `HeightTile` | 3 (water labels after 4) |
| 6 | SDF, volumetric modifiers, top-surface search | 3 |
| 7 | Chunk mesher: band, marching cubes, Transvoxel transitions (after the licence check), parts by label, water parts | 5, 6 |
| 8 | Bevy crate: streaming, material, debug overlay, viewer (no water material: the user brings their own) | 7 |
| 9 | Exports (OBJ, glTF, raw heightmaps) | 7 (in parallel with 8) |
| 10 | `terrain/textures.yml` plus a tools entry; studio Terrain tab | 8 |
| 11 | Docs, READMEs, polish | all |

Phases 3 and 6 can run in parallel after phase 1, with one agent each. They
touch separate modules.

**Terrain features** ([features_plan.md](features_plan.md), reviewed in
[features_plan_review.md](features_plan_review.md)) add two steps at the
start of G3, before Phase 3's landforms and before Phase 4:

1. Rivers, roads and ponds become one feature type with one index. Their
   results may change where the new structure makes that simpler (the
   repo is early); any test whose expected values move is reported and
   re-checked for its intent, never loosened.
   **Built 2026-11-24 (`src/feature.rs`), results unchanged** (heights,
   gradients and every feature bit-identical on five test worlds; no test
   moved). `FeatureId` (kind and key; the key became stable in step 2),
   `TerrainFeature` with its `FeatureShape` and bounds,
   `Terrain::features`, `feature` and `features_within`, and one
   `FeatureIndex` of feature pieces with their effect boxes, which
   carving reads. Rivers, ponds and roads keep their own lists, so
   `rivers()`, `ponds()` and `roads()` still return slices, and step 2
   can cache hydrology and roads as separate plans. Carving keeps its two
   stages (rivers and ponds lower, then roads hold), since gullies need
   the nearest road first. Tests `the_index_gives_what_every_feature_gives`
   and `no_feature_changes_anything_outside_its_bounds`.
2. Planning is split into stages that are cached on their own, with
   stable feature ids, one batched sample path, and cold and warm
   benchmarks, so a far view needs relief and seam offers but not inland
   road routing.
   **Built 2026-11-24, results unchanged** (the same bit-for-bit dump).
   - **Stages:** `TerrainGenerator::trace_rivers` gives a `RiverTrace`
     (cheap to clone) that `offers_traced` and `build_traced` share, so
     a region is traced once (`StitchedWorld` now does). `build_traced`
     plans rivers and ponds but not roads; `Terrain::with_roads` adds them
     without redoing anything (`has_roads` says which). `build` is all
     three. Inside, `plan::hydrology` and `plan::roads` are separate.
   - **Stable ids:** `FeatureId` is the kind and a hash of where the
     feature starts (its first two points, or a pond's centre), so ids
     don't change when roads are added or other features come and go.
   - **One sample path:** `Terrain::query(p, gradient)` gives height,
     water, road tags and optionally the gradient from one bend and one
     carve; `height`, `water_level`, `road_tags` and `sample` all go
     through it, and the border's pull at `p` is computed once for height
     and gradient. `HeightTile::sample` is heights only again;
     `sample_with_gradients` adds the gradient grid.
   - Labels (`annotate`, G4) will join `query`.
   - Tests: `roads_come_as_a_stage_of_their_own_and_ids_stay`,
     `one_query_gives_what_each_accessor_gives`.

   **Benchmarks (2026-11-24): provisional.** Measured on a shared machine
   while other lanes built (load average 8 to 10 over 1 minute, 30 to 45
   over 15 minutes); to be rerun on a quiet machine. Rerun with:

   ```bash
   cargo test -p procgena_terrain --release --test bench -- --ignored --nocapture --test-threads 1
   ```

   It prints the load average beside each table. Worlds: two terrains and
   the triple junction, straight and warped, and hills into plains (12
   regions of a few km², plains, hills and mountains).

   **Cold ÷ warm** (survives load better than raw times): each stage's
   median over the median time of a 64 × 64 tile of heights at 1 m on one
   thread on the same terrains, in the same run (30 ms here). River trace
   1.6, offers 0.8, bend and hydrology 1.4, roads 12.4; **a whole region
   plan 16.5 tiles, 4.0 without roads.**

   | Cold, per region (load ~8) | p50 | p95 | max |
   |---|---|---|---|
   | River trace | 48 ms | 210 ms | 304 ms |
   | Offers (from the trace) | 23 ms | 56 ms | 94 ms |
   | Bend and hydrology | 42 ms | 165 ms | 238 ms |
   | Roads | 378 ms | 1597 ms | 2289 ms |
   | Whole plan | 501 ms | 2028 ms | 2925 ms |
   | Without roads | 123 ms | 431 ms | 637 ms |
   | Whole world ÷ regions (negotiation, threads) | 334 ms | 1065 ms | — |

   | Warm, per call (load ~8) | p50 | p95 | p99 |
   |---|---|---|---|
   | `height` | 1.5 µs | 23 µs | 38 µs |
   | `query` without gradient | 1.3 µs | 20 µs | 34 µs |
   | `sample` (with gradient) | 5.3 µs | 139 µs | 208 µs |
   | `height` within 200 m of a border | 6.1 µs | 32 µs | 44 µs |
   | `sample` within 200 m of a border | 29 µs | 162 µs | 234 µs |

   | Warm tile, 64 × 64, rayon (load ~8) | p50 | p95 |
   |---|---|---|
   | 1 m step, heights | 3.7 ms | 16 ms |
   | 1 m step, with gradients | 14 ms | 111 ms |
   | 8 m step, heights | 3.5 ms | 20 ms |
   | 8 m step, with gradients | 18 ms | 132 ms |

   - **Roads are most of a cold region plan** (p95 1.6 s, mountain
     routing). A far view built with `build_traced` (trace, offers,
     hydrology) needs about 0.1 s at p50 and 0.4 s at p95. Whether that
     calls for macro cells (Conflict 4) is the user's call; nothing here
     needs them for the hydrology.
   - **The gradient costs 3–5 times the height**, near borders and in
     the fade from the pull differences (5 pulls for the bend and 5 for
     each fade blend) and the relief layers worked out again. The pull's
     own closed-form derivative would cut most of it; not done.

The review's mapping table shows where the proposal's later steps fall in
Phases 3 to 7. The user settled its four open points on 2026-11-23 (see
the review's "Decisions"):

- Phases 6 and 7 mesh a heightfield by default, with volume patches only
  for caves and overhangs;
- rock outcrops in Phase 6 are meshes the terrain builds;
- lakes are ROADMAP milestone LK;
- Phase 4 plans per region (macro cells were dropped, 2026-11-25).

Step 2's one sample path is where Phase 5's `annotate` and the gradient
for `Ground` (roadmap G2) belong; the review's "Milestone order" gives the
order. Bridges keep their Phase 4 water fix (the road stops cutting or
filling over water); only the deck mesh moves to the proposal's step 5.
The worlds signed off on 2026-11-23 stay identical through steps 1 and 2.

## Non-goals for v1

- Simulated erosion of any kind (the user chose analytic only).
- River flow accumulating across region borders.
- A GPU evaluation path (the design keeps it possible).
- Building the world generator itself (only the reference stitcher).
- Physics colliders (the meshes are usable for them, but there is no
  integration).

## Open questions

1. ~~**Volumetric reading.**~~ **Answered 2026-11-23:** the SDF with a
   heightfield base and a top-surface heightmap API stands, and meshing
   is a heightfield by default with volume patches only for caves and
   overhangs (see [features_plan_review.md](features_plan_review.md),
   "Decisions"). Question 5 now only matters for those patches.
2. ~~**Sea level.**~~ **Answered 2026-11-22:** oceans are not this crate's
   concern. Sea level is the zero of `WorldTerrain`'s elevation field E,
   used for labels and coasts; an ocean is just a terrain type below it.
   Lakes stay local.
3. ~~**River flow across borders.**~~ **Answered 2026-11-22: acceptable.**
   A river that crosses into a neighbour ends there or merges into the
   neighbour's own drainage. Its width and flow do not add up downstream
   across regions.
4. **Tags.** Which terrain tags go into `procgena_specimen` (shared), and which
   stay in `procgena_terrain`?
5. **Transvoxel licence.** Lengyel's terms for the tables were not found.
   If they are not permissive: our own surface nets with skirts, or the
   `transvoxel` crate (MIT/Apache) as a new dependency?
6. **Rock outcrops.** **Answered 2026-11-22:** a Voronoi-rock SDF in the
   terrain; procgena_stone scatter is out of scope (this plan is just
   terrain). **Changed 2026-11-23 (user):** outcrops are rock meshes built
   by the terrain and laid into the ground, not solids in the volume. The
   terrain still does not depend on `procgena_stone`.
7. **Edge not yet known.** When a neighbour region appears later, chunks
   within the margin rebuild. Is that acceptable for the world generator,
   or must the region graph be known before generation?
8. ~~**Convergence agreement rule.**~~ **Answered 2026-11-22:** both, as
   the world option `relief_meeting`, the exponent of a power mean
   (−∞ = rougher side only, −1 = in proportion to roughness, the default,
   1 = meet in the middle). See "Relief convergence (Phase 2.5)".
9. **Per-region variation.** With shared world noise a region cannot be
   rerolled alone. Is an optional seeded variation layer (fading out at
   edges) needed?
10. **Calmer blended borders.** Mixing two uncorrelated fields lowers the
    relief (about 0.71 times at equal weights). Accept it, or scale the
    blend's variation back up where the fields are known to be
    uncorrelated (different seeds or different wavelengths per layer)?
11. **`relief_blend` default and test coverage.** Default **answered
    2026-11-22: 0.5.** Test coverage, unless the user says otherwise:
    every configuration at 0.5, plus the seam and feature suites at 0
    and 1 for the default meeting exponent.
12. **Road ends as points of interest.** Decided 2026-11-22: the end of a
    region's road network (its hub) is a `PointOfInterest` of kind
    `RoadEnd`, listed by `Terrain::points_of_interest()`, for the caller
    to fill. Built as data only. Phase 5's classification gives it a
    provenance label. Still open: whether the terrain flattens a clearing
    there (perhaps a setting); where the hub goes (item 4 of the road
    fixes: it is chosen for flatness and centrality and ignores height,
    so a lone crossing in mountains climbs to a summit plateau).
13. **Roads with no route within grade.** Found 2026-11-22: in the
    blend-1 mountains and one triple-junction region, no route from the
    crossing to the hub stays within `max_grade` on the 25 m grid (with
    a hard cap the road isn't built at all). **Decided 2026-11-22:**
    roads cut as deep as they need, so the route search keeps its soft
    grade costs plus a cost for expected cutting and filling. Banks
    steepen with the terrain up to near vertical, and `Terrain::road_tags`
    marks them (see "Road rework (Phase 2.7)"). Embankments stay capped
    at 10 m in the feature test, and road levels take 3 m of cut to save
    1 m of fill. Moving the hub is still open (question 12).
14. **Switchbacks.** **Answered 2026-11-22** by the 8 m road grid, a 45°
    turn limit per step and the leg-spacing re-route (see "Road rework
    (Phase 2.7)").

## Maybe later

- **Edge heights in offers, to steer rivers.** Each offer already carries
  the side's own height along the edge, so both sides know each other's
  average edge height without any chaining. A region could steer its own
  rivers towards borders where the neighbour is lower, rather than only
  where its own relief happens to lead. It is cheaper than a drainage tilt
  from the world generator, but weaker: it knows only the edge, not what
  lies beyond. Decided 2026-11-22: the world generator's low-frequency
  drainage tilt comes first; this stays a maybe.
