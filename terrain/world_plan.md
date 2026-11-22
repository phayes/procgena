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
  - Offers can keep carrying absolute heights. The mix, the smoothing and
    the corner mean are all linear, so the agreement comes out as E plus
    the agreed relief. W1 checks this by adding the same plane to both
    sides of a test border and confirming the agreement shifts by exactly
    that plane.
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
- **Biome choice: the one decision to make** (see "Choosing biomes").
  Recommended: **field first, with authored overrides**. Low-frequency
  fields (E, plus moisture and ruggedness) pick each region's terrain type
  through a rules table. A user can pin a region's type, which raises or
  lowers E locally to suit it.
- **Caches, all rebuildable:**
  - region plans (`Terrain`), built lazily from the region's own options
    and its neighbours' offers (neighbours are never fully built);
  - optional per-region feature plans;
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
    pub max_slope: f32,           // bound on |∇E|, about 0.05
    pub sea_level: f32,           // the datum; heights are reported above it
}
```

- Built from the crate's calibrated fBm, so heights are in metres.
- The slope bound is enforced by construction: amplitude ÷ wavelength
  limits the slope of each layer. `validate()` rejects settings that
  could exceed `max_slope`.
- Overrides (see below) add smooth bumps with a bounded radius.
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
  largest margin in the biome table, so seams, tapers and crossings have
  room. `validate()` checks it.
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

**Recommended: field first with authored overrides.**

- A `BiomeTable` maps field values to terrain types, for example:
  - high E with high ruggedness → mountains;
  - high E with low ruggedness → high steppe (plains preset);
  - mid E → rolling hills;
  - E near zero → coast;
  - below zero → ocean floor.

  Each rule names a `TerrainOptions` preset. WorldTerrain gives every
  region the world seed, so all regions sample the same shared noise
  layers (terrain Phase 2.5) and differ only in their layer weights. A
  rule does not vary the seed; per-region variation, if wanted, is an
  optional variation layer that fades out at the region's edges (terrain
  open question 9).
- **Ruggedness** (proposed here; not yet agreed) is a second
  low-frequency field. It decides *which* kind of terrain sits at a given
  height, so not every highland is mountains. It is a selector, not a
  height or a relief scale.
- **Overrides:** a list of `{ region or area, terrain type, target base
  height }`. Each adds a smooth bump to E with a bounded radius, so a
  hand-placed mountain range actually stands high, and pins the region's
  type.

## Interface to the terrain crate

```rust
pub struct WorldOptions {
    pub seed: u64,
    pub elevation: ElevationOptions,
    pub regions: RegionGraphOptions,
    pub biomes: BiomeTable,
    pub overrides: Vec<Override>,
    /// Where neighbouring regions' relief meets at their shared edge, as
    /// a power-mean exponent: −∞ = only the rougher side fades, −1 = each
    /// side changes in proportion to its roughness (default), 1 = halfway.
    pub relief_meeting: f32,
}

impl WorldTerrain {
    pub fn new(options: WorldOptions) -> Result<Self, WorldError>;
    pub fn elevation(&self, p: DVec2) -> f32;                 // E
    pub fn region_at(&self, p: DVec2) -> RegionKey;
    pub fn region(&self, key: RegionKey) -> Region;            // polygon, edges, corners
    pub fn terrain(&self, key: RegionKey) -> Arc<Terrain>;     // cached, built on demand
    pub fn height(&self, p: DVec2) -> f32;                     // E + the owner's relief
    pub fn water_level(&self, p: DVec2) -> Option<f32>;
}
impl HeightField for WorldTerrain { … }
```

- **Terrain changes:** a `Terrain` gets E through a small trait object (or
  closure) at build time and adds it to its relief everywhere. Offers are
  made on relief only. `TerrainOptions::base.elevation` becomes an offset
  above E.
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
  overrides raise E within their radius only.
- **Seams with E:** the existing border tests (height and slope
  continuity, triple junctions, cracks) on lattice-generated worlds.
- **Drainage:** on a world with a steady E slope, own rivers flow down it
  and cross borders far more often than in the fixed test worlds (measure
  before and after; the current rate is 6 seeds in 300).
- **Biome rules:** types follow the table; overrides pin types.

## Phases

| Phase | Work | Depends on |
|---|---|---|
| W1 | E: options, calibration, slope bound, sea level; `Terrain` adds E; preset offsets made equal; the plane-shift check on negotiation; existing tests pass on E = 0 and on a sloped E. **Exit test:** measure the river crossing rate on a sloped E against today's 6 in 300 | terrain 2.5 |
| W2 | Region graph: staggered jittered lattice, Voronoi polygons, short-edge collapse, corner incidence, owner lookup; purity, tiling and window-size tests | — (in parallel with W1) |
| W3 | Border segment index in `Terrain`; border warp (subdivided, noise-displaced edges); no-crossing check. **Exit test:** the existing border, crack and feature suites pass on warped fixtures | W2 |
| W4 | `WorldTerrain`: region cache, on-demand builds, `height`, `water_level`, `HeightField`; seam tests on lattice worlds | W1, W3 |
| W5 | Biomes: ruggedness and moisture fields, `BiomeTable`, overrides | W4 |
| W6 | Image example of a large world; drainage measurement | W5 |

## Open questions

1. **Biome choice:** field first with overrides (recommended), or biomes
   first?
2. **Region size and shape:** a jittered square lattice gives fairly even
   regions. Is that wanted, or should region sizes vary more (a
   multi-scale lattice: large regions with occasional small ones)?
3. **Moisture:** include it now as a biome selector (deserts, wetlands)
   or later?
4. **Bounded worlds:** besides infinite lattice worlds, should
   `WorldTerrain` take a designer's polygon map directly (as
   `StitchedWorld` does), or keep that in `StitchedWorld`?
