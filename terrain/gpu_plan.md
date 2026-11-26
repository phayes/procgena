# Terrain generation on the GPU: study and plan

2026-11-26. Asked for by the user: "lets look at what part of terrain
generation can go on the GPU." This is a study and a plan. It contains no
engine code. Status lives in [ROADMAP.md](../ROADMAP.md), and the
milestones named here link there.

**Scope (user, via the coordinator, 2026-11-26):** "Not roads or rivers.
Since those are disabled for now, and we'll circle back later." The plan
covers what loads with roads and rivers off (`RoadOptions::enabled` and
the river options off). Gullies go with rivers: they cut only river
valley walls (`carve.rs::gully_depth`). Roads, rivers and gullies have a
short note under "Later: roads and rivers".

The numbers come from [load_profile.md](load_profile.md), measured with
roads and rivers on, and from a reading of the code at e4b4656. The
profiler has been asked for numbers with them off; until those land, the
shares below remove carving, gullies and the river trace by hand (see
"Expected savings").

## Bottom line

- **Most of the load is one visual product, and the GPU can take it
  without touching the canonical ground.**
  - About three quarters of all terrain CPU goes on the 129 × 129
    `GroundSite`s that each chunk samples for paint
    (`GroundGrid::chunk`).
  - The drawn mesh heights are another 2.2–2.4%.
  - Paint is visual-only under gate D6, and so is the drawn mesh of every
    chunk that is not canonical (see "Which drawn chunks are canonical").
    So D6's bitwise gate does not bind the biggest cost.
- **A CPU-only win comes first, and it was found in this study.**
  - The default painter, `GroundStack::bare`, has a riverbed layer that
    asks for `HeightAboveWater`, which is classed as
    `SampleDetail::Measures`. So every paint site also reads four full
    heights for curvature (`Terrain::measures`, `CURVATURE_STEP`), which
    no layer of the stack uses.
  - With rivers off there is no water to measure at all, yet the
    curvature is still read.
  - Splitting "nearby water" from "curvature" removes those four heights
    per site. This is step 1a.
- **Then slope moves to the GPU.** The CPU samples heights only, and the
  existing paint pass takes slope by differences (step 1b). Steps 1a and
  1b together should cut terrain CPU by about **55–75%** on one thread.
- **Next comes the CPU floor (step 2).** Region plans, TIN probes, labels,
  placement and queries are canonical and stay on the CPU. Each of their
  `H*` reads is mostly border fade and pulls: 46–62% of all CPU today,
  and 78–88% of a base read. Cheaper pulls on the CPU bound how fast the
  near ground becomes canonical on one thread.
- **The long finish is `H*` on the GPU for drawn non-canonical ground
  (step 3, GPU3).**
  - It removes most of the CPU sampling that is left, about another
    10–20% of today's cost.
  - With roads and rivers off, the kernel is only relief, E, the owner
    lookup and the pulls. No carving is needed.
  - It is large, and the f64 border-pull port is the risk.
- **Canonical stages stay on the CPU.**
  - WGSL has no `f64`, and the canonical maths is f64 with `std`
    transcendentals.
  - Quantising an approximate float is not deterministic (TIN design
    §14.2).
  - The one route to canonical GPU stages is a fixed-point `H*` profile.
    It belongs with D6a's later "pinned maths" milestone, as one design
    decision (question 3).
- **Order of work:**
  1. load-profile #0 (the deadlock; the hang was in road tiers, so check
     whether it occurs with roads off);
  2. step 1 (1a, then 1b);
  3. step 2;
  4. step 3 (GPU3a–c).

