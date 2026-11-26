# Terrain generation on the GPU: study and plan

2026-11-26. Asked for by the user: "lets look at what part of terrain
generation can go on the GPU." This is a study and a plan. It contains no
engine code. Status lives in [ROADMAP.md](../ROADMAP.md), and the
milestones named here link there. The numbers come from
[load_profile.md](load_profile.md), the TE6 block in ROADMAP and a reading
of the code at e4b4656.

## Bottom line

- **The GPU can take most of the load, without touching the canonical
  ground.**
  - 74–82% of all terrain CPU goes on one product: the 129 × 129
    `GroundSite`s that each chunk samples for paint, at
    `SampleDetail::Gradient`.
  - The drawn mesh heights are another 2.2–2.4%.
  - Paint is visual-only under gate D6, and so is the drawn mesh of every
    chunk that is not a canonical Fine TIN chunk. So the bitwise gate does
    not bind the biggest cost.
  - On one worker thread the cold mixed view at Rough is about 2.0 T
    instructions of terrain work (about 200 s on one core). The first two
    steps below should bring that to about 0.2 T (about 20 s). Every
    figure is an estimate to measure (see "Expected savings").
- **The canonical work stays on the CPU.** This is the analytic surface
  `H*` wherever it decides something: TIN probes, river traces, hydrology,
  road grades, placement, masks, slope and labels.
  - WGSL has no `f64`.
  - Its division and `sqrt` are not correctly rounded.
  - It does not promise when multiplies and adds are fused.
  - Quantising an approximate float does not make it deterministic (TIN
    design §14.2).
  - The only realistic route to canonical GPU stages is a fixed-point
    `H*`. That belongs with D6a's later "pinned maths" milestone, not with
    this plan (question 3).
- **The canonical remainder is then the floor on one thread.** Region
  plans and builds, about 270 k TIN probes per view, and queries all cost
  `H*` reads on the CPU, and the GPU cannot help them. So the CPU per-point
  cost of the border pulls and the fade remains a load-time item next to
  the GPU work (step 3).

### The order (each step stands alone and keeps a CPU path)

| # | Step | Moves to the GPU | Expected saving, one thread | ROADMAP |
|---|---|---|---|---|
| 1 | **Heights-only sites, slope on the GPU.** The CPU samples one height grid per chunk (131², with a one-texel apron). The mesh takes every other point, and the existing paint pass derives slope by central differences. | Slope, and the second sampling pass | About **65%** of terrain CPU: the gradient half of the sites (41–43% of all CPU), the mesh's separate pass (about 2%), and sites at mesh-point cost (about 10% left of 74–82%) | GPU2: its site part. Propose a step **GPU2b**. |
| 2 | **`H*` heights on the GPU for every drawn non-canonical chunk.** These are levels 1 and up, the Rough and Mid tiers, and the paint grid of level-0 chunks. The heights stay resident: displacement and paint read them, and there is no readback. | Relief, E, warp, border pulls and fade, carving, gullies, water and road tags, in f32 | Most of what step 1 leaves: about **10% of today's CPU**, which is about 50% of what is left after step 1. The CPU then reads `H*` only for TIN probes, plans and queries. | **GPU3** (no block yet). Propose GPU3a–d; GPU1 folds into GPU3a. |
| 3 | **Cheaper `H*` per point on the CPU.** This means analytic pull gradients instead of four offsets, cheaper far integrals, and evaluating a neighbour's relief only where its blend share is above zero. | Nothing. This is the CPU floor. | Unknown until measured. The pulls and fade are 88% of a base read (TE6), and every canonical read pays that, river traces included. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled): propose scheduling it |

Not worth moving now: TIN refinement and CDT (about 0% of CPU), roads
(0–0.2%), river carving (0.6–1.6%), and the river trace (0.3–0.4%). This
covers CT6, RBG, CT7 and CT8 (see "Stages" for each).

Load-profile items #0 (the deadlock), #2 (skipping Rough → Fine re-reads)
and #3 (nearest upgrades first) are independent of the GPU and should
land first or alongside. #2 also saves GPU time once step 2 lands.

## Expected savings: how the estimate is made

- **What it is based on:** the shares and instruction counts in
  `load_profile.md` (10 Rayon workers, under load). Wall times there run
  2–4× long, so they are not used.
