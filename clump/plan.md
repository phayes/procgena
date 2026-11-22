# procgena_clump plan

## Bottom line

`procgena_clump` adds clumps of snow, moss, mud, clods and leaf litter on top of a
surface mesh you supply, or on a built-in flat ground patch. Clumps can also
have stalks (twigs, moss sporophytes, top leaves) sticking out of them. The
input is a plain triangle mesh. This plan doesn't cover integration with
procgena_stone, procgena_tree, Bevy or zeapp.

**Key architectural decisions (agreed 2026-11-21):**

- **Overlay shell, not a fused remesh.** Each generation pass returns its own
  clump mesh and never changes the input mesh. Clump edges taper to zero
  thickness and sink just below the input surface, so there is no visible edge
  and no z-fighting. Fusing only works on closed meshes. Input meshes such as
  trees, with overlapping tubes and leaf cards, aren't closed. A fused mode for
  closed inputs is listed under later work.
- **Triplanar texturing, like procgena_stone.** Clump shells have no UVs. Each preset
  names a CC0 texture set (snow, moss, mud, soil, leaf litter), fetched with
  procgena_stone-style `tools/textures` scripts. The shell carries a per-vertex
  `fade` value (0 at the edge, 1 at full thickness) that a material can use to
  blend into the surface underneath. A leaf pile is a mound-shaped clump with a
  leaf-litter texture, not a separate system.
- **Stalks work like procgena_tree leaves.** Generation produces placement records
  (`ClumpStalk { clump, origin, orientation, size }`: +Y along the stalk, +Z
  facing) and meshes them as single or crossed alpha-textured cards with 0–1
  UVs (`Billboard::Single`/`Double`, as in procgena_tree's `mesh_leaf`). The records
  are public, so a renderer can instance them instead of using the baked
  cards. Stalk kinds (twig, sporophyte, leaf, grass) are texture kinds, like
  procgena_tree's `LEAF_KINDS`.
- **Levels of detail (LOD) work like procgena_stone's.** A `DetailLevel { resolution,
  distance, hysteresis, texture_size }` and a `ClumpDetail` set with `LOW`,
  `MEDIUM`, `HIGH` and `ULTRA` presets (most detailed first, at least one
  level). Each LOD re-meshes the same clump with a longer target edge length
  and thins the stalks with a stride. Seeds and thickness are computed once,
  so clumps don't move between levels. The clump equivalent of `resolution`
  is `mesh.edge_length`.
- **World units with a `scale` setting.** All lengths (radius, thickness,
  sink, stalk size, spacing) are in world units, the same units as the input
  mesh. `ClumpOptions::scale` (default 1.0) multiplies them all, like
  procgena_stone's `scale`, so you can resize a preset in one step.
- **No new dependencies.** We hand-roll the following in procgena_stone's style,
  without depending on procgena_stone:
  - a glam-native BVH;
  - area-weighted surface sampling with Yuksel sample elimination;
  - a seeded hash RNG and noise;
  - crack-free edge splitting, and the clump displacement.

  Dependencies are glam 0.33, serde and serde_json, plus optional rayon and
  gltf-json, at the siblings' versions.
- **CPU only for v1.** A GPU twin is later work.
- **Same house pattern as the siblings.** A `crates/procgena_clump` workspace member
  with `Options`, `Preset`, JSON, `validate()` and `OptionsError`, deterministic
  output for a given seed, and OBJ and glTF export behind features.

### Data flow

```
Surface (borrowed positions/normals/u32 indices, or SurfaceMesh::ground)
  + ClumpOptions (seed, up, scale, mask, seeding, shape, detail, stalks, material)
        │
        ▼
1. Prepare   BVH over the input triangles. Per-vertex ray-cast attributes:
             openness toward each direction's source (hemisphere rays, one
             set per direction) and cavity (short-range occlusion).
2. Mask      Summed over directions: strength × facing (n · toward source) ×
             openness. Within each, the preset's weighted product of facing, openness, cavity,
             height and noise, giving 0..1. It is evaluated at every vertex of
             the refined clump mesh (step 5). The ray-cast attributes are
             blended from the input vertices. Up-ness, falloff and noise use
             the refined vertex itself, so coarse input triangles don't show
             in clump outlines.
