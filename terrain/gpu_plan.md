# Terrain on the GPU: a TIN-centred plan

A plan only: it contains no engine code. Status lives in
[ROADMAP.md](../ROADMAP.md), and the milestones named here link there.

**Scope (the user, 2026-11-26):**

- **Roads and rivers are off.** They come back later (see "Later: roads
  and rivers").
- **Terrain paints nothing.** Ground paint will be decided at the Biome
  layer from labels and segmentation data, apart from placement
  (milestone BP). This plan says what that painter will need from the
  ground, not how it paints.
- **`H*` stays as it is:** the analytic surface, on the CPU, with cheap
  borders ([TB](../ROADMAP.md#tb-cheap-borders)), adaptive TIN sampling
  in centimetres ([CT-A](../ROADMAP.md#ct-a-adaptive-tin-sampling)) and
  free disabled features ([TO](../ROADMAP.md#to-disabled-features-cost-nothing)).
  The user may revisit `H*` later.
- **Most of the work should move to the GPU, using the TIN.**

## Bottom line

- **What a load costs now is `H*` reads for drawn chunks.**
  - The coordinator measures the default world, on one thread with
    features off, at about 1.2–1.7 s to the first picture and 2.4–4.8 s to
    settle.
  - Nothing else in a chunk read is left of any size since paint went.
    The split by stage since TB and the paint removal is not yet measured
    ([load_profile.md](load_profile.md) predates both).
- **Most of those reads are for far ground, which has no TIN.**
  - The default view has 252 chunks: 64 at level 0 and 188 at levels 1–4.
  - TINs exist only for Fine level-0 chunks.
  - A regular chunk reads 4,489 points at level 0 and 6,061 at coarser
    levels (see "Read counts"). That is about 1.43 M reads a view, **80%
    of them for far ground**.
  - With TIN streaming on, level 0 drops to 240–3,520 reads a chunk plus
    normals, and far ground becomes 79–98% of the reads.
  - At about 1.5 µs a read (CT-A's edge-contract timing), the far reads
    alone are about 1.7 s on one thread.
- **Far ground stays on regular `H*` grids on the CPU, read on a bare
  65 × 65 lattice.**
  - Everything else about drawing moves to the GPU: normals, seams
    between levels, LOD, bounding boxes and the Biome painter's inputs.
  - These are built from the resident ground: the TIN's integer
    centimetres at level 0, and the bare grids beyond.
  - The ring of samples outside each chunk and the side rows for finer
    neighbours go. Their values come from the neighbours already
    resident.
  - Two other sources for far ground are open question 3: coarse TINs
    built on the CPU, and `H*` itself on the GPU.
- **Canonical work stays on the CPU (D6b):**
  - `H*`;
  - every TIN build (probes, refinement, triangulation);
  - placement, masks, slope and labels for placement;
  - ground queries (`TinIndex`, `GroundSurface`);
  - colliders.
- **TIN building does not move to the GPU now.**
  - Its reads are `H*`.
  - Its scoring can be exact in integers on the GPU, but only with 64-bit
    integers or their emulation.
  - It runs as a sequential greedy loop over an f64 triangulation.
  - Its non-read work is a CPU cost to measure and cut first (see "TIN
    building").

### The top three steps

| # | Step | Gain, in its own unit | Effort | ROADMAP |
|---|---|---|---|---|
| 1 | **Bare-lattice far ground, with seams and normals from resident neighbours.** Regular chunks read only their 65 × 65 vertices. Normals, and the vertices a coarse side adds for a finer neighbour, come from the neighbours' resident heights in a GPU pass. | **Reads a chunk:** 6,061 → 4,225 at levels 1–4 (−30%) and 4,489 → 4,225 at level 0. **A default view:** 1.43 M → 1.06 M reads (−26%), about 0.55 s of one thread at 1.5 µs a read. **Remeshing:** a level change needs no CPU work. | Medium: a stream change plus one compute pass | New (proposed **GPU-T1**); the far-ground part of [TE7](../ROADMAP.md#te7-intermediate-terrain-detail-levels) |
| 2 | **Resident TIN buffers and TIN normals on the GPU.** Every loaded TIN chunk is uploaded once, as integer-centimetre vertices and triangles. Vertex normals are area-weighted from the chunk's own faces and its neighbours' faces across shared edges. Breaklines (`is_breakline`) get hard normals. | **Reads:** 4 `H*` reads a vertex are gone, 68 of a plains chunk's 310 and 1,344 of a mountain chunk's 4,864 (22–28%), with none kept even on borders. **Per view:** 4 k–86 k reads with TIN streaming on. | Small to medium | [CT5](../ROADMAP.md#ct5-products-and-streaming)'s ready follow-up and CT7 (proposed **GPU-T2**) |
| 3 | **The Biome painter's inputs from the resident ground, on the GPU.** Per texel: height, slope and aspect from the drawn faces, shape curvature at `SHAPE_STEP` (12 m), and height relative to the neighbourhood. All are textures with an apron, read from step 2's buffers at level 0 and step 1's grids beyond. | **BP gets every terrain input with no CPU reads.** The removed CPU painter paid 16,641 `H*` reads a chunk (129² sites), about 4.2 M a view, before its own measures. | Medium; it waits for BP to name its inputs | New, under BP (proposed **GPU-T3**) |

**Order of work:**

1. Steps 1 and 2 can run side by side. Step 1 touches regular chunks;
   step 2 touches TIN chunks.
2. Step 3 starts once BP names its inputs.
3. Then, if numbers ask for them:
   - a quiet-machine measure of the TIN build's non-read work (question
     5);
   - coarse TINs or GPU `H*` for far ground (question 3).

**Where the credit belongs:** CT5's follow-up already removes the
interior normal reads on the CPU, and step 1's read saving is a stream
change.

- **What the GPU adds:**
  - border normals and level seams with no `H*` at all;
  - refitting when levels change, at no CPU cost;
  - nothing per vertex left on the CPU's drawing path.
- **Gains are in reads.** A full load already takes seconds, so
  percentages of load time mislead. Time follows reads only while reads
  stay the cost, which the profiler is re-checking.

## Read counts

Reads of `H*` for one chunk, from the code (`ChunkSamples::sample`,
`tin::refine`, `TinChunk::shading_normals`) and the CT-A table.

| Chunk | Today | After steps 1 and 2 |
|---|---|---|
| Regular, level 0 | 67² = 4,489 (vertices plus a normal ring) | 65² = 4,225 |
| Regular, levels 1–4 | 4,489 + 4 sides × 3 rows × 131 = 6,061 | 4,225 |
| TIN, level 0, `GENERAL`, with the contract cache | 242 (plains), 2,220 (rolling hills), 3,520 (mountains) | Unchanged |
| TIN shading normals | 4 a vertex: 68 (plains) to 1,344 (mountains) | 0 |

- **A default view, regular:** 64 × 4,489 + 188 × 6,061 = 1.43 M reads →
  64 × 4,225 + 188 × 4,225 = 1.06 M.
- **A default view, with TIN at level 0:** far ground is 1.14 M of
  1.16–1.45 M reads → 0.79 M of 0.81–1.02 M.

## What runs on the GPU

Everything here is visual or derived. None of it decides spots, masks,
slope or labels for placement, so D6's bitwise gate does not bind it. Its
seams must still match exactly, which "Parity" covers.

### Resident ground buffers (steps 1 and 2)

- **TIN chunks:**
  - **Vertices:** x and z in centimetres from the chunk's corner, as
    `u16` (a level-0 chunk is 6,400 cm), and height as `i32`
    centimetres. Eight bytes a vertex.
  - **Triangles:** `u16` indices, since the vertex budget is 4,096 and the
    border adds a few hundred.
  - **Constraints:** the breakline edges, for hard normals.
- **Regular chunks:** a 65 × 65 grid of heights, as `f32` metres (what
  `H*` gives), with the chunk's level and corner.
- **An index of the resident chunks:** a small table from chunk key to
  buffer slot, with each side's neighbour and its level. It is rebuilt when
  chunks load or unload.
- **Upload once a chunk, no readback.** Bounding boxes come from the CPU,
  which already holds the integer heights and the grid, as an exact min
  and max.

### Normals

- **TIN chunks:**
  - **Interior vertices:** each vertex's normal is the area-weighted sum
    of its faces' normals.
  - **Border vertices:** they add the faces of the chunk across the edge,
    from its buffer. Both chunks hold the same border vertices (edge
    contracts), so they compute the same normal.
  - **Breakline vertices:** split by side, for hard shading along
    `is_breakline` edges.
  - **Order:** a gather per vertex over its faces in index order, never
    float atomics, so the result is the same every frame and on both
    sides of a seam.
- **Regular chunks:** central differences on the grid, as today. At a
  border, the sample one step beyond comes from the neighbour of the same
  level.
- **Where a neighbour is not loaded:** a one-sided difference for now,
  recomputed when the neighbour arrives (one dispatch).
- **Where a regular chunk meets a TIN chunk:** CT5 gives the regular
  border vertices a 1 m spacing. Their normals come from the TIN's faces,
  so the shading agrees across the seam.

### Seams between levels, and LOD

- **Seams:**
  - A coarse side that meets a finer chunk takes the finer chunk's edge
    vertices and heights from its buffer. This is what `fit_neighbour`
    does on the CPU for TIN seams, and what the side rows read from `H*`
    today.
  - The values are the finer chunk's own, so the seam is exact.
  - Skirts stay as the cover while a neighbour loads.
- **Geomorphing within a level band** ([TE7](../ROADMAP.md#te7-intermediate-terrain-detail-levels)):
  - a vertex shader blends towards the parent's surface, read from the
    parent's resident grid;
  - it needs no CPU read;
  - how geomorphing works on a TIN stays with CT8.
- **Stand-ins** (a parent's piece shown while a child loads): a
  sub-rectangle of the parent's resident grid, with no CPU read.

### Inputs for the Biome painter (step 3)

The painter decides paint later, at the Biome layer. From the terrain it
will need fields per texel over every drawn chunk.

- **Height**, and height relative to the mean over a radius, for valleys
  against ridges.
- **Slope and aspect** from the drawn faces. These are exact facets on
  TIN chunks, and grid differences beyond.
- **Shape:** curvature across the four directions at `SHAPE_STEP`
  (12 m at scale 1), the stencil `Landform` uses.
- **Landform classes** by the same thresholds as `label.rs`
  (`STEEP_SLOPE`, `CLIFF_SLOPE`, `SHAPE_CURVATURE`), as a segmentation
  texture.
- **Later, with roads and rivers:** distance to water and road lines, and
  height above water, from the feature lines as buffers.

Each is a texture per chunk with an apron as wide as its widest stencil
(12 m), filled from neighbours' buffers. They are drawing inputs only.
Queries keep reading `TinIndex` and `GroundSurface` on the CPU, and a
height texture made here is never canonical.

These labels come from the drawn ground, not from `H*`, so they can
disagree with the placement labels at the same spot:

- open land is held to 0.5 m in height with no slope bound;
- on a 32 m facet, the slope can differ from `H*`'s by a few degrees near
  a threshold.

That is question 2.

## What stays on the CPU, and why

| Stage | Why it stays |
|---|---|
| `H*`: relief, E, the border field, owner lookup | Canonical (D6b) and f64. WGSL has no `f64`, and the user may revisit `H*`, so a GPU twin now could be wasted (question 3). |
| TIN probes (`sample_heights`) | They are `H*` reads, rounded to centimetres. Rounding an approximate float is not deterministic (TIN design §14.2). |
| TIN refinement and triangulation | Canonical, sequential and f64 (spade). See "TIN building". |
| Placement, masks, slope and labels for placement (`Terrain::annotate`, `Measures`) | D6b: `H*` decides them. GPU Scatters keep slope as a texture from the tile's gradient grid (ARCHITECTURE). |
| Queries (`TinIndex`, `GroundSurface`, `ResidentTins`) | They answer in 40–250 ns on one thread already, and a readback would cost more. |
| Colliders (`TinCollider`) | Physics runs on the CPU, and the collider is the canonical triangles unchanged. |
| Bounding boxes | Exact from the integer heights, at no cost. |

## TIN building

**Where a build's time goes.**

- CT-A's timing (load average 40–100, so only indicative):
  - plains: 7 ms a chunk;
  - rolling hills: 11 ms;
  - mountains: 42–69 ms.
- Four edge contracts, 260 reads, take 0.4 ms, about 1.5 µs a read.
- So outside plains, the non-read work (lines, triangulation, scoring)
  weighs as much as the reads, and on mountains several times more.
- Over 64 level-0 chunks that is roughly 0.7–4.4 s of one thread. It
  sits outside today's 2.4–4.8 s only because TIN streaming is off by
  default.

**The reads cannot move.** They are `H*`.

**The scoring could, exactly, under the integer-centimetre profile.** In
`Scorer::score`, coordinates are centimetres from the chunk's corner, at
most 6,400:

- **Orientation and twice the area** (`orient`): at most 2 × 6,400² ≈
  8.2 × 10⁷. This fits `i32`.
- **Distance from the centre** (`off_centre`, nine times the squared
  distance): at most about 7.4 × 10⁸. This fits `i32`.
- **The interpolated height** (`flat`, weights times heights): about 27
  bits times the height's bits, up to 58 bits. The CPU uses `i128`. Taking
  heights relative to one corner shortens it, but not below 32 bits on
  steep ground. The floored quotient by the area decides which sample
  wins, so it must be exact.
- **64-bit integers in shaders:** WGSL itself has only 32-bit integers.
  wgpu offers `Features::SHADER_INT64`, which is native only: Vulkan, DX12
  with DXC, and Metal with MSL 2.3 or later (checked in wgpu-types 30.0.1,
  the version in `Cargo.lock`). It is not available on WebGPU. Without it,
  pairs of `u32` emulate 64-bit multiplies and the 64-by-32 division
  exactly, at several times the cost.
- **The f64 parts have exact integer forms:**
  - `limits` are already floored to whole centimetres;
  - the `max_edge` test can compare squared lengths in integers;
  - the slope test applies only near water, which is off.

**What blocks it is the loop, not the maths.**

- Refinement inserts one vertex at a time, the worst-scored triangle
  first, and rescores the new faces against spade's f64 triangulation.
- A GPU can score every triangle at once, but a round trip per insertion
  costs more than the scoring.
- It would pay only with batched refinement: insert every independent
  worst sample per round, in a fixed order.
- Batching changes the canonical meshes. That is allowed, since results
  may change at this stage, but it needs a CPU reference first.

**So:** measure the non-read share on a quiet machine. Cut it on the CPU
first (incremental rescoring, the triangulation's own cost). Consider GPU
scoring only if batched refinement exists and scoring is still a measured
cost (question 5). CT8's GPU triangulation stays research.

## Parity

These products are visual, but seams and level changes must not crack or
pop.

- **Integer centimetres become `f32` exactly.** A chunk-relative position
  is at most 6,400 × 2⁴ cm, and a height is well under 2²⁴ cm (167 km). So
  every GPU product starts from the same exact inputs on both sides of a
  seam.
- **Exact seams come from shared data, not shared maths.** Both sides read
  the same border vertices and heights from buffers. Neither recomputes
  them.
- **Normals and other gathers run in a fixed order,** with no float
  atomics, so a frame repeats the last.
- **Between the GPU and the CPU, a drawn product may differ in its last
  bits.** Nothing canonical reads a GPU product, so that is allowed.
  Tests hold normals within 10⁻⁵ of a CPU reference, and seams bit-exact
  on the GPU.

## Later: roads and rivers

Out of scope until the user brings them back. When they return:

- **The canonical work stays on the CPU:** the river trace, hydrology, and
  road search, fitting and sections.
- **RBG,** the integer road cost field, can match exactly, but roads cost
  almost nothing at load.
- **The TIN keeps water and road breaklines as constraints.** Step 2's hard
  normals and step 3's distance inputs read them as buffers.
- **Carving and gullies** reach the GPU only through the drawn ground, never
  as a GPU twin of `H*`.

## Proposed ROADMAP changes (for the coordinator; not edited here)

- **GPU2:** retire it with the paint removal, and give its "Done when" to
  BP.
- **GPU-T1 (new): bare-lattice far ground.**
  - Regular chunks read 65² points.
  - Normals and level seams come from resident neighbours on the GPU.
  - **Done when:** the read counts above are measured, there are no cracks
    or shading seams at any level change, and stand-ins read no `H*`.
- **GPU-T2 (new; CT5's follow-up together with CT7's first part):**
  resident TIN buffers, and normals from faces across chunk edges, with
  hard breaklines.
  - **Done when:** TIN chunks read no `H*` for normals, and the shading
    across TIN-to-TIN and TIN-to-regular seams matches.
- **GPU-T3 (new, under BP):** the painter's terrain inputs as textures
  with aprons, once BP lists them.
- **CT6:** recast as "batched refinement, CPU reference first; GPU scoring
  only on a measured need" (question 5).
- **GPU1** (the relief preview) **and the GPU `H*` idea:** park them until
  `H*` settles (question 3).
- **CT8:** unchanged, as research.
- **No new dependencies:** wgpu and naga come through Bevy.
- **Echoes these changes would touch:**
  - `load_profile.md`, which names "step 1a of `terrain/gpu_plan.md`" and
    GPU2;
  - ROADMAP's GPU2 status, which names GPU2a;
  - the TIN design doc's milestones 8 and 10.

## Open questions (for the coordinator; a recommendation for each)

1. **Do canonical stages stay on the CPU until a fixed-point `H*`?** That
   point would come with D6a's pinned maths.
   - Recommended: yes.
   - With `H*` kept as it is, nothing canonical moves.
   - CT6's GPU scoring and CT8 wait for a measured need or for that
     decision.
2. **May the Biome painter's labels come from the drawn ground,
   disagreeing a little with the placement labels near thresholds?**
   - Recommended: yes, for paint only. BP's acceptance should state the
     disagreement rate, measured.
   - Where paint must match placement exactly (for example, rocky paint
     under placed rocks), the CPU passes its placement masks as a texture,
     instead of the GPU reclassifying.
3. **What source should far ground use?**
   - Recommended: regular `H*` grids on the bare lattice (step 1).
   - **Coarse TINs** (CT5's "TIN LODs") would cut reads on gentle land,
     about 12× on plains at level 0. But:
     - at coarser lattice steps the same tolerance leaves fewer cells
       coarse;
     - each chunk adds the TIN build's non-read work, 7–69 ms today
       against about 6 ms of reads for a bare grid;
     - so measure that work first.
   - **`H*` on the GPU** for far ground (the old GPU3) would remove the
     far reads entirely. But it is a large f32 port of relief, E and the
     border field, and the user may revisit `H*`. Park it.
4. **What is the GPU compute budget?**
   - Recommended: 2 ms a frame in the game, uncapped while the studio
     loads.
   - Steps 1–3 run once per loaded chunk and per level change, and stay
     well inside it.
5. **Should the TIN build's non-read cost be measured on a quiet machine
   before TIN streaming becomes the default?**
   - Recommended: yes.
   - Cut it on the CPU first.
   - Consider GPU scoring only after a CPU batched refinement. If it comes
     to that, emulate 64-bit integers rather than require
     `SHADER_INT64`, so the code runs wherever wgpu does.
