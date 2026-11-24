# WorldTerrain: plan

`WorldTerrain` builds a whole world: an infinite tiling of regions, each
with its own terrain type, joined seamlessly. It lives in
`procgena_terrain` (module `world`) and replaces `StitchedWorld`, which
stays as a small test fixture.

## Bottom line

- **One low-frequency world elevation field, E(x, z).**
  - It is pure: a function of the world seed and position only (plus any
    authored overrides, which are a fixed input).
  - Every region's ground is `E(p) + relief(p)`.
  - E is the "dynamic starting height". Mountains sit high because E is
    high there, not because they ramp up from the plains. A plains region
    on high E is a high steppe.
  - Its slope `∇E` is the **drainage tilt**: it is part of the ground the
    rivers trace over. Whether that is enough is a hypothesis: preset
    relief slopes are several times any allowed `|∇E|`, so hollows will
    still catch many rivers. Phase W1 ends by measuring the crossing rate
    on a sloped E; if the gain is small, a separate drainage hint (for
    example, tracing rivers over E plus damped relief) follows.
  - Its zero is the **sea level**. An ocean is a terrain type whose relief
    sits below zero; the crate never models water bodies beyond that.
  - Its slope is bounded (about a gentle road grade, `|∇E| ≤ 0.05`), so
    neighbouring regions never differ by a cliff's worth of base height.
- **Seams need no change for E.**
  - Both sides of a border evaluate the same E, so base height is
    continuous automatically.
  - Offers keep carrying absolute heights, but negotiation smooths, mixes
    and bends towards the corners only the heights above E, then adds E
    back (decided by the user 2026-11-23, built after W1). So the
    agreement is E plus the agreed relief exactly, on bent edges and
    curved land too; each terrain keeps its border profiles as heights
    above E and adds E at the point itself.
- **Preset base offsets become equal.** `base.elevation` turns into an
  offset above E. The presets' current 20 m / 50 m / 120 m elevations
  would otherwise bring back a 100 m step at every plains–mountains
  border, so they are set to the same offset and E carries the
  differences.
- **Relief convergence (terrain Phase 2.5) handles the rest of the wall.**
  Near each edge both sides fade their relief toward one agreed relief
  amplitude, so mountains fade their peaks to foothills over about a
  kilometre (see "Relief convergence" in `plan.md`). In the measured
  plains–mountains border, E absorbs about 105 m of the 239 m mean gap and
  convergence is expected to absorb most of the ridge relief (to be
  measured).
  - **Order of work:** convergence changes a region's own relief, so it
    acts *before* offers are made. The target comes from each neighbour's
    stated relief amplitude, computed from the neighbour's options (pure,
    no negotiation round), and from `WorldOptions::relief_meeting`, a
    power-mean exponent (−∞ = only the rougher side fades, −1 = in
    proportion to roughness, the default, 1 = meet halfway). WorldTerrain passes each region its
    neighbours' amplitudes when it builds the generator: step 1 converge
    own relief, step 2 make offers, step 3 negotiate, step 4 build.
  - **Shared world noise (also Phase 2.5):** every region's noise layers
    are seeded from the world seed and the layer's role, so neighbours of
    the same family match point by point once their weights converge, and
    mountain ranges continue across borders. WorldTerrain passes the world
    seed into every region's `TerrainOptions`.
  - **E is the same idea one level up.** E is a value every region shares
    exactly, so it needs no convergence; region values that differ are
    converged near edges. A later "ruggedness" field, if adopted, would be
    another shared field of this kind.
- **WorldTerrain also defines the region graph: regions, edges, corners.**
  - Regions are cells of a jittered lattice (a Voronoi diagram of one
    seeded site per lattice cell), so the graph is a pure function of
    position. Any region's polygon, neighbours and corner incidence are
    computed from the sites in a fixed neighbourhood of lattice cells.
  - Borders are the Voronoi edges, subdivided and displaced by noise seeded
    from the edge id, so they wander naturally. Both regions compute the
    same polyline.
  - Because the graph is pure, it is known arbitrarily far ahead. The
    "neighbour appears later" problem (terrain plan, open question 7)
    disappears, and exploration order (single or multiplayer) never
    matters.
- **WorldTerrain gives the land; the World chooses what fills it**
  (decided 2026-11-23, see "Choosing biomes").
  - This crate owns geometry and height: the region graph, E, and E's
    height bumps.
  - The World crate (see ARCHITECTURE.md) owns the choice: which Biome
    fills each region, picked **field first, with authored pins**, from
    E plus its own selector fields (ruggedness, moisture). Each Biome
    names its one Terrain.
  - WorldTerrain gets each region's `TerrainOptions` from a **terrain
    source** it is handed, and never sees Biomes.
  - Land with nothing on it is built through the World with **empty
    Biomes**: a Biome holding a Terrain and no Ecotopes.
