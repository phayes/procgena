# Terrain generation on the GPU: study and plan

2026-11-26. Asked for by the user: "lets look at what part of terrain
generation can go on the GPU." This is a study and a plan. It contains no
engine code. Status lives in [ROADMAP.md](../ROADMAP.md), and the
milestones named here link there. The numbers come from
[load_profile.md](load_profile.md), the TE6 block in ROADMAP and a reading
of the code at e4b4656.

## Bottom line

- **Most of the load is one visual product, and the GPU can take it
  without touching the canonical ground.**
  - 74–82% of all terrain CPU goes on the 129 × 129 `GroundSite`s that
    each chunk samples for paint (`GroundGrid::chunk`).
  - The drawn mesh heights are another 2.2–2.4%.
  - Paint is visual-only under gate D6, and so is the drawn mesh of every
    chunk that is not canonical (see "Which drawn chunks are canonical").
    So D6's bitwise gate does not bind the biggest cost.
- **A CPU-only win comes first, and it was found in this study.**
  - The default painter is `GroundStack::bare`. Its riverbed layer asks
    for `HeightAboveWater`, which is classed as
    `SampleDetail::Measures`.
  - So every paint site also reads four more full heights for curvature
    (`Terrain::measures`, `CURVATURE_STEP`), which no layer of the bare
    stack uses. The load profile assumed `Gradient` detail.
  - Splitting "nearby water and roads" from "curvature" removes those
    four heights per site. This is step 1a.
- **Then slope moves to the GPU.** The CPU samples heights only, and the
  paint pass takes slope by differences (step 1b). Steps 1a and 1b
  together should cut terrain CPU by about **55–75%** on one thread.
- **Next comes the CPU floor (step 2).** Region plans, river traces, road
  grades, TIN probes and queries are canonical and stay on the CPU. Each
  of their reads is 78–88% border fade and pulls (TE6). Making those
  cheaper per point bounds how fast the near ground becomes canonical on
  one thread.
- **The long finish is full `H*` on the GPU for drawn non-canonical
  ground (step 3, GPU3).** It removes most CPU sampling that is left,
  about another 10–20% of today's cost, so the CPU reads `H*` only where
  it decides something. It is large, and its border-pull port is the
  risk.
- **Canonical stages stay on the CPU.**
  - WGSL has no `f64`, and the canonical maths is f64 with `std`
    transcendentals.
  - Quantising an approximate float is not deterministic (TIN design
    §14.2).
  - The one route to canonical GPU stages is a fixed-point `H*` profile.
    It belongs with D6a's later "pinned maths" milestone, as one design
    decision (question 3).
- **Order of work:**
  1. load-profile #0 (the deadlock);
  2. step 1 (1a, then 1b), with load-profile #2 and #3, which need no
     GPU;
  3. step 2;
  4. step 3 (GPU3a–d).

| # | Step | Where | Expected saving, one thread (estimates; see below) | ROADMAP |
|---|---|---|---|---|
| 1a | **Stop reading curvature that nothing paints.** A detail between `Gradient` and `Measures` gives nearby water and roads without the four curvature heights. `HeightAboveWater`, `WaterDistance` and `RoadDistance` ask for it. | CPU, small | Up to about half of each site's cost, if the curvature heights cost what a site's own height does: **about 15–35% of terrain CPU** | New item, beside GPU2 (proposed **GPU2a**). |
| 1b | **Heights-only sites, slope on the GPU.** One CPU height grid per chunk (131², a one-texel apron). The mesh takes every other point, and the paint pass takes slope by central differences. | CPU and the existing GPU paint | With 1a, sites fall to about one height plus tags each: **about 55–75% in all**, leaving about 0.45–0.9 T of 1.99 T | GPU2's site part: proposed **GPU2b** |
| 2 | **Cheaper `H*` per point on the CPU:** analytic pull gradients, cheaper far integrals, a neighbour's relief only where its blend share is above zero | CPU | Not measured. Pulls and fade are most of every canonical read, so halving them would cut the canonical remainder by roughly 30–40%. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled): propose scheduling it |
| 3 | **`H*` on the GPU for drawn non-canonical ground:** heights and paint inputs computed and kept on the GPU, with no readback | GPU, large | Most of what step 1 leaves of sampling: **about 10–20% of today's terrain CPU**, leaving about 0.2 T (about 20 s) on the mixed view | **GPU3** (no block yet; defined in `edit_cache_plan.md`). Proposed GPU3a–d; GPU1 becomes GPU3a. |