| # | Step | Where | Expected saving, one thread (estimates; see below) | ROADMAP |
|---|---|---|---|---|
| 1a | **Stop reading curvature that nothing paints.** A detail between `Gradient` and `Measures` gives nearby water without the four curvature heights. `HeightAboveWater` and the distance conditions ask for it; only `Curvature` asks for `Measures`. | CPU, small | `ground_cost` puts a `Measures` site grid at 1.8–5.7× a `Gradient` one, so curvature is 44–82% of a site's cost: **about 35–70% of terrain CPU**. Paint is bit-identical. | New, beside GPU2 (proposed **GPU2a**) |
| 1b | **Heights-only sites, slope on the GPU.** One CPU height grid per chunk (131², a one-texel apron). The mesh takes every other point, and the paint pass takes slope by central differences. | CPU and the existing GPU paint | With 1a, a site costs about one height: **about 55–75% in all**, leaving about 0.45–0.85 T of about 1.9 T | GPU2's site part (proposed **GPU2b**) |
| 2 | **Cheaper `H*` per point on the CPU:** analytic pull gradients, cheaper far integrals, a neighbour's relief only where its blend share is above zero | CPU | Not measured. Halving the pulls and fade would cut the canonical remainder by roughly 30–40%, and the first picture with it. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled): propose scheduling it |
| 3 | **`H*` on the GPU for drawn non-canonical ground:** relief, E, owner lookup, pulls and fade; heights and paint inputs kept on the GPU, with no readback | GPU, large | Most of the sampling step 1 leaves: **about 10–20% of today's terrain CPU**, leaving about 0.2 T (about 20 s) on the mixed view | **GPU3** (no block yet; defined in `edit_cache_plan.md`). Proposed GPU3a–c; GPU1 becomes GPU3a. |

Not worth moving now: TIN refinement and CDT (about 0% of CPU), the
region graph and negotiation (1–2%), and measures and labels, which no
shipped stack paints with. CT6, CT7 and CT8 are covered under "Stages".

## Expected savings: how the estimate is made

All of these are estimates. **The first task of step 1 is to measure
them:**

- one site's cost at Height, at Gradient, at the new detail and at
  Measures;
- inside the border margin and outside it;
- with `--example ground_cost` (or an addition to it);
- then a re-run of `load_timeline` with roads and rivers off.

**The inputs:**

- `load_profile.md` was measured on 10 Rayon workers under load, with
  roads and rivers on. Only its shares and instruction counts are used
  here.
- The site share (74.4%) is from the default run. The instruction count
  (2.32 T) is from the Rough-only run of the same view, which has rivers
  and no roads.
- **Roads and rivers off, by hand:**
  - Remove river carving (1.6%), gullies (1.3%), the river trace (0.4%)
    and part of hydrology (about 0.2%): about 3.5%.
  - Remove Bevy's uncapped headless frames (14.4%).
  - That leaves about **1.9 T of terrain work, about 190 s** at an
    assumed 10 G instructions/s on one M1 Max performance core.
  - Mountains: about 6.1 T, or about 610 s.
  - With roads off, Rough and Fine chunks differ in nothing, so the
    Rough → Fine re-reads (load-profile #2) should vanish once the stream
    knows that.
- **Per point:**
  - The view has 252 × 16,641 ≈ 4.19 M paint sites and 252 × 4,225 ≈
    1.06 M mesh points.
  - A site costs about 410 k instructions, and a mesh height about 52 k.
  - About 8× a height matches a site at `Measures`: one height, the
    gradient (several heights near borders) and four curvature heights.
  - The profile's "gradient-only frames" (41–43% of all CPU, 52–54% of
    site time) put the gradient at about 230 k a site.
- **Measured by the profiler with `ground_cost`** (warm, with Rayon, load
  120–165):
  - A 129² grid at `Measures` costs 1.8–5.7× one at `Gradient`. On hills
    at level 0 that is 317 ms against 176 ms, and 31 ms for the 65² mesh
    samples.
  - Per point that is about 19 µs a `Measures` site, 10.6 µs a
    `Gradient` site and 7.3 µs a mesh height.
  - So curvature is 44–82% of a site (1a). A heights-only site at about
    mesh-point cost is 62–90% cheaper than today's (1a with 1b).
  - Sites are about 87% of terrain CPU once Bevy's frames are left out.
    That gives 1a about 38–71%, and 1a with 1b about 54–78%.
  - The split between inside the border margin and outside it is still
    not measured.

