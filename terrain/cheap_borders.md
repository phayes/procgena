# Cheap region borders (TB): design

> **Ground paint removed 2026-11-26** (the user): mentions of paint below
> are history; the terrain draws flat grey.

Decided by the user 2026-11-26: "the cheap borders" and "no blending at all
between same-preset neighbours. Lets start there." Options 6 and 7 of
[speedup_options.md](speedup_options.md). Status lives in
[ROADMAP.md](../ROADMAP.md#tb-cheap-borders). Paths are relative to
`terrain/crates/procgena_terrain/src/`.

## Bottom line

- **One border field per region replaces three pull systems.** Today a
  height read near a border runs up to three integrated pulls: relief
  scaling and the blend into the neighbours' ground (`converge.rs`
  `Fade`), and the seam bend (`terrain.rs` `Terrain::base`). Each
  integrates `r^-(p+1)` along every edge segment within reach (`border.rs`),
  and a gradient repeats that at four offsets. They are replaced by one
  cheap per-point pass over nearby edge **segments**: a plain projection
  per segment, a clipped `(d/ρ)^-16` weight, and a lookup into a 1-D
  profile. No integrals, no `atan2`, no series, no differences. Gradients
  are analytic.
- **Height on a border is one shared 1-D profile `P(s)`.** Each edge
  between non-equivalent regions gets `P(s)`: the stiffness-weighted mean
  of both sides' amplitude-scaled relief, sampled along the edge every
  32 m and halved where linear interpolation misses by more than 0.1 m
  (down to 1 m). Its ends are pinned to the shared corner heights. Both
  regions build it from the same inputs in sorted region order, so they
  hold the same knots bit for bit. On the edge a read returns exactly
  `P(s)`; inside a strip `W` wide it blends `H = B + g(D/W)·(R̃ − B)`,
  `g` = smootherstep, where `B` is the weighted mix of the nearby
  segments' `P` and `D` the soft distance to them. Cross-slopes are
  neither offered nor agreed: the slope may crease on the border.
- **Same-relief borders are no-ops.** Two regions have **equivalent relief**
  when their `(default_seed, base)` are equal: then `Relief` is the same
  function on both sides, bit for bit, and E is the world's. Their shared
  edge gets no profile and no strip. A region whose every edge and corner
  is equivalent reads its own relief directly: no fade, no field. An
  equivalent edge that ends at a non-equivalent corner, or lies within
  reach of a non-equivalent edge of either side, becomes a **relative
  edge**: its target is the shared relief itself, scaled by the edge's
  shared layer scales `σ(s)`, plus a corner offset `c(s)` that is 0 away
  from the corners: `T = R_σ(s)(p) + E(p) + c(s)`. Both sides compute the
  same `R_σ(p)`, so they agree exactly on it, and the offset gives the
  corner its shared height. A region is **plain** when its field has no
  active edge at all.
- **Every border input is a function of settings alone, so the plan
  builds the whole field.** On an edge each side's scales are exactly that
  edge's `σ(s)`, so `P` and the corner heights come from the settings of
  the regions on the edge and at its corners and from the edge targets,
  never from a neighbour's plan or built terrain. `with_edge_targets`
  builds the field, the corner heights and every profile when the plan is
  made; plans read neighbours' settings, never their plans. Order
  independence follows from summing in region order (tested in
  `world.rs`, `neighbouring_plans_agree_on_their_border_in_either_order`).
  The plan's river trace, wishes and every height read then see the final
  ground; only crossing offers still read the neighbour's plan.
- **Amplitude control stays, cheaply.** Convergence's job beyond the
  border is to scale each region's relief layers towards an agreed
  amplitude over a fade `F` (up to about 450 m for mountains beside
  plains), so mountains don't meet plains in a cliff. That stays, with the
  same targets (`relief_target`, corner ramps, `level_for`, `scales_at`),
  but its per-point mix uses the same segment weights instead of pulls.
  The **blend into the neighbours' ground** (`Convergence::blend`, the
  neighbour relief evaluated per point) goes: `P` now gives both sides
  the same ground on the border.