Not worth moving now: TIN refinement and CDT (about 0% of CPU), roads
(0–0.2%), river carving (0.6–1.6%) and the river trace (0.3–0.4%). This
covers CT6, RBG, CT7 and CT8 (see "Stages").

## Expected savings: how the estimate is made

All of these are estimates. **The first task of step 1 is to measure
them:**

- one site's cost at Height, at Gradient, at the new detail and at
  Measures;
- inside the border margin and outside it;
- with `--example ground_cost` (or an addition to it);
- then a re-run of `load_timeline`.

**The inputs:**

- `load_profile.md` was measured on 10 Rayon workers under load, so its
  wall times run 2–4× long. Only its shares and instruction counts are
  used here.
- The site share (74.4%) comes from the default run. The instruction
  count (2.32 T) comes from the Rough-only run of the same view. Both are
  on the mixed world, and they are combined here.
- **One thread:**
  - 2.32 T less Bevy's uncapped headless frames (14.4%) leaves about
    1.99 T of terrain work.
  - At an assumed 10 G instructions/s on one M1 Max performance core,
    that is about **200 s**. Mountains is about 6.4 T, or about 640 s.
- **Per point:**
  - The view has 252 × 16,641 ≈ 4.19 M paint sites and 252 × 4,225 ≈
    1.06 M mesh points.
  - A site costs about 410 k instructions on average, and a mesh height
    about 52 k.
  - About 8× a height matches a site at `Measures`: one height, the
    gradient (several heights near borders) and four curvature heights.
  - The profile's "gradient-only frames" (41–43%) put the gradient's part
    at about 230 k. The curvature heights' part has not been measured, so
    1a is given as a range.

**After step 1:**

- A site costs about one height plus the water and road lookups: about
  52–100 k, or 0.22–0.42 T for the view.
- The separate mesh pass (0.056 T) goes.
- The rest is about 0.21 T: region stages, stream bookkeeping and the
  remainder.
- Total: **about 0.45–0.65 T, or 45–65 s**, against 200 s.
- **The pessimistic case:** curvature is cheap and the non-gradient part
  of a site stays about 185 k. Then about 0.9 T remains (about 55%
  saved).

**After step 3:**

- The CPU reads `H*` only for:
  - the TIN's 1 m lattice on level-0 chunks: 64 × 4,225 ≈ 270 k probes,
    about 14 G;
  - level-0 mesh heights for collision when the TIN is off: the same
    count;
  - the region stages (2–4%) and bookkeeping (about 7%).
- Total: **about 0.2 T, or about 20 s, on the mixed view.** Mountains
  has heavier region stages, so expect roughly 40–60 s.
- The GPU work is about 4.2 M points × a few thousand flops: on the order
  of 10–20 ms per full view on an M1 Max, before dispatch overhead.