**After step 1:**

- A site costs about one height: 52–100 k, or 0.22–0.42 T for the view.
- The separate mesh pass (0.056 T) goes.
- About 0.18 T is left elsewhere: region stages, stream bookkeeping and
  the remainder.
- Total: **about 0.4–0.6 T, or 40–60 s.**
- **The pessimistic case:** curvature is cheap and the non-gradient part
  of a site stays about 185 k. Then about 0.85 T remains, about 55%
  saved.

**After step 3:**

- The CPU reads `H*` for:
  - the TIN's 1 m lattice on level-0 chunks (64 × 4,225 ≈ 270 k probes,
    about 14 G), or the level-0 collision mesh when the TIN is off (the
    same count);
  - region stages (about 2–3% with rivers off);
  - bookkeeping (about 7%).
- Total: **about 0.2 T, or about 20 s, on the mixed view.** Expect
  roughly 40–60 s on mountains, whose plans are heavier.
- The GPU work is about 4.2 M points × a few thousand flops: on the order
  of 10–20 ms per full view on an M1 Max, before dispatch overhead.

**What the GPU does not shorten:** the first picture waits for region
plans and builds, which run serially on one thread. TE2's "nearest
regions first" and plan-ahead (load-profile #5), and step 2, decide that.

## Which drawn chunks are canonical

- **Only the Fine tier is canonical** (D6). Rough and Mid chunks are
  provisional at every level.
- **The TIN** (opt-in streaming, CT5a) is the physical ground of
  level-0 Fine chunks. Its mesh is built from about 4,225 CPU `H*`
  probes and is canonical. With rivers and roads off, it has only its
  edge contracts as constraints.
- **With the TIN off,** level-0 chunks draw a regular mesh from CPU `H*`
  and give collision. It stays on the CPU, because collision must not
  depend on the GPU.
- **Everything else drawn is visual:** levels 1 and up, Rough and Mid
  tiers, paint at every level, normals, stand-ins and skirts. In the
  measured view that is 188 of 252 chunks, and paint on all 252.

## Stages (roads and rivers off)

Shares are of all CPU on the mixed world, with mountains in brackets,
from the profile with roads and rivers on.

- **Canonical** means the output feeds the TIN, plans, placement, masks
  or labels (D6b).
- **Visual** means paint, drawn non-canonical meshes, previews and far
  terrain.

### Per region

| Stage | Code | CPU share | Data shape | Maths | Output | GPU fit |
|---|---|---|---|---|---|---|
| Region graph, owners, E agreement | `graph.rs`, `elevation.rs` | in "other" | Graph over a few dozen sites | f64, small | Canonical | **No.** Small and branchy. The GPU needs only the per-point owner lookup. |
| Border negotiation: offers, warp of border lines, agreed edge samples | `ports.rs`, `seam.rs`, `stitch.rs`, `warp.rs`, `plan.rs` | 1.3% (1.9%) for offers; part of the Rough build's 0.3% (1.8%) | Per edge: hundreds of samples, then negotiation | f64, `exp` in `seam.rs`; the offers read `H*` along edges | Canonical | **No.** Sequential, small, and its reads decide the agreement. The outputs (segments, agreed and own edge samples) become GPU buffers. Step 2 makes its reads cheaper. |
| Convergence targets | `converge.rs` (`relief_target`, `blend_weights`, `level_for`) | ~0 | Per edge | `ln`, `exp`, `powf`, `asin`, `sin` | Canonical | **No.** Per edge, not per point. The results become buffers. |

### Per point (the inside of `H*`, paid by every read)

With roads and rivers off, a read is:

1. the owner region (`WorldTerrain::sample_batch`);
2. its relief: noise layers, scaled and blended by the convergence fade
   (`Relief::height_above_elevation`), plus E;