3. Seed      Single (one cap at the best or a given point) or Scatter (N clumps by
             mask-weighted sample elimination, with minimum spacing).
4. Thickness Weight w(p) = clamp(Σ clumps: falloff(dist/radius) · mask ·
             lumpiness noise, 0, 1). The top height is
             h = (thickness + sink)·w − sink, which is −sink (buried) at w = 0
             and full thickness at w = 1. fade = clamp(h / thickness, 0, 1).
             Clumps whose footprints overlap form one group and one sheet.
             `clump` holds the group id. The spike has a single clump.
5. Shell     Copy the input triangles the clump may cover, and split them
             until no edge is longer than mesh.edge_length. Edges are shared,
             so there are no cracks. Push each vertex out by its height h:
               - along normals blurred over about the thickness;
               - held under half the gap to any surface facing it;
               - lowered until no edge is steeper than shape.max_slope.
             The result is an open sheet whose edges sink below the surface.
             Triangles well below the surface are dropped. Repeat per detail
             level.
6. Stalks    Mask-weighted points on the shell's upper faces → ClumpStalk records
             → cards (thinned per detail level).
        │
        ▼
Clump { levels: Vec<ClumpMesh> }
ClumpMesh { positions, normals, fade, clump, indices,     // the sheet (spike: no clump ids)
            stalks: CardBuffers {positions, normals, uvs, tangents, indices} }
Clump::stalks() -> &[ClumpStalk]
```

Snow and moss on the same input are two passes with two presets, giving two
`Clump` results. The input mesh must already be in its final orientation.
`directions` defaults to straight down (`[0, -1, 0]`, strength 1).

**Closed clumps (added 2026-11-21, on by default).** `MeshOptions::closed`
turns each clump into a watertight solid for physics, volume and voxelizing:
- **Underside:** the top sheet's triangles, reversed, with each vertex moved
  straight in along the surface normal by `sink + thickness / 4`. The depth
  is capped at half the input's thickness at that point, and every top is
  kept at least `thickness / 20` above its underside.
- **Side walls:** a strip of quads along the sheet's border.
- **Pinch vertices:** where the border touches itself at one vertex, each
  fan of triangles gets its own copy of the vertex, so every edge is shared
  by exactly two triangles.
- **Tests:** every directed edge is matched by its reverse, and a flat cap's
  volume matches the expected value.
- **On the 8 procgena_stone rocks:** no open edges and no edge shared by three or
  more triangles, at about twice the triangles of the open sheet.
- **Known gaps:**
  - Underside vertices still show 3.6 cm above the surface on one spot of
    the flat slab (15 vertices) and 2.4 cm at branch-trunk junctions, where
    input parts overlap.
  - Folded top triangles (0.1–0.5%) mean the solid can self-intersect
    slightly.

**Directions (added 2026-11-21).** `ClumpOptions::directions` lists where
material arrives from: falling snow, wind, or the damp side of a rock. Each
direction has a travel vector and a strength (default 1). Each direction
computes its own facing and openness. The per-direction masks add up, the
coverage weight is capped at 1, and one sheet is built from the total, so
overlaps merge. Directions are evaluated in list order, but the result
doesn't depend on that order. One shared `MaskOptions` (facing thresholds,
sky rays, sky power) applies to every direction.

## Public API sketch

Items already in the spike are marked "(spike)"; the rest are planned.

```rust
// (spike) Borrowed input; SurfaceMesh owns buffers and makes ground patches.
pub struct Surface<'a> { positions: &'a [[f32; 3]], normals: &'a [[f32; 3]], indices: &'a [u32] }
impl<'a> Surface<'a> {
    pub fn new(positions, normals, indices) -> Result<Self, SurfaceError>; // checks lengths and indices
}
pub struct SurfaceMesh { positions, normals, indices }   // SurfaceMesh::ground(size, divisions).surface()

