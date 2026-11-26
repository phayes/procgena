# The TIN as the only canonical ground: costs (2026-11-26)

> **Decided since (the user, 2026-11-26):** every chunk level is a canonical
> TIN level, and each Scatter places on one level
> ([terrain_lod.md](terrain_lod.md), ROADMAP CT-T). This note measures the
> earlier proposal of a single canonical level 0 out to the place range;
> its per-chunk costs and place ranges still hold, and its disc figures
> show why one level alone wasn't enough.

Input for a conversation with the user, who was weighing making `H*` only
the generator the TIN samples, with everything reading the TIN (D6b option
a). This note only measures. Paths are relative to
`terrain/crates/procgena_terrain/`.

## Bottom line

- **Per level-0 chunk, on one thread:** 0.5–0.7 ms on plains, 3.3–4.0 ms on
  rolling hills and 6.6–12 ms on mountains, at `GENERAL`. `LOW_POLY` costs
  0.5, 2.0 and 10 ms. About half of each build is not `H*` but refinement
  scoring and triangulation.
- **The place ranges in the code are about 10× the documented "about 2 km":**
  tree Scatters place out to about 20 km, logs 3.3–4.6 km, boulders 1 km.
- **A 2 km canonical disc** (about 3,100 chunks) takes about 2 s cold on
  plains, 10–12 s on hills and 20–37 s on mountains, and 20–55 MB. **A
  20 km disc** (about 307,000 chunks) takes 34–62 min on mountains and
  2–5.5 GB.
- **Keeping up while travelling at 2 km** costs under 2% of a thread
  walking, and 5–18% gliding at 15 m/s. At 20 km it is 10× that, which
  gliding over mountains can't sustain.

## Build cost per chunk

Master at `11d77a4`, after TE6's `H*` speed-ups and TB's cheap borders.
Measured with `--example tin_cost`: seed 1, an 8 × 8 block round the origin,
regions already built, the fastest of 3 builds of each chunk. Time spent
reading `H*` is measured inside the reads. The machine was shared, at a
load average of 35–60 (89 shortly after), so times are upper bounds. A
quieter run earlier that day gave 0.53, 3.28 and 6.61 ms at `GENERAL`.

| Level | Preset | Triangles | `H*` reads (edges shared) | Build | of which `H*` |
|---|---|---|---|---|---|
| `GENERAL` | plains | 24 | 356 (243) | 0.72 ms | 0.31 ms (43%) |
| | rolling hills | 98 | 2,341 (2,228) | 4.02 ms | 1.89 ms (47%) |
| | mountains | 624 | 3,623 (3,509) | 12.19 ms | 4.89 ms (40%) |
| `LOW_POLY` | plains | 5 | 310 (196) | 0.48 ms | 0.26 ms |
| | rolling hills | 45 | 1,070 (956) | 1.96 ms | 0.94 ms |
| | mountains | 277 | 3,162 (3,048) | 10.17 ms | 4.86 ms |

Shading normals add 33–180 reads and under 0.5 ms a chunk. Region builds
are not included; they come on top when new regions enter the range.

## Place ranges

A Scatter's draw range is where the default distance chain culls its
largest copy at High quality, at 0.002 of the screen's height, about 1.4 px
at 720p. Its place range is the largest draw range of itself and every
Scatter below it, so in practice the trees'.

| Ecotope | Scatter | Draw range |
|---|---|---|
| pine forest floor | pines (tree) | 19,876 m |
| | logs (deadwood) | 3,320 m |
| | boulders (stone) | 1,000 m (fixed) |
| | mushrooms | about 30 m (ARCHITECTURE; needs built specimens to compute) |
| | undergrowth (plant) | placed where drawn |
| oak–maple forest | canopy (tree) | 21,086 m |
| | bushes (tree) | 10,757 m |
| | logs | 4,647 m |
| | stumps | 598 m |
| | boulders | 1,000 m |

## Cold disc

A disc of radius R holds πR² / 4096 level-0 chunks. Build times are the
quiet and loaded figures above, on one thread. Memory is today's
`TinChunk`: about 4 KB a hills chunk and 18 KB a mountains chunk. A compact
form (8 bytes a vertex, 6 bytes a triangle) needs about 1 and 6.4 KB.

| Radius | Chunks | Plains | Hills | Mountains | Memory, hills / mountains (compact) |
|---|---|---|---|---|---|
| 1 km | 767 | 0.4–0.6 s | 2.5–3.1 s | 5–9 s | 3 / 14 MB (1 / 5 MB) |
| 2 km | 3,068 | 1.6–2.2 s | 10–12 s | 20–37 s | 13 / 55 MB (3 / 20 MB) |
| 4.6 km (logs) | 16,230 | 9–12 s | 53–65 s | 1.8–3.3 min | 70 / 290 MB (16 / 100 MB) |
| 20 km (trees) | 306,800 | 2.7–3.7 min | 17–21 min | 34–62 min | 1.3 / 5.5 GB (0.3 / 2 GB) |

## Cost per metre of travel

Each metre adds about 2R / 4096 chunks: 0.98 at 2 km, 9.8 at 20 km.
Walking is 1.4 m/s (demo setting); gliding is taken as 15 m/s, since the
demo has no glider speed yet.

| Radius | Speed | New chunks a second | Hills | Mountains |
|---|---|---|---|---|
| 2 km | walk 1.4 m/s | 1.4 | 5–6 ms/s | 9–17 ms/s |
| 2 km | glide 15 m/s | 14.7 | 48–59 ms/s | 97–179 ms/s |
| 20 km | walk 1.4 m/s | 14 | 46–56 ms/s | 92–171 ms/s |
| 20 km | glide 15 m/s | 147 | 0.5–0.6 s/s | 1.0–1.8 s/s |

## Notes from the user's later decisions

- **Paint:** Biome paint will use soft landform class weights on each TIN
  vertex, plus shading per fragment, with no dense raster. Raw shape at
  vertices is a maybe; wetness is dropped.
- **If building the TIN is too slow,** the fallback is an integer `H*`, with
  both `H*` sampling and the TIN build on the GPU.