3. the bend towards the border agreement (`Terrain::base`: pulls).

Nothing is cut in.

| Part | Share | Data shape | Maths | GPU port effort and parity risk |
|---|---|---|---|---|
| **Relief noise and E** (`noise.rs`, `relief.rs`, `elevation.rs`) | 22.9% (16.9%) | Per point, independent | Each octave rotates and scales the world position in f64 (`octave_rotation`, `LACUNARITY`), then splits it into an i64 lattice cell and an f32 fraction. The PCG hash is wrapping u32 maths, with cells folded to u32. The Perlin fade is quintic f32. The masks use `exp`. Eroded fBm sums noise gradients inside the loop. | **Low to medium effort, low risk for visual use.** The GPU has neither f64 nor i64, so the CPU passes, per chunk and per octave, the chunk origin already rotated, scaled and split into integer cell and fraction. The GPU adds the rotated, scaled local offset in f32. Cell indices then fit in i32 for any planned world, and the hash is bit-exact. Fractions differ from the CPU by a few f32 ULP. This is GPU1's kernel. |
| **Owner lookup** (`world.rs`) | in "other" | Per point, a few nearby graph sites | Distances, comparisons | **Low.** Upload the chunk's nearby sites. A point exactly on an owner boundary may choose differently from the CPU. The bend makes both sides meet there, but test it. |
| **Convergence fade** (`converge.rs`: `scales`, `blended`, their gradients) | 17.8% (22.6%) | Per point: segment index cell → a few fade edges, then closed-form pulls | The pull integrals (next row), plus `smootherstep`. Blend mode evaluates the neighbours' own relief at the point, a variable fan-out of 1–3 relief evaluations. | **High effort, medium risk.** It shares the pull kernel with the next row. The kernel takes relief parameters indexed by region. |
| **Border pulls and bend** (`border.rs`, `terrain.rs::base`) | 28.2% (39.5%) | Per point: index cell → segments. Each segment's pull is a closed-form integral of `r^-(p+1)`, with p = 3 for targets and 16 for distance. | All f64. `powi` up to 16. `powf(-1/16)` for the soft distance. `atan2` and `sqrt` in `cos_power_integral`. A series in `(a/x)²` stopped at a relative 1e-17. Subtractions of the constant 1 (`zeroth - (x1 - x0)`). The gradient path takes four more evaluations by differences (`pull_gradients`). | **The hardest part.** In f32 the "less 1" subtractions and the far series lose digits. Plan: segment-relative coordinates scaled by the reach (all lengths ≤ 1), and measure the error against the CPU. If it misses the tolerance, emulate two-float (df64) arithmetic for the moments only, which costs about 10–20× flops there and is affordable. Step 1b already removes the four-offset gradients from paint. |
| **Measures and labels** (`Terrain::measures`, `label.rs`) | At `Measures`: four more heights (curvature) plus nearby features. At `Labels`: about eight more. | Stencils of heights | Differences, `sin`, `cos`, `powi` | **Curvature for paint can come by differences on the GPU height grid.** Labels are canonical for placement (D6b), so label layers keep the CPU path (question 4). With rivers and roads off, nearby water and road distances are empty. |

### Per chunk