pub struct Clump { options: ClumpOptions, levels: Vec<ClumpMesh>, stalks: Vec<ClumpStalk> }
impl Clump {
    pub fn new(surface: Surface, options: &ClumpOptions) -> Result<Self, ClumpError>; // (spike)
    pub fn from_preset(surface: Surface, preset: ClumpPreset) -> Self;
    pub fn from_json(surface: Surface, json: &str) -> Result<Self, ClumpError>;
    pub fn stats(&self) -> BuildStats;      // (spike) folded and thinned counts
    pub fn levels(&self) -> &[ClumpMesh];   // one per DetailLevel, most detailed first
    pub fn mesh(&self) -> &ClumpMesh;       // levels()[0]
    pub fn stalks(&self) -> &[ClumpStalk];
}

pub struct ClumpOptions { seed: u32, directions: Vec<Direction>, scale: f32, mask: MaskOptions,
                          seeding: Seeding, shape: ShapeOptions, mesh: MeshOptions,
                          detail: ClumpDetail, stalks: StalkOptions, material: MaterialOptions }
// (done) Direction { vector: [f32; 3] (travel direction), strength: f32 (default 1) }
//        Default [Direction::DOWN] = [0, -1, 0]; 1 to MAX_DIRECTIONS (8) entries.
// (spike) MaskOptions { facing_min, facing_full, sky_rays, sky_power }
// (spike) ShapeOptions { at, radius, edge_width, thickness, sink, max_slope, lumpiness, lump_size }
//         `at` moves into Seeding::Single in phase 3.
// (spike) MeshOptions { edge_length }   // clumps always grow along the (blurred) surface normal
pub enum Seeding { Single { at: Option<[f32; 3]> }, Scatter { count: u32, min_spacing: f32 } }
pub enum ClumpPreset { SnowCap, SnowPatches, MossCushions, MossCarpet, Mud, Clods, LeafPile }
```

Preset intent:

| Preset | Seeding | Mask | Shape | Stalks |
|---|---|---|---|---|
| SnowCap | Single | up-ness, sky | wide, smooth, steep edge falloff | twigs (sparse) |
| SnowPatches | Scatter | up-ness, sky, noise | medium, smooth | none |
| MossCushions | Scatter | cavity, moderate up-ness, noise | small, very lumpy domes | sporophytes |
| MossCarpet | Single, large radius | cavity, noise | thin, even | sporophytes (sparse) |
| **MossPatches** ✅ | Single, whole surface | up-ness (no sky term), strong noise | 3 cm, patchy islands | sporophytes |
| Mud | Scatter | cavity, low height, noise | thin, flattened | none |
| Clods | Scatter | low height, weak | small, thick, lumpy | grass (sparse) |
| LeafPile | Single | low height, up-ness | mound, soft falloff | leaf cards |

## Phases

| Phase | Scope | Status |
|---|---|---|
| 0 | Workspace restructure into `crates/procgena_clump`, README, this plan | ✅ (README still to do) |
| 1 | **Spike:** `Surface`, ground, BVH, up-ness and sky masks, and a single snow cap, exported to OBJ (see the details below). Decision gate on the geometry approach. | ✅ subdivide-then-displace chosen, Up offset dropped; border and fold questions open |
| 2 | Options, presets, JSON, `validate()`, `OptionsError` and `ClumpError` | 🟡 `ClumpPreset` (`SnowCap`, `MossPatches`), `validate()` and the errors are done; JSON and the remaining presets are not |
| 3 | Full masks (cavity, height, noise), scatter seeding with sample elimination, lumpiness, edge sink, clump groups, and the `fade` and `clump` attributes | ⬜ |
| 4 | Stalks: `ClumpStalk` records, procgena_tree-style single and crossed cards, stalk texture kinds | ⬜ |
| 5 | `DetailLevel` and `ClumpDetail` (procgena_stone model), OBJ and glTF export, determinism and geometry tests | ⬜ |
| 6 | `tools/textures`: CC0 sets for snow, moss, mud, soil and leaf litter, plus alpha textures for stalks | ⬜ |

### Phase 1 spike details

- **Test inputs:** built in the crate's tests and examples with no
  dependencies on the sibling crates:
  - the ground patch;
  - a noise-displaced sphere (rock-like, with crevices);
  - a few open, overlapping cylinders (branch-like forks);
  - an upside-down plane (the negative case).
- **Geometry approaches compared** (round 1): a surface-nets shell on a band
  field, its top sheet alone, and a displaced copy of the covered input
  patch. Round 2 replaced them with subdivide-then-displace.
- **Snow direction variant:** also try taking snow thickness from a downward
  ray hit (a true "up" offset) instead of the nearest point. Compare on steep
  sides and on cylinders.
- **Decision criteria:**
  - no visible self-intersection in concave regions (crevices, forks);
  - every edge vertex buried, checked against the BVH;
  - triangle count, including the share spent on the buried underside;
  - generation time and peak memory for a cap that covers the whole input.
- **Manifold policy:** settled by the decision below. The clump is an open
  sheet copied from the input's connectivity, so it is manifold wherever the
  input is. The spike's meshes have no edge shared by three or more
  triangles, and phase 5 asserts exactly that.
- **Fold guarantee: open decision.** About 0.1% of triangles still fold on
  the crevice rock (0.06–0.13% at every density). Either remove the residue
  (for example by relaxing only the folded patches) and assert zero folds,
  or settle on a different guarantee. Don't write the residue into a test as
  an allowed failure.

### Phase 1 spike results (2026-11-21)

Round 1 compared three methods; that code has since been removed. Run the
current spike with `cargo run --release -p procgena_clump --example spike`. OBJ
files go to `target/spike/`. Each round 1 run built a default snow cap (12 cm
thick, 3 cm sink, 2 cm voxels), single-threaded.

| Input (input triangles) | Method | Triangles | Time | Notes |
|---|---|---|---|---|
| ground 3 m (4.6k) | Band | 114k | 4.2 s | no holes; 46% of triangles buried underside |
| ground | Displace | 4.6k | 14 ms | exactly as fine as the input |
| crevice rock (20k) | Band | 64k | 5.1 s | 206 non-manifold edges; some rim vertices 7 cm above the surface |
| crevice rock | Band, resolution 4 (coarsest allowed) | 28k | 2.4 s | still 44% underside |
| crevice rock | Displace | 8k | 0.8 s | 111 folded triangles (1.4%); rim buried |
| crevice rock, fine (82k) | Displace | 31k | 2.4 s | 542 folded (1.8%) |
| branches (2.3k) | Band | 48k | 1.0 s | wraps around open tube ends |
| branches | Displace | 650 | 36 ms | coarse: as fine as the input |
| thin 2 cm layer, ground | Band | **1.5M** | **20 s** | sink ≥ voxel forces 5 mm voxels |
| thin 2 cm layer, ground | Displace | 4.6k | 20 ms | |
| upside-down plane | all | 0 | <1 ms | mask works |

Findings:

- **Band (closed voxel shell) doesn't scale.** It is watertight with no
  holes, but about 45% of its triangles are the buried underside. It
  produces non-manifold edges, and thin layers (mud, moss carpet) explode,
  because the sink must cover a voxel. Even with threads it would need a
  simplifier.
- **BandTop** (top sheet only) has about 40% fewer triangles, but curls
  under open borders (up to 7 sinks deep) and leaves holes. Rejected.
- **Displace is cheap and keeps rims buried, but is limited by the input's
  detail and folds in tight crevices** (1–2% of triangles on the crevice
  rock).
- **The Up offset** is slightly worse than Normal on every input: misplaced
  top vertices (2–16%) and rims a few mm to 4 cm above the surface on steep
  sides.
- **Open borders** (patch edges, tube ends) get a rounded lip from the band
  methods. Displace simply stops at the border.

**Decision (2026-11-21): subdivide, then displace.** Band and BandTop were
removed. The first round of results above is kept for the record.

### Phase 1 round 2: subdivide-then-displace

Default snow cap, 4 cm target edges, offset along the normal. Time is single-threaded and mostly sky ray casting on the input
vertices.

| Input (input triangles) | Triangles | Time | Folded | Rim max height | Notes |
|---|---|---|---|---|---|
| ground 3 m (4.6k) | 37k | 17 ms | 0% | no rim | whole-ground cap |
| ground, 1 quad (2) | 33k | 11 ms | 0% | no rim | detail no longer depends on the input |
| 40 cm clump, ground (4.6k) | 1.9k | 6 ms | 0% | 0 | rim buried |
| 40 cm clump, 1 quad (2) | 1.7k | 0.8 ms | 0% | −0.3 mm | far triangles aren't refined |
| crevice rock (20k) | 10k | 0.34 s | 0.09% | −0.1 mm | plain Displace folded 1.4% |
| crevice rock, coarse (5k) | 15k | 83 ms | 0.10% | 0 | |
| crevice rock, fine (82k) | 28k | 1.5 s | 0.06% | 0 | plain Displace folded 1.8% |
| crevice, 2 cm edges | 39k | 0.38 s | 0.13% | −0.1 mm | |
| crevice, 8 cm edges | 7.5k | 0.35 s | 0.08% | 0 | |
| branches (2.3k) | 9.5k | 34 ms | 0% | +7.8 mm | where a branch enters the trunk; see below |
| thin 2 cm layer, ground | 37k | 14 ms | 0% | no rim | thin layers cost the same as thick |
| upside-down plane | 0 | <1 ms | — | — | |

Visual check: rendered in a three.js viewer (`target/spike/viewer.html`,
served by the `spike-viewer` entry in `.claude/launch.json`). Back faces are
drawn red so inside-out triangles show.

- **Crevice rock:** a smooth cap with buried edges. The crevice walls are bare.
- **Branches:** snow lies along the upper sides of the branches only.
- **40 cm clump:** a soft-edged mound with a flat top, because the coverage
  weight is capped at 1.
- **No red** anywhere except looking into an open tube end, where the open
  sheet's underside shows (the open-border question below).

What made the difference:

- **Splitting edges to `mesh.edge_length`** makes detail independent of the
  input. Splitting is crack-free: each pass halves every edge that is too
  long, and splits the triangles around it to match.
- **`shape.max_slope`** (rise over run, default 1 = 45°, like an angle of
  repose) lowers heights until no clump edge is steeper. Most folds were
  overhanging cliffs at the crevice lip, where the mask jumps from bare to
  full within a few cm, not clumps passing through each other. This cut
  folds by 90–97%.
- **Blurring the push direction** over about the thickness, as a world
  distance (passes = (thickness / mean edge)²), stops pushes converging in
  tight hollows. Folds now stay at about 0.1% at every mesh density, rather
  than growing as the mesh gets finer.
- **Refinement skips triangles the clump can't reach**, so a small clump on
  a large triangle only pays for its own area.
- **The gap limit** (a clump fills at most half the gap to a surface facing
  it) rarely triggers on these inputs, because the crevice walls are too
  steep to be covered. It is cheap insurance for overhangs and branch forks.
- **Dropping triangles sunk more than half the sink depth** removed the
  10–30% of triangles that were invisible.

**Decision (2026-11-21): the Up offset is dropped.** It folded more
(28–605 triangles), misplaced 2% of top vertices and left rims about 1 cm
above steep sides. Snow gets its look from the up-facing mask and
`max_slope` instead. `MeshOptions::offset` and the `Offset` enum were removed.

**Decision (2026-11-21): keep the mask on the raw surface normals.** On
real procgena_stone rocks the snow breaks into speckles and gaps that follow the
rock's surface roughness. Patrick likes that look, so don't smooth the mask
normals. Folds on those rocks are 0.3–1.1%.

Still open:

- **Overlapping input parts:** the highest rim vertex on the branches sits
  where a side branch enters the trunk. It is buried relative to its own
  tube but 7.8 mm outside the trunk. Sheets on overlapping parts cross
  instead of merging. The voxel shell merged them, at its much higher cost.
- **Other trade-offs against the voxel shell:**
  - a cap over a whole open input ends as an open sheet above the border,
    with no side wall;
  - every clump is a heightfield over the input, so there are no overhangs
    beyond what `max_slope` allows.
- **Fold residue:** about 0.1% (see the fold guarantee above).
- **Open borders:** the sheet stops at an input border (a ground patch edge
  or an open tube end) with a visible cut. A lip would need extra geometry.
- **Speed:** sky visibility casts 64 rays per input vertex and dominates on
  dense inputs (1.5 s at 82k triangles). Rayon (already planned as an
  optional feature) or fewer rays on dense inputs would fix it.

### Tests (phase 5 onwards)

- The same seed and options give identical output, and a different seed gives
  different output.
- No edge is shared by three or more triangles. The fold guarantee is still
  open (see the phase 1 spike details).
- Edge burial: every vertex with `fade == 0` is at or below the input
  surface it was pushed from, and no vertex is more than `sink + ε` inside
  it. Measure against the vertex's own source triangle, not the nearest
  surface: where input parts overlap (a branch entering a trunk), the
  nearest surface can be the other part.
- The mask holds: a SnowCap on an upside-down plane produces nothing.
- Lower detail levels have fewer triangles, and clumps don't move between
  levels.
- Stalk roots lie on the shell, and stalk orientations are unit quaternions.
- Export round-trips through `gltf` (a dev-dependency), as in the siblings.

### Verification commands

```
cargo test --workspace
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo run -p procgena_clump --example spike --features export-obj
```

## Later

- Integration: a `procgena_clump_bevy` crate, adapters for procgena_stone and procgena_tree
  meshes, and a zeapp panel.
- A fused mode for closed inputs: smooth-union the clumps with the input's
  signed distance field and remesh.
- Exclusion between passes, so moss avoids snow: pass an earlier `Clump` as an
  occluder or mask input.
- Geodesic falloff (Dijkstra on the edge graph), if Euclidean falloff bleeds
  through thin walls.
- Camera-facing stalk cards, and wind data for stalks.
- A GPU twin.

## Open items

- Leaf-pile textures, from the texture session: `leaves_dry_decay`,
  `leaves_forest_ground` and `leaves_forest_03` (Poly Haven, CC0). They use
  the same layout as snow and moss:
  `assets/textures/leaves/<id>-<size>-{color,normal,orh}-<format>.ktx2`.
  The full hand-over is in `textures.md` in the shared notes folder.
- Stalk textures, from the texture session:
  - **Kinds for `StalkOptions`:** `twig_bare` and `twig_budding` (Nobiax,
    CC0), and `sporophyte_sparse` and `sporophyte_dense` (AI-generated; no
    licence chosen yet).
  - **Files:** `assets/textures/stalks/<kind>-{color,normal}-{bc7,uastc}.ktx2`.
    The colour map is sRGB RGBA with straight alpha.
  - **Card layout:** square, with the stalk bases centred on the bottom edge.

- Texture hosting at `gs://ze_textures/clump/`. Patrick uploads, because
  `gcloud-claude` is read-only.

## Prior art

- Blender Real Snow add-on: selects faces by normal and fills them with
  metaball blobs (an overlay). https://projects.blender.org/extensions/real_snow
- Fearing 2000, "The Computer Modelling of Fallen Snow": launch sites on the
  surface and occlusion from the sky.
  https://www.cs.ubc.ca/labs/imager/th/2000/Fearing2000/
- Infinigen moss: clumps on rocks, masked to upward-facing and concave areas.
  https://arxiv.org/pdf/2306.09310
- Desbenoit et al. 2004: lichen growth driven by moisture, light and
  accessibility. https://perso.liris.cnrs.fr/egalin/Articles/2004-lichen.pdf
- Hsu & Wong 1995, dust accumulation, and Miller 1994, accessibility shading:
  the basis of cavity masks.
- Yuksel 2015, "Sample Elimination for Generating Poisson Disk Sample Sets".
  https://www.cemyuksel.com/research/sampleelimination/sampleelimination.pdf
- procgena_tree `mesh_leaf` and `SkeletonLeaf`: the card and placement-record pattern
  used for stalks.
- procgena_stone `DetailLevel` and `StoneDetail`: the LOD model.
