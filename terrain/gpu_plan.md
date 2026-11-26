# Terrain on the GPU: a TIN-centred plan

> **Paint parts removed 2026-11-26** (the user): ground paint, its sites,
> weights and GPU2 no longer exist; the terrain draws flat grey. Mentions
> of paint below are history. Biome ground paint from labels is ROADMAP
> BP.

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

- **What a load costs now is `H*` reads for drawn chunks, and the worker
  waiting.**
  - [load_profile.md](load_profile.md) ("Post-TB") measures the default
    view on one thread with features off:
    - 1.3–2.0 s to the first picture and 2.6–5.9 s to settle;
    - the worker busy for only 0.9–1.8 s of that, and idle 19–74% of the
      load, waiting for frames to queue reads.
  - Mesh samples are all of the chunk work. Inside each height, the owner
    lookup is 37–53% and relief noise 25–40%. The TIN is 6–27%.
  - **The biggest load levers are now on the CPU, not the GPU:**
    - keep the worker fed (the profile's #2);
    - resolve region ownership once per chunk (#3).
  - The GPU steps below remove reads and per-vertex CPU work from
    drawing, and give BP its inputs. On their own they shorten a load by
    little.
- **Most of those reads are for far ground, which has no TIN.**
  - The default view has 252 chunks: 64 at level 0 and 188 at levels 1–4.
  - TINs exist only for Fine level-0 chunks.
  - A regular chunk reads 4,489 points at level 0 and 6,061 at coarser
    levels (see "Read counts"). That is about 1.43 M reads a view, **80%
    of them for far ground**.
  - With TIN streaming on, each level-0 chunk is read regular first and
    then again for its TIN (316 reads for 252 chunks). Once that second
    read is dropped, level 0 costs 240–3,520 reads a chunk plus normals,
    and far ground becomes 79–98% of the reads.
  - At the measured busy time, about 0.9 µs a read (1.3 s for the
    1.43 M reads of the mixed world), the far reads are about 1 s of the
    worker.
- **Far ground stays on regular `H*` grids on the CPU, read on a bare
  65 × 65 lattice.**
  - Everything else about drawing moves to the GPU: normals, seams
    between levels, LOD, bounding boxes and the Biome painter's inputs.
  - These are built from the resident ground: the TIN's integer
    centimetres at level 0, and the bare grids beyond.
  - The ring of samples outside each chunk and the side rows for finer
    neighbours go. Most of their values are held by neighbours already
    resident. The rest, at level seams, are interpolated (see "Seams
    between levels").
  - Coarse TINs built on the CPU, or `H*` itself on the GPU, could serve
    far ground instead. Which is open question 3: measure one coarse TIN
    build first.
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
| 1 | **Bare-lattice far ground, with seams and normals from resident neighbours.** Regular chunks read only their 65 × 65 vertices. Normals, and the vertices a coarse side adds for a finer neighbour, come from the neighbours' resident heights in a GPU pass. Samples at level seams that no chunk holds are interpolated. | **Reads a chunk:** 6,061 → 4,225 at levels 1–4 (−30%), or 4,749 (−22%) if the half-step row inside a coarse side stays an `H*` read; 4,489 → 4,225 at level 0. **A default view:** 1.43 M → 1.06 M reads (−26%), about 0.33 s of the worker's 1.3 s at 0.9 µs a read. **Remeshing:** a level change needs no CPU work. | Medium: a stream change plus one compute pass | New (proposed **GPU-T1**); the far-ground part of [TE7](../ROADMAP.md#te7-intermediate-terrain-detail-levels) |
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
- **Gains are in reads.** A full load already takes seconds, and the
  worker is idle for much of it. So read savings show up as load time only
  once the worker is kept fed.

## Frame time: drawing the terrain

The steps above are about load. This section is about drawing, ranked by
expected frame-time saving.

**What the demo measured** (bare terrain, no Scatters, 7 × 7 tiles,
2560 × 1440, load average about 70):

- the render schedule took 29 ms a frame with shadows and 20.5 ms
  without;
- the main schedule took about 9–10 ms.

So shadows cost about 8.5 ms, and drawing the terrain alone takes most of
a 16.7 ms frame.

[TE7](../ROADMAP.md#te7-intermediate-terrain-detail-levels) measured the
regular mesh at its 7 px target: 2.2 M triangles over 252 chunks, each
chunk 8,192 triangles plus skirts.

**How triangles turn into time.** TE7's viewer, at 720p and a load
average of about 20, gives the only slope there is:

- 0.56 M triangles: about 7 ms a frame;
- 2.2 M triangles: 10–13 ms.

That is roughly 2–4 ms per million triangles in the main pass. Each
shadow cascade a chunk lands in draws its triangles again. Both
measurements were taken under load, so read them as orders of magnitude.

### Ranked

| # | Change | Expected frame-time saving | Effort | ROADMAP |
|---|---|---|---|---|
| 1 | **Cheaper terrain shadows.** First cap what terrain casts (a, b), then replace its shadow pass (c). | **Up to about 8.5 ms**, the whole shadow cost, if the terrain's casting is most of it (to measure, E1) | (a, b) small; (c) medium | New (proposed **GPU-T4**) |
| 2 | **Fewer triangles: the TIN at level 0, then coarse TINs beyond.** | **Level 0:** about 0.5 M of 2.2 M triangles go (−23%), about 1–2 ms in the main pass. More comes off in the shadow pass, where level-0 chunks are the casters. **With coarse TINs at every level:** 2.2 M → roughly 0.02–0.3 M (252 chunks × 24–678 triangles, plus the split sides), about 4–9 ms in the main pass, before shadows. | Level 0 is built (`--tin`). Coarse TINs: medium to large (open question 3). | [CT5](../ROADMAP.md#ct5-products-and-streaming) (streaming TIN by default) and its "TIN LODs" |
| 3 | **GPU-driven terrain draws.** All resident chunks are culled and drawn on the GPU, per camera and per shadow cascade, with geomorphing in the vertex shader (steps 1 and 2's buffers). | Smaller: the main thread's per-chunk draw preparation, for 252 chunks times 1 view plus 4 cascades, perhaps 1–2 ms. Also fewer casters per cascade than whole-chunk culling gives. To measure. | Medium: the shared instanced renderer in `procgena_core_bevy` already culls on the GPU | Steps 1 and 2 (proposed GPU-T1/T2) |

**Measure before building.** At a load average of 70 these are estimates.
Three cheap experiments split the costs.

- **E1:** put `NotShadowCaster` on every terrain chunk. Terrain still
  receives shadows, so this splits the shadow cost into terrain casting
  and shadow receiving and filtering.
- **E2:** run the same view with `--tin` and without. Only level 0
  changes, so this shows how much triangles cost the main pass and the
  shadow pass.
- **E3:** run with `--cell-pixels 14` against 7. This gives about 4× fewer
  triangles at every level, so it is the same question across the whole
  view.

### 1. Terrain shadows

The demo's sun uses 4 cascades out to 220 m, the first ending at 12 m
(`sky.rs`). A directional cascade draws every caster between it and the
light, not only those in view. A whole chunk lands in every cascade its
box touches. So the near 8,192-triangle chunks are drawn up to four times
more each frame.

- **(a) Fewer casters.**
  - Chunks beyond the last cascade, or at levels ≥ 2, get
    `NotShadowCaster` (`bevy_light` 0.20.1).
  - Shadow receiving and its filtering
    (`ShadowFilteringMethod::Hardware2x2` against the default `Gaussian`)
    are a separate knob, and E1 says which half matters.
- **(b) A coarse shadow proxy.**
  - Chunks cast from a cheaper mesh than they draw: the level-0 TIN (0.5 m
    tolerance, about 47× fewer triangles in CT5's measure) or a coarser
    level.
  - Shadows at that tolerance look the same.
  - It needs a shadow-only entity per chunk. Whether Bevy 0.20 can draw a
    mesh into shadow maps without drawing it in the main pass is **to
    verify**.
- **(c) Terrain self-shadow from heights, on the GPU.**
  - Terrain stops casting into the shadow maps altogether.
  - A compute pass marches the resident heights from steps 1 and 2
    toward the sun and writes a horizon or shadow texture. It reruns when
    the sun moves or a chunk loads, not every frame.
  - The terrain material samples that texture, and still receives shadows
    from Scatters through the shadow maps.
  - For ridges to shade trees in valleys, Scatter materials sample the same
    texture: one shared shader module in `procgena_core_bevy`.
  - It saves the terrain's whole share of the shadow pass, and gives
    terrain shadows past 220 m for free.
  - Recommended once E1 confirms that casting is the cost.

### 2. Triangles from the TIN

- **Level 0 (built):**
  - CT5 measured 557,056 → 11,832 triangles over 64 chunks, skirts and
    split sides included, and the whole 600 m view from 1.19 M to
    0.64 M.
  - In TE7's 252-chunk view, level 0 is about 0.52 M of the 2.2 M
    triangles.
  - CT-A's `GENERAL` TINs hold 24 (plains), 102 (hills) and 678
    (mountains) triangles a chunk, against 8,192 regular.
- **Coarser levels (open question 3):**
  - A coarse TIN per level-`k` chunk would hold its error fixed on screen,
    at the open-land tolerance × 2^k (0.5 m at level 0, 8 m at level 4),
    which is what TE7's 7 px rule does for cells.
  - If such chunks hold triangles in the range level-0 TINs do, the 188 far
    chunks drop from about 1.54 M triangles to roughly 5 k–130 k.
  - That is the biggest triangle saving available, and it also cuts the
    reads (step 1). It is not measured: build one level and count.
- **Larger triangles also help the fragment side.** At 14 px a cell on a
  1440p screen, regular cells are small, so GPU shading is wasted on the
  thin triangles' edge pixels. A TIN's large flat facets waste less.

### 3. What moves to the GPU for drawing

- **Per frame:**
  - culling of every resident chunk per camera and per cascade;
  - one indirect draw per material;
  - geomorphing in the vertex shader.
- **Per chunk load or level change:**
  - normals (steps 1 and 2);
  - level seams (step 1);
  - the shadow texture (1c);
  - the Biome painter's inputs (step 3).
- **Stays on the CPU:**
  - choosing levels by size on screen (`ChunkDetail`);
  - deciding what to load;
  - bounding boxes (exact from the integer heights).

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
- **A default view, with TIN at level 0, once its second read is
  dropped:** far ground is 1.14 M of 1.16–1.45 M reads → 0.79 M of
  0.81–1.02 M. Today the TIN chunks also pay their regular 4,489 reads.
- **What neighbours hold:**
  - A level-0 chunk's ring is held by the neighbours of the same level.
  - A coarse chunk's side rows for a finer neighbour are held as follows:
    - the row half a step outside is the finer chunk's own lattice;
    - the row on the side is the finer chunk's edge;
    - the row half a step inside lies between the coarse chunk's own
      rows, so no chunk holds it.
  - A finer chunk's ring beyond a coarser neighbour also falls between
    that neighbour's rows.
  - Those samples are interpolated in step 1. Keeping the inside row as
    `H*` reads costs 4 × 131 = 524 reads a coarse chunk.

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
    from its buffer. A corner vertex also adds the faces of the three
    chunks around it, the diagonal one included. Both chunks hold the
    same border vertices (edge contracts), so they compute the same
    normal.
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
  - **Normals at a level seam:** the coarse side's vertices take the finer
    chunk's edge normals. The finer chunk takes its differences across the
    seam from the coarse chunk's surface, interpolated. So both sides
    shade alike.
    - These normals differ slightly from today's `H*` differences.
    - The exact alternative keeps the coarse side's inside row as `H*`
      reads (see "Read counts").
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

- **After TB** (the load profile's `--tin` runs): the TIN is 6% (mixed),
  11% (hills) and 27% (mountains) of the features-off worker. That is
  about 54 ms, 0.2 s and 0.5 s over 64 chunks, or roughly 1, 3 and 8 ms a
  chunk, reads included.
- **The second read:** each level-0 chunk is also read regular before its
  TIN (316 reads for 252 chunks). Dropping that is a CPU stream fix, not a
  GPU one.
- **Before TB,** CT-A timed builds at 7–69 ms a chunk under a load average
  of 40–100, with its reads at about 1.5 µs. There, outside plains, the
  non-read work (lines, triangulation, scoring) weighed as much as the
  reads or more.
- **The split between reads and non-read work after TB is not
  measured.**

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
- **GPU-T4 (new): cheaper terrain shadows.** Run E1–E3 first, then cap
  the casters (1a, 1b), then terrain self-shadow from heights (1c).
  - **Done when:** the shadow pass's terrain share is measured before
    and after on a quiet machine.
- **GPU-T3 (new, under BP):** the painter's terrain inputs as textures
  with aprons, once BP lists them.
- **CT6:** recast as "batched refinement, CPU reference first; GPU scoring
  only on a measured need" (question 5).
- **GPU1** (the relief preview) **and the GPU `H*` idea:** park them until
  `H*` settles (question 3).
- **CT8:** unchanged, as research.
- **No new dependencies:** wgpu and naga come through Bevy.
- **Echoes these changes would touch** (found by searching every `.md`
  for this plan's old step names):
  - `load_profile.md`, which names "step 1a of `terrain/gpu_plan.md`",
    GPU2 and GPU3;
  - ROADMAP's GPU2 status, which names GPU2a and links this plan;
  - `docs/coordination/away-decisions-2026-11-26.md`, which names step 1a;
  - the TIN design doc's milestones 8 and 10 (CT6, CT7).

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
   - Recommended: build step 1 on regular `H*` grids now, and measure one
     coarse TIN build at levels 1–4 before choosing.
   - Step 1's GPU normals, seams and LOD carry over to coarse TINs as
     resident buffers (as in step 2).
   - **Coarse TINs** (CT5's "TIN LODs") are the source most in line with
     "using the TIN".
     - **For:** after TB a level-0 TIN costs about 1–8 ms a chunk with its
       reads, against about 4 ms for a bare 65² grid. CT-A reads about 12×
       fewer points on plains.
     - **Against, not measured:** at coarser lattice steps the same
       tolerance leaves fewer cells coarse, so reads on hills and mountains
       may not fall.
     - **Also:** a coarse TIN's tolerance would be set per level, not the
       world's canonical one.
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