- **Converting to one thread:**
  - The mixed view at Rough only: 2.32 T instructions for 252 chunks.
    That is 252 × 16,641 ≈ 4.19 M paint sites and 252 × 4,225 ≈ 1.06 M
    mesh points.
  - Bevy's uncapped headless frames (14.4%) are left out, which leaves
    about 1.99 T of terrain work.
  - At an assumed 10 G instructions/s on one M1 Max performance core,
    that is about 200 s. Mountains: about 6.4 T, or about 640 s.
- **Per point:**
  - A paint site at `Gradient` costs about 410 k instructions on average
    (74.4% of 2.32 T over 4.19 M).
  - A mesh height costs about 52 k (2.4% over 1.06 M).
  - The averages hide the border margin. Inside it a point pays the pulls
    and the fade; outside, it pays almost only relief noise. The profiler
    was asked for the split; it is not in the data yet.
- **After step 1:**
  - The profile's own estimate is that the sites fall to about 10% of
    today's CPU (4× the mesh points, at mesh-point cost). The separate
    mesh pass goes.
  - That leaves about 1.99 − 1.73 + 0.23 ≈ 0.5 T, or about 50 s.
  - Risk: a heights-only site still computes water and road tags and
    relief height. If that costs more than a mesh point, the saving
    shrinks towards the 41–43% floor of the gradient frames alone.
- **After step 2:**
  - The CPU reads `H*` for the TIN's 1 m lattice on 64 level-0 chunks
    only: 270 k probes, about 14 G.
  - Region stages (about 2–4%) and stream bookkeeping (about 7%) remain.
  - Total about 0.2 T, or about 20 s, on the mixed view. Mountains has
    heavier region stages (plans up to 6 s each, loaded), so expect
    roughly 40–60 s.
  - The GPU work is small. About 4.2 M points × a few thousand flops is
    on the order of 10–20 ms on an M1 Max per full view, before
    dispatch overhead.