**What the GPU does not shorten:** the first picture waits for region
builds, which run serially on one thread. TE2's "nearest regions first"
and plan-ahead (load-profile #5), and step 2, decide that.

## Which drawn chunks are canonical

- **Only the Fine tier is canonical** (D6). Rough and Mid chunks are
  provisional at every level.
- **The TIN** (opt-in streaming, CT5a) is the physical ground of
  level-0 Fine chunks. With it on, their mesh is built from about 4,225
  CPU `H*` probes and is canonical.
- **With the TIN off,** level-0 chunks draw a regular mesh from CPU `H*`
  and give collision. That mesh stays on the CPU in this plan, because
  collision must not depend on the GPU.
- **Every other drawn product is visual:** levels 1 and up, Rough and Mid
  tiers, paint at every level, normals, stand-ins and skirts. In the
  measured view that is 188 of 252 chunks, and paint on all 252.

## Stages

Shares are of all CPU on the mixed world (mountains in brackets).

- **Canonical** means the output feeds the TIN, plans, river or road
  decisions, placement, masks or labels (D6b).
- **Visual** means paint, drawn non-canonical meshes, previews and far
  terrain.

### Per region

| Stage | Code | CPU share | Data shape | Maths | Output | GPU fit |
|---|---|---|---|---|---|---|
| Region graph, owners, E agreement | `graph.rs`, `elevation.rs` | in "other" | Graph over a few dozen sites | f64, small | Canonical | **No.** Small and branchy. The GPU needs only the per-point owner lookup. |
| Border negotiation, warp of border lines, agreed edge samples | `ports.rs`, `seam.rs`, `stitch.rs`, `warp.rs`, `plan.rs` | 1.3% (1.9%) | Per edge: hundreds of samples, then negotiation | f64; `exp` in `seam.rs` | Canonical | **No.** Sequential and small. Its outputs (segments, agreed and own edge samples) become GPU buffers. |
| Convergence targets | `converge.rs` (`relief_target`, `blend_weights`, `level_for`) | ~0 | Per edge | `ln`, `exp`, `powf`, `asin`, `sin` | Canonical | **No.** Per edge, not per point. The results become buffers. |
| River trace, hydrology, ponds | `river.rs`, `drainage.rs`, `plan.rs` | 0.4% + part of 0.3% (0.3% + 1.8%) | The faded relief on a 25 m grid (95% of the trace, TE6), then a sequential downhill walk | `H*` maths, then comparisons | Canonical | **No.** The grid reads decide rivers, so they would need bitwise parity. A GPU proposal that the CPU re-reads saves nothing. Step 2 is the lever here. |
| Roads: search, fitting, sections, earthworks | `road/` | 0.0% (0.2%) | 8 m and 16 m cost grids, A*, profile solves | f64 | Canonical | **Not now.** RBG (an integer delta-stepping cost field) can reach exact parity, but roads cost nothing at load. Keep RBG for road edits, after RB2. |

### Per point (the inside of `H*`, paid by every read)

A read is a chain:

1. The owner region (`WorldTerrain::sample_batch`).
2. Its relief: noise layers, scaled and blended by the convergence fade
   (`Relief::height_above_elevation`), plus E.
3. The bend towards the border agreement (`Terrain::base`: pulls).
4. Rivers, ponds and roads cut in, then gullies (`carve.rs`, `gully.rs`).

| Part | Share | Data shape | Maths | GPU port effort and parity risk |
|---|---|---|---|---|
| **Relief noise and E** (`noise.rs`, `relief.rs`, `elevation.rs`) | 22.9% (16.9%) | Per point, independent | Each octave rotates and scales the world position in f64 (`octave_rotation`, `LACUNARITY`), then splits it into an i64 lattice cell and an f32 fraction. The PCG hash is wrapping u32 maths, with cells folded to u32. The Perlin fade is quintic f32. The masks use `exp`. Eroded fBm sums noise gradients inside the loop. | **Low to medium effort, low risk for visual use.** The GPU has neither f64 nor i64, so the CPU passes, per chunk and per octave, the chunk origin already rotated, scaled and split into integer cell and fraction. The GPU adds the rotated, scaled local offset in f32. Cell indices then fit in i32 for any planned world, and the hash is bit-exact. Fractions differ from the CPU by a few f32 ULP. This is GPU1's kernel. |
| **Owner lookup** (`world.rs`) | in "other" | Per point, a few nearby graph sites | Distances, comparisons | **Low.** Upload the chunk's nearby sites. A point exactly on an owner boundary may choose differently from the CPU. The bend makes both sides meet there, but test it. |
| **Convergence fade** (`converge.rs`: `scales`, `blended` and their gradients) | 17.8% (22.6%) | Per point: segment index cell → a few fade edges, then closed-form pulls | The same pull integrals as the border, plus `smootherstep`. Blend mode evaluates the neighbours' own relief at the point, a variable fan-out of 1–3 relief evaluations. | **High effort, medium risk.** It shares the pull kernel with the next row. The kernel takes relief parameters indexed by region. |
| **Border pulls and bend** (`border.rs`, `terrain.rs::base`) | 28.2% (39.5%) | Per point: index cell → segments. Each segment's pull is a closed-form integral of `r^-(p+1)`, with p = 3 for targets and 16 for distance. | All f64. `powi` up to 16. `powf(-1/16)` for the soft distance. `atan2` and `sqrt` in `cos_power_integral`. A series in `(a/x)²` stopped at a relative 1e-17. Subtractions of the constant 1 (`zeroth - (x1 - x0)`). The gradient path takes four more evaluations by differences (`pull_gradients`). | **The hardest part.** In f32 the "less 1" subtractions and the far series lose digits. Plan: segment-relative coordinates scaled by the reach (all lengths ≤ 1), and measure the error against the CPU. If it misses the tolerance, emulate two-float (df64) arithmetic for the moments only, which costs about 10–20× flops there and is affordable. Step 1b already removes the four-offset gradients from paint. |
| **Carving: rivers, ponds** (`carve.rs`) | 1.6% (0.6%) | Per point: feature index → nearby lines; valleys reach up to 300 m | Distances to polylines, bank profiles, `smootherstep`, one `exp` | **Medium.** Lines, profiles and the feature index become buffers. It also gives the water level and nearby water for paint. |
| **Carving: roads** (`carve.rs`, `road/section.rs`) | 0.5% (~0) | Per point: nearby stations' cross-sections | Interpolated sections and bands | **Medium.** Upload the sections as per-station arrays. It also gives road tags and distances for paint. |
| **Gullies** (`gully.rs`) | 1.3% (1.1%) | Per point, independent pattern | `exp` ×4, `floor`, fading near roads | **Low.** |
| **Measures and labels** (`Terrain::measures`, `label.rs`) | At `Measures`: four more heights (curvature) plus nearby features. At `Labels`: about eight more. | Stencils of heights | Differences, `sin`, `cos`, `powi` | **Curvature for paint can come by differences on the GPU height grid.** Labels are canonical for placement (D6b), so paint keeps the CPU path for label layers (question 4). |

### Per chunk

| Stage | Code | Share | Shape | Output | GPU fit |
|---|---|---|---|---|---|
| **Ground sites for paint** | `GroundGrid::chunk` (`ground/weights.rs`, `ground/site.rs`); detail from `GroundStack::detail` | **74.4% (82.4%)** | 129² `H*` reads at the bare stack's `Measures`: height, gradient, four curvature heights, nearby features | Visual | **Yes.** 1a drops the curvature, 1b drops the gradient (slope on the GPU), and step 3 moves the heights. |
| Mesh heights | `ChunkSamples::sample` (`chunk.rs`) | 2.4% (2.2%) | 65² `H*` reads | Visual, except level-0 collision and canonical TIN chunks | **Yes for non-canonical chunks.** 1b shares the paint grid; step 3 moves it. |
| Ground paint: layer stack, weights | `procgena_terrain_bevy::gpu_ground`, `ground_weights.wgsl` | Already on the GPU | Per texel | Visual | **Built.** Parity: within one byte per channel against the CPU painter (Metal readback test), with unsplit weights summing to 255. |
| TIN probes | `tin/probe.rs::sample_heights`, `tin/refine.rs` (1 m lattice) | ~0% (milliseconds per chunk) | 65² Fine `H*` reads, rounded to mm | **Canonical** | **No.** Rounding to mm makes CPU/GPU parity a threshold problem (§14.2). |
| TIN refinement, CDT | `tin/refine.rs`, `tin/cdt.rs` (spade) | ~0% | Sequential topology changes, robust predicates | Canonical | **No** (TIN design §13). CT8 stays research. |
| CT6: GPU error probes | (not built) | — | Dense probes | Canonical selection | **Not now.** The probes are about 0% of CPU, and a GPU proposal changes which probe wins. Revisit only with a fixed-point `H*`. |
| Render products: normals, stand-ins, bounding boxes, skirts | `procgena_terrain_bevy` (`ChunkSamples::stand_in`, mesh build) | In "stream bookkeeping": 2.0% (0.8%) | Per vertex | Visual | **Yes, in step 3.** Normals by differences. Stand-ins take a sub-rectangle of the parent's height texture, as paint's `part` already does. Bounding boxes come from a tiny async readback or a conservative bound. |
| CT7: GPU-resident products derived from the TIN | (not built) | ~0% | Per vertex | Visual | **Later.** It reuses step 3's resident buffers, with no load win. |

## Step 1: cheaper paint sites

### 1a: no curvature unless a layer paints by it (CPU)

- **The change:** a sample detail, or flags, that gives the nearby water
  height, water distance and road distance without the four curvature
  heights.
  - `Condition::HeightAboveWater`, `WaterDistance` and `RoadDistance` ask
    for it.
  - Only `Curvature` asks for `Measures`.
  - The nearby-feature lookup (`features.nearby`) reads the feature
    index, not heights.
- **Results:** paint is bit-identical. Every site value the bare stack
  reads is unchanged, and curvature becomes `None` where no layer reads
  it.
- **Check:** paint weights from a fresh read equal today's, bit for bit,
  on the shipped presets.
- **Effort:** small. `measure.rs` and `terrain.rs::query` (`SampleDetail`),
  `ground/layer.rs` (`Condition::detail`), `ground/site.rs`, tests.

### 1b: heights only, slope on the GPU (GPU2b)

- **The CPU, per chunk:**
  - One grid at the new detail without the gradient: 131² points (129²
    plus a one-texel apron), at the world points the stream reads
    today.
  - The mesh takes every other interior point, at the same world
    positions, so there is no second pass.
  - Water, road tags, relief height and nearby water stay CPU site
    values.
- **The GPU paint pass:**
  - It computes slope (and curvature, if a layer asks) by central
    differences over the apron.
  - Shared edge texels are evaluated from the same world samples on
    both sides, so neighbours agree at seams.
  - Retained grids keep working, so a material edit repaints without a
    read.
- **Parity (visual):**
  - `gpu_progress.md` asks that the differences' error be measured before
    they replace analytic slopes.
  - Measure the slope error against the analytic gradient on the shipped
    presets at every level, near rivers, roads and borders, and how far
    material boundaries move in texels.
  - Proposed acceptance (question 1): the 95th-percentile error under
    1°, and boundaries moving no more than one texel away from creases
    (bank and road edges), with an image of the creases.
- **The CPU painter:**
  - It stays the reference and the fallback: devices without compute,
    custom and noise painters, more than eight materials.
  - For it to agree with the GPU in its inputs, it takes slope from the
    same differences (question 2).
- **What it does not cover:** stacks with label layers keep the CPU path.
  No shipped terrain stack has them today.
- **Effort:** small to medium.
  - `procgena_terrain`: the grid with an apron, and the mesh taken from
    it.
  - `procgena_terrain_bevy`: the site layout and slope in
    `ground_weights.wgsl`, the single read in the stream, and tests.
- **What step 3 reuses:** the slope-by-differences shader is exactly what
  step 3 needs, and the CPU heights path stays as the fallback.
- **A note on ARCHITECTURE:** it says GPU Scatters take slope from the
  tile's gradient grid, not by differencing a height texture, so that
  Scatters and labels agree on what is steep. Paint is not a Scatter and
  decides nothing. Taking its slope by differences departs from that
  rule's spirit only for a visual product (question 1).

## Step 2: the CPU floor (Border-pull cost)

After steps 1 and 3, the CPU still reads `H*` for:

- the TIN's probes;
- level-0 collision heights;
- the river trace (25 m grid) and hydrology;
- road grades (8 m and 16 m grids) and earthworks (a few hundred heights
  per station);
- every query and placement.

Each read is 78–88% fade and pulls (TE6). On one thread this sets how
soon the near ground becomes canonical and a region is planned. It also
sets the first picture, which waits for region builds.

The candidates are already in ROADMAP:

- fewer border pieces;
- cheaper far-piece integrals;
- a neighbour's relief only where its blend share is above zero;
- analytic pull gradients for `Gradient` queries and the TIN's slope
  checks.

Changed results are allowed in this early repo, but are reported. This
step comes before step 3 because it helps the canonical path, which no
GPU step can, and because it is much smaller.

## Step 3: `H*` on the GPU for drawn non-canonical ground (GPU3)

**Scope.** These take their heights and paint inputs from a GPU kernel:

- every chunk at level 1 and up;
- every chunk drawn at the Rough or Mid tier;
- the paint grid of level-0 chunks.

The CPU keeps `H*` for:

- level-0 meshes (TIN or collision);
- TIN probes;
- region stages;
- `Terrain::query`, placement, masks and labels;
- devices without compute.

**Kernel structure.**

- **Region buffers**, built once per region tier and kept resident:
  - relief and E parameters;
  - segment and fade-edge indexes;
  - agreed and own edge samples;
  - river, pond and road lines with sections and profiles;
  - gully parameters.
- **Per chunk:**
  - the nearby graph sites;
  - the regions it touches (up to N);
  - the per-octave origin splits (see "Relief noise and E" under per
    point).
- **Dispatches** are batched over chunks. Each point writes its height,
  water level, nearby water and road tags to resident storage textures.
- **Drawing:** a flat grid displaced in the vertex shader, with normals
  by differences. Stand-ins are sub-rectangles of a parent texture.
- **Coordinates:**
  - Chunk origins are integer metres; positions inside a chunk are f32.
  - Feature geometry is stored relative to its region's origin.
  - Shared edge points are computed from integer grid positions, never
    accumulated, so neighbouring GPU chunks agree exactly.
- **The batch seam:** `WorldTerrain::sample_batch` already groups a
  batch's points by owner region. The GPU path is a second backend
  behind it for visual products, chosen by the stream, as `gpu_ground`
  is today.

**Proposed stages, each landing on its own with parity tests:**

| Stage | Contents | Replaces | Notes |
|---|---|---|---|
| GPU3a | Relief, E and owner lookup, without the bend or fade. This is GPU1's kernel. | GPU1 | It serves the studio's relief preview (TE2). Drawn terrain switches only after GPU3c. |
| GPU3b | The pull kernel: border bend and convergence fade with the blend fan-out | — | The df64 decision is made here, from measured error. |
| GPU3c | Rivers, ponds, roads, gullies; water and road inputs for paint | — | The first stage at which drawn ground can switch. |
| GPU3d | Resident displacement, normals, stand-ins, bounding boxes; the stream switches drawn non-canonical chunks | TE3b for drawing | Retained height textures replace retained `GroundGrid`s for repaint. |

**Parity (visual; TIN design §13.1, §13.6 and §14.10).**

- **Against the CPU `H*`:** GPU1 and `gpu_progress.md` already record
  "within a few millimetres" (millimetre-scale) parity for drawn GPU
  heights. Keep that criterion and measure it over the shipped presets
  and fixtures, including large and negative world coordinates. If the
  pull kernel cannot reach it even with df64, loosening is question 5.
- **Seams:** exact between GPU chunks at the same level (same kernel,
  same inputs).
- **GPU beside CPU chunks:** the existing level skirts and stand-ins
  cover a GPU chunk next to a CPU level-0 chunk.
- **Paint:** within one byte per channel against the CPU painter fed with
  CPU heights.
- **Never canonical:** no query, placement or plan reads a GPU height.
  `gpu_progress.md` already records "Terrain queries and planning never
  use GPU readback". That becomes a test.

**Readback and latency.**

- **No height readback** for drawing.
- **Bounding boxes for culling:** a two-float min and max per chunk read
  back asynchronously, a frame or two late. A conservative bound from
  the region's E and relief amplitude serves until it arrives.
- **Uploads:** region buffers once per region tier build, about 100 KB to
  1 MB each (to measure).
- **Latency:** a chunk is drawable one frame after its dispatch, within
  a per-frame compute budget (question 6).
- **The main thread:** the CPU mesh build leaves chunk swaps, which
  should help the 20 ms swap frames in the load profile.

**Effort:** large. About 2,500 lines of f64 Rust maths go to WGSL:
`noise`, `relief`, `elevation`, `converge`, `border`, `terrain::base`,
`carve`, `road/section` and `gully`, each with the same stencil of parity
tests. The risk sits in GPU3b.

## What stays on the CPU, and why

- **Canonical `H*` and everything that reads it:** plans, rivers,
  hydrology, roads, TIN probes, refinement and CDT, labels, masks,
  placement, queries and level-0 collision.
  - D6 requires bitwise or exactly quantised parity.
  - The canonical maths is f64 with `std` transcendentals, and WGSL has
    no f64.
  - WGSL's accuracy for division and `sqrt`, and whether it may fuse a
    multiply and an add, are **to verify** against the spec before any
    float stage is called exact.
- **Sequential searches:** the river walk, road A*, fitting and the
  negotiation. They are small and irregular.
- **The CPU paths for visual products:** kept as reference and fallback.

## Canonical stages on the GPU: the only route

The long-term "GPU everywhere" goal may want canonical GPU stages. The
route is a fixed-point `H*` profile:

- the integer hash and lattice, which are already integer;
- fixed-point noise polynomials;
- polynomial `exp`, `pow` and `atan2`, defined bit for bit;
- integer pull moments.

The same profile would give D6a's cross-platform identity. So it should
be one design decision, taken when D6a's pinned-maths milestone opens.
The border integrals with power-16 weights are the hard part. Until then,
CT6, CT8 and any canonical GPU `H*` stay research.

## Proposed ROADMAP changes (for the coordinator; not edited here)

- **GPU2a (new): no curvature unless painted.** Done when paint is
  bit-identical, the curvature heights are gone from the bare stack's
  reads, and the saving is measured.
- **GPU2b (new): heights-only sites, slope by differences on the GPU.**
  Done when the slope error is measured and accepted, one CPU read is
  made per chunk, and `load_timeline` is re-run on one thread.
- **GPU3 (new block): GPU3a–d as above.** Done when every stage passes
  its parity tests, drawn non-canonical chunks read no CPU `H*`, and a
  cold mixed view on one thread is measured against this plan.
- **GPU1:** becomes GPU3a. The studio preview (TE2) uses it without the
  bend.
- **Border-pull cost:** schedule it, as the CPU floor for canonical
  reads.
- **CT6, CT7, CT8, RBG:** link this plan. CT6 and CT8 wait on the
  fixed-point decision; RBG and CT7 have no load win.
- **load_profile.md:** its #1 says the bare stack samples at `Gradient`.
  It samples at `Measures` (finding 1a), so that line should be
  corrected (sent to the profiler).
- **No new dependencies:** wgpu and naga come through Bevy.

## Open questions (for the coordinator; a recommendation for each)

1. **Is slope by differences good enough for paint, and does it fit
   ARCHITECTURE's GPU Scatter slope rule?**
   - Recommended: yes for paint only. Accept it with step 1b's measured
     acceptance (95th-percentile error under 1°, boundaries within one
     texel away from creases).
   - Scatters keep gradient grids.
2. **Should the CPU painter also take slope by differences, so both paths
   read the same inputs?**
   - Recommended: yes.
   - The fallback then needs no gradient either, and analytic gradients
     stay for queries.
3. **Should canonical stages stay on the CPU until a fixed-point `H*` is
   designed together with D6a's pinned maths?**
   - Recommended: yes.
   - Start no CT6, CT8 or canonical GPU `H*` before that decision.
4. **Should paint use GPU-derived landform labels, which may differ
   slightly from placement's canonical labels?**
   - Recommended: not yet. Label layers keep the CPU path.
   - Revisit when a shipped stack paints by label.
5. **If the pull kernel cannot meet millimetre parity even with df64,
   may drawn GPU ground at levels 1 and up loosen to 1 cm?**
   - Recommended: yes for those levels only. A level-1 cell is 2 m, far
     copies rest on `H*`, and 1 cm is well below a pixel there.
   - This loosens a recorded criterion, so it needs an explicit yes.
6. **What compute budget per frame should terrain have?**
   - Recommended: 2 ms a frame in the game.
   - Uncapped while the studio loads with nothing yet drawn.