| Stage | Code | Share | Shape | Output | GPU fit |
|---|---|---|---|---|---|
| **Ground sites for paint** | `GroundGrid::chunk` (`ground/weights.rs`, `ground/site.rs`); detail from `GroundStack::detail` | **74.4% (82.4%)** | 129² `H*` reads at the bare stack's `Measures`: height, gradient, four curvature heights, the nearby-feature lookup | Visual | **Yes.** 1a drops the curvature, 1b drops the gradient (slope on the GPU), and step 3 moves the heights. |
| Mesh heights | `ChunkSamples::sample` (`chunk.rs`) | 2.4% (2.2%) | 65² `H*` reads | Visual, except level-0 collision and canonical TIN chunks | **Yes for non-canonical chunks.** 1b shares the paint grid; step 3 moves it. |
| Ground paint: layer stack, weights | `procgena_terrain_bevy::gpu_ground`, `ground_weights.wgsl` | Already on the GPU | Per texel | Visual | **Built.** Parity: within one byte per channel against the CPU painter (Metal readback test), with unsplit weights summing to 255. |
| TIN sampling | `tin/probe.rs::sample_heights`, `tin/sampling.rs` (adaptive on the 1 m lattice since CT-A) | ~0% (milliseconds per chunk) | Up to 65² Fine `H*` reads, rounded to cm | **Canonical** | **No.** Rounding to cm makes CPU/GPU parity a threshold problem (§14.2). |
| TIN refinement and CDT | `tin/refine.rs`, `tin/cdt.rs` (spade) | ~0% | Sequential topology changes, robust predicates | Canonical | **No** (TIN design §13). CT8 stays research. |
| CT6: GPU error probes | (not built) | — | Dense probes | Canonical selection | **Not now.** The probes are about 0% of CPU, and a GPU proposal changes which probe wins. Revisit only with a fixed-point `H*`. |
| Render products: normals, stand-ins, bounding boxes, skirts | `procgena_terrain_bevy` (`ChunkSamples::stand_in`, mesh build) | In "stream bookkeeping": 2.0% (0.8%) | Per vertex | Visual | **Yes, in step 3.** Normals by differences. Stand-ins take a sub-rectangle of the parent's height texture, as paint's `part` already does. Bounding boxes come from a tiny async readback or a conservative bound. |
| CT7: GPU-resident products derived from the TIN | (not built) | ~0% | Per vertex | Visual | **Later.** It reuses step 3's resident buffers, with no load win. |

## Step 1: cheaper paint sites

### 1a: no curvature unless a layer paints by it (CPU)

- **The change:** a sample detail, or flags, that gives the nearby water
  height (and the water and road distances, for when they return) without
  the four curvature heights.
  - `Condition::HeightAboveWater`, `WaterDistance` and `RoadDistance` ask
    for it.
  - Only `Curvature` asks for `Measures`.
  - The nearby-feature lookup (`features.nearby`) reads the feature
    index, not heights, and is empty with rivers and roads off.
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
    presets at every level, near borders included, and how far material
    boundaries move in texels.
  - Proposed acceptance (question 1): the 95th-percentile error under
    1°, and boundaries moving no more than one texel.
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
- the border offers;
- labels and masks;
- every query and placement.

Each read is mostly fade and pulls. On one thread this sets how soon the
near ground becomes canonical and a region is planned. It also sets the
first picture, which waits for region plans.

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

- **Region buffers**, built once per region and kept resident:
  - relief and E parameters;
  - the segment and fade-edge indexes;
  - agreed and own edge samples.
- **Per chunk:**
  - the nearby graph sites;
  - the regions it touches (up to N);
  - the per-octave origin splits (see "Relief noise and E" under per
    point).
- **Dispatches** are batched over chunks, and each point writes its
  height to a resident storage texture.
- **Drawing:** a flat grid displaced in the vertex shader, with normals
  by differences. Stand-ins are sub-rectangles of a parent texture.
- **Coordinates:**
  - Chunk origins are integer metres; positions inside a chunk are f32.
  - Border geometry is stored relative to its region's origin.
  - Shared edge points are computed from integer grid positions, never
    accumulated, so neighbouring GPU chunks agree exactly.
- **The batch seam:** `WorldTerrain::sample_batch` already groups a
  batch's points by owner region. The GPU path is a second backend
  behind it for visual products, chosen by the stream, as `gpu_ground`
  is today.

**Proposed stages, each landing on its own with parity tests:**

