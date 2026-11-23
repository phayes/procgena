# procgena_deadwood — plan

Procedural dead wood on the ground: fallen logs, fallen forked branches,
scattered sticks and twigs, and stumps with root flare. Stretch goal: a stump
and its fallen trunk with matching breaks. The library makes several levels of
detail, rests each piece on flat ground with an optional cheap settle, and
uses one decay control that takes wood from freshly fallen to a soft,
half-buried mound.

## Status (2026-11-22)

- **Done:** Phases 0–8, plus 5b (stumps): the core crate `procgena_deadwood`,
  with tests and an export example.
- **Done:** Phase 9, the Bevy crate `procgena_deadwood_bevy`, with headless
  tests, a GPU shader test and a viewer.
- **Done:** Phase 10, `deadwood/textures.yml` (the 29 bark sets and the
  two end-grain sets) and the end-grain photos in the material. The
  textures are converted locally; they still need uploading to
  `gs://ze_textures/deadwood/`.
- **Done:** Phase 11, the studio's Dead Wood tab: presets, seed, every
  option (grouped into Piece, Ends, Shape, Parts, Bark, Colours,
  Textures, Resting), a Level of Detail picker, save/load JSON and
  GLB/OBJ export. Textures come through the studio's downloading asset
  source. Not built: the scatter-patch scene and the "add moss" toggle.
- **Ends too long for the stem** are no longer an error: the core shrinks
  both ends by one factor until they fit, and turns an axe cut or natural
  tip that still can't fit into a flat cut. `OptionsError::EndsTooLong`
  and scatter's own `fit_ends` are gone.
- **Not started:** Phase 13 (the `FallenTree` stretch goal).

How the Bevy crate differs from this plan:

- **Bark textures** are tree's bark sets in tree's layout (colour, normal,
  roughness), listed in the core's `BARK_TEXTURE_SETS`. Deadwood's
  `textures.yml` lists them again, so its download is self-contained (the
  bucket holds a second copy). The material divides the bark image by its
  smallest mip (its average colour), so the vertex colour stays the bark's
  overall colour. New core option: `MaterialOptions::bark_texture_size`.
- **No extra decay tint in the shader.** The core already bakes
  weathering, rot, heartwood and sapwood into the vertex colours; the
  shader adds only detail: rings, pith, drying cracks, grain, bark.
- **Wood values go in UV1** (zone × 4 + bark cover, and radius), not a
  custom attribute, so the standard vertex outputs carry them. Rot is not
  passed; it is in the colours.
- **End-grain photos** (ambientCG `TreeEnd003` and `TreeEnd005`) are
  whole tree ends, not tiling textures. They are drawn only on sawn ends
  (a new `Zone::Cut` for `Saw` and `Chainsaw`), never on snapped or axe
  ends. End UVs now span each end's own outline (0.5 at the middle), and
  `END_TEXTURE_SIZE` is gone. The shader maps the end's middle to the
  photo's pith and its outline to a hand-measured circle inside the
  photo's wood, fading out past it, and divides by a hand-measured average
  colour of that circle. The drawn rings are kept at a quarter strength
  under the photo, with the drawn cracks. Drying cracks now scale with
  each end's own radius. New core option: `MaterialOptions::end_texture`
  (default `TreeEnd005`; stump presets use `TreeEnd003`).
- **Scatter ground** is chosen explicitly: `ScatterGround::Flat` or
  `ScatterGround::Host` (the parent's `GeneratedObject`, sampled into a
  `HeightGrid`).
- **Burial** is reported through a `DeadwoodPlacement` component, not
  applied.

How the core crate differs from this plan:

- **`Stick` is not its own kind.** It became a small `Branch`, with
  `Stick` and `Twig` presets. `PieceKind` is `Log | Branch | Stump`.
- **Not built:**
  - twist;
  - the extra vertex ring that would give a peeled patch a raised bark
    edge (the edge is a sharp step in the surface instead);
  - the rot cavity.
- **`DeadwoodDetail` changed:**
  - `rings_per_metre` became `segments_per_diameter`;
  - `stripped_lip` was dropped;
  - `ribbon_radius` was added: stems thinner than this become strips.
  - The ladder has four levels (`FULL` from 0 m, `HIGH` from 8 m, `LOW`
    from 25 m, `STRIP` from 45 m) and a cull at 90 m, for a piece 45 cm
    across. `STRIP` (`strip_all`) draws every stem, the main one included,
    as a camera-facing strip, and casts no shadows in Bevy. Distances
    *and* the `min_radius`/`ribbon_radius` thresholds scale with the
    piece's size (`max(diameter, 0.15 × length)`), so one ladder serves
    logs and twigs.
