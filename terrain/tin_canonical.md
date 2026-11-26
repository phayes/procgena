# The TIN as the only canonical ground: feasibility (2026-11-26)

The user's tentative decision is that `H*` becomes only the generator the TIN
samples, and everything reads the TIN (D6b option a). This study measures
whether building the TIN is cheap enough, and sketches the design. Nothing in
the engine changes yet. Paths are relative to
`terrain/crates/procgena_terrain/src/`.

## Bottom line

**Go, with conditions.** Building canonical TIN is cheap enough near the
camera and out to about 2 km, but not out to the place ranges the code
computes today.

- **Per chunk, on one thread:** 0.5–0.7 ms on plains, 3.3–4.0 ms on rolling
  hills and 6.6–12 ms on mountains, at `GENERAL`. `LOW_POLY` is cheaper on
  plains and hills (0.5 and 2.0 ms) but barely on mountains (10 ms).
- **About half of that is not `H*`.** Refinement scoring and triangulation
  take 53–60% of a build, so they are the first target, ahead of the GPU.
- **The place ranges in the code are about 10× the documented "about 2 km".**
  Tree Scatters cull, and therefore place, out to about 20 km (pines
  19.9 km, oak and maple canopy 21.1 km). Logs reach 3.3–4.6 km and boulders
  1 km. A 20 km canonical disc is about 307,000 chunks: 34–62 minutes cold
  on mountains and 2–5 GB, which is not feasible. A 2 km disc is about 3,100
  chunks: 20–37 s cold on mountains, about 2 s on plains, and 20–55 MB.

**Conditions:**

1. **Settle the place range first** (a design question for the user). Either:
   - cap the canonical range near 2 km, and let long-range Scatters place
     beyond it from a coarser canonical product (below); or
   - shorten the draw ranges themselves. The cull at 0.2% of the screen's
     height is about 1.4 px at 720p, which may be further than intended.
2. **Halve the non-`H*` work before scaling out** (estimated 1.5–2× on
   hills and mountains): score each triangle against a per-chunk list of
   sampled points instead of scanning its bounding box of lattice cells;
   rescore only the changed faces' samples.
3. **Build ahead on the single worker, and keep a session or disk cache of
   chunks.** A chunk is a pure function of the world seed, its options, its
   key and the tolerance, and its hash already names it.
4. **Store a compact, GPU-ready form:** 8 bytes a vertex and 6 a triangle
   (below), so a 2 km mountain disc is about 20 MB.

## Measurements

Master at `11d77a4` (TE6's `H*` speed-ups and TB's cheap borders landed).
`tin_cost`, seed 1, an 8 × 8 block round the origin, regions already built,
fastest of 3 builds per chunk. The machine was shared: load average 35–60
during the run (89 shortly after), so times are upper bounds. A quieter run
earlier the same day gave 0.53 / 3.28 / 6.61 ms at `GENERAL`. `H*` time is
measured inside the read calls themselves.

| Level | Preset | Triangles | Reads (cached edges) | Build | of which `H*` | Normals |
|---|---|---|---|---|---|---|
| `GENERAL` | plains | 24 | 356 (243) | 0.72 ms | 0.31 ms (43%) | 33 reads, 0.04 ms |
| | rolling hills | 98 | 2,341 (2,228) | 4.02 ms | 1.89 ms (47%) | 60 reads, 0.06 ms |
| | mountains | 624 | 3,623 (3,509) | 12.19 ms | 4.89 ms (40%) | 180 reads, 0.42 ms |
| `LOW_POLY` | plains | 5 | 310 (196) | 0.48 ms | 0.26 ms | 16 reads |
| | rolling hills | 45 | 1,070 (956) | 1.96 ms | 0.94 ms | 34 reads |
| | mountains | 277 | 3,162 (3,048) | 10.17 ms | 4.86 ms | 111 reads |

- **`H*` reads** cost about 0.9 µs on plains and 1.35 µs on mountains each
  (TE6 and TB included), against 2–4 µs earlier.
- **Low-poly mountains still read most of the lattice:** the bend limit
  scales with the open-land tolerance, but mountain relief bends past it
  everywhere. Their build is dominated by `H*` and scoring, not triangles.
- **Region builds are not included.** They come on top whenever new
  regions enter the range.

## Place ranges (from the code)

Each Scatter's draw range is the cull distance of its largest copy at High
quality (`DistanceChain::DEFAULT`, the cull at 0.002 of the screen's
height). Its place range is the largest draw range of itself and every
Scatter below it (ARCHITECTURE, "Loading by chunk"), so in practice it is the
trees'.