| Stage | Contents | Replaces | Notes |
|---|---|---|---|
| GPU3a | Relief, E and owner lookup, without the bend or fade. This is GPU1's kernel. | GPU1 | It serves the studio's relief preview (TE2). Drawn terrain switches only after GPU3b. |
| GPU3b | The pull kernel: border bend and convergence fade with the blend fan-out | — | The df64 decision is made here, from measured error. With roads and rivers off this completes `H*`. |
| GPU3c | Resident displacement, normals, stand-ins, bounding boxes; the stream switches drawn non-canonical chunks | TE3b for drawing | Retained height textures replace retained `GroundGrid`s for repaint. |

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
- **Uploads:** region buffers once per region, on the order of 100 KB
  each (to measure).
- **Latency:** a chunk is drawable one frame after its dispatch, within
  a per-frame compute budget (question 6).
- **The main thread:** the CPU mesh build leaves chunk swaps, which
  should help the 20 ms swap frames in the load profile.

**Effort:** large, but smaller without carving. About 1,500–2,000 lines
of f64 Rust maths go to WGSL: `noise`, `relief`, `elevation`, `converge`,
`border` and `terrain::base`, each with the same stencil of parity tests.
The risk sits in GPU3b.

## What stays on the CPU, and why

- **Canonical `H*` and everything that reads it:** plans and negotiation,
  TIN probes, refinement and CDT, labels, masks, placement, queries and
  level-0 collision.
  - D6 requires bitwise or exactly quantised parity.
  - The canonical maths is f64 with `std` transcendentals, and WGSL has
    no f64.
  - WGSL's accuracy for division and `sqrt`, and whether it may fuse a
    multiply and an add, are **to verify** against the spec before any
    float stage is called exact.
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

## Later: roads and rivers

Out of scope until the user brings roads and rivers back. When they
return:

- **Rivers:** the river trace (25 m grid reads of the faded relief),
  hydrology and the downhill walk are canonical, and stay on the CPU.
- **Roads:** search, fitting, sections and earthworks are canonical, and
  stay on the CPU. RBG, the integer road cost field, can reach exact
  parity, but roads cost almost nothing at load (0–0.2%).
- **Carving and gullies** (about 3.4% of CPU with them on) would join
  GPU3 as a stage after GPU3b, for drawn ground only. That stage would
  carry:
  - river, pond and road lines with their sections and profiles, as
    buffers;
  - water levels and road tags for paint;
  - gully stripes.
- **Paint:** distance and water layers go through 1a's new detail.

## Proposed ROADMAP changes (for the coordinator; not edited here)

- **GPU2a (new): no curvature unless painted.** Done when paint is
  bit-identical, the curvature heights are gone from the bare stack's
  reads, and the saving is measured.
- **GPU2b (new): heights-only sites, slope by differences on the GPU.**
  Done when the slope error is measured and accepted, one CPU read is
  made per chunk, and `load_timeline` is re-run on one thread with roads
  and rivers off.
- **GPU3 (new block): GPU3a–c as above.** Done when every stage passes its
  parity tests, drawn non-canonical chunks read no CPU `H*`, and a cold
  mixed view on one thread is measured against this plan. Carving comes
  later with roads and rivers.
- **GPU1:** becomes GPU3a. The studio preview (TE2) uses it without the
  bend.
- **Border-pull cost:** schedule it, as the CPU floor for canonical
  reads.
- **CT6, CT7, CT8, RBG:** link this plan. CT6 and CT8 wait on the
  fixed-point decision; CT7 has no load win; RBG waits for roads.
- **load_profile.md:** its #1 says the bare stack samples at `Gradient`.
  It samples at `Measures` (finding 1a). This has been sent to the
  profiler.
- **No new dependencies:** wgpu and naga come through Bevy.

## Open questions (for the coordinator; a recommendation for each)

1. **Is slope by differences good enough for paint, and does it fit
   ARCHITECTURE's GPU Scatter slope rule?**
   - Recommended: yes for paint only. Accept it with step 1b's measured
     acceptance (95th-percentile error under 1°, boundaries within one
     texel).
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