- **Limits applied in the code:**
  - wobble is capped at 0.1 of the control-point spacing;
  - a side stem's tip that doesn't fit becomes a flat cut;
  - scatter replaces ends that don't fit a short variant (`fit_ends`) and
    turns a `Stump` into a `Branch`;
  - stems get at most 16 diameters' worth of rings, and their edges are
    never shorter than about 2 mm.
- **Different meanings:**
  - `hollow.depth` is a fraction of the stem between the two ends' cuts;
  - each end takes up twice its reach (`EndKind::span`);
  - `Instance::position` was added.
- **Tests from this plan not written:**
  - the centre of mass lies over the supports;
  - a class-4 log touches the ground along its length (only "flatter and
    lower" is tested);
  - a real `procgena_clump` build on a log (only the tag filter is
    tested).

## Bottom line

- **Two new crates**, following the stone/tree/mushroom split. The folder is
  `deadwood/crates/`.
  - `procgena_deadwood` is engine-free. It holds options, presets, the sweep
    mesher, end and break shapes, branching, settling, LOD, scatter,
    `procgena_mesh::Object` output, and OBJ/glTF export. It depends on `glam`,
    `procgena_mesh`, `serde` and `serde_json`, plus an optional `gltf-json`
    that is already in the lock. **No new dependencies, and no dependency on
    `procgena_tree`** (decision: standalone).
  - `procgena_deadwood_bevy` has the plugin, async build, cache,
    `VisibilityRange` LOD, and a WESL material extension. The material covers
    bark, exposed sapwood, procedural end-grain rings and rot tint. It also has
    the stick ribbon billboards and scatter components.
  - The studio gets a **Deadwood** tab. Shipping core, Bevy and studio in v1
    was the user's decision.
- **One shape model: a swept, non-circular cross-section along a curved
  centre line.** A single function `surface(piece, t, θ, depth)` gives the bark
  surface, the stripped-bark inset and the hollow inner wall. Every LOD
  resamples it, so the silhouettes agree. We do not use stone's SDF and
  marching cubes. Bark textures need UVs along the grain, and end grain needs
  polar UVs around the pith. Only a sweep gives both cleanly.
- **Four asset kinds, plus a stretch pair,** share the sweep mesher:
  1. `Log`: one trunk section. It has two ends, each a saw cut, chainsaw cut,
     axe wedge, snapped break or natural taper. It can have branch stubs, a
     hollow core, stripped bark patches, ovality and sag.
  2. `FallenBranch`: a limb with child limbs at two depth levels and twigs at
     a third. Its own small standalone branching generator builds these, with
     no tree dependency. Each child is a separate closed shell sunk into its
     parent, as in tree.
  3. `StickScatter`: instance records over a small variant library of sticks
     and twigs, for a ground patch. A renderer instances the shared meshes. At
     distance, sticks become tree-style ribbons and are then culled.
  4. `Stump`: a short upright trunk anchored at y = 0, so it never settles.
     Its top can be any end kind, including a tall, jagged snapped break, and
     decay drives bark loss, hollowing and rot as on logs. The new design work
     is the base:
     - a **root flare** term in the cross-section (base widening, flare
       height, buttress lobes);
     - a few **surface roots**, built like stubs but angled outward and down
       into the soil;
     - a flat cap below ground, so the stump stays a closed solid.

     Infinigen's `TruncatedTreeFactory` supplies only the snapped top, which
     we already port. It has no root flare, so the flare is our own design.
  5. **Stretch: `FallenTree`**, a stump and its fallen trunk, from
     Infinigen's `FallenTreeFactory`. Both pieces read the *same* break: the
     stump's top and the log's base use one break surface, the log's side
     flipped. The log is laid down pivoting on the stump's highest break point
     and rests with its other end on the ground.
- **One `decay: f32` (0–5, the forestry decay class as a continuous value)**
  drives all of the following. Each has an explicit override, with `None`
  meaning "derive it from decay":
  - bark loss;
  - twig and stub loss;
  - ovality (flattening);
  - sag between supports;
  - burial depth;
  - colour shift;
  - softening of snapped edges.
- **Resting pose: optional, cheap, deterministic, on flat ground**, modelled
  on `procgena_stone`'s `StonePlacement`. Detection uses a fixed analysis
  sampling that does not depend on LOD, so every level shares one pose. Stubs
  and forks lift a fresh log off the ground. Ovality and sag are applied
  after the rigid rest, in the settled frame, so decay flattens the log
  against the real ground and lets it sag and sink. The rest search ports
  stone's `settle_direction` onto sampled surface points.
  `settle: false` keeps the generated pose and only lifts the piece to y = 0.
  Physics engines are a non-goal.
- **Tags and parts.** Bark and exposed wood are *not* split into separate
  `Part`s. A bark wall without its end caps is open, so splitting would make
  both parts open, and clump's inside tests need closed parts. Instead, each
  level's parts are split by size, as tree does:
  - `TRUNK` for a log;
  - `LIMB` for a fallen branch's main axis;
  - `BRANCH` for children, stubs and sticks.

  Each part is a **closed** set of shells, one per limb or stub, and holds
  both its bark and its exposed wood. Every part carries `DEADWOOD` + `WOOD`
  + its size tag, plus `BARK` while any bark remains on the piece. The bark
  and wood zones the material needs are in a per-vertex attribute, not in
  parts. Only two tags are new: `DEADWOOD` and `WOOD`.
- **Clump on logs.** The trunk-moss preset (`TRUNK | LIMB`) works unchanged.
  The moss and snow presets filter on `BARK | ROCK | GROUND | MOSS`, so on
  their own they would miss a bark-less class 4–5 log, which would give
  `NoSurface`. Phase 0 adds `WOOD` to those presets' `grows_on`, which is a
  coordinator edit because the clump files are being edited now.
- **Coordinator-only edits (Phase 0):**
  - root `Cargo.toml` and `Cargo.lock`, with the member added *last*, after
    the crate files exist;
  - the new tags in `procgena_mesh/src/tag.rs`;
  - `WOOD` in the clump presets' `grows_on`;
  - `.gitignore` and `_typos.toml` for `deadwood/prior_art`;
  - the `CLAUDE.md` package rows;
  - the textures tool entry;
  - studio wiring.

---

## Prior art

Clone each repository shallowly into `deadwood/prior_art/`. Keep it untracked.

| Source | Licence | Use |
|---|---|---|
| [princeton-vl/infinigen](https://github.com/princeton-vl/infinigen) `assets/objects/deformed_trees` | BSD-3, with Princeton headers per file (the CVPR paper says GPL, but the repo and the headers say BSD-3) | **May port with attribution and keep the headers.** Snapped break: noise displacement along the axis, weighted by distance from a cut plane tilted 0.1–0.2, with strength 0.3–0.5. Hollow: an angular noise mask with an inner wall bridged to the outer. Rotten cavity: a sunken off-axis sphere at 0.4–0.9·r. End-grain shader: rings with a saw profile and distortion 4–10, in rest-position space. A clone is already in `mushroom/prior_art/infinigen`. The `deformed_trees/*.py` files do not import `infinigen_gpl` (checked with grep). They import `infinigen.assets.objects.trees`, which is out of scope. |
| [emparer/procedural-log-generator](https://github.com/emparer/procedural-log-generator) | MIT | **May port with attribution.** Taper, oval sections, global and local bend, cracked bark plates, knots that deform nearby wood. |
| [marialarsson/procedural_knots](https://github.com/marialarsson/procedural_knots) (Larsson et al. 2022) | MIT | **May port with attribution.** GLSL for ring distortion around knots, for the end-grain shader (Phase 9b). |
| [dgreenheck/ez-tree](https://github.com/dgreenheck/ez-tree) | MIT | The swept-tube reference that procgena_tree already ports. Its ring and UV conventions are reused here so bark textures line up. |
| SpeedTree Cap generator (docs) | Proprietary | **Ideas only.** An end cap with an outer bark lip and an inner cut face, inset by the bark thickness. Parameters: offset, slope, rings. |
| SideFX Labs dead-tree examples | BSD-style | **Ideas only** (needs Houdini). |
| Poly Haven models: `dead_tree_trunk*`, `dry_branches_medium_01`, `bark_debris_01` | CC0 (not checked) | Visual reference for shape, breaks and settling. Nothing is shipped. |

**Ecology parameters.** Decay classes are from Maser et al. 1979 / Sollins,
using USDA PNW GTR-164 and the Andrews Forest TD022 metadata.

| Class | Bark | Twigs and branches | Shape | Ground contact |
|---|---|---|---|---|
| 1 | Intact | Fine twigs present | Round | Raised on support points |
| 2 | Intact | Twigs falling off | Round | Raised, sagging slightly |
| 3 | Traces only | Many branches gone | Round | Sagging toward the ground |
| 4 | Absent | Stubs loose | Round to oval | Whole log on the ground |
| 5 | Absent | — | Elliptical | Mound embedded in the floor |

Woody debris is grouped by diameter. Coarse woody debris (CWD) is 7.6 cm or
more. Fine woody debris is split into fuel classes: 1-hr is under 0.6 cm,
10-hr is 0.6–2.5 cm, and 100-hr is 2.5–7.6 cm. These ranges set the presets.

The following figures came from the researcher's memory and are **unverified**.
Treat them as tuning starting points, not facts:

- the reddish-to-dark colour shift in classes 4–5;
- a minimum log length of about 1 m;
- class 5 being partly sunken.

---

## Workspace patterns to reuse

- **Options:** stone style.
  - snake_case serde;
  - `from_json` deep-merges over `Default`;
  - `to_json`;
  - `validate()` rejects bad values and never clamps;
  - `#[non_exhaustive] OptionsError`;
  - `MAX_*` constants.
- **Presets:** a `DeadwoodPreset` enum with `ALL`, `name()` and `options()`,
  written in Rust.
- **RNG and noise:** copy clump's counter-based `pcg3d` (`hash.rs`) and
  stone's noise. No `rand`.
- **LOD:** `LodLevel { distance, hysteresis, detail }` plus a cull distance.
  Distances are written for the default size and scale with the piece's
  diameter, as in stone and mushroom.
- **Billboards for sticks:** copy tree's `BillboardVertex { pivot, axis,
  offset }`, its ribbon builder and its `tree_billboard.wesl`, as mushroom
  does. This is the same open question as mushroom's about sharing them
  through `procgena_mesh`.
- **Settle:** follow `procgena_stone::placement`. Lift to y = 0, put the
  centre of mass above the support, offer an optional settle and a separate
  suggested burial, and use one analysis sampling for all LODs.
- **Object model:** `procgena_mesh::{Object, Level, Part, Tag, TagSet,
  TagFilter, SurfaceMesh}`, with `GeneratedObject` behind `bevy`.
- **Bevy plugin shape:**
  - `DeadwoodPlugin`, `DeadwoodParams::{Options, Asset}`, a
    `.deadwood.json` loader;
  - `DeadwoodPart { lod, kind }`, `DeadwoodGenerating`, and a
    `DeadwoodFinished` `EntityEvent`;
  - a cache keyed by the options JSON, `AsyncComputeTaskPool`,
    `VisibilityRange` per level;
  - public mesh and material helpers, `examples/viewer.rs`, and headless
    `MinimalPlugins` tests.
- **Studio:** the same ten touch points as stone and mushroom:
  - tab and editor params;
  - export panel;
  - `SceneMode` and `ViewerCommand`;
  - subject spawn and plugin;
  - assets, environment and Cargo.

---

## Core design (`procgena_deadwood`)

### Modules

`options`, `presets`, `rng`, `noise`, `curve` (centre line and frames),
`section` (cross-section function), `ends` (cut and break caps), `stubs`,
`branching`, `mesh` (buffers, sweep, LOD), `billboard`, `settle`, `object`,
`scatter`, `export/{obj,gltf}`.

### Options

```rust
pub struct DeadwoodOptions {
    pub seed: u32,
    pub kind: PieceKind,             // Log | FallenBranch | Stick | Stump
    pub decay: f32,                  // 0..=5, continuous decay class
    pub length: f32,                 // metres, along the main axis
    pub diameter: f32,               // metres at the base
    pub taper: f32,                  // tip diameter / base diameter
    pub shape: ShapeOptions,         // bend, wobble, ovality, bark lumps, twist
    pub ends: [EndOptions; 2],       // base, tip
    pub stubs: StubOptions,          // count/density, length, angle, end kind mix
    pub branching: BranchingOptions, // FallenBranch only: levels, children, angles, twigs
    pub bark: BarkOptions,           // thickness, stripped coverage (Option = from decay), plate cracks
    pub hollow: Option<HollowOptions>, // wall fraction, open at base/tip, rot cavity
    pub roots: Option<RootOptions>,  // Stump only: flare height/width, buttress lobes, surface roots
    pub material: MaterialOptions,   // bark/sapwood/heartwood/rot colours, bark texture kind, roughness
    pub settle: SettleOptions,       // settle on/off, sag, burial (Option = from decay)
    pub detail: DeadwoodDetail,      // quality for single-level use
}

pub enum EndKind {
    Saw { tilt: f32 },
    Chainsaw { tilt: f32, step: f32 },
    Axe { wedge_angle: f32, facets: u32 },
    Snapped { jaggedness: f32, splinters: u32, one_sided: f32 },
    Taper,                            // natural tip, closes to a point
}
```

All lengths are in metres. Shape values are ratios of `diameter` or `length`
so that presets scale.

### Shape

- **Centre line.**
  - A Catmull-Rom curve through control points, with a low-frequency bend and
    a small wobble.
  - Frames come from parallel transport.
  - Sag is applied later, by settle.
- **Cross-section.** `radius(t, θ)` is built from:
  - the taper;
  - low-frequency bark lumps;
  - bark-plate cracks: a narrow angular and lengthwise Voronoi groove;
  - a twist.

  Where the stripped-bark mask is on, the surface drops inward by the bark
  thickness, which exposes sapwood. The mask is noise in (θ, length) space,
  and its threshold falls with decay. The rim of each stripped patch is a
  steep ring of extra vertices, so the bark edge reads as a lip.
- **Ends** (`ends.rs`). Each end is a polar cap grid (radial rings × sides)
  joined to the last sweep ring.
  - The outer annulus, one bark thickness wide, is bark. The inside is end
    grain and gets polar UVs around an off-centre pith.
  - `Saw`: a flat tilted plane.
  - `Chainsaw`: two offset half-planes with a hinge strip.
  - `Axe`: a V-wedge with facet noise.
  - `Snapped`: this follows Infinigen. Height along the axis comes from
    anisotropic noise stretched about 1:8 along the grain. It is weighted
    toward one side, and a few long splinter spikes are added. Splinter
    spikes are part of the same closed cap grid (raised vertices), not loose
    shards. Decay rounds and lowers them.
  - `Taper`: closes to a point, as tree tips do.
- **Hollow.** An inner wall is swept from the same function at
  `wall_fraction · radius`, displaced inward by noise. It opens at one or both
  ends, where the cap becomes an annulus that bridges outer and inner. An
  optional rot cavity, an off-axis sunken sphere as in Infinigen, is a v1.1
  candidate.
- **Stubs.** Short sub-sweeps with their own ends (mostly `Snapped` or `Saw`),
  sunk into the parent as separate closed shells. Their count falls with
  decay: twigs are gone by class 2 and most stubs by class 4.
- **Branching** (`FallenBranch`). A small recursive generator:
  - each limb places children by length fraction, with a spread angle, a
    roll and a size ratio;
  - it has up to two child levels plus a twig level;
  - every limb uses the same sweep;
  - twig thickness is in the fine woody debris (FWD) classes, under 2.5 cm.

  The generator is independent of procgena_tree, but its parameter names
  follow tree's where they mean the same thing.
- **Stump** (`Stump`). The centre line is vertical, from below ground up to
  the top end, with a small lean.
  - **Root flare:** in the lowest `flare_height`, the cross-section radius
    swells by up to `flare_width`, shaped by `lobes` buttresses. The lobes
    are angular bumps whose height dies off with height up the stump, with a
    random phase and noisy amplitude.
  - **Surface roots:** a few sub-sweeps leave the base between lobes, run
    outward with a downward pitch and taper, and dip below y = 0. They are
    separate closed shells sunk into the stump, like stubs. Decay softens
    them and sinks them further.
  - **Below ground:** the trunk sweep and the roots end in flat caps at a
    fixed depth under y = 0, so every shell is closed.
  - **Top:** any `EndKind`. Presets use `Saw` (felled), `Chainsaw` with a
    hinge (felled with a notch), and a tall `Snapped` (Infinigen's truncated
    break: cut height 0.8–1.5 m, tilt up to ±0.4, strength 0.6–1.0).
- **Shared break** (stretch, for `FallenTree`). A `Snapped` end is a function
  of (break seed, θ, r) in the cut-plane frame. The stump's top evaluates it
  as is. The log's base evaluates the same function mirrored, so the two
  faces fit together.

### Settle (`settle.rs`)

The settle is cheap and deterministic, and it can be turned off. Ovality and
sag depend on which way is up, so they are applied *after* the rigid rest.
The order is: round shape, rest, flatten and sag in the settled frame, then
mesh.

1. **Support set.** Sample the round, unsagged bark surface at a fixed
   analysis resolution (independent of LOD), and add stub, end and fork
   tips. The result is a point cloud.
2. **Centre of mass.** Estimate it from the section areas along each limb's
   curve, minus any hollow. Stone instead counts occupied cells in an SDF
   grid, which deadwood does not have.
3. **Rigid rest.** Port stone's `settle_direction` and `rotation_to_down`.
   They already work on surface-point offsets from the centre of mass, so
   they carry over unchanged in spirit. Candidate down-directions are tried
   on a golden-angle spiral, with descent to a lower, stable side and
   refinement. To stop the descent standing a log on its end, the search is
   biased toward lying with the main axis near-horizontal. A fresh log comes
   to rest on its stubs and forks.
4. **Ovality** (rises with decay). Squash each section along the settled
   world-up direction, projected into the section plane.
5. **Sag** (decay ≥ 2). Between the contact supports, bend each limb's centre
   line down toward y = 0. The allowed deflection rises with decay, from
   stiff at class 1 to fully conforming at class 4. Child limbs and stubs
   ride with their parent's deformation at their attachment points. Sag
   changes the curves before meshing, so every LOD sees it.
6. **Lift and burial.** Lift the lowest point back to y = 0. Then compute
   the burial suggestion, a separate number as in stone, from 0 to about
   0.3·diameter in classes 4–5. The caller applies it.

`settle: false` skips steps 3–5 and only lifts the piece to y = 0. The pose
is recorded as `DeadwoodPlacement { rotation, translation, burial }`, and the
sagged and flattened curves are kept with the piece.

**Stumps** never settle. Ground level is fixed at the base of the flare, and
burial is not applied. **`FallenTree`** (stretch) runs the rigid rest on the
log with one extra fixed support: the stump's highest break point, lowered by
a small random amount, following Infinigen. It then sags between that support
and the ground contacts. This is the one place a piece rests on something
other than flat ground. General resting on other objects stays a non-goal.

### Detail and LOD

```rust
pub struct DeadwoodDetail {
    pub shape: BodyShape,      // Solid | Ribbon (sticks/twigs only)
    pub sides: u32,            // around the main axis at base diameter
    pub rings_per_metre: f32,  // along the axis (plus curvature refinement)
    pub cap_rings: u32,        // radial rings in end caps
    pub min_radius: f32,       // drop stubs/twigs thinner than this
    pub stripped_lip: bool,    // mesh the bark-edge lip ring
}
```

The default ladder below is for a log 0.35 m across and 3 m long. It scales
with diameter.

| Level | From | Sides | Rings/m | Cap rings | Min radius | Notes |
|---|---|---|---|---|---|---|
| 0 | 0 m | 32 | 24 | 6 | 0 | Snapped splinters, lips, all stubs |
| 1 | 8 m | 16 | 8 | 3 | 0.01 | Splinters flattened, no lips |
| 2 | 25 m | 8 | 2 | 1 (n-gon) | 0.03 | Stubs above 3 cm only |
| cull | 80 m | — | — | — | — | — |

- Sticks use `Ribbon` from level 1 and cull at about 15 m.
- Side counts scale with each limb's radius, with a minimum of 3. A helper
  picks counts from a target edge length.
- Every level is a closed solid with shells, so a level can serve as a
  collider.
- Numbers are starting points to tune in the viewer.

### Mesh output

```rust
pub struct DeadwoodMesh {
    pub surfaces: MeshBuffers,   // positions, normals, uv0, tangents, wood attr, indices
    pub billboards: Option<Billboards>,
    pub cull: MeshBuffers,       // fallback ribbon strips
    pub parts: Vec<PartSpan>,    // ranges per Bark / Wood, per limb (shells)
}
```

- **UVs.** `uv0` is the bark layout from tree: u around, v along the arc
  length in metres times the texture scale, so tree bark textures tile the
  same way. End-grain and hollow surfaces use uv0 for their own maps.
- **Wood attribute.** Each vertex carries a `[f32; 4]` *wood attribute*:
  `zone` (bark, sapwood, end grain, inner wall), radius from the pith (for
  the ring shader), angle, and local decay or rot weight. Storing these in
  object space keeps the rings stable across LODs.

### Object

`Piece::object(&lod) -> Object`. Each level has up to three parts, one per
size tag (`TRUNK`, `LIMB`, `BRANCH`). Every part carries `DEADWOOD + WOOD`,
plus `BARK` while the piece has bark. Each part is `closed: true`, with one
shell per limb or stub covering its bark, caps, stripped patches and hollow
wall together. Ribbon sticks are skipped, as tree does with its ribbons.

A stump's trunk shell is tagged `TRUNK`, and its surface roots are shells in
a `BRANCH` part, so trunk moss grows on the stump but not on the roots.
`FallenTree::object(&lod)` returns one `Object` holding both pieces' parts,
with the log already in its rest pose.

### Scatter (`StickScatter`)

This follows `procgena_plant`'s field model, without its GPU generation.

A plant field is a rectangle in XZ. Its ground is one of these:

- flat;
- a heightmap image;
- a height function from code;
- a mesh baked once into a heightmap.

A density mask (from an image or a function) thins it out.

Sticks reuse that model on the CPU. They are few and they are meshes, so
records over a variant library are enough.

```rust
pub struct ScatterOptions {
    seed, size: Vec2 /* field rectangle, metres, centred */, density /* per m² */,
    size_mix /* FWD class weights */, variants: u32, decay_range, min_spacing,
}
pub trait Ground {                 // heights in metres; NaN = no ground here
    fn height(&self, xz: Vec2) -> f32;
}
// Implemented for: Flat, a closure, a sampled heightmap (cell size + values),
// and `HeightGrid::from_object(&Object, cell_size)` (bakes the highest
// surface, like PlantGround::Mesh).
pub struct StickScatter { variants: Vec<Piece>, instances: Vec<Instance /* variant, Affine3A */> }
StickScatter::new(&ScatterOptions, &dyn Ground, density: Option<&dyn Fn(Vec2) -> f32>)
```

- **Variants.** Each variant is a `Stick` or a small `FallenBranch` (a twig
  sprig), settled once in its own space on flat ground.
- **Placement.** Points come from jittered-grid sampling with `min_spacing`
  rejection, kept by the density function (0 bare to 1 full).
- **Fitting to the ground.** Each instance gets a random yaw. The ground is
  sampled under the variant's support points, at both ends and the middle.
  The stick is tilted to fit them and lowered so it touches. A NaN height
  drops the instance.
- **Overlap.** Sticks don't stack in v1.
- **Surfaces.** Flat ground is the default, and any surface works through
  `Ground`. `HeightGrid::from_object` lets sticks lie on a stone or a ground
  part.
- **Baking.** `object()` bakes the instances into one `Object` on request.

### Exports

- Features: `export-obj`, `export-gltf`.
- One node per LOD.
- The wood attribute is exported as a custom glTF attribute (`_WOOD`).
- Options go in `asset.extras.deadwoodOptions`.
- Ribbon sticks export their fallback strips.

---

## Bevy design (`procgena_deadwood_bevy`)

- `DeadwoodPlugin`, `DeadwoodParams`, `DeadwoodScatterParams`; async build
  and cache; `VisibilityRange` LOD.
- `GeneratedObject` is inserted when the build finishes, so a `ClumpParams`
  child (moss or snow) works on logs with no new code.
- **`DeadwoodMaterial`** extends `StandardMaterial` with WESL:
  - **Bark zone:** tree-style bark texture sets via uv0, with the colour
    shifted toward grey with decay.
  - **Sapwood zone:** a pale grey-silver tint with fine streak noise along
    the grain.
  - **End grain:** procedural rings from the wood attribute's pith radius,
    using a saw profile with noise distortion, as in Infinigen. Also a darker
    heartwood disk and radial checking cracks (angular Voronoi near the rim).
    An optional CC0 end-grain texture can be multiplied on top.
  - **Rot weight:** darkens and reddens toward decay class 5 (unverified
    tuning).
- The stick ribbon billboard shader is copied from tree.
- Includes an `examples/viewer.rs` and headless tests.

## Studio

A **Deadwood** tab with the ten touch points:

- presets and the decay slider up front;
- end-kind pickers;
- an export panel;
- a scene mode that shows a single piece, a scatter patch, or (stretch) a
  stump with its fallen trunk;
- an "add moss" toggle that spawns an existing clump preset as a child.

## Textures

`deadwood/textures.yml`, for the textures tool:

- **Bark:** reuse the tree bark set ids from Poly Haven (`pine_bark`,
  `bark_brown_01/02`, `knotted_pine_bark` and others).
- **End grain:** ambientCG `TreeEnd003` (stump top) and `TreeEnd005` (cut
  end, photogrammetry, about 45 cm). Both are multiplied over the procedural
  rings. Their licence is **CC0 1.0** (checked on
  https://docs.ambientcg.com/license/: commercial use allowed, no credit
  needed). Which maps are in the zips is not checked yet; the textures tool
  will show this.

---

## Presets (v1)

| Preset | Kind | Decay | Notes |
|---|---|---|---|
| `FreshCutLog` | Log | 0.5 | Saw both ends, intact bark, stubs, raised |
| `WindthrowLog` | Log | 1.5 | Snapped tip, saw base, twig stubs |
| `MossyLog` | Log | 3 | Stripped patches, sagging, pairs with the moss clump |
| `HollowLog` | Log | 3.5 | Open hollow, snapped ends |
| `RottenLog` | Log | 4.5 | Oval, no bark, buried, rounded breaks |
| `BirchLog` | Log | 2 | Pale bark colour, thin, saw ends |
| `FallenLimb` | FallenBranch | 1 | Two levels plus twigs |
| `DeadBranch` | FallenBranch | 2.5 | Bare, one level, snapped |
| `Kindling` | Stick scatter | 1–3 | Mix of 10-hr and 100-hr classes |
| `TwigLitter` | Stick scatter | 1–4 | 1-hr and 10-hr classes, mostly ribbons |
| `FelledStump` | Stump | 0.5 | Saw top, low, moderate flare |
| `NotchedStump` | Stump | 1 | Chainsaw top with hinge, buttressed |
| `SnappedStump` | Stump | 2 | Tall jagged break, surface roots |
| `RottenStump` | Stump | 4 | Hollow open top, no bark, soft roots |
| `Windthrow` (stretch) | FallenTree | 1.5 | Matching snapped break, log resting on stump |

## Tests

- **Options:**
  - JSON round trip and deep merge;
  - `validate` rejects out-of-range decay, lengths and counts;
  - every preset validates.
- **Determinism:** the same options give bit-identical meshes. A different
  seed gives a different mesh.
- **Closedness:**
  - every Solid level is closed per shell after welding
    (`Surface::is_closed`);
  - this holds for each `EndKind`, for hollow logs open at one or both ends,
    with stubs, and for stumps with flare lobes and surface roots;
  - no non-finite values appear.
- **Tags:**
  - `Object::validate` passes;
  - each size part carries the expected tags, and `BARK` disappears once no
    bark remains;
  - `merged(0, moss_preset.grows_on)` is non-empty on a log, including a
    bark-less `RottenLog`;
  - a clump moss build on `RottenLog` succeeds.
- **Settle:**
  - the lowest point is at y = 0 before burial;
  - the centre of mass is over the support hull;
  - all LODs share the pose;
  - a fresh log with stubs rests above the ground along part of its length;
  - a decay 4 log touches along most of its length;
  - `settle: false` keeps the orientation.
- **Stumps:**
  - the flare base sits at y = 0;
  - the roots and below-ground caps are under y = 0;
  - the radius at the flare base exceeds the trunk radius by about
    `flare_width`.
- **FallenTree (stretch):**
  - the stump's top and the log's base, put back together, coincide within
    a tolerance sample by sample;
  - the rested log touches the stump's highest point and the ground.
- **LOD:** the triangle count falls level by level, and levels 1 and 2
  respect `min_radius`.
- **Scatter:** reproducible, respects spacing, and each instance's variant
  index is valid.
- **Export:** OBJ and GLB parse, with one node per LOD.
- **Bevy:** a headless build finishes, `GeneratedObject` is inserted, and a
  clump child builds on a log.

---

## Phases

| Phase | Work | Depends on |
|---|---|---|
| 0 | **Coordinator:** tags `DEADWOOD` and `WOOD` in `procgena_mesh/src/tag.rs`; `WOOD` in the clump moss and snow presets' `grows_on`; `.gitignore` / `_typos.toml` for `deadwood/prior_art`; clone the remaining prior art (Infinigen is already in `mushroom/prior_art`); `CLAUDE.md` rows. The workspace members are added at the end of Phase 1, once the crate compiles. | — |
| 1 | Scaffolding, `options`, `presets` (data only), `rng`, `noise`, `curve` and frames | 0 |
| 2 | Sweep mesher: cross-section, `Saw` and `Taper` ends, UVs and tangents, wood attribute, LOD ladder, closedness tests | 1 |
| 3 | The remaining ends (`Chainsaw`, `Axe`, `Snapped` with splinters); stripped bark with lip; bark-plate grooves | 2 |
| 4 | Stubs; hollow core with annulus ends | 2 (parallel with 3) |
| 5 | Branching (`FallenBranch`); stick ribbons, billboard data and `cull` | 2 |
| 5b | Stumps: vertical centre line, root flare and lobes, surface roots, below-ground caps | 3–4 |
| 6 | Settle: support set, rigid rest, sag, burial; tie the decay mapping together | 3–5 |
| 7 | `object()`, `StickScatter` | 6 |
| 8 | Exports (OBJ, glTF) and an example | 7 (parallel with 9) |
| 9 | Bevy crate: plugin, `DeadwoodMaterial` and WESL (zones, rings, checking, rot), billboard shader, LOD, scatter, viewer | 7 (the material can start after 2) |
| 10 | `deadwood/textures.yml` and the tools entry (after the licence check) | 9 |
| 11 | Studio Deadwood tab | 9 |
| 12 | Tune presets and the LOD ladder in the viewer; docs, READMEs, comment rewrite | all |
| 13 | **Stretch:** `FallenTree`: shared break surface, rest on the stump, combined `Object`, Bevy and studio scene mode | 6, 7, 9, 11 |

## Non-goals for v1

- Physics-engine settling (rapier or avian). The settle is geometric only.
- Settling single logs onto a host `Object` or terrain, and logs resting on
  other logs or rocks. Stick scatter does fit the ground, through `Ground`.
- GPU generation of sticks, as plant does for blades.
- Octahedral impostors. The coarsest level is a low-side closed tube, then
  cull.
- Late-decay fracture blocks (cubical brown rot) as geometry.
- Knots as geometry. Knot ring distortion in the end-grain shader is v1.1.
- Species matching with procgena_tree presets. Bark kind and colour are
  chosen by hand.
- Generating moss directly. Moss comes from existing clump presets as a
  child.

## Decisions (2026-11-22)

1. **Name:** `procgena_deadwood`, in the folder `deadwood/`.
2. **Billboard code:** copied from tree, as mushroom does. Tree is not
   changed.
3. **Burial:** left to the user, as stone does. The depth is reported and
   not applied by default.
4. **End grain:** the procedural rings, plus the ambientCG `TreeEnd*`
   textures (CC0, checked).
5. **Stick scatter:** works like plant fields, on flat ground by default.
   Any ground surface is supported through `Ground`.
