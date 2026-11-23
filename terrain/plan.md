# procgena_terrain: plan

A procedural terrain generator for Rust and Bevy. One generator makes one
terrain type (plains, hills, mountains, badlands, coast, and so on) inside a
region it is given. The region can be bounded or infinite. The terrain is
built in chunks, labelled (underwater, road, cliff, bluff, beach and so on),
and joins seamlessly with any other terrain generator on its border.

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

  **Please confirm this reading.** Everything below depends on it.
- **Two new crates**, in the stone/tree split:
  - `terrain/crates/procgena_terrain`: engine-free core. It holds the
    options, presets, field, the seam protocol, features, classification,
    the chunk mesher and `procgena_mesh::Object` output. It depends on
    `glam`, `procgena_mesh`, `serde`, `serde_json`, and optionally
    `rayon`. **No new dependencies.**
  - `terrain/crates/procgena_terrain_bevy`: chunk streaming around cameras,
    async builds, LOD, a label-driven triplanar material, a water material,
    a debug label overlay, and a viewer.
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
  weights. They are also split into `procgena_mesh` `Part`s by primary
  label, so clump and mushroom `TagFilter`s work on terrain. A separate
  water `Part` comes with each chunk. Vector features (road polylines,
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
  └─ chunk(key, lod, transitions) -> TerrainChunk -> procgena_mesh::Object
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
| [Johansen, erosion filter](https://blog.runevision.com/) (via [80.lv](https://80.lv/articles/fast-terrain-erosion-filter-that-emulates-erosion-without-simulation)) and [`bevy_erosion_filter`](https://docs.rs/crate/bevy_erosion_filter/0.1.0) | 2026 | reported MPL-2.0, and a core function reportedly derives from CC BY-NC-SA code | **Ideas only.** Gullies steered by analytic gradients, every point evaluated alone. We build our own from IQ's articles. |
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
- **Object model:** `procgena_mesh::{Object, Level, Part, Tag, TagSet,
  TagFilter, SurfaceMesh}`; `GeneratedObject` behind `bevy`.
- **Bevy crate shape:** `TerrainPlugin`, `TerrainParams`, a `.terrain.json`
  loader, `TerrainGenerating` / `TerrainFinished`, `AsyncComputeTaskPool`,
  headless `MinimalPlugins` tests, `examples/viewer.rs`.
- **Materials:** clump's triplanar texturing, weighted by label.
- **Plant integration:** `PlantField::with_ground_from_fn(|p| terrain.height(p))`
  and `with_density_from_fn(|p| …annotate(p)…)` work, given a `Vec2` to
  `DVec2` conversion (plus the field's world offset) in the closure.
- **Units:** metres, Y up. The terrain plane is XZ, as in plant.
- **Marching-cube tables:** stone has them in `stone/.../mesh/tables.rs`.
  Moving them into `procgena_mesh` so both crates can use them would be a
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
     shore profile.
   - Rivers: a parabolic bed plus a bank shoulder, at monotone levels.
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
     idea), or left to procgena_stone scatter (open question).
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
    pub lake: Option<LakeWish>,     // a lake that wants to span the edge, with its level
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
     broken by region order.
   - **Border valleys:** where both sides slope down towards the edge,
     nothing can cross or run along it. Both sides end their rivers in
     lakes or sinks inside their margins.
4. **Road ports.** The union of both sides' wishes, snapped to the
   low-grade points of the agreed profile and merged by spacing. The road
   level is the agreed height.
5. **Lakes across an edge.** Both sides' wishes must overlap. The level is
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
   spill along a carved channel.

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
- **Mesh output.**
  - Each vertex carries up to four `(label, weight)` pairs.
  - Triangles are grouped into `Part`s by primary landform. Each part is
    tagged `terrain` plus its label tags, and `underwater` where the flag is
    set.
  - Water is its own `Part`, tagged `water` plus `sea`, `lake` or `river`,
    and `closed: false`.
- **Tags.** Generic tags (`TERRAIN`, `WATER`, `ROAD`, `CLIFF`, `BEACH`,
  `UNDERWATER`, `CAVE`) are added to `procgena_mesh::tags`. That file is a
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
- **Water.** A simple water material on the water `Part`s.
- **Debug.** A label overlay (colour by landform or flag), seam-band and
  margin visualisation, and chunk borders.
- `GeneratedObject` goes on each chunk entity, so clump, mushroom and
  stone scatter can use terrain tags.
- `examples/viewer.rs`: presets; "two biomes" and "triple junction" scenes;
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
  without gaps), the material compiles, and `GeneratedObject` tags are
  present.

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
  full-strength mountain ground; blend 1 only puts a road there. Until
  that is fixed, `rivers_and_roads_stay_close_to_the_ground` fails on
  this world.
- **Coverage:** scaling alone (blend 0) is now tested only at meeting −1;
  −∞, 0 and 1 at blend 0 dropped out of the seam and feature suites.
- `BLEND_RATE` is 0.25, not the planned 0.1 (see "Fade width").
- Cost: the seam suite went from about 30 s to 61 s: 12 configurations
  instead of 8, and the neighbour heights inside the fade band (only
  there; the interior skips them).

## Phases

| Phase | Work | Depends on |
|---|---|---|
| 0 | **Coordinator only:** add both crates to the root `Cargo.toml`; add `terrain/prior_art/` to `.gitignore` and the `_typos.toml` excludes; add `CLAUDE.md` package rows and a README row; add the generic tags to `procgena_mesh/src/tag.rs`. | — |
| 1 ✅ | **Seam spike (2D heightfield only).** Done 2026-11-22: three presets (plains, rolling hills, mountains); continuity in height and slope tested for every pair, at a triple junction and at 10⁷ m. Review pending. Hash, noise with derivatives, calibration, a minimal base relief for two presets, `Region` types, offers, `negotiate`, `negotiate_corner`, conformance, the reference stitcher, and an image dump of two biomes and a triple junction. Includes the symmetry, seam and triple-junction tests. **Stop and review with the user.** | 0 |
| 2 ✅ | River and road ports across one edge in the spike (monotone check, fallback ending). Done 2026-11-22, see "Phase 2 as built". Review pending. | 1 |
| 2.5 | **Shared world noise and relief convergence** (replaces the relief taper; see "Relief convergence (Phase 2.5)" above): layer seeds come from the world seed and the layer's role, not the region seed; near each edge both sides fade their relief toward one agreed relief amplitude, so same-family neighbours match before negotiation. Exit: the border gap probe (239 m mean, plains–mountains) and the crossing count (6 in 300 seeds) re-run with both on. Done already: 16-direction river and road routing, 3 smoothing passes, rivers merge on meeting and never double back. | 2 |
| 2.6 | **Relief blending** (built 2026-11-22, review pending; one road problem at blend 1, see its Results) (see "Relief blending (Phase 2.6)" above): near borders each terrain also fades towards a weighted mix of its neighbours' actual height fields; the world option `relief_blend` (0 to 1) sets how much of that against Phase 2.5's scaling. Exit: border gap, the triple-junction road and crossing counts measured at blend 0, 0.5 and 1. | 2.5 |
| W | **WorldTerrain** (the big follow-up): world elevation field E (its slope is the drainage tilt, its zero the sea level), region graph and region edges, biome choice. See `terrain/world_plan.md`. | 2.6 |
| 3 | Full options and presets as data, base relief, landforms, analytic erosion | 1 |
| 4 | Macro-cell caches: drainage, **lakes at hollows** (fill to spill level, overflow as a river), rivers, roads, **bridges** (where a road crosses a river it stops cutting or filling and carries a bridge label), with the spatial index | 2, 3 |
| 5 | Classification, `annotate`, `HeightTile` | 3 (water labels after 4) |
| 6 | SDF, volumetric modifiers, top-surface search | 3 |
| 7 | Chunk mesher: band, marching cubes, Transvoxel transitions (after the licence check), parts by label, water parts | 5, 6 |
| 8 | Bevy crate: streaming, material, water, debug overlay, viewer | 7 |
| 9 | Exports (OBJ, glTF, raw heightmaps) | 7 (in parallel with 8) |
| 10 | `terrain/textures.yml` plus a tools entry; studio Terrain tab | 8 |
| 11 | Docs, READMEs, polish | all |

Phases 3 and 6 can run in parallel after phase 1, with one agent each. They
touch separate modules.

## Non-goals for v1

- Simulated erosion of any kind (the user chose analytic only).
- River flow accumulating across region borders.
- A GPU evaluation path (the design keeps it possible).
- Building the world generator itself (only the reference stitcher).
- Physics colliders (the meshes are usable for them, but there is no
  integration).

## Open questions

1. **Volumetric reading.** Is the "SDF with a heightfield base, plus a
   pure top-surface heightmap API" reading right?
2. ~~**Sea level.**~~ **Answered 2026-11-22:** oceans are not this crate's
   concern. Sea level is the zero of `WorldTerrain`'s elevation field E,
   used for labels and coasts; an ocean is just a terrain type below it.
   Lakes stay local.
3. ~~**River flow across borders.**~~ **Answered 2026-11-22: acceptable.**
   A river that crosses into a neighbour ends there or merges into the
   neighbour's own drainage. Its width and flow do not add up downstream
   across regions.
4. **Tags.** Which terrain tags go into `procgena_mesh` (shared), and which
   stay in `procgena_terrain`?
5. **Transvoxel licence.** Lengyel's terms for the tables were not found.
   If they are not permissive: our own surface nets with skirts, or the
   `transvoxel` crate (MIT/Apache) as a new dependency?
6. **Rock outcrops.** Voronoi-rock SDF in the terrain, or procgena_stone
   scatter tagged on cliffs and scree?
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
    grade costs plus a cost for expected cutting and filling. Cuttings
    steepen towards vertical; `Terrain::cutting_face(p)` reports a face
    of 45° to 65° as a bluff and 65° to 90° as a cliff (Phase 5 turns
    these into labels). Embankments stay capped at 10 m in the feature
    test, and road levels take 3 m of cut to save 1 m of fill. Moving
    the hub is still open (question 12).
14. **Switchbacks.** Climbing roads still double back on the spot (zero
    radius hairpins), and their legs can overlap. A turn limit in the
    route search made routes steeper (1 to 22 over-grade steps) and a
    turn cost didn't stop the hairpins, so both were taken out. Still to
    design: hairpins with a radius and a minimum spacing between legs.

## Maybe later

- **Edge heights in offers, to steer rivers.** Each offer already carries
  the side's own height along the edge, so both sides know each other's
  average edge height without any chaining. A region could steer its own
  rivers towards borders where the neighbour is lower, rather than only
  where its own relief happens to lead. It is cheaper than a drainage tilt
  from the world generator, but weaker: it knows only the edge, not what
  lies beyond. Decided 2026-11-22: the world generator's low-frequency
  drainage tilt comes first; this stays a maybe.