- **Roads and rivers keep working.** Offers, crossings and pieces stay.
  Offers describe each side's own ground before it meets the border
  (`Relief::own_height`); rivers and their wishes' `ground` are traced on
  the final ground. `negotiate` keeps the ports and fills the 8 m agreed
  samples from `P` (cross-slope from the offers, sampled only when a
  crossing is wished). `SideWishes::bent_at` models the new strip instead
  of the old bend.
- **E in f64.** The world elevation's noise placed points in its lattice
  cells as f32 fractions, so at E's kilometre wavelengths it stepped by up
  to about 2 mm every few millimetres. Once profiles carry E exactly, those
  steps showed next to borders. E now uses an f64 noise (a separate
  commit, landed first).
- **What changes:** `H*` near every non-equivalent border, and everywhere
  in fully equivalent regions (no fade). Figures that move: seams, labels,
  measures, shipped, TIN `GOLDEN_PLANE`, CT3's lines, convergence tests.
- **Removed:** the pull integrals (`border.rs`, except the segment index),
  `Fade::blended*`, the four-offset `pull_gradients`, the seam bend and
  shared border detail (`DETAIL_*`, `Targets`, `bend_*`),
  `CornerOffer::gradient` and `CornerAgreement::gradient`,
  `EdgeSample::detail`. `Convergence::blend` and `EdgeOptions::roughness`
  become unused; they are read by `procgena_world` (`options.rs`,
  `world.meta.yml`) and `terrain.meta.yml`, so removing them is a separate
  last step, coordinated with those files' owners.

## What the code does today

Per height read, with roads and rivers off (`WorldTerrain::sample_batch` →
`Terrain::query` → `Terrain::base`):

1. **Relief with the convergence fade** (`Relief::height_above_elevation`).
   - **Scaling:** `Fade::scales` mixes each edge's target layer scales,
     `scales_at(level_for(spreads, amplitude_at(along)))`, by a pull
     integrated along every segment within `2F`, read every 64 m along
     the edge, then fades them in over `F = max(margin,
     |Δamplitude|/0.25)`.
   - **Blend:** `Fade::blended` evaluates every neighbour's own relief at
     the point (1–3 extra noise stacks), mixes them by the edge's
     `blend_weights` shares and pulls, and fades over
     `max(F, |Δmean|/0.25)`. The default mixes scaling and blend half and
     half (`Convergence::blend = 0.5`).
2. **Seam bend** (`Terrain::base`). Within `margin` (125–250 m) the height
   is shifted by `(agreed − own)` mixed by pull along every segment within
   `2·margin`, each target carried across the edge by its cross-slope; then
   within 48 m it gives way to the agreed profile plus a shared detail noise.
3. **Gradients** (`SampleDetail::Gradient`, every paint site) repeat both
   pulls at four offsets (`pull_gradients`).

The profile measured these as 46% (mixed world) to 62% (mountains) of all
CPU, inside the ground sites (load_profile.md, "Where the CPU goes").

**Negotiation** (`seam.rs`): each side offers its faded height and
cross-slope every 8 m; the agreement is the stiffness-weighted mean,
Gaussian-smoothed over 48 m, bent to each corner's agreed plane over the
wider margin, plus border detail tapering to 0 at the corners.

## Design

### Equivalence (part A)

- `TerrainGenerator::same_relief(&other) -> bool`: `default_seed` and
  `base` equal (`BaseOptions: PartialEq`, f32 fields, validated so no
  NaN). Everything else in `TerrainOptions` (edges, features) does not
  change the height before carving.
- Per region, at plan time, each shared edge and corner is classified:
  - **equivalent edge**: both sides `same_relief`;
  - **equivalent corner**: every incident region `same_relief` with every
    other;
  - a region is **plain** if its field has no active edge (below): every
    edge equivalent and none of them relative.
- A plain region has no fade and no border field: `H = R + E`, its own
  relief, exactly. No offers are needed for the height (they stay for
  ports while features may be on).
- In `WorldTerrain` the graph already keeps edges between equal Biome
  presets straight (W0d.1, `EdgePolicy`). Equivalence here is on relief
  settings, not Biome keys, so it is correct for any graph, warped or not.

### Edge coordinates

- An edge's polyline is canonical (`RegionEdge::new` orders it; both
  regions hold the same points, `warp_side` works in the canonical
  direction). Its `segments()` carry `offset` (arc length at the start) as
  a running sum, so both regions compute the same segment data bit for bit.