- **Caches, all rebuildable:**
  - region plans (`Terrain`), built lazily from the region's own options
    and its neighbours' offers (neighbours are never fully built);
  - optional per-region feature plans, in stages cached on their own
    (relief and seam offers, rivers, roads, local features), so a far
    region can be drawn from its relief and offers before its roads are
    routed (see [features_plan.md](features_plan.md), "Real-time
    generation and far vistas");
  - chunks, which stay pure and disposable.

## Why a field, not a starting height chosen while exploring

- **Determinism.** A height chosen from whatever was generated first
  differs with exploration order, so two players (or two runs) would get
  different worlds. A chunk must be the same whatever order it is built
  in.
- **Loops.** Going round any loop of neighbouring regions, the base height
  steps must add up to zero. That holds only if every region's base comes
  from one field over the world.

## What E looks like

```rust
pub struct ElevationOptions {
    pub continents: NoiseLayer,   // very long wavelength (50–200 km): land and ocean
    pub uplift: NoiseLayer,       // 10–50 km: highlands and lowlands within land
    pub tilt: DVec2,              // a constant slope (added 2026-11-23)
    pub max_slope: f32,           // bound on |∇E|, about 0.05
    pub bumps: Vec<HeightBump>,   // hand-placed rises: centre, radius, height
}
```

- Built from the crate's calibrated fBm, so heights are in metres.
- **No `sea_level` field** (decided 2026-11-23): E is in metres above sea
  level, so its zero is the sea level. The World owns the sea level as a
  world-wide setting (ARCHITECTURE.md §5, `WorldContext`).
- **`tilt`** (added 2026-11-23, not in the first plan): a constant slope
  added everywhere. Heights grow without limit with distance along it, so
  it suits test worlds and bounded areas; the plane-shift check and the
  drainage measurement on a steady slope use it.
- The slope bound is enforced by construction. Each layer's slope is at
  most half its height times its fBm's slope bound over its wavelength;
  the fBm's bound comes from the noise's measured steepest gradient
  (`NOISE_SLOPE_MAX`, 2.1 per lattice unit), summed over the octaves. That
  is several times stricter than amplitude ÷ wavelength: a 2-octave layer
  can be about 16 times as steep as height ÷ wavelength. The tilt and the
  bumps count too; overlapping bumps add. `validate()` rejects settings
  that could exceed `max_slope`.
- Bumps (see below) add `height · (1 − t²)³`, with `t` the distance over
  the radius: smooth, and zero beyond the radius.
- E is evaluated per sample (cheap: a few octaves at long wavelengths) and
  cached per chunk where useful.

## Region graph

```rust
pub struct RegionKey(pub I64Vec2);        // lattice cell of the region's site
pub struct RegionGraphOptions {
    pub size: f32,                         // lattice spacing, metres (2–8 km)
    pub jitter: f32,                       // 0..1, how far sites move in their cell
    pub border_warp: f32,                  // amplitude of border wander, metres
    pub border_wavelength: f32,            // wavelength of border wander, metres
}
```

- **Sites:** one per lattice cell, jittered by a hash of the cell. The
  lattice is **staggered** (alternate rows offset by half a cell, a
  hexagonal arrangement). On a square lattice, near-square groups of four
  sites make many near-zero Voronoi edges.
- **Short edges:** crossings need edges of at least about 400 m (195 m
  clear of each end), and very short edges give fragile corners. Edges
  shorter than a minimum (an option, default 2 × `CORNER_CLEARANCE`) are
  collapsed: their two corners merge into one corner touched by four or
  more regions, which `negotiate_corner` already accepts. Jitter is capped
  so collapsing never merges more than a handful of corners.
- **Polygon of a region:** the Voronoi cell of its site, computed from the
  sites within a window of lattice cells. The window size is derived from
  the maximum jitter (with full jitter a Voronoi neighbour can be up to
  about 2√2 cells away), and a brute-force test against a much larger
  window confirms it.
- **Region size:** at least several times (margin + taper) for the
  terrain the source gives that region, so seams, tapers and crossings
  have room. WorldTerrain can't see every terrain the source might
  return, so it checks each region when it builds it; the World can check
  its whole set of Biomes up front.
- **Edges:** each Voronoi edge between two sites becomes a `RegionEdge`.
  It is subdivided every few tens of metres and each interior point is
  moved along the edge normal by 1D noise seeded from the `EdgeId`.
  Displacement fades to zero at the corners. The warp amplitude is bounded
  so neighbouring edges never cross (checked in `validate()`).
- **Warped borders against the seam code.** Warping adds many segments
  per edge, which the seam code is sensitive to:
  - every height sample currently loops over all border segments, so a
    spatial index of border segments in `Terrain` comes first;
  - the soft border distance shrinks the margin slightly where many
    segments are about equally near;
  - crossings are refused near bends that bring another segment within
    reach.

  So the warp wavelength must be well above the 65 m feature clearance
  (default several hundred metres, with points every few tens of metres),
  and W3 ends by running the existing border, crack and feature suites on
  warped fixtures before W4 builds on them.
- **Corners:** Voronoi vertices, each touched by exactly three regions
  (generically). Incidence comes from the same site neighbourhood.
- **Owner lookup:** a point's region is the nearest site, adjusted by the
  border warp. It is found by testing the point against the warped
  polygons of the few nearest sites.
- **Bounded regions** (an island, a designed area) are supported by
  passing explicit polygons instead of the lattice, as `StitchedWorld`
  does today.

## Choosing biomes

Two ways E and terrain types can relate:

| | Field first (recommended) | Biomes first |
|---|---|---|
| How | E (and other low-frequency fields) are pure noise. Each region's type is chosen by rules on the field values at its site. | Regions get types first (by rules or by hand). Each type has a target base height; E is made by smoothing those targets over a few kilometres. |
| Purity | Fully pure per point. | Each point's E depends on the region types within the smoothing radius: a bounded ring, still order-free. |
| Control | Through the rules table and overrides. | Direct: you say where mountains go. |
| Fit | The type always suits its base height. | Base height follows the layout, slope-bounded. |

**Decided (2026-11-23): field first, and the choice belongs to the
World.** The rules below are the World crate's, not this crate's; they
are kept here because they shape what E must offer.

- The World's Biome rules map field values to Biome presets, for example:
  - high E with high ruggedness → an alpine Biome on mountains;
  - high E with low ruggedness → a steppe Biome on the plains preset;
  - mid E → a Biome on rolling hills;
  - E near zero → a coast Biome;
  - below zero → an ocean-floor Biome.

  Each Biome names its `TerrainOptions` preset, and the World hands
  WorldTerrain a terrain source that returns it per region. Until
  Ecotopes exist, every rule names an **empty Biome**: a Terrain and no
  Ecotopes. WorldTerrain gives every region the world seed, so all regions
  sample the same shared noise layers (terrain Phase 2.5) and differ only
  in their layer weights. A rule does not vary the seed; per-region
  variation, if wanted, is an optional variation layer that fades out at
  the region's edges (terrain open question 9).
- **Ruggedness and moisture** (proposed; not yet agreed) are the World's
  selector fields. They decide *which* Biome sits at a given height, so
  not every highland is mountains. They are selectors, not heights or
  relief scales, so they live in the World, not here.
- **Overrides split in two.** A **height bump** `{ area, target base
  height }` belongs to E in this crate: it adds a smooth bump with a
  bounded radius, so a hand-placed mountain range actually stands high. A
  **pin** `{ region, Biome }` belongs to the World. A hand-placed range is
  usually both.

## Interface to the terrain crate

```rust
pub struct WorldTerrainOptions {
    pub seed: u64,
    pub elevation: ElevationOptions,       // includes the height bumps
    pub regions: RegionGraphOptions,
    /// Where neighbouring regions' relief meets at their shared edge, as
    /// a power-mean exponent: −∞ = only the rougher side fades, −1 = each
    /// side changes in proportion to its roughness (default), 1 = halfway.
    pub relief_meeting: f32,
}

/// Gives each region its terrain. The World passes one that reads each
/// region's Biome; tests and examples pass a closure.
pub trait TerrainSource: Send + Sync {
    fn terrain_options(&self, region: RegionKey, elevation: &Elevation) -> TerrainOptions;
}

impl WorldTerrain {
    pub fn new(
        options: WorldTerrainOptions,
        source: Arc<dyn TerrainSource>,
    ) -> Result<Self, WorldError>;
    pub fn elevation(&self, p: DVec2) -> f32;                 // E
    pub fn region_at(&self, p: DVec2) -> RegionKey;
    pub fn region(&self, key: RegionKey) -> Region;            // polygon, edges, corners
    pub fn terrain(&self, key: RegionKey) -> Arc<Terrain>;     // cached, built on demand
    pub fn height(&self, p: DVec2) -> f32;                     // E + the owner's relief
    pub fn water_level(&self, p: DVec2) -> Option<f32>;
}
impl HeightField for WorldTerrain { … }
```

- **Terrain changes (built in W1):** `TerrainGenerator::with_elevation`
  gives a terrain its `Elevation`, and every height becomes E plus the
  relief; it may come before or after `with_edge_targets` with the same
  result. The fade (scaling and blending) works on the relief alone.
  Offers carry absolute heights, E included; `StitchedWorld::build`
  takes the `Elevation` (`Elevation::flat()` for land at sea level).
  `TerrainOptions::base.elevation` is documented as an offset above E.
  - **E through the border, exactly** (built after W1, 2026-11-23):
    `negotiate` takes the `&Elevation`. It takes E's height and
    cross-slope away from both offers' samples, smooths and mixes what is
    left, bends it towards each corner's agreement less E there, and adds
    E back. Offers, agreements and crossings keep heights above sea
    level: river and road crossings are checked against the absolute
    profile as before. On flat land nothing changes. A terrain's border
    targets are heights above E, with E at the point added after mixing,
    and its gradient is E's analytic gradient plus the relief's by
    central differences (more precise than differencing E plus relief in
    f32). Before this, E went through the smoothing: up to 29 cm off at
    the test border's sharp bends on a 0.02 tilt, and up to 0.71 m on
    bare ground on curved land.
- **Thread safety:** `terrain(key)` builds under a per-key once-lock, so
  several threads asking for the same region build it once. The cache is
  bounded (LRU); evicted regions rebuild identically.
- **Cost:** about 0.3 s per 10 km region today (measured for two 10 km
  regions together: 556 ms). Region builds run on worker threads ahead of
  the player; chunks never wait on regions beyond their own and their
  neighbours' offers.

## Tests

- **Purity:** the same region, edge, corner and height whatever order
  regions are requested in, and on several threads.
- **Graph consistency:** for many random regions, every shared edge is
  identical from both sides (points and id), every corner lists the same
  three regions from each, and the polygons tile the plane (no gaps or
  overlaps, by sampling).
- **Border warp:** neighbouring edges never cross, and warped polygons
  stay simple.
- **E:** the slope bound holds (sampled); sea level is E's zero;
  height bumps raise E within their radius only.
- **Seams with E:** the existing border tests (height and slope
  continuity, triple junctions, cracks) on lattice-generated worlds.
- **Drainage:** on a world with a steady E slope, own rivers flow down it
  and cross borders far more often than in the fixed test worlds (measure
  before and after; the current rate is 6 seeds in 300).
- **Terrain source:** each region builds from the options its source
  returns, and the same source gives the same world. (Biome rules and pins
  are tested in the World crate.)

## Phases

| Phase | Work | Depends on |
|---|---|---|
| W1 | E: options, calibration, slope bound (tilt and bumps included), no sea-level field (E's zero is sea level); `Terrain` adds E; the plane-shift check on negotiation and a seam test on curved E (a bump across the border, noise on); existing tests pass on E = 0 and on a sloped E. **Deferred to W4:** making the preset offsets equal (today 20 / 50 / 120 m), as a separate commit with the border figures re-recorded, after the user's terrain review. **Exit test:** measure the river crossing rate on a sloped E (built: 65 in 300 tilted towards the plains, against 24 flat; see "W1 as built") | terrain 2.5 |
| W2 | Region graph: staggered jittered lattice, Voronoi polygons, short-edge collapse, corner incidence, owner lookup; purity, tiling and window-size tests | — (in parallel with W1) |
| W3 | Border segment index in `Terrain`; border warp (subdivided, noise-displaced edges); no-crossing check. **Exit test:** the existing border, crack and feature suites pass on warped fixtures | W2 |
| W3b | Cheaper border pulls: an exact skip for points beyond the fades and a faster integral; shared nodes measured and not built (see "W3b as built") | W3 |
| W4 ✅ | `WorldTerrain`: `TerrainSource`, region cache, on-demand builds, `height`, `water_level`, `HeightField`; seam tests on lattice worlds with a closure source (built; see "W4 as built"). The deferred W1 item, equal preset offsets, was not done: it waits for the user (see open question 5) | W1, W3b |
| W5 | Moved to the World crate: selector fields, Biome rules and pins, with empty Biomes (milestones B0 and W0 in ROADMAP.md) | — |
| W6 | Image example of a large world; drainage measurement. Built on W4 with a closure source; redone through the World once W0 exists | W4 |

### W1 as built (2026-11-23)

- **New:** `Elevation`, `ElevationOptions`, `HeightBump`, `ElevationError`
  (`src/elevation.rs`); `TerrainGenerator::with_elevation`;
  `StitchedWorld::build` takes `&Elevation`. Noise: `fbm_gradient`,
  `fbm_slope_bound` and the measured `NOISE_SLOPE_MAX` (the ignored test
  `measure_steepest_noise` tries every choice of corner gradients).
- **Defaults:** continents 400 m over 150 km and uplands 120 m over
  40 km, both 2 octaves at roughness 0.4: a worst-case slope of 0.047
  against `max_slope` 0.05. (The first guess, 600 m / 120 km and 200 m /
  30 km, could reach 0.096 under the real bound.)
- **Flat land is unchanged:** with `Elevation::flat()` every scene dump is
  byte-identical to before.
- **Tests:** slope bound sampled with layers, tilt and overlapping bumps
  (and exact for bumps alone); bumps change E only within their radius;
  the plane-shift check (exact on straight stretches when W1 landed;
  exact everywhere since the fix below); call order; every height is
  E plus the relief. The seam and feature suites also run on a tilted
  world (0.02 down to the west) and a curved one (default layers plus a
  40 m bump across the border), including the triple junction, which
  sits inside the bump. The far-from-origin seam test runs on flat and
  curved land (there only its bounded noise layers); not on the tilt,
  which 10 000 km out stands 250 km high, beyond f32.
- **River sources ranked by relief, not by E** (fixed in W1): sources
  were the region's highest cells by absolute height, so on a tilt E put
  every source at the region's high end, far from a border downhill.
  Sources are now ranked by height above E; rivers still trace over E
  plus the relief, so they run down E. On flat land nothing changes.
  Test: `river_sources_stay_spread_on_a_tilt` (2 near-border sources on
  the tilt against 14 flat with the old rule).
- **Exit measurement** (`river_crossings_on_a_slope`, hills into plains,
  default settings, seeds 0–299): flat **24** of 300 give a river
  crossing; tilted 0.02 down towards the plains **65**; tilted up towards
  them **6**. Before the source fix the same probe gave 24, 8 and 12. The
  drainage tilt works once sources ignore E. Re-recorded after the
  string-key seed salts and the erosion-aware relief amplitude (see
  "Re-recorded after the string-key seed salts" in `plan.md`): flat
  24 → **20**, down towards the plains 65 → **64**, up towards them
  6 → **4**.
- **The flat baseline:** 24 is not comparable with Phase 2.5's 10–13.
  Those came from the `river_crossing_seeds` probe before relief
  blending existed (blend 0, today's default is 0.5), and Phases 2.6 to
  2.8 changed rivers since. Pre-W1 code (commit 68778ff), counted the
  same way as today, also gives 24 of 300 on flat land: W1 doesn't change
  rivers on flat land, and the rise came before it.
- **Bare ground against E** (`measure_bare_ground_against_e`, two regions
  with no relief, rivers or roads on the curved test land): beyond 300 m
  from the shared edge the ground is exactly E. On the edge the agreement
  differs from E by up to 0.16 m, the profile smoothing (a hand Gaussian
  of E along the edge gives the same 0.16 m, differing from the agreement
  by up to 0.12 m, from end handling). The negotiation's border detail
  adds up to 0.50 m, whatever the relief. Through the border bending the
  two reach 0.71 m within 100 m of the edge, 0.07 m at 100–200 m and
  0.005 m at 200–300 m. With the plains and mountains on the same land the
  agreement differs from E plus the flat-land agreement by up to 0.49 m:
  the smoothing alone gives 0.16 m, so the rest comes from steps that
  mix E with the relief, not yet found. These figures are from W1 as
  committed (eabd6b1).
- **Fixed after W1** (see "E through the border, exactly" above): the
  probe became the test `bare_ground_is_e_everywhere`, on two regions
  and on three meeting at a corner, with border irregularities off. Bare
  ground is now E to within 2·10⁻⁸ m everywhere (0.71 m before); the
  plane-shift check holds to 1 mm and 10⁻⁴ in slope at the bends too.
  Both tests fail with the fix switched off. On the curved land with the
  plains and mountains on it, the agreement now differs from the
  flat-land agreement plus E by at most 1.9·10⁻⁵ m (f32 rounding), down
  from 0.49 m.
- **Not done:** `ElevationOptions` has no serde support, unlike
  `TerrainOptions`.

### W2 as built (2026-11-23)

- **Public types** (`src/graph.rs`): `RegionKey(IVec2)`, the lattice
  cell of a region's site, with `id()` and `from_id()` to and from the
  region's `RegionId` (bijective); `RegionGraphOptions { size, jitter,
  min_edge }` (defaults 4 km, 0.6, 390 m) with `validate()`;
  `RegionGraphError`; `MAX_JITTER` (0.8) and `MIN_EDGE` (390 m,
  2 × `CORNER_CLEARANCE`, a hard floor: shorter edges can't carry a
  crossing); `RegionGraph` with `new`, `options`, `site`, `region` and
  `region_at`. W4's public surface keys on `RegionId`, not `RegionKey`,
  since designer polygons have no lattice. Deviations from the sketch
  above: `IVec2` (packs exactly into a `RegionId`), f64 options, no border
  warp yet (W3), and a `min_edge` field.
- **How it works:** a staggered hexagonal lattice (jitter 0 is an exact
  triangular lattice: no four sites on one circle). Sites are snapped to
  whole millimetres (i64), so they can be compared exactly; `site()` and
  `region_at` use the snapped sites. Sizes are capped at `MAX_SIZE`
  (50 km), so the exact tests' i128 terms can't overflow (below about
  10³⁷ within the widest search). Sites use their own seed salts (201,
  202; 1 and 2 had clashed with the relief's swell and hill layers).
- **Raw corners, exactly** (redesigned 2026-11-23, replacing clipping): a
  raw corner is an empty circle through three or more sites, held as the
  sorted set of every site on it, so 4, 5 or 6 exactly cocircular sites
  make one corner with no tie rule. A region's ring is walked round its
  site from the nearest site: at each neighbour, the next corner is the
  circle with the site to its left that leaves every other left-hand site
  outside (exact in-circle in i128), every site on that circle joins the
  corner, and the next neighbour is the corner's most anticlockwise site,
  by exact orientation, never by sorting f64 angles. The walk ends back
  at its first corner. Collinear sites are skipped by exact orientation.
  Rings are a pure function of the sites, so every region sees the same
  corners by construction. A corner's circle provably lies within about
  2.3 lattice spacings of the site at full jitter. The first search, 4
  cells either way, holds every site within about 3.5 spacings; a circle
  reaching past that widens the search (up to 8 cells), and is never
  treated as empty.
- **Merging:** raw corners joined by chains of edges shorter than the
  minimum merge, as connected groups found from the sites' rings, at the
  mean of their positions (each the circumcentre of its first three sites
  in sorted order, summed in sorted order), so every region computes
  identical bits. `region_at` tests the nearby regions nearest first
  until one holds the point (merging can put a point in a region that
  isn't the nearest site's; the old version tried only four).
- **Tests:** neighbours agree on every edge and corner (exact equality);
  no edge shorter than the minimum; regions tile the plane and
  `region_at` finds the owner;
  polygons stay simple and anticlockwise after merging; the same regions
  on four threads in any order; settings checked (NaN, infinity, the
  floor); regular hexagons with no jitter. The graph tests run at jitter
  0, 0.6 and 0.8. A four-way seam test (`four_regions_meet_at_one_corner`)
  checks four terrains meeting at one point, on flat, tilted and curved
  land. Each test was checked by breaking the code it covers.
- **Exact-corner tests:** the walk's corners match a brute-force search
  (every triple of a 9 × 9 patch against every site, same predicates) at
  jitter 0, 0.6 and 0.8; 5 and 6 exactly cocircular sites (hand-placed at
  whole millimetres on a 1 km circle) make exactly one corner, with no
  corner holding three or more of them otherwise, and the regions agree,
  stay simple and tile; the same with one or two of those sites moved a
  millimetre in or out, with and without merging. Checked by breaking
  them: without the on-circle sites, both circle tests fail; with a
  1-cell search and no reach check, the brute-force test and most others
  fail; with a 1-cell search and the reach check, widening makes every
  test pass. Hand-placed sites widen the search by how far they moved, as
  real jitter does. The regular-hexagon test allows 1 mm, the snapping.

### W3 so far: border pull along whole edges (2026-11-23)

- **What changed** (`src/border.rs`): a border pulls on nearby ground by
  an integral along each straight piece, in closed form, instead of once
  per piece. Cutting an edge into more pieces changes heights by under
  1e-9 m, which the warp needs. Pieces are found through a grid index with
  cells one reach wide, so a height reads only nearby pieces.
- **Soft distance:** distance power 16 (chosen from a sweep of 8–24). On
  the bent test border it is up to 5.8% long outside convex corners and
  3.9% short inside concave ones (4.2% short in a region-graph cell); the
  old per-piece soft minimum was about 8% short near any bend. The crease
  along a concave bend's bisector stays smooth (slope change 0.51 across
  ±10 m at 100 m out). A correction for the convex overshoot is a later
  option, once warped borders exist.
- **Cost:** 2.0–2.4× the old time per height, whatever the power.
- **Height changes** (uncarved, against before): 121 m in the two-terrain
  world, 697 m outside a convex bend, where the relief fade reaches; 31 m
  at the triple junction; 2.6 m in hills into plains. Away from corners
  and bends, at most 2.7 m.
- **Seeds:** the neighbouring-rivers test moved from seed 30 to 217;
  `RIVER_SEEDS` are unchanged.
- **Still to do:** done; see "W3 as built: border warp".

### W3 as built: border warp (2026-11-23)

- **What it does** (`src/warp.rs`): each straight polygon side between
  two corners becomes a wiggly polyline with a point every eighth of a
  wavelength. The offset is amplitude × noise, faded to zero over half a
  wavelength at each corner, so corners stay put. The noise seed comes
  from the world seed and the two corner positions, taken in a fixed
  order, so both regions on a side get identical bits.
- **Bounds per side:** the amplitude is cut down where a side meets
  another at a sharp corner (half the room the corner's angle leaves over
  the fade) or runs close to another side (half the gap above the minimum
  edge length). Neighbouring regions compute the same bound. On the
  region-graph tests 21 to 39 edges in about 360 are held under half
  amplitude, and none to zero.
- **Settings:** `BorderWarp { amplitude, wavelength }`, default
  100 m / 800 m. Checked: wavelength at least 260 m (four feature
  clearances) and a border slope of at most 1, which allows up to about
  134 m at 800 m. `RegionGraphOptions::warp` warps lattice graphs;
  `warp_borders` warps a designer's polygons.
- **Cost:** per height, warped against straight (machine loaded, so
  rough): 6.1× for two terrains (74 pieces against 14), 4.1× at the triple
  junction (147 against 19). The features suite with warped worlds takes
  over 5 minutes; accepted until the performance sweep.
- **Tests:** warped polylines are identical on both sides and stay within
  their amplitude and slope; sides at a sharp corner don't cross; the two
  sides of a thin strip stay apart; held-back edges are counted (the last
  three checked by breaking the bounds). The seam, crack and feature
  suites run on four warped worlds (flat, tilted, curved and unrelated
  seeds, 130 m / 800 m). The warped seam test measures slope changes over
  0.5 m steps, because f32 height noise (about 0.5 mm at 109 m) swamps
  shorter steps; the differences are second order, since first-order ones
  read curvature as a kink on steep ground (0.079 on mountains at slope 2,
  where the two sides agree exactly). A control seam with a slope kink
  1.1× the 0.05 limit fails at that step.
- **Follow-up:** keep the height path in f64 until the final cast, so the
  seam test can go back to short steps.

### W3b as built: cheaper border pulls (2026-11-23)

The plan was to share target nodes along a whole edge. Measuring first
showed that would save little, so W3b changed course (the user chose
the faster integral).

- **Where warped cost goes** (two-terrain world, 30,000 heights on a
  20 m grid, at the start of W3b): per height, 39 distance integrals
  against 5 straight, 27 pieces read for targets against 3.4, 44 target
  intervals against 20, and 77 target reads against 27. The closed-form
  integral is about 35% of warped time, split evenly between the
  distance and target passes; target reads, the only part shared nodes
  would cut, are about 10%.
- **Exact skip** (`border::surely_beyond`): before adding pulls, a bound
  from each piece's plain distance and length proves that a point is
  beyond the fade or the bending. Before it, 60% of fade passes and 80%
  of bending passes worked out the whole soft distance only to find that;
  it now skips almost all of them (1.18 of the 1.19 fade passes per
  height that ended beyond the width). Heights don't change.
- **Faster integral:** the far-from-foot series is worked out per power
  at compile time, and its value at twice the distance across, which
  most stretches start from and which needs the most terms, is a
  constant times a power of the distance. A near-and-far integral went
  from 79 to 33 ns.
- **Result:** every f32 height is bit-identical to before (hashed over
  carved and uncarved grids of six worlds, warped and straight). Heights
  are about 10–17% faster for both straight and warped borders, measured
  under heavy machine load. Warped is still about 2.7× straight.
- **Left for later:** the gap comes from the number of pieces, which only
  height-moving changes can cut: points every quarter wavelength instead
  of every eighth, cheaper approximate integrals for far pieces, or
  shared target nodes (at most about 10%, and the per-edge anchor must
  stay continuous at bends). These wait for the performance sweep unless
  the user decides otherwise.

### W4 as built (2026-11-24)

- **Public types** (`src/world.rs`): `WorldTerrain`, `WorldTerrainOptions
  { seed, elevation, regions, convergence, cache }`, `TerrainSource`
  (any `Fn(RegionId, DVec2, &Elevation) -> TerrainOptions + Send + Sync`
  closure is one), `WorldError` and `DEFAULT_CACHE` (64). `WorldTerrain`
  has `new`, `options`, `elevation`, `graph`, `site`, `region_at`,
  `region`, `terrain`, `terrain_at`, `height` and `water_level`, and
  implements `HeightField` and `procgena_core::Ground` (the owner's
  height, with Ground's default slope by central differences). Once G2's
  analytic `Terrain::sample` and `gradient` are merged, `WorldTerrain`
  should give inherent `sample` and `gradient` from the owning terrain and
  use them for both traits.
- **Deviations from the sketch above:** the surface keys on `RegionId`
  (see W2); the seed is `u32`, as for the graph and E; `convergence`
  (meeting and blend) replaces `relief_meeting`; the source is given the
  region's id, its site and E, not a `RegionKey`; `region` returns
  `Arc<Region>`; `terrain` returns `Result<Arc<Terrain>, WorldError>`, and
  `height` is NaN (and `water_level` `None`) where the region can't be
  built. The source's `default_seed` is replaced by the world seed.
- **Stages, each cached per region** (bounded, least recently used;
  dropped entries rebuild identically): the region from the graph; the
  source's settings (a generator standing on E, and its relief amplitude);
  the **plan** (the generator converged towards every region on its edges
  and corners, and its edge offers, rivers traced); the built `Terrain`.
  Building a terrain reads the plans of the region and every region
  touching its corners, and those read the settings one ring further out,
  so no neighbour is ever fully built. Corners and edges are negotiated
  by each region for itself from the cached plans; both sides get
  identical bits, since negotiation doesn't depend on which side asks.
  Each cache holds a per-key once-lock, so several threads asking for one
  stage build it once; the map lock is never held while building. The
  plan stage is where the staged feature plans (features_plan_review,
  step 2) will split: relief and offers first, then rivers, roads and
  local features.
- **Owner lookup:** the 25 nearest lattice cells, nearest site first,
  tested against their cached warped outlines; a point no outline holds
  (right on a border) goes to the region whose border is nearest. It
  depends only on the point, so tiles stay bit-identical.
  `RegionGraph::region_at` would rebuild the neighbourhood on every call.
- **Tests** (`tests/world.rs`, default 4 km graph with the default warp,
  presets picked by a hash of the region id): every shared edge round the
  centre region joins in height (1 mm) and slope (0.05, long step) on
  flat, tilted and curved land; every corner meets at one height with no
  step on a 0.5 m ring, and so does a merged corner of four or more
  regions, two of which share no edge (so a build reads a plan that is
  not an edge neighbour's); the world's water level is the owner's along
  its rivers; no cracks along three 6 km lines; a 121 × 121
  tile is bit-identical cold and with the regions built first in reverse
  order on four threads; a dropped terrain rebuilds bit-identically; each
  region's options are its source's; the owner agrees with the graph's
  `region_at`; and the centre terrain is **bit-identical to a
  `StitchedWorld` of the same polygons two rings out**. Bad settings are
  refused, and a region with invalid source settings (and its neighbour)
  gets the error, NaN heights and no water. Checked by breaking: owner by
  nearest site alone fails the owner test; negotiating against the
  neighbour's unconverged offers fails the border test (a 2 m step) and
  the stitched comparison. The file takes about 10 minutes in the dev
  profile.
- **Not done:** the region size check ("Region size" above): no
  criterion exists yet, so too-small regions are not refused. Edge
  agreements are worked out once by each side, not cached; the caches take
  a mutex on every lookup, so heavy multi-threaded sampling may contend;
  the cost per region was not measured separately. `ElevationOptions` and
  `WorldTerrainOptions` have no serde support. The converge and negotiate
  steps repeat `StitchedWorld`'s in `world.rs`; `StitchedWorld` could be
  rebuilt on the same stage functions.

## Open questions

1. ~~**Biome choice:** field first with overrides, or biomes first?~~
   Decided 2026-11-23: field first, and the World makes the choice (see
   "Choosing biomes").
2. **Region size and shape:** a jittered staggered (hexagonal) lattice gives fairly even
   regions. Is that wanted, or should region sizes vary more (a
   multi-scale lattice: large regions with occasional small ones)?
   This moved to decision gate D4. Region size also sets how long a
   region's feature plan takes. Planning stays per region, with macro
   cells only if step 2's benchmarks call for them (decided 2026-11-23;
   see [features_plan_review.md](features_plan_review.md), "Decisions").
3. **Moisture:** include it now as a Biome selector (deserts, wetlands)
   or later? It is now the World crate's question.
4. **Bounded worlds:** besides infinite lattice worlds, should
   `WorldTerrain` take a designer's polygon map directly (as
   `StitchedWorld` does), or keep that in `StitchedWorld`?
5. **Equal preset offsets** (deferred from W1, left for the user in W4):
   which common `base.elevation` should the presets share? Today plains,
   rolling hills and mountains sit 20, 50 and 120 m above E. **20 m**
   leaves plains unchanged and lowers hills by 30 m and mountains by
   100 m; **50 m** raises plains by 30 m and lowers mountains by 70 m;
   **0** puts plains' mean at E (sea level on flat land) and lowers every
   preset. Each changes how the hills and mountains worlds look, and moves
   the border gap probe and the river crossing counts, which would be
   re-recorded. Recommended: 20 m, the smallest visible change.