| Ecotope | Scatter | Draw range |
|---|---|---|
| pine forest floor | pines (tree) | 19,876 m |
| | logs (deadwood) | 3,320 m |
| | boulders (stone) | 1,000 m (fixed) |
| | fly agarics, porcini (mushroom) | short (about 30 m, from ARCHITECTURE; needs built specimens to compute) |
| | undergrowth (plant) | placed where drawn |
| oak–maple forest | canopy (tree) | 21,086 m |
| | bushes (tree) | 10,757 m |
| | logs | 4,647 m |
| | stumps | 598 m |
| | boulders | 1,000 m |

ARCHITECTURE's "about 2 km for big trees" is out by about 10×.

## What it costs

Level-0 chunks are 64 m, so a disc of radius R holds πR² / 4096 chunks.
Each new metre of travel adds about 2R / 4096 chunks. Costs use the build
times above (quiet to loaded), one thread, regions already built.

| Radius | Chunks | Plains | Rolling hills | Mountains |
|---|---|---|---|---|
| 1 km | 767 | 0.4–0.6 s | 2.5–3.1 s | 5–9 s |
| 2 km | 3,068 | 1.6–2.2 s | 10–12 s | 20–37 s |
| 4.6 km (logs) | 16,230 | 9–12 s | 53–65 s | 1.8–3.3 min |
| 20 km (trees) | 306,800 | 2.7–3.7 min | 17–21 min | 34–62 min |

**Incremental cost, at 2 km** (0.98 new chunks a metre). Walking is 1.4 m/s
and running 5 m/s (demo settings); gliding is taken as 15 m/s, since the
demo hasn't set a glider speed:

| Speed | New chunks a second | Hills | Mountains |
|---|---|---|---|
| walk 1.4 m/s | 1.4 | 5–6 ms/s | 9–17 ms/s |
| run 5 m/s | 4.9 | 16–20 ms/s | 32–60 ms/s |
| glide 15 m/s | 14.7 | 48–59 ms/s (5–6% of a thread) | 97–179 ms/s (10–18%) |

At 20 km everything is 10× more. Gliding over mountains would then need
1–1.8 s of building every second, which is not sustainable.

**Memory per chunk.** Today's `TinChunk` stores 24 bytes a vertex
(`i64` x and z, `i32` h, padded), 12 bytes a triangle, about 40 bytes a
constraint, plus the query index. That comes to about 4 KB for a hills
chunk and 18 KB for a mountains chunk.

A compact canonical form fits in integer centimetres within a chunk:
- x and z as `u16` offsets (0–6,400 cm), and h as `i32` centimetres: 8 bytes
  a vertex;
- `u16` triangle indices: 6 bytes a triangle.

That is about 1 KB (hills) and 6.4 KB (mountains), with constraints kept
apart for the products that need them.

| Disc | Today's form, hills / mountains | Compact form, hills / mountains |
|---|---|---|
| 2 km | 13 MB / 55 MB | 3 MB / 20 MB |
| 20 km | 1.3 GB / 5.5 GB | 0.3 GB / 2 GB |

## Ways to make it affordable

1. **Cheaper scoring and triangulation (no change to the mesh).** The scorer
   walks every lattice cell in a triangle's bounding box at every insertion
   (`tin/refine.rs`, `Scorer::score`). Kept per triangle, a list of the
   sampled points inside it, split when the triangle splits, makes a
   rescore cost the samples of the new faces only. The canonical output
   can stay bitwise identical: same samples, same ties.
2. **Probe and contract reuse.** This is already built: `ContractCache`
   shares each edge's 65 reads between its two chunks, and contracts hand
   their samples to the inside. Next would be sharing a corner region's
   coarse cells, which saves little.
3. **Batched refinement across chunks** (design §13.2) only pays on the GPU.
   On one thread it saves nothing.
4. **Building ahead.** The single worker builds the ring the camera is
   heading into. The worker is free once the near ground settles (seconds
   after a load), and walking at 2 km costs under 2% of a thread.
5. **A session or disk cache.** A chunk is a pure function of (world options,
   key, tolerance, numeric profile), which is its hash's inputs. Compact
   chunks at 1–6 KB make a disk cache of a whole explored world cheap. The
   numeric profile must be in the cache key, since a profile change must
   invalidate it.
