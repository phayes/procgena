# IX plan: lean heavily on impostors

Worker plan for roadmap milestone IX ("Experiment: lean heavily on
impostors"). Written 2026-11-24 from the decisions recorded in
[ROADMAP.md](../../ROADMAP.md) (IX, "Decided by the user (2026-11-25)" and
"The rest of the planning talk"), before any code. Each section says
whether it is **decided** (by the user, cited), **proposed** (by the
worker or coordinator, waiting for a yes) or an **open question**.

## Bottom line

- **The model (decided).** A preset is content: what the thing is, its
  shape, its style and its `game_level`, the one detail level that is the
  object's real geometry. Drawing is presentation, the same for every
  kind. Every copy is drawn through one chain:
  `game_level` mesh → one lower mesh (derived from `game_level` by the
  kind's default rule) → impostor (baked from `game_level`). Steps switch
  by size on screen alone (a copy's bounding-sphere diameter as a
  fraction of the screen's height), with the same thresholds for every kind; the quality setting
  scales the thresholds. The chain is not on the preset: one project
  default for now, Bevy hooks to change it later. A game cannot override
  `game_level`.
- **Dropped (decided).** The ribbon far forms `FarForm::Strips`,
  `BillboardLeaves`, `Card` and `FlatCapRibbon`, the levels that use them,
  per-level metre distances and hysteresis on presets, and the up-to-8
  level table. `GroundMaterial` stays for grass, which keeps its own
  thinning for now.
- **API changes (agreed 2026-11-25).** In `procgena_core::chain` (a new
  module beside the old `lod`; see "Step order"):
  - `Lod<D> { game: D, lower: D }`, built with `Lod::from_game(game)`
    through a `LowerDetail` trait each kind implements (its default rule).
  - `Step { Game, Lower, Impostor }` replaces `LevelForm`.
  - `DistanceChain { lower_below, impostor_below, cull_below, hysteresis }`
    holds thresholds as fractions of screen height. `DistanceChain::DEFAULT`
    is the project default (0.10, 0.04, 0.002, hysteresis 0.1).
  - `ViewScale`, from a camera's field of view, and a copy's size (its
    bounding-sphere diameter) turn a chain into today's metre `LodRange`s.
    The dither shader, `LodRange`, `visibility_range` and the GPU fade code
    keep working in metres, unchanged.
  - `Quality::distance_scale` divides every threshold, so higher quality
    means smaller thresholds and detail kept further away.
  - Impostor frame size (`procgena_impostor::frame_size_for(chain.largest_impostor_size())`) is sized for
    the largest on-screen size any quality shows (`Quality::Low`), at a
    reference screen height, rounded up to a power of two, clamped to
    32–512, mipmapped, and part of the bake key. No rebake on a quality
    change.
- **Step order (decided).** Core first, then stone,
  then dead wood, mushroom, tree and fruit one at a time, then the
  renderers and the studio. To keep the workspace building after every
  step, Step 1 **adds** the new types beside the old ones, each asset step
  moves one kind across, and the old `Lod`/`LodLevel`/`LevelForm` and the
  ribbon `FarForm` variants are deleted in the step that moves their last
  user. That is ordering, not a compatibility layer: nothing converts
  between old and new except the private stone bridge (`renderer_lod` in
  `procgena_stone_bevy`, see Step 2's "As built"), deleted in Step 4, and
  no asset keeps both.
- **Deferred (decided).** Physics colliders reading `game_level`; stone
  cluster specimens; more cluster specimens for other kinds; grass;
  impostor shadows and tree sway in impostors; Bevy hooks for a game's own
  chain; shipping pre-baked impostors; other machines.
- **Experiment (decided).** On this Mac only. Stones first, then dead wood
  and mushrooms, trees last. Judged by a blind side-by-side on the oak and
  maple forest, the pine forest floor and one walk through a Valheim-scale
  World. The bar for a new default is **proposed, not confirmed**: no
  visible difference and at least 20% less frame time.
- **Answered by the user (2026-11-25):** see "Answers" at the end.

## The model (decided)

From ROADMAP IX, the user, 2026-11-25:

- **`game_level` is a setting of the preset.** It is a style: each preset
  says which detail is its real geometry. The default is per kind
  (the user, 2026-11-25): trees default to level 0 (full detail); every
  other kind defaults to level 1 (today's second level), with level 0
  opt-in per preset. Colliders, picking, CPU
  queries and impostor bakes all read it. Coarser geometry exists only for
  drawing far away.
- **The preset carries only its `game_level` detail.** The lower mesh is
  derived by a default rule per kind, not listed in the preset.
- **At most one lower mesh, and every kind has one.** Mushrooms and small
  dead wood pass through their lower mesh too, just quickly.
- **The distance chain is not on the preset.** One default for now,
  scaled by quality.
- **Switching is by size on screen**, a copy's largest dimension (its
  bounding-sphere diameter) over the screen's height, the same thresholds
  for every kind. No kind has its own rule. (First decided as the copy's
  height; changed by the user 2026-11-25 so fallen logs don't switch
  early.)
- **Impostors are baked from `game_level`**, carry normals (they do
  today) and cast no shadows. They bake while loading and are cached on
  disk, keyed by the preset's content hash plus the impostor settings.
  The studio pre-bakes every preset's impostors in a low-priority
  background task.
- **A cluster is just a specimen** and gets one impostor.
- **One churn window, no compatibility layer.**
- **The studio** shows `game_level` by default; ST7b's viewing distance
  becomes "preview the chain": pretend a distance, force the lower mesh
  or the impostor, and read back which step is drawn and why.

## API (proposed)

### `procgena_core::chain`

The new types live in a new module, `procgena_core::chain`, while the old
`procgena_core::Lod` (in `lod`) still has users. Assets import
`procgena_core::chain::{Lod, Step, DistanceChain, ...}`. When the last
user of the old types moves, the old types go, and the crate root
re-exports the new ones. `LodRange` stays in `lod` and is shared by both.

```rust
/// One step of the drawing chain, nearest first.
pub enum Step {
    /// The preset's `game_level` mesh: the object's real geometry.
    Game,
    /// The coarser mesh derived from `game_level`.
    Lower,
    /// The baked impostor, from `game_level`.
    Impostor,
}

/// The two mesh details of one kind's chain. `D` is the kind's detail type
/// (`StoneDetail`, `DeadwoodDetail`, ...).
pub struct Lod<D> {
    pub game: D,
    pub lower: D,
}

/// A kind's default rule for its lower mesh.
pub trait LowerDetail: Sized {
    /// The coarser detail drawn after `self` as the game level.
    fn lower(&self) -> Self;
}

impl<D: LowerDetail> Lod<D> {
    /// `game`, and its lower mesh by the kind's rule.
    pub fn from_game(game: D) -> Self;
}
impl<D> Lod<D> {
    pub fn detail(&self, step: Step) -> Option<&D>; // None for Impostor
    pub fn map<E>(self, f: impl FnMut(D) -> E) -> Lod<E>;
}
```

`Lod` no longer holds distances, a reference size, a cull distance,
hysteresis, forms or shadow flags. Which steps cast shadows is fixed:
`Game` and `Lower` do, `Impostor` does not (decided: impostors cast no
shadows).

### `procgena_core::chain::DistanceChain`

```rust
/// Where each step of the chain takes over, as a copy's size on screen
/// (its bounding-sphere diameter) over the screen's height, at
/// Quality::High.
pub struct DistanceChain {
    /// Below this size the lower mesh takes over from `game_level`.
    pub lower_below: f32,
    /// Below this size the impostor takes over from the lower mesh.
    pub impostor_below: f32,
    /// Below this size nothing is drawn.
    pub cull_below: f32,
    /// Half the width of each cross-fade, as a share of the switching
    /// distance, from 0 up to but not including 1.
    pub hysteresis: f32,
}

impl DistanceChain {
    pub const DEFAULT: Self; // 0.10, 0.04, 0.002, 0.1
    pub fn validate(&self) -> Result<(), ChainError>; // at every quality
    /// size × view scale: the share of the screen a copy covers 1 m away.
    pub fn factor(size: f32, view: ViewScale) -> f32;
    /// The three thresholds at `quality`, each scaled on its own (so IX2
    /// can leave `impostor_below` out of the scaling).
    pub fn thresholds(&self, quality: Quality) -> [f32; 3];
    /// The fade bands of each step, in metres from the camera, for a copy
    /// with `factor` at `quality`. Index with `Step::index`.
    pub fn ranges(&self, factor: f32, quality: Quality) -> [LodRange; 3];
    /// Like ranges, but ending where a host's impostor begins (fruit).
    pub fn ranges_ending_at(&self, factor: f32, quality: Quality, end: Range<f32>)
        -> Vec<LodRange>;
    /// The single step to draw `distance` metres away, or None past the
    /// cull; for picking and the studio's read-back.
    pub fn step_at(&self, distance: f32, factor: f32, quality: Quality) -> Option<Step>;
    /// The steps copies within `distances` and `factors` can be drawn at.
    pub fn steps_touched(&self, distances: RangeInclusive<f32>,
        factors: RangeInclusive<f32>, quality: Quality) -> Option<RangeInclusive<Step>>;
    /// The largest share of the screen's height an impostor is ever drawn
    /// at, at `quality`, fade band included.
    pub fn impostor_max_size(&self, quality: Quality) -> f32;
    /// The largest of those over every quality: what one bake is sized for.
    pub fn largest_impostor_size(&self) -> f32;
}

/// How big something looks on screen: a copy `s` metres across, `d` metres
/// away, covers `s × scale / d` of the screen's height. For a perspective
/// camera, 1 / (2 tan(fov_y / 2)).
pub struct ViewScale(f32);
impl ViewScale {
    pub const DEFAULT: Self; // Bevy's default 45° field of view
    pub fn perspective(fov_y: f32) -> Self;
}
```

How it works: a copy `s` metres across at `d` metres covers `s × k / d`
of the screen, where `k` is the `ViewScale`. A threshold `t` is therefore
the distance `s × k × distance_scale / t`. `ranges` turns the three
thresholds into the same metre fade bands as today, so
`LodRange::dither_level`, `to_vec4`, the dither shader and
`visibility_range` stay as they are, and the instanced renderer keeps
passing per-slot bands scaled by each copy's size. What changes in the
renderers is that the bands are computed per view from `k` and quality,
for a 1 m copy, and multiplied by each copy's bounding-sphere diameter on
the GPU, as `InstancedCopy::size` is today.

- **Several cameras:** each view has its own `k`. Shadow passes use the
  main view's bands, as today.
- **Orthographic cameras** (the studio's camera modes): size on screen
  does not change with distance, so one step is chosen for the whole
  view from the copy's size over the view's height. `ViewScale` grows
  an orthographic case if the studio needs it; first use decides.
- **`levels_touched`** (fruit's coarse per-host check) becomes
  `steps_touched` over distance and factor ranges, same idea.

### Quality

`Quality::distance_scale` (0.4, 0.7, 1, 1.5) stays, because terrain uses
it. The chain divides each threshold by it: at Low the lower mesh takes
over when a copy is 2.5 times larger on screen than at High. Higher
quality, smaller thresholds, detail kept further away. Quality never
changes a copy's `game_level`.

### Impostor frame size

An impostor is never shown larger than its frame (decided). The largest
it is drawn is `impostor_max_size(quality)` of the screen's height: where
it starts fading in, `impostor_below / (distance_scale × (1 − hysteresis))`.
The frame covers the specimen's bounding sphere, the same measure as the
copy's size, so:

```rust
// procgena_impostor
/// Pixels along each side of one impostor view for an impostor drawn at
/// most `max_size` of the screen's height, on a screen
/// REFERENCE_SCREEN_HEIGHT pixels tall, rounded up to a power of two, from
/// 32 to 512. Pass `chain.largest_impostor_size()`.
pub fn frame_size_for(max_size: f32) -> u32;
pub const REFERENCE_SCREEN_HEIGHT: u32 = 1440;
```

It takes a size on screen rather than a chain or a quality, so IX2 (a
later milestone: the impostor switch fixed for every quality, Quality
moving only `lower_below` and `cull_below`) changes only
`DistanceChain::thresholds` and what callers pass here.

**Decided (the user, 2026-11-25): one bake per preset**, sized for the
largest on-screen size any quality can show (`Quality::Low`, where the
impostor threshold is largest), with mipmaps so higher qualities sample a
smaller mip. A quality change never rebakes, and an impostor is never
shown larger than its frame. The frame size goes in the bake key
(`BakeSettings::frame_size`, as today) but does not depend on the active
quality. With the default chain: Low shows an impostor up to
0.04 / (0.4 × 0.9) = 0.111 of the screen, 160 pixels, so frames are 256;
a High-sized bake would be 0.044 × 1440 = 64 pixels, 64 frames. That is
16 times the atlas memory per specimen (plus a third for mipmaps); the
experiment reports it.

### How an asset declares `game_level` and derives its lower mesh

Each kind's preset gets a `game_level` field holding its detail type (it
replaces `detail` where there is one). The kind implements
`LowerDetail` for its detail type. The Bevy crate builds
`Lod::from_game(options.game_level)`, builds those two meshes, bakes the
impostor from `game_level`, and draws through the shared chain.

**The default `game_level` is per kind** (decided: trees level 0, the
rest level 1). Each kind says it in one place: the `Default` of its
detail type, which the preset's `game_level` field uses when a preset
file leaves it out (`#[serde(default)]`). So `LodDetail::default()` is
full detail for trees, and `StoneDetail`, `DeadwoodDetail`,
`MushroomDetail` and `ClusterDetail` default to today's level 1. Core
adds no per-kind table; the chain and the lower rule stay the same for
every kind. A tree's lower mesh is then derived from full detail, which
is roughly today's "middle" or "far" level, not its "farthest".

Concrete first proposals (the experiment tunes them):

| Kind | Today | `game_level` default | Lower rule |
|------|-------|------------------------------------------|------------|
| Stone | `detail: StoneDetail`, a 3-level table | One `{ resolution, texture_size }`. The six names keep their level-0 resolutions; the default is Medium Low (48, 512; decided). | Half the resolution (at least `MIN_RESOLUTION`), texture one size smaller (at least 512) |
| Dead wood | `detail: DeadwoodDetail` (used by `Piece::mesh`); LOD on `DeadwoodParams` | `HIGH` (16 sides, 1.2 per diameter, 3 cap rings) | Today's `LOW` scaled from the game detail: half the sides (at least 4), a third of the segments, 1 cap ring, `min_radius` and `ribbon_radius` × 4, half the leaf share. `strip_all` and `STRIP` go. |
| Mushroom | `detail: MushroomDetail` (used by `Mushroom::mesh`); LOD on `MushroomParams` | `HIGH` (solid, 24/8 cap, 12/6 stem) | `Shell`, half of each count (at least 3 sides, 1 segment), no annulus. `BodyShape::Flat` and `Billboard` go with the ribbon forms. |
| Tree | No detail on `TreeOptions`; `default_lod()` on `TreeParams` | New `game_level: LodDetail` on `TreeOptions`, default today's "near" (level 0, full detail; decided) | Between today's "middle" and "far", relative to the game detail: section stride and leaf stride × 3, segment factor × 0.5, open ends, `fruit_distance` dropped (see fruit). `BranchShape::Ribbons` and camera-facing leaves go. |
| Fruit | `default_lod()`, attached to trees | `ItemDetail::Simple` (today's level 1) | `Minimal` without flat items. Its chain ends where its host's impostor begins (`ranges_ending_at`), and the host's impostor already shows it. |

Dead wood's `ribbon_radius` (thin twigs as camera-facing strips inside a
mesh level) is not one of the dropped far forms, so it stays unless the
user says otherwise.

Tree gaining `game_level` on its options changes its content hash and its
preset files; that touches ST2's preset registry, so Step 3's tree part
is coordinated with ST2.

### Size on screen: today's distances converted

Today's switches as a share of screen height, at each kind's reference
size, with Bevy's default 45° vertical field of view (`k` = 1.207). The
reference sizes measure different things today (stone and mushroom a
diameter, tree a height, dead wood `max(diameter, 0.15 × length)`).

| Kind (reference) | Today's level 1 → 2 | Level 2 → far form or 3 | Impostor | Cull |
|------------------|---------------------|-------------------------|----------|------|
| Stone (2.5 m) | 0.20 (15 m) | 0.075 (40 m) | 0.038 (80 m) | none |
| Tree (10 m) | 0.48 (25 m) | 0.24 (50 m, far form) | 0.10 (120 m) | yes |
| Dead wood (0.45 m) | 0.022 (25 m) | 0.012 (45 m, strips) | none | 0.006 (90 m) |
| Mushroom (8 cm) | 0.024 (4 m) | 0.0097 (10 m) | none (card at 18 m, 0.0054) | 0.0024 (40 m) |
| Fruit (10 cm) | 0.015 (8 m) | | 0.006 (20 m) | host's |

Under the new chain the game level is today's level 1 (level 0 for
trees), so for every kind but trees "lower from" corresponds to today's
second column and "impostor from" to the third.
Trees switch at 5 to 20 times the size on screen of the small kinds today.
One shared pair has to sit between them. **Proposed default:**
`lower_below = 0.10`, `impostor_below = 0.04` (about 58 pixels on a
1440-pixel screen), `cull_below = 0.002` (about 3 pixels),
`hysteresis = 0.1`. At the reference sizes that gives:

| Kind (reference) | Lower mesh from | Impostor from | Cull at |
|------------------|-----------------|---------------|---------|
| Stone (2.5 m) | 30 m (today 40 m) | 75 m (today 80 m) | 1.5 km |
| Tree (10 m) | 121 m (today 25 m) | 302 m (today 120 m) | 6 km |
| Dead wood (a 3 m log, whose `lod_size` is today's 0.45 m reference; measured now by its ~3 m bounding sphere) | 36 m (today 25 m) | 91 m (today none) | 1.8 km |
| Dead wood (a 0.45 m branch) | 5.4 m | 14 m | 270 m |
| Mushroom (8 cm) | 1.0 m (today 4 m) | 2.4 m (today none) | 48 m |

Small things (mushrooms, branches) reach their impostor much sooner than
they reach any far form today; long logs, measured by their length now,
keep their meshes somewhat further; a 10 m tree keeps its meshes much
further than today. If
trees need to switch sooner, that argues for different shared numbers
or, against the decision, a per-kind rule; the experiment should show
which. Its first measurement is this table at a few candidate pairs.

### The size measure

Decided (the user, 2026-11-25, replacing "height"): a copy's largest
dimension, its bounding-sphere diameter, as a share of the screen's
height. Every kind measures it the same way: the diameter of the sphere
around the specimen's game-level mesh, times the copy's scale. A fallen
log therefore keeps its meshes as far as its length suggests, and dead
wood's own `lod_size` (`max(diameter, 0.15 × length)`) goes. The same
sphere is what an impostor frame covers. The ROADMAP still lists
"revisit the unit after the experiment".

## Steps

Each step is one commit (or a few), checked with the scoped checks, and
reported to the coordinator ready to merge.

0. **This plan**, and the API agreed.
1. **Core.** Add `Step`, the new `Lod<D>`, `LowerDetail`, `DistanceChain`,
   `ViewScale` and `ChainError` to `procgena_core::chain`, with tests in
   `tests/chain.rs`. Add `frame_size_for` to `procgena_impostor`. The old `Lod`, `LodLevel`, `LevelForm` and
   `FarForm` stay until their last user moves (see Bottom line). Touches
   core only, but core rebuilds most of the workspace: the coordinator
   times it.
2. **Stone** (after R3a lands). `StoneDetail` becomes one
   `{ resolution, texture_size }`; the six names stay as choices;
   `distance`, `hysteresis` and the level list leave the preset;
   `LowerDetail for StoneDetail`; `procgena_stone_bevy` draws game →
   lower → impostor by size on screen, impostor baked from the game mesh.
   Stone preset YAML files change (`detail:` → `game_level:`).
   **As built (Step 2):**
   - `MeshOptions::game_level: StoneDetail` (`{ resolution, texture_size }`,
     default `MEDIUM_LOW`); `DetailLevel`, `StoneLevel`,
     `LOD_REFERENCE_SIZE` and `MAX_DETAIL_LEVELS` are gone. A `Stone`
     always builds two meshes, `mesh()` and `lower_mesh()`.
   - The lower rule: half the cells (at least `MIN_RESOLUTION`), one
     texture size smaller (at least 512).
   - `game_level` stays out of the options hash and so out of the pool id
     and variant seeds (changing detail must not re-seed rocks; rule 3).
     It is in the impostor bake key and the single rock's cache key.
   - Size measure: `StoneOptions::bounding_diameter()`, the stretched
     containing box's diagonal, used by single rocks and pools alike. On
     the presets it is 1.5–1.9× the mesh's widest span (4.5× for `ore`),
     so stones keep their meshes further than the plan's table says. Real
     bounds don't exist yet: the GPU mesher's `bounding_radius` is the same
     cube bound, and the pool culls with a fixed radius. Follow-up in
     ROADMAP: real per-specimen bounding spheres on both paths.
   - The specimen's lower level keeps `Level::start` until the last asset
     step: the distance where the default chain switches to the lower mesh
     at the default view and High quality.
   - No impostor (still baking, or `StonePlugin { impostor: None }`): the
     lower mesh is drawn until the cull. Impostors carry
     `NotShadowCaster`; the default bake is 8 × 8 views of
     `frame_size_for(DEFAULT.largest_impostor_size())` = 256 pixels.
   - Pools draw through the shared instanced renderer, which still takes
     the old metre `Lod`. Until Step 4, `procgena_stone_bevy`'s private
     `renderer_lod` translates the chain into that table (exact at the
     default 45° view, tested against `DistanceChain::ranges` at every
     quality). This is the one temporary translation between old and new;
     it goes when the renderer takes the chain.
3. **Dead wood, mushroom, tree, fruit**, one at a time, each dropping its
   ribbon far forms and moving to the chain. The last one deletes the old
   core types and the ribbon `FarForm` variants (`GroundMaterial` stays,
   possibly moving to plant).
4. **Renderers and the studio.** `procgena_core_bevy`'s instanced renderer
   takes three steps and per-view bands; impostors bake from `game_level`,
   with normals and no shadows; the disk cache (key: preset content hash +
   `BakeSettings`, which includes the frame size); the studio's
   low-priority background pre-bake; ST7b's chain preview. Studio edits
   are coordinated with ST2. "The preset content hash" in the IX
   decision means the bake key: the specimen id plus the game level's hash
   plus `BakeSettings`. Step 4 also deletes the stone bridge.
   - **Bake only impostors that can be on screen** (from the ED worker,
     2026-11-25; measured: a cold studio load of the 40 m Ecotope bakes 10
     tree and 7 stone impostors, 3–4 s plus pipeline compiles, none of
     which the orbit camera can ever show). A resource beside `LodQuality`
     holds the farthest view distance (default unlimited; the studio sets
     it from its orbit limits). Before requesting a bake, an asset asks a
     shared gate whether the specimen's impostor step can be on screen:
     its size on screen at the farthest view distance against
     `impostor_below` at the current quality. A specimen held back draws
     its lower mesh to the cull and asks again when the limit grows.
     Disk-cached impostors still load when reachable. Test: a limit below
     the impostor's start makes no request; raising it makes one. To
     decide in Step 4: how the gate sits with the studio's background
     pre-bake, which bypasses it on purpose.
5. **The experiment** on the new model, recorded in this file.

Where a renderer change is needed to keep an asset drawing in steps 2 and
3 (the instanced renderer reads `Lod` today), the asset step includes the
smallest such change, and Step 4 finishes the rest.

## Experiment setup (decided, bar proposed)

- **Question:** how close impostors can come, per kind and size, before
  they look wrong, and what that saves in frame time, draw calls,
  triangles, memory and bake time.
- **Looks wrong:** parallax, lighting and shadow mismatches, the fade from
  mesh, views from below on slopes, wind.
- **Order:** stones, then dead wood and mushrooms, trees last.
- **Scenes:** the oak and maple forest, the pine forest floor, one walk
  through a Valheim-scale World.
- **Judged by:** a blind side-by-side, and the user's eye.
- **Machine:** this Mac only.
- **Variants:** impostor threshold pulled in; more views or larger frames
  for near impostors; depth or normal lighting; cluster impostors (once
  cluster specimens exist); impostor shadows and tree sway later.
- **Bar for a new default (proposed by the coordinator, not confirmed by
  the user):** no visible difference in the blind side-by-side, and at
  least 20% less frame time.

## Deferred (decided)

Physics colliders from `game_level`; stone cluster specimens (a
`StoneScatter` class places up to `MAX_CLUSTER_MEMBERS` separate copies
today); clusters for trees and other kinds; grass (keeps its thinning and
far ground colour); impostor shadows and sway; Bevy hooks for a game's
own chain; shipped pre-baked impostors; other machines; revisiting the
size unit.

## ARCHITECTURE.md passages this supersedes

- Rule 2, "One LOD model": metre distances with hysteresis written for a
  reference size; "every asset names its far form"; "meshes, then the far
  form, then the impostor"; "one quality setting that scales every
  asset's distances". Updated as Step 1 and each asset step land.
- The TODO "Impostors per specimen": "Deadwood and mushroom have no
  impostor: their far forms are already 2 to 22 triangles ... an impostor
  would save nothing." Under IX every kind ends in an impostor.
- The TODO "Content hash for the LOD types": `FarForm` and `LevelForm`
  hashing goes away; `Lod<D>` and `DistanceChain` get hashes instead.
- The survey table under "Asset alignment" (detail levels, far forms).
- "Loading by chunk" hangs the Scatter's draw range on the cull distance;
  see the cull question.

## Questions asked (2026-11-24)

Kept as asked; the answers follow.

1. **Terrain's chunk levels.** `procgena_terrain::chunk` and
   `procgena_terrain_bevy` use core's metre `Lod`/`LodLevel` and
   `distance_scale` for chunk levels. Those are not an asset chain, and
   G4b and Valheim own those files. Proposal: the metre table moves into
   `procgena_terrain` as its own chunk-level type when the last asset
   leaves the old `Lod` (end of Step 3), with a two-line import change in
   terrain coordinated with its owner. Alternative: it stays in core
   under a new name (`DistanceLod`). Which?
2. **`procgena_specimen::Level::start`** is a metre distance too, and
   every generator depends on that crate. Proposal: it goes in Step 1's
   window (a `Specimen` holds its `Game` and `Lower` levels, no
   distances), migrated kind by kind with the assets. Yes?
3. **Cull.** Today's chains end at a cull distance, and "Loading by chunk"
   bounds a Scatter's draw range by it. The decisions say nothing about
   it. Proposal: keep `cull_below` in the chain (default 0.002, about
   3 pixels), and derive a Scatter's draw range from its largest copy.
   Yes, or no cull for now?
4. **Default thresholds.** Proposed 0.10 / 0.04 / 0.002 with hysteresis
   0.1, at a 45° field of view. Fine as a starting point?
5. **Stone's six names.** Keep today's level-0 resolutions (Medium = 96)
   and change the default preset choice to Medium Low (48, today's
   Medium level 1), or shift every name down one level so "Medium" stays
   the default and means 48? Proposed: keep the resolutions, default
   Medium Low.
6. **Sequencing.** Step 1 adds the new types beside the old and the last
   asset step deletes the old, so the workspace builds after every step.
   That changes the brief's Step 1 wording ("removing the ribbon
   `FarForm` variants and metre distances"). Yes?
7. **Impostor frames follow quality** (rebake on a quality change, each
   size cached) rather than fixed at High. Yes?

## Answers (the user, 2026-11-25, through the coordinator)

1. **Terrain's chunk table** moves into `procgena_terrain` as its own
   type at the end of Step 3, coordinated with its owner then.
2. **Yes:** metre distances leave `procgena_specimen`'s levels in the
   same window, kind by kind.
3. **Keep cull** (`cull_below` 0.002), and derive each Scatter's draw
   range from its largest copy.
4. **The default thresholds** (0.10, 0.04, 0.002, hysteresis 0.1) are
   fine as the starting point.
5. **Stone** keeps its six names' level-0 resolutions; the default
   `game_level` is Medium Low (48).
6. **Yes:** new types beside the old; the last asset step deletes the old
   ones; the workspace builds after every step.
7. **Changed:** one bake per preset, sized for the largest on-screen size
   any quality can show (`Quality::Low`), with mipmaps so higher
   qualities sample a smaller mip. No rebake on a quality change, never
   shown larger than its frame, clamped to 32–512. The frame size is in
   the bake key but does not depend on the active quality. The
   experiment reports the memory cost against a High-sized bake.

Also changed: a copy's size on screen is its largest dimension (its
bounding-sphere diameter) over the screen's height, not its height, so
fallen logs don't switch early. That replaces `InstancedCopy::size` as a
height.