- **What step 2 does not shorten:** the first picture still waits for
  the CPU's region builds, which run serially on one thread. TE2's
  "nearest regions first" and plan-ahead (load-profile #5) decide it.

## Stages

Shares are of all CPU on the mixed world (mountains in brackets), from
the load profile.

- **Canonical** means its output feeds the TIN, plans, river or road
  decisions, placement, masks or labels (D6b).
- **Visual** means paint, a drawn non-canonical mesh, previews or far
  terrain.

### Per region

| Stage | Code | CPU share | Data shape | Maths | Output | GPU fit |
|---|---|---|---|---|---|---|
| Region graph, owners, E agreement | `graph.rs`, `elevation.rs` | in "other" | Graph over a few dozen sites | f64, small | Canonical | **No.** Tiny and branchy. The GPU needs only the per-point owner lookup (see "Relief and E" under per point). |
| Border negotiation: offers, agreements, edge samples | `ports.rs`, `seam.rs`, `stitch.rs`, `plan.rs` | 1.3% (1.9%) | Per edge: hundreds of samples, then a negotiation | f64, `exp` in `seam.rs` | Canonical | **No.** Sequential, small. Its outputs (agreed and own edge samples, segments) become GPU buffers. |
| Convergence targets | `converge.rs` (`relief_target`, `blend_weights`, `level_for`) | ~0 | Per edge | `ln`, `exp`, `powf`, `asin`, `sin` | Canonical | **No.** Per edge, not per point. The results become buffers. |
| River trace, hydrology, ponds | `river.rs`, `drainage.rs`, `plan.rs` | 0.4% + part of 0.3% (0.3% + 1.8%) | The trace reads the faded relief on a 25 m grid (95% of it, TE6), then makes a sequential downhill walk | The reads are `H*` maths; the walk is comparisons | Canonical | **No.** The grid reads are parallel but decide rivers, and the walk is a sequential search. A GPU grid would need bitwise parity. Without parity it could only be a proposal the CPU re-reads, which saves nothing. |
| Roads: search, fitting, sections, earthworks | `road/` | 0.0% (0.2%) | 8 m and 16 m cost grids, A*, profile solves | f64 | Canonical | **Not now.** RBG (an integer delta-stepping cost field) is exact-parity feasible, but roads are no load cost. Keep RBG for road edits, after RB2. |
| Gully patterns | `gully.rs` | (per point) | Per point | `exp` ×4, `floor` | Canonical in `H*` | See "Carving and gullies" under per point. |

### Per point (the inside of `H*`, paid by every read)

Each read is a chain. It finds the owner region (`WorldTerrain::sample_batch`)
and evaluates its relief (noise layers scaled and blended by the
convergence fade). It then adds E, bends towards the border agreement
(`Terrain::base`: pulls), and cuts in rivers, ponds, roads and gullies
(`carve.rs`, `gully.rs`).

| Part | Share | Data shape | Maths | GPU port effort and parity risk |
|---|---|---|---|---|
| **Relief noise** (`noise.rs`, `relief.rs`, `warp.rs`) | 22.9% (16.9%) | Per point, independent | The integer PCG hash (`hash2`: wrapping u32 maths, with i64 cells folded to u32). The fraction is f32 Perlin with a quintic fade. Relief masks use `exp`; `ridged` uses `abs` and `max`; eroded fBm needs noise gradients inside the sum. Octave rotations are constants. | **Low effort, low risk.** The hash is bitwise portable: pass cells as two i32 words, and `fold` is exact. The lattice split keeps f32 fractions small, so f32 GPU noise is within a few ULP of the CPU's. `exp` in the masks is approximate in WGSL, which is fine for visual output. This is GPU1's kernel. |
| **E** (`elevation.rs`) | in relief | Per point | fBm with gradient | As relief. |
| **Owner lookup** (`world.rs`) | in "other" | Per point, a few nearby sites | Distances, comparisons | **Low.** Upload the chunk's nearby sites. A point exactly on an owner boundary may choose differently on the GPU. That is visual and the bend makes both sides meet, but test it. |
| **Convergence fade** (`converge.rs`: `scales`, `blended`, and their gradients) | 17.8% (22.6%) | Per point: segment index cell → a few segments, then closed-form pulls | The same pull integrals as the border, plus `smootherstep`. Blend mode evaluates the neighbours' own relief at the point, which is a variable fan-out of 1–3 relief evaluations. | **High effort, medium risk.** It shares the pull kernel with the border (next row). The fan-out means the kernel evaluates up to N regions' relief per point, with relief parameters indexed per region. |
| **Border pulls and bend** (`border.rs`, `terrain.rs::base`) | 28.2% (39.5%) | Per point: index cell → segments. Each segment's pull is a closed-form integral of `r^-(p+1)` with p = 3 and 16. | f64 throughout: `powi` up to 16, `powf(-1/16)` for the soft distance, `atan2` and `sqrt` in `cos_power_integral`, a series in `(a/x)²` stopped at a relative 1e-17, and subtractions of the constant 1 (`zeroth - (x1 - x0)`). The gradient path takes four more evaluations by differences (`pull_gradients`). | **The hardest part.** There is no f64 on Metal or WGSL. In f32 the "less 1" subtractions and the far series lose digits. Plan: work in segment-relative coordinates scaled by the reach (all lengths ≤ 1), and measure the error against the CPU. If it misses the tolerance, use two-float (df64) emulation for the moments only, which costs about 10–20× flops there and is affordable on the GPU. Step 1 already removes the four-offset gradients from the paint. |
| **Carving: rivers, ponds** (`carve.rs`) | 1.6% (0.6%) | Per point: feature index → nearby lines; valley reach up to 300 m | Distances to polylines, bank profiles, `smootherstep`, one `exp` | **Medium.** Lines, profiles and the feature index become buffers. It also gives the water level for paint. |
| **Carving: roads** (`carve.rs`, `road/section.rs`) | 0.5% (~0) | Per point: nearby stations' cross-sections | Interpolated sections, bands, `smootherstep` | **Medium.** Sections are per station, so upload them as arrays. It also gives road tags and distances for paint. |
| **Gullies** (`gully.rs`) | 1.3% (1.1%) | Per point, independent pattern | `exp` ×4, `floor`, faded near roads | **Low.** |
| **Measures and labels** (`measure.rs`, `label.rs`) | Only for stacks that ask (`Measures`: about 4 more heights; `Labels`: about 8 more) | Per point, stencils of heights | Differences, `sin`, `cos`, `powi` | **Visual measures (curvature) by differences on the GPU height grid: yes.** Labels are canonical for placement (D6b). Paint could use GPU-derived labels only if paint is allowed to differ from the placement labels (question 4). |

### Per chunk

| Stage | Code | Share | Shape | Output | GPU fit |
|---|---|---|---|---|---|
| **Ground sites for paint** | `GroundGrid::chunk` (`ground/weights.rs`, `ground/site.rs`) | **74.4% (82.4%)** | 129² `H*` reads with analytic gradients; a few heights more near borders | Visual | **Yes.** Step 1 drops the gradient and lets the paint pass take slope from the heights. Step 2 moves the heights too. |
| Mesh heights | `ChunkSamples::sample` (`chunk.rs`) | 2.4% (2.2%) | 65² `H*` reads | Visual, except canonical Fine chunks drawn as TIN | **Yes for non-canonical chunks.** Step 1 shares the paint grid; step 2 moves it. |
| Ground paint: layer stack, weights | `procgena_terrain_bevy::gpu_ground`, `ground_weights.wgsl` | Already on the GPU | Per texel | Visual | **Built.** Parity: within one byte per channel against the CPU painter (Metal readback test). |
| TIN: probes | `tin/probe.rs::sample_heights`, `tin/refine.rs` | ~0% (TIN builds take milliseconds) | 65² Fine `H*` reads on the 1 m lattice, rounded to mm | **Canonical** | **No.** The rounding to mm makes CPU/GPU parity a threshold problem (§14.2). |
| TIN: refinement, CDT | `tin/refine.rs`, `tin/cdt.rs` (spade) | ~0% | Sequential topology changes, robust predicates | Canonical | **No** (TIN design §13). CT8 stays research. |
| CT6: GPU error probes | (not built) | — | Dense probes | Canonical selection | **Not now.** The probes are about 0% of CPU. A GPU proposal changes which probe wins unless a margin keeps the CPU's decision independent of it. Revisit only with a fixed-point `H*`. |
| Render products: normals, stand-ins, AABBs, skirts | `procgena_terrain_bevy` (`ChunkSamples::stand_in`, mesh build) | In "stream bookkeeping" 2.0% (0.8%) | Per vertex | Visual | **Yes, as part of step 2.** Normals by differences. Stand-ins take a sub-rectangle of the parent's height texture, as paint's `part` already does. AABBs come from a tiny async readback of min and max, or a conservative bound. |
| CT7: GPU-resident derived products from the TIN | (not built) | ~0% | Per vertex | Visual | **Later.** It reuses step 2's resident buffers. There is no load win. |

## Step 1: heights-only sites, slope on the GPU (GPU2b)

- **The CPU, per chunk:**
  - One `GroundGrid` at `SampleDetail::Height`, at the same world points
    the stream reads today, plus a one-texel apron: 131² instead of
    129² + 65².
  - The mesh takes every other interior point, which are the same world
    positions, so there is no extra read.
  - Water, road tags and relief height still come from the CPU site.
- **The GPU paint pass:**
  - It reads the height grid and computes slope by central differences
    over the apron.
  - Every chunk evaluates its shared edge texels from the same world
    samples, so neighbours agree at seams.
  - Retained grids keep working, so a material edit repaints without
    reading.
- **Parity:**
  - This is visual only, and `gpu_progress.md` asks that the
    differences' error be measured before they replace analytic slopes.
    Measure the slope error against the analytic gradient on the shipped
    presets, at every level, near rivers, roads and borders.
  - Measure how far material boundaries move, in texels.
  - Proposed acceptance (question 1): the 95th-percentile slope error
    under 1°, and no material boundary moving more than one texel except
    at creases (bank and road edges), which are shown in an image.
- **The CPU painter:**
  - It stays the reference and the fallback (devices without compute,
    custom and noise painters, more than eight materials).
  - Its slope comes either from the same differences, so both paths
    agree, or from the analytic gradient as today (question 2).
- **What it does not cover:** stacks at `Measures` or `Labels` still take
  the CPU path. Today that is custom stacks only, not the standard
  terrain stack.
- **Effort:** small to medium.
  - `procgena_terrain`: a heights-plus-apron grid, and the mesh taken from
    it.
  - `procgena_terrain_bevy`: the shader's site layout and slope in
    `ground_weights.wgsl`, the stream's single read, and tests.
- **What survives into step 2:** the slope-by-differences shader is
  exactly what step 2 needs, and the CPU heights path stays as the
  fallback. Nothing is thrown away.

## Step 2: `H*` on the GPU for drawn non-canonical ground (GPU3)

**Scope.** These chunks take their heights and paint sites from a GPU
kernel:

- every chunk at level 1 and up;
- every chunk drawn at the Rough or Mid tier (both provisional by D6);
- the paint grid of level-0 Fine chunks, whose mesh is the CPU TIN.

The CPU keeps `H*` for:

- TIN probes;
- region stages;
- `Terrain::query`, placement, masks and labels;
- devices without compute.

**Kernel structure.**

- **Buffers that stay resident per region**, built once after the
  region's tier build:
  - relief and E parameters;
  - the segment index;
  - the agreed and own edge samples and fade edges;
  - river, pond and road lines with their sections and profiles;
  - gully parameters.
- **Per chunk:** the ring of nearby graph sites and the up-to-N regions
  it touches.
- **Dispatches** are batched over many chunks. Each point writes its
  height, plus water level and road tags for paint, to a resident storage
  texture.
- **Drawing:** the mesh is a flat grid displaced in the vertex shader from
  that texture, with normals by differences. Stand-ins are
  sub-rectangles of a parent texture.
- **Coordinates:**
  - Chunk origins go in as integer metres (two i32 words where worlds
    exceed 2³¹ mm), with chunk-local f32 positions.
  - Feature geometry is stored relative to its region's origin.
  - Lattice cells are integer, so the hash matches the CPU bit for bit.
  - Shared edge points are computed from integer grid positions, never
    accumulated, so neighbouring GPU chunks agree exactly.
- **The batch seam:** `WorldTerrain::sample_batch` already groups a
  batch's points by owner region. The GPU path is a second backend
  behind it for visual products, chosen by the stream (as `gpu_ground` is
  today).

**Proposed stages, each landing on its own with parity tests:**

| Stage | Contents | Replaces | Notes |
|---|---|---|---|
| GPU3a | Relief, warp, E and owner lookup, without the border or fade. This is GPU1's kernel. | GPU1 | It already serves the studio's relief preview (TE2). Drawn terrain switches only after GPU3b, since without the bend it would not match the CPU near borders. |
| GPU3b | The pull kernel (border bend and convergence fade, including blend fan-out) | — | The df64 decision is made here, from measured error. |
| GPU3c | Rivers, ponds, roads, gullies; water level and road tags for paint | — | The first point at which drawn ground can switch. |
| GPU3d | Resident displacement, normals, stand-ins, AABBs; the stream switches drawn non-canonical chunks to GPU3 | TE3b for drawing | Retained height textures replace retained `GroundGrid`s for repaint. |

**Parity (visual products; TIN design §13.1, §13.6 and §14.10).**

- **Against the CPU `H*`:** |Δh| ≤ 1 cm at every sampled point over the
  shipped presets and fixtures, including large and negative world
  coordinates (question 5).
  - Far copies rest on `H*` (D6b), so drawn far ground must not visibly
    float under them.
  - A level-1 cell is 2 m, so 1 cm is far below a pixel.
- **Seams:** exact between GPU chunks at the same level (same kernel,
  same inputs).
- **Between GPU and CPU chunks** (a TIN chunk beside a GPU level-1
  chunk): the existing level skirts and stand-ins cover the gap, as they
  do between levels today.
- **Paint:** the weights test stays within one byte per channel against
  the CPU painter fed with CPU heights.
- **Never canonical:** no query, placement or plan reads a GPU height.
  `gpu_progress.md` already records "Terrain queries and planning never
  use GPU readback", and that becomes a test.

**Readback and latency.**

- **No height readback** for drawing.
- **Chunk bounds for culling:** either a two-float min and max per chunk
  read back asynchronously (a few bytes, a frame or two late, with a
  conservative bound from the region's E and relief amplitude until it
  arrives), or the conservative bound alone.
- **Upload:** region buffers once per region tier build, about 100 KB to
  1 MB each (to measure).
- **Latency:** a chunk is drawable one frame after its dispatch. This is
  bounded by a per-frame compute budget (question 6), so terrain never
  starves the renderer.
- **Main thread:** step 2 removes the CPU mesh build from swaps, which
  should help the 20 ms swap frames in the load profile.

**Effort:** large. About 2,500 lines of f64 Rust maths go to WGSL across
`noise`, `relief`, `warp`, `elevation`, `border`, `converge`, `terrain::base`,
`carve`, `road/section` and `gully`. Each needs the same stencil of parity
tests. Risk sits mostly in GPU3b.

## Step 3: the CPU floor (Border-pull cost)

After steps 1 and 2 the CPU still reads `H*` for:

- the TIN's 270 k probes per view;
- the river trace (25 m grid) and hydrology;
- road grades (8 m and 16 m grids);
- earthworks (a few hundred heights per station);
- every query and placement.

Each of those reads is 78–88% border fade and pulls (TE6, Border-pull
cost). On one thread this sets how quickly the near ground becomes
canonical and how quickly a region is planned. The candidates are already
listed in ROADMAP:

- fewer border pieces;
- cheaper far-piece integrals;
- a neighbour's relief only where its blend share is above zero;
- analytic pull gradients (a query at `Gradient`, and the TIN's slope
  checks).

All of them must keep `H*` bitwise identical, or be reported as changed
results, which is allowed in this early repo (memory: results may
change). This step is not GPU work, but without it the GPU plan leaves
the canonical path where it is.

## What stays on the CPU, and why

- **Canonical `H*` and everything that reads it:** plans, rivers,
  hydrology, roads, TIN probes and refinement, CDT, labels, masks,
  placement and queries. D6 requires bitwise or exactly quantised parity.
  WGSL gives neither for floats, and the canonical maths is f64 with
  `std` transcendentals.
- **Sequential searches:** the river walk, road A*, fitting and the
  negotiation. They are small and irregular.
- **The CPU paths for visual products:** these are kept as reference and
  fallback (no compute, custom and noise painters, more than eight
  materials, servers and tests).

## Canonical stages on the GPU: the only route

If canonical GPU stages are ever wanted (the long-term "GPU everywhere"
goal), the route is a fixed-point `H*` profile:

- integer hash and lattice (already integer);
- fixed-point noise polynomials;
- polynomial approximations of `exp`, `pow` and `atan2`, defined bit for
  bit and run identically on both sides;
- integer pull moments.

The same profile would give D6a's cross-platform identity ("pinned
maths"). The two should be one design decision, made when D6a's later
milestone opens, and not before. The border integrals with power-16
weights are the hard part. Until then CT6, CT8 and any canonical GPU
`H*` stay research.

## Proposed ROADMAP changes (for the coordinator, not edited here)

- **GPU2:** add the step **GPU2b**, "Heights-only ground sites, slope by
  differences on the GPU", with step 1's done-when: the measured slope
  error accepted, one CPU read per chunk, and the load profile re-run.
- **GPU3:** add a block with GPU3a–d as above. Its done-when: parity
  tests at each stage; drawn non-canonical chunks read no `H*` on the
  CPU; a cold mixed view on one thread measured against this plan's
  estimate.
- **GPU1:** mark it as becoming GPU3a. The studio preview (TE2) uses the
  kernel without the border.
- **Border-pull cost:** schedule it as the CPU floor for canonical reads.
- **CT6, CT7, CT8, RBG:** unchanged, behind the fixed-point decision
  (CT6, CT8) or with no load win (RBG, CT7). Add a line linking this plan.
- **New dependencies:** none needed (wgpu and naga come through Bevy).

## Open questions (for the coordinator; a recommendation for each)

1. **Is slope by differences good enough for paint?** Recommended: yes,
   with the acceptance in step 1 (95th-percentile error under 1°,
   boundaries within one texel away from creases), measured on the
   shipped presets before switching.
2. **Should the CPU painter also take slope by differences, so the CPU
   and GPU paths agree exactly in their input?** Recommended: yes. It
   also removes the gradient from the CPU fallback, and analytic
   gradients remain for queries.
3. **Should canonical stages stay on the CPU until a fixed-point `H*`
   designed together with D6a's pinned maths?** Recommended: yes. Don't
   start CT6, CT8 or a canonical GPU `H*` before that decision.
4. **Should paint use GPU-derived landform labels and curvature, and
   differ slightly from placement's canonical labels?** Recommended: not
   yet. Stacks at `Labels` keep the CPU path, and the question is revisited
   when a shipped stack paints by label.
5. **Is 1 cm the height tolerance for drawn GPU ground against `H*`?**
   Recommended: yes for levels 1 and up. Measure first and tighten if
   it's cheap.
6. **What compute budget per frame should terrain have?** Recommended:
   2 ms a frame in the game and uncapped while the studio loads with
   nothing to draw.
7. **What order of work?** Recommended: load-profile #0 (deadlock), then
   step 1 with #2 and #3, then step 3, then GPU3a–d. Step 1 is the
   biggest win per unit of effort. Step 3 bounds canonical latency on one
   thread. Step 2 is the long, large finish.