6. **A coarser canonical product for long-range Scatters (design question).**
   One canonical tolerance gives one canonical mesh per level-0 chunk.
   Placement beyond about 2 km could read a canonical coarse level: either
   - the same TIN builder on larger chunks (128 m, 256 m and so on) at a
     looser tolerance, keyed and hashed like level 0; or
   - a canonical simplification of level-0 TINs, which needs the level-0
     chunks built first and so saves nothing.

   Either way, a copy placed from the coarse level must keep its address and
   identity when the camera comes near. Since a copy's identity never
   includes its rest height (D6b), a copy can be placed from coarse ground
   far away and settle onto level 0 up close. Its spot, slope and masks must
   not change, though, so the coarse level's slope and labels must agree
   with level 0 within the masks' thresholds, or masks must read only the
   coarse level beyond some distance for every Scatter. That conflicts
   with "placement does not depend on distance" and is the user's call.

## What reads the TIN, and how each would run on the GPU

All canonical data is integer centimetres (`SamePlatformV2`). The GPU path
must reproduce the CPU's integer decisions exactly wherever they are
canonical; floats are fine for drawing only.

| Consumer | Reads | CPU today | GPU later |
|---|---|---|---|
| **Placement: height** | barycentric height of the triangle under a spot | `TinIndex::sample` (bin grid, exact `i64` orientation tests) | One compute thread per candidate. Point-in-triangle with integer `i32`/`i64`-emulated orientation, then `i64` barycentric interpolation rounded to cm, so it is bitwise like the CPU. Bins as a per-chunk triangle list uploaded once. |
| **Placement: slope** | smoothed vertex normals, interpolated, not raw faces | not built | Normals precomputed per chunk on the CPU (below) and uploaded as `i16` or snorm per vertex. Slope is a mask threshold, so it must be canonical: store normals quantized (for example 1/32767 units) and compare in integers. |
| **Labels and measures** | stencils of height, slope and curvature over the TIN | read `H*` | Rasterize each chunk's TIN to a height texture of integer cm at a fixed spacing (1 m), then run the existing stencils on it. Rasterizing is integer barycentrics, so it is bitwise. Curvature from texture differences, all in integers. |
| **Biome paint segmentation** | as labels, plus water and feature constraints | — | The same height raster plus a constraint-ID raster (river, shore, road) from the TIN's kept edges. |
| **Collision** | the canonical triangles | `TinCollider` | CPU physics (Rapier or Avian); nothing to move. |
| **Drawing** | near: the TIN; far: a simplified mesh derived from it | `render_mesh`, regular chunks far | Upload compact chunks; simplify per draw level with an error-bounded edge-collapse on the GPU (visual only, needs no parity), or draw the coarse canonical level above. |

## Normals, pure TIN

Under this decision normals read no `H*`.

- **Interior vertices** take the area-weighted average of their triangles'
  normals. This is how they are computed since `34e10a7`.
- **Border vertices** need the triangles of both chunks, so the two sides
  agree exactly. A chunk can't see its neighbour's triangles without
  building it, so carry the information in the edge contract: for each
  contract vertex, the summed (unnormalized) face normal of the triangles
  on each side, in integer units. Each chunk fills in its own side when
  built; the vertex's normal is the sum of both, once both are known.
  - Until the neighbour is built, a border vertex has only one side. Draw
    it from that side, so lighting changes slightly when the neighbour
    arrives. Placement slope near a border must wait for the neighbour,
    which is fine, since placement needs the neighbouring chunk built
    anyway for its reach.
  - Alternatively, close the gap from the contract alone: each side's sum
    is a function of that side's triangles, which depend on the whole
    chunk. A border vertex's normal could instead come only from the
    contract polyline and the two adjacent rows of samples, which both
    sides can read the same way. That needs `H*` reads only on the border,
    which is the rule `34e10a7` already uses, and it seams exactly.
  - **Recommended:** keep `34e10a7`'s rule (border vertices from `H*`
    differences) while `H*` is still the generator. Switch to
    two-sided sums in the contract only if `H*` stops being readable at
    run time.

## What this needs from the user

1. **The place range:** cap canonical TIN near 2 km with a coarse canonical
   level beyond it for long-range Scatters, or shorten draw ranges (the
   0.002 cull).
2. **Whether placement may read a coarse canonical level** beyond a fixed
   distance, accepting that masks there come from the coarse ground for
   every Scatter.
3. **Whether `LOW_POLY` worlds** should also loosen the bend limit, so
   low-poly mountains get cheaper (today they read 87% of `GENERAL`'s
   heights).