- A point's coordinate on segment `j` is `s_j = offset_j +
  clamp(foot, 0, length_j)`; its distance `d_j` is the plain distance to
  the segment. Both are continuous; `s_j` is constant past the ends.
- Edges in `StitchedWorld` may be arbitrary polylines (L-shaped runs of
  polygon sides). Nothing here assumes an edge is a graph over its chord.

### The border field

Built per region in its plan, by `TerrainGenerator::with_edge_targets`,
from the settings of the region and the regions on its edges and corners.
Holds, for each **active** edge (every
non-equivalent edge, plus the relative edges), its segments and target, a
`SegmentIndex` with cell size about `ρ/2`, the strip width `W`, the fade
width `F`, and the corner heights.

Per point `p`:

1. **Candidates:** the index cell's `(edge, segment)` list.
2. **On a corner** (within `ON_EDGE` of a corner position): return the
   corner height `H_c`. Every region at the corner returns the same value.
3. **On an edge:** the first candidate with `d_j < ON_EDGE`, in canonical
   segment order within its edge: return the edge's target at `s_j`. Only
   one edge is that close away from corners, so both regions pick the
   same segment and the same `s_j`.
4. **Weights:** `u_j = (d_j/ρ)^-16 − 1` for `d_j < ρ`, else 0 (four
   squarings, no `powf`). `U = Σ u_j`. The clip makes a segment's weight
   reach 0 at `ρ`, so entering or leaving the candidate list is
   continuous. Soft distance `D = ρ·U^(-1/16)`, one `powf` per point; `D ≈
   d_min` (within 0.96–1.0 for a straight edge cut into pieces).
   Fast reject: if every `d_j ≥ ρ`, or `D ≥ max(W, F)`, the point is
   outside both strip and fade.
5. **Fade (amplitude control):** per layer, `S_k = Σ u_j σ_{k,j}(s_j) /
   U` with `σ` the edge's target scales, sampled along it; `scales_k = 1 +
   (1 − g(D/F))·(S_k − 1)`. `R̃ = scaled relief + E`, one noise stack.
   Every active edge takes part, relative ones included: their `σ` is 1
   along the middle and ramps to the corner target's near a non-equivalent
   corner (`EdgeTarget::amplitude_at`), so the edge itself is scaled like
   the ground either side of it, not left at full amplitude.
6. **Strip:** `B = Σ u_j T_j / U` over active edges; `H = B + g(D/W)·(R̃ −
   B)`, and `H = R̃` where `D ≥ W`.
7. **Gradient** (analytic): `∇d_j` is the segment normal (signed) or the
   unit vector from the clamped end; `∇s_j` is the direction or 0; `∇u_j
   = −16 (u_j + 1) ∇d_j / d_j`; `∇D = −D ∇U / (16 U)`; `∇B = (Σ (∇u_j T_j
   + u_j T_j′ ∇s_j) − B ∇U) / U`; `∇scales` likewise from `σ′`; `∇R̃` from
   the noise's own gradients (`Relief::layers`). About 2× the height's
   cost, not 5×.

Cost per point inside the reach: `n` segment projections (`n` about 5–20
at `ρ/2` cells and 50 m warp segments) at ~25 flops, one `powf`, and one
noise stack. Outside: the cell lookup and `n` distances, or nothing for a
plain region.

### Tile classification (speed-up option 3)

Most points of most chunks are far from every strip. The field decides
that once per tile, when the region is built, instead of once per point.

- **As built, the tiles are the field's index cells** (`SegmentIndex`),
  world-anchored squares `ρ/2` wide, classified once per region:
  - an **empty** cell: no active segment comes within `ρ` of it. Every
    weight `u_j` is then exactly 0, so a read there returns the region's
    own relief, `R + E`, after one cell lookup: no distances, no corners.
  - a **near** cell keeps the `(edge, segment)` pairs within `ρ` of it, in
    edge and segment order. Points in it walk only that list.
- **Why cells, not each batch's box (a change from the first design):**
  the batch readers (`ChunkSamples`, `GroundGrid`, the TIN probes) reach
  the terrain through `Terrain` methods (`surface`, `site_at`) in other
  areas' files. Per-batch covers would thread a new argument through all
  of them. Cells give the same skip, are classified once per region rather
  than per chunk, and need no change outside the field. What a per-chunk
  cover would add is only the cell lookup itself.
- **The bound is the exact box-to-segment distance, not a corner test.**
  It is 0 if the segment meets the box (an endpoint inside, or crossing
  one of the four sides). Otherwise it is the least of: each segment
  endpoint to the box (clamp-to-box distance), and each box side to the
  segment (`warp::segment_distance`). A segment passing between two of
  the box's corners is caught, which testing the four corners would miss.
  Comparisons use `<` against `ρ` with no slack, since a weight is 0
  exactly at `d ≥ ρ`.
- **Why this is exact, not approximate:** the per-point path drops
  segments at `d_j ≥ ρ` and returns `R + E` when the sum is 0. A segment
  at least `ρ` from the cell is at least `ρ` from every point in it, so
  the cell drops the same segments. A near cell's list is a superset of
  what each point keeps; the extra segments carry weight 0.
- **Stencils need no halo:** measures and labels read their stencil
  points as separate reads, each through its own cell.
- **Per region:** each region's field has its own cells; a plain region
  has no field at all.
- **Cost:** the classification is `n_segments × (cells in each segment's
  box) × (a few distance tests)` per region build.

### Targets

- **Edge scales `σ(s)`:** each side's layer scales along the edge,
  `scales_at(level_for(spreads, amplitude_at(s)))` from today's
  `EdgeTarget`, sampled every 16 m and interpolated linearly. Pure
  functions of the settings of the regions on the edge and its corners.
- **Non-equivalent edge, `P(s)`:** knots `(s, h)` from the two sides'
  relief above E, scaled by their `σ(s)`, `R̃_a`, `R̃_b`, at
  `point_at(s)`: `m(s) = (k_a R̃_a + k_b R̃_b)/(k_a + k_b)`,
  `k` = stiffness, `a < b` by region id. Base step `L/ceil(L/32)`; each
  interval is halved while the midpoint misses the chord by more than
  0.1 m, down to 1 m. Ends pinned: `P(s) = m(s) + Σ_ends (H_c − E_end −
  m(end))·(1 − g(s_from_end/τ))`, `τ = min(L/2, 250 m)`, with the end
  knots set to `H_c − E_end` exactly. A read adds E at the point, so E
  passes through the profile exactly. Linear between knots; the lookup
  goes to the 32 m base interval first, then searches its few knots.
- **Relative edge:** `T(p) = R_σ(s)(p) + E(p) + c(s)`, the shared relief
  scaled by the edge's `σ` (the same on both sides, since their spreads
  are equal). `c(s) = Σ_ends (H_c − R_σ(corner) − E(corner))·(1 −
  g(s_from_end/τ))`, nonzero only at non-equivalent corners. Both regions
  compute `R_σ(p)` with the same code from the same settings and the same
  `σ`, so it agrees bit for bit. The region's own `R̃` and the edge's
  `R_σ` come from one noise evaluation: the layers are computed once and
  combined with each set of scales.
- **Which equivalent edges are relative (as built, simpler than first
  designed):** an equivalent edge `e = AB` is relative when A or B
  touches, on an edge or at a corner, any region of other relief
  (`WorldTerrain::edge_is_active`; the same rule in `StitchedWorld`). It
  needs only settings, two rings out, and A and B decide alike. Otherwise
  it is skipped: neither region's field reaches it, so both read `R + E`
  there. The rule is generous (a relative edge far from any other relief
  costs strip work for a result equal to `R + E`); a geometric reach test
  could narrow it later.
- **Corner heights `H_c`:** the stiffness-weighted mean, in region order,
  of every region at the corner's relief scaled to the corner target's
  amplitude, plus E at the corner. Each plan works it out from settings.
- **Corners, against the brief's "explicit corner patches":** there is no
  separate patch shape. Corners are shared by construction instead: every
  edge's target at a corner is pinned to the one `H_c`, a read within
  `ON_EDGE` of a corner returns `H_c` itself, and near the corner the
  weighted mix of the edges' targets is continuous and meets each edge's
  target exactly on that edge. Each region's side of a corner is
  different ground, but no edge through the corner disagrees.

### Widths

- `F` (fade): as today's scaling width, `max(margin, max_targets
  |amplitude − own| / FADE_RATE)`.
- `W` (strip): `max(margin, max over the region's non-equivalent edges of
  |mean_P − mean_own| / BLEND_RATE)`, where `mean_own` is the region's
  mean height with its layers scaled by the edge's middle `σ`
  (`Relief::mean_height` with scales) and `mean_P` the stiffness-weighted
  mean of both sides' such means. A settings-only stand-in for the mean
  offset between the region's ground and the shared profile, as today's
  blend width is from the mean heights.
- `ρ = 1.25 · max(F, W)`: at `ρ = 1.25 d`, the clip changes `D` by under
  0.1%.

### Roads, rivers and crossings

- Offers stay (natural, inner, unfaded samples and wishes): crossings need
  them. They describe each side's own ground before it meets the border.
  `EdgeOffer::roughness`, corner offers and corner agreements go.
- `negotiate` takes the side's profile (`TerrainGenerator::border`, or
  `None` for an edge with no border) and fills `EdgeAgreement::profile`
  (8 m samples, as today, for the ports) with `P`'s height and the
  stiffness-weighted cross-slope of the offers; `agree_ports` is unchanged.
- `SideWishes::bent_at` becomes the strip: `bent = P + g(depth/W)·(inner −
  P)`, with `W` in place of `margin` in the offer, and no slope carry.
- A river wish's `ground` is already the final ground, so the crossing
  check no longer lowers it by the bend.
- Pieces and carving are unchanged: both sides carve the same piece.
- As TO left it, offers sample only heights unless a crossing is wished;
  the cross-slopes, inner and unfaded samples come with the crossing
  samples.

### `StitchedWorld`

`converge` gives every generator its targets and every region's settings,
so each builds its own field and profiles as a plan does. Only crossings
are negotiated between regions. Specs may differ in `default_seed`, which
the equivalence test covers.

## Bonus: stencils beyond the border

`Terrain::measures` (`terrain.rs:1087`) and the label stencil
(`terrain.rs:1055`, `label.rs:234`) read the centre region's own
extension past its border. With the new field that extension is
continuous through the border (it blends to the same `P` outside) and
returns to the region's own scaled relief beyond `W`, so the error is in
the curvature and shape only, within one stencil step of a border.

ARCHITECTURE.md ("Labels at region borders", decided by the user
2026-11-25) accepts this on purpose: reading the neighbour's ground would
make labels near borders depend on a neighbour's plan, which Ecotope
border offers must not. **Left as it is** (coordinator, 2026-11-26): it is
that decision, not a bug. The world-composed fix would be small (a `query`
that takes a height source for stencil points, `WorldTerrain` passing its
owner lookup), but it would reverse the decision, so it goes to the user
only if it matters.

## Checks

- Seams: both sides' `H` equal bit for bit at edge points (existing
  `seams.rs` and `world.rs` seam tests, now with exact equality), corners
  equal to `H_c`.
- No-op: in a world of one preset, `H` equals `R + E` at every point.
  A mixed corner of two equivalent regions and a third: exact on all three
  edges. Two mountain regions meeting plains at a corner: no
  full-amplitude ridge along the mountains' shared edge (a test and a
  `seam_spike` image).
- Continuity: no jump above 1 mm between points 1 cm apart along lines
  crossing strips, corners and segment-candidate boundaries.
- Gradients: the analytic gradient matches central differences inside
  the strip (as `slopes_match_the_heights`).
- Speed: `ground_cost`, `load_timeline` and the load profiler's
  commands, single-thread instructions before and after.
- Look: `seam_spike` images and viewer screenshots of borders, before
  and after.

## Order of work

1. Wait for TS (the Rayon removal) to land, then take every "before"
   measurement on that one-thread tree.
2. A, then B, then the tile classification.
3. A last, separate step removes `Convergence::blend`, `blend_weights`
   and `EdgeOptions::roughness`, once the coordinator has cleared
   `procgena_world`'s files. Until then their doc comments (and their
   `.meta.yml` tooltips) say they have no effect. The corner gradients
   and `EdgeSample::detail` are inside `procgena_terrain` and go with B.

## Open question

- `W` from the mean offset, or a fixed `margin`? Recommended: the mean
  offset rule above, since a fixed 125 m strip turns a 50 m offset into a
  65% ramp.
