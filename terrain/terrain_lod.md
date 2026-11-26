# Terrain levels: every chunk level is a canonical TIN

Decided by the user, 2026-11-26. This is the design; ROADMAP's CT-T block
schedules it, and it depends on TF (terrain features as proper features).
It revises D6b: the TIN is the ground at every distance, and `H*` is only
the generator the TINs sample. The defaults marked "coordinator's
default" were chosen by the coordinator on the user's instruction
(2026-11-26), tuned for performance, for the user to review.

## Bottom line

- **Every chunk level is a canonical TIN level.** Level `L` covers
  64 m × 2^L, as the chunk grid does today, extended to levels 5, 6, 7 and
  on as needed. It samples a 65 × 65 lattice at 1 m × 2^L and follows
  `H*` within the world's base tolerance × 2^L (strict doubling, the
  user's start). Each chunk at each level is a pure function of the world
  settings and its `ChunkKey`, with edge contracts per level.
- **Each Scatter places on exactly one level**, its *placement level*.
  Spots and slope come from that level's TIN at every distance, so
  placement does not depend on distance. Near the player, rest height
  settles onto level 0, the finest ground (D6b's settle).
- **Each level has a build radius around the camera**, separate from what
  is drawn. Levels 0–3 are built canonically out to 150 m, 500 m, 1.5 km
  and 4 km; a Scatter's place range is clamped to its level's radius.
  Beyond 4 km, trees become the far field's forest tint. Levels 4 and up
  are drawn only.
- **Each feature type declares two levels:** an edge level (the highest
  level where its lines are kept as TIN edges) and a carve level (the
  highest level where its effect on the ground is sampled). This replaces
  the Rough, Mid and Fine feature tiers. Keep-off rules such as "no trees
  on roads" read the feature lines themselves, so they hold at every
  placement level.
- **It's affordable:**
  - Cost per chunk is about flat across levels: at `GENERAL` × 2^L, a
    level-6 chunk (4 km wide) costs about what a level-0 chunk does, 0.4 ms
    on plains and 4.5–7.7 ms on mountains, on one thread.
  - The default build radii come to about 365 chunks: about 0.15 s cold on
    plains and 1.6–2.8 s on mountains.
  - Frame-time E4 found that triangle count, not chunk count, drives render
    time.

## The model

| Level | Chunk | Lattice step | Tolerance (`GENERAL`) | Max edge |
|---|---|---|---|---|
| 0 | 64 m | 1 m | 0.5 m | 32 m |
| 1 | 128 m | 2 m | 1 m | 64 m |
| 2 | 256 m | 4 m | 2 m | 128 m |
| 3 | 512 m | 8 m | 4 m | 256 m |
| 4 | 1,024 m | 16 m | 8 m | 512 m |
| 5 | 2,048 m | 32 m | 16 m | 1,024 m |
| 6 | 4,096 m | 64 m | 32 m | 2,048 m |

**Rules:**

- **Pure and canonical.** A level-`L` chunk is a function of the world
  settings (including `WorldTerrainOptions::tin`) and its key. It uses the
  same integer-centimetre profile, hashing and edge contracts as level 0
  today. The water band (rivers' and ponds' banks) keeps its own tolerance
  on every level that carries water edges.
- **No cross-level refinement constraint by default.** A level-`L` chunk
  doesn't have to contain its children's vertices or edges. Whether a
  parent must refine its children at shared edges is an open choice
  (below).
- **One level per Scatter.** A Scatter reads only its placement level's
  TIN, for spots, height and slope (normals smoothed from the TIN's own
  faces, see `tin/product.rs`). Masks pass down an Ecotope as today; a
  mask doesn't depend on the ground once placed, so Scatters on different
  levels still mask each other.
- **Rest height settles.** A copy's identity never includes its rest
  height. Near the player, where level 0 is built, a copy rests on level
  0, so a tree placed on level 3 stands on the finest ground up close.
- **`H*` is only the generator.** Nothing reads `H*` but the TIN builder
  and the tests that measure it.

## Default placement levels and build radii (coordinator's default)

Placement level is a Scatter setting; build radius is a world setting per
level.

| Level | Chunk | Build radius | Default placement users | Chunks in the disc |
|---|---|---|---|---|
| 0 | 64 m | 150 m | grass, mushrooms, undergrowth, sticks, small stones; also colliders | 17 |
| 1 | 128 m | 500 m | boulders, logs, other dead wood | 48 |
| 2 | 256 m | 1.5 km | bushes, small trees | 108 |
| 3 | 512 m | 4 km | big trees (the tree cap) | 192 |
| 4+ | 1 km and up | none for placement | drawn only | — |
| **Total** | | | | **about 365** |

- **Clamping.** A Scatter's place range (the largest draw range of itself
  and every Scatter it masks) is clamped to its level's build radius.
  Today's computed ranges are much longer: trees about 20 km (the cull at
  0.002 of the screen's height, about 1.4 px at 720p), logs 3.3–4.6 km,
  bushes 10.8 km. Beyond 4 km, trees become the far field's (FF) forest
  tint.
- **Draw selection is separate.** TE7 picks which level to draw where, by
  size on screen, and draws that level's TIN. Seams between drawn levels
  are a drawing matter only (skirts or transitions), never a change to a
  canonical chunk. Levels 4 and up are built only where they are drawn.

**Worked counts against alternatives.** A disc of radius R holds about
πR² / (4,096 × 4^L) chunks at level `L`. Cold times use the measured cost
per chunk below (plains 0.3–0.43 ms, mountains 4.5–7.7 ms):

| Build radii | L0 | L1 | L2 | L3 or L4 (trees) | Total | Cold, plains | Cold, mountains |
|---|---|---|---|---|---|---|---|
| **Default:** 150 m, 500 m, 1.5 km, 4 km (trees on L3) | 17 | 48 | 108 | 192 | **365** | about 0.15 s | 1.6–2.8 s |
| Trees capped at 2 km (on L4), bushes 2 km, logs 1 km, L0 200 m | 31 | 192 | 192 | 12 | 427 | about 0.17 s | 1.9–3.3 s |
| Uncapped, today's computed ranges (L0 200 m, logs 3.3–4.6 km, bushes 10.8 km, trees 20–21 km on L4) | 31 | 2,090–4,060 | 5,590 | 1,200–1,330 | 8,900–11,000 | about 4 s | 40–85 s |

The 2 km row puts trees on level 4 and gives logs a wider level-1 radius,
so it ends up slightly more chunks than the 4 km default. A single
canonical level 0 out to 20 km would be about 307,000 chunks.

## Cost

Measured with `--example tin_levels`. It builds a level-`L` chunk as a
level-0 chunk over the world shrunk by 2^L, which samples the same points
within the same scaled tolerance. Setup: seed 1, 3 × 3 chunks round the
origin, regions already built, fastest of 3 builds, one thread, load
average about 30.

| Level | Preset | Triangles | Reads | Build | Error p50 / p95 / max |
|---|---|---|---|---|---|
| 0 | plains | 26 | 352 | 0.30 ms | 0.03 / 0.11 / 0.34 m |
| | rolling hills | 98 | 2,244 | 1.68 ms | 0.07 / 0.24 / 0.54 m |
| | mountains | 468 | 4,034 | 4.47 ms | 0.13 / 0.36 / 0.92 m |
| 3 | plains | 27 | 335 | 0.38 ms | 0.21 / 0.70 / 1.58 m |
| | rolling hills | 142 | 3,584 | 3.61 ms | 0.71 / 2.21 / 5.18 m |
| | mountains | 1,208 | 3,775 | 6.35 ms | 0.93 / 3.24 / 12.4 m |
| 6 | plains | 24 | 309 | 0.43 ms | 1.0 / 3.2 / 6.3 m |
| | rolling hills | 41 | 2,465 | 2.61 ms | 5.7 / 15.5 / 36.6 m |
| | mountains | 1,349 | 3,900 | 7.67 ms | 7.7 / 27.9 / 96 m |

- **Cost per chunk stays about flat:** plains go from 0.3 to 0.4 ms, hills
  from 1.7 to 2.6–3.6 ms, mountains from 4.5 to 7.7 ms. Reads stay under
  about 4,100, and triangles under the vertex budget.
- **Error stays within each level's tolerance at p95** (0.5 m, 4 m and
  32 m). The worst case exceeds it on rough ground, by up to 3×, because a
  level can't see relief finer than its lattice step. Placement on a
  coarse level accepts coarse ground.
- **Reads cost a little more on coarse levels:** 0.34 µs at level 0 and
  0.8–0.9 µs at level 6, since a batch spans more regions.
- **Travel.** At radius R, level `L` adds about 2R / (4,096 × 4^L) chunks
  per metre: with the default radii, 0.21 chunks a metre over levels 0–3.
  - Gliding at 15 m/s, that is about 3.2 chunks a second: about 15–25 ms a
    second on mountains (2% of a thread) and 1 ms on plains.
  - Walking at 1.4 m/s, it is a tenth of that.
  - Uncapped, gliding adds about 14 chunks a second (about 110 ms a second
    on mountains).
- **Memory.** Today's form takes 4–18 KB a chunk; a compact form (8 bytes a
  vertex, 6 a triangle) takes 1–8 KB. The default 365 chunks are 1.5–6.6 MB
  today.
- Region builds come on top whenever new regions enter a radius.

## Features per level

Each feature type declares its own levels, as part of TF (terrain features
as proper features):

- **Edge level:** the highest level where its lines are kept as TIN edges.
- **Carve level:** the highest level where its effect on the ground is
  sampled. Above it, the feature doesn't change that level's ground.

| Feature | Edge level | Carve level | Why |
|---|---|---|---|
| Roads | 1 | 2 | Narrow, so invisible beyond level 2. |
| Rivers and ponds | 2 | all levels | Valleys up to 300 m wide shape the far landscape. |

(Coordinator's defaults.)

- **This replaces the Rough, Mid and Fine feature tiers.** A chunk above a
  feature's carve level doesn't carry it, so a coarse level never waits
  for a costly feature, which was what the tiers were for.
- **No promotion.** Today a chunk read at a cheaper tier can stand for a
  later one. With levels, every drawn chunk is its own TIN (see "No
  promotion" under the stream).
- **Keep-off from vector data.** Rules such as "no trees on roads", "keep
  off rivers" and "clear of a road's edge by `d`" read the feature lines
  (`lines_within`) directly, never the TIN. So they hold at every
  placement level, including levels above the feature's edge level.
- **Until TF lands,** roads and rivers stay embedded in the terrain plan
  and carve, and stay off by default in the built-in presets. Level-0
  TINs keep river and pond lines as edges, as today. Above level 0 a TIN
  keeps no feature line and holds no ground to the water's tolerance, but
  samples the carved ground, so a river or road shows only as far as the
  level's lattice and tolerance catch it. Feature edges, the water band
  and dense sampling near roads and water above level 0 wait for TF.

## The stream: what is built, what is drawn, what copies rest on

The terrain stream (`procgena_terrain_bevy`) keeps two sets of chunks
around the viewers: the chunks it **builds**, and among them the chunks it
**draws**. Each one is that level's canonical TIN; there are no regular
heightfield chunks and no feature tiers.

### Which chunks are built

**Rule:** a chunk at level `L` is built when it is drawn, or when it is
within level `L`'s build radius of a viewer. The built set is the union of
the two.

- **Drawn:** TE7's size-on-screen selection (`ChunkSelection::around`),
  unchanged.
- **Within the build radius:** the chunk's nearest point is within
  `radius(L) × factor` of a viewer, measured on the ground (x and z;
  height above the ground doesn't shrink a placement range).
  - `factor` is the same quality distance scale the draw selection uses,
    so a lower quality builds less, as it draws less. It is 1 at the
    default quality. (Decided by the user 2026-11-26.)
  - Levels 4 and up have no radius, so they are built only where drawn.
- **Hysteresis:** a chunk built for its radius stays built until it is
  10% past the radius, like the draw selection's switching bands, so
  walking along a radius doesn't rebuild chunks.

Drawn chunks outside every radius must be built too, because drawing a
chunk means drawing its TIN. With the default detail, level 0 is drawn out
to about 256 m while its radius is 150 m, so those level-0 chunks are built
for drawing only.

**Counts.** Default detail, 4 km view, viewer 2–120 m above the ground,
counting every chunk a disc touches:

| Level | Drawn | Within the radius | Built (union) |
|---|---|---|---|
| 0 | 64 | 32 (150 m) | 64 |
| 1 | 48 | 60 (500 m) | 64 |
| 2 | 48 | 132 (1.5 km) | 132 |
| 3 | 48 | 224 (4 km) | 224 |
| 4 | 44 | — | 44 |
| **Total** | **252** | **448** | **528** |

- **Why these are higher than "about 365" above:** that figure is the
  discs' area divided by a chunk's area. A chunk counts here when the disc
  touches it, which comes to 448. Drawing adds 80 more.
- **A viewer 600 m up** draws 156 chunks, with no level 0 or 1 drawn, and
  builds 492.
- **An 8 km view** adds level 5: 300 drawn, 576 built.
- **Cost:** the build radii set most of it. Of the 528 chunks, only 80
  are built for drawing alone. At the measured cost per chunk, a cold
  view is about 0.2 s on plains and 2.4–4.1 s on mountains, one thread.

**Stand-ins while chunks are built.** Drawn chunks never overlap, so a
coarse chunk can't stand in for just one of its children. The rule:

- A chunk is drawn in place of its four children until every child can be
  drawn: built, failed (drawn as a hole), or itself replaced the same way
  by its own children.
- Coarse chunks build about as fast as fine ones but cover 4^k times the
  area. So a cold view first builds the top-level chunks covering it, and
  then refines.
- This replaces today's coarse-first Rough reads and
  `ChunkSamples::stand_in`.

**The cost of stand-ins.**
- The top-level cover holds 60 chunks with the default detail. 44 of them
  are drawn anyway, so 16 more are built as stand-ins only, near the
  viewer, and dropped once everything under them is drawn. That makes 544
  builds for a cold view.
- The levels in between near the viewer are within their build radii, so
  they cost nothing extra.

**Order of building:**

1. The top-level cover, nearest first, for the first picture.
2. Drawn chunks, together with those of their ancestors within a build
   radius, coarse and near first: in order of distance divided by chunk
   size.
3. Then the remaining chunks within a radius, in the same order.

**Mixing old and new ground.** After the source changes, the old chunks
stay drawn until the new top-level cover is built. From then on only new
ground is drawn, and old and new are never drawn side by side.

**Seams between drawn levels** are skirts.
- **Depth:** each chunk's skirt is deep enough to cover the largest height
  difference, along their shared edge, against the chunks drawn beside it
  now. A stand-in can sit beside chunks more than one level finer.
- **Depends only on what is drawn now,** never on load history, so a
  settled view always meshes the same.
- **No cross-level constraints,** and no transition strips (`Transitions`
  no longer changes a TIN mesh).

### No promotion

Every drawn chunk is its own canonical TIN: no parent is drawn in place of
its children once they are built. (Decided by the user 2026-11-26.)

- **What promotion would save:** only builds for drawing. Of the 528
  chunks above, that is the 80 drawn chunks outside every radius. A chunk
  within its radius is built anyway, since its Scatters place on it.
- **What it would cost:** canonicality, since a parent drawn for its
  children isn't bit-exact with them, and a maximum-error report from
  every TIN build.
- **A possible later optimisation:** if measurements show the draw-only
  builds matter, a parent at level `L` could stand for its children in
  drawing when two things hold. No feature whose edge or carve level is
  `L − 1` reaches its footprint (read from `lines_within`), and its TIN
  is within `tolerance(L − 1)` at every sample it took.

### Rest height at mid range

**The problem:** take a level-3 tree at 300 m, where the drawn ground is
level 1. The tree's spot and slope come from level 3, but level 3 differs
from level 1 by p95 2.2 m on hills, and up to 12 m on mountains. Rested on
level 3, the tree floats or sinks by that much.

**Rule:** a copy rests on the TIN drawn under its spot now. It keeps its
placement level's spot and slope (its identity), and takes its height from
the drawn chunk that holds the spot.

- **Where the copy changes:**
  - when the drawn chunk under it changes, at a level switch or when a
    stand-in is replaced;
  - re-resting only takes point queries on the new chunk.
- **Why it never pops on its own:** it moves only in the frame the ground
  under it moves, so the two pop together.
- **No ambiguity at seams:** drawn chunks tile without overlap, so the
  rest height at a seam is never in doubt.
- **Near the player:** level 0 is drawn out to about 256 m, so this is
  D6b's settle onto level 0, with no separate rule.

**For the stream:**
- `CanonicalGround` answers rest heights from the drawn chunks.
- Placement levels' TINs are kept apart, for spots and slope.
- When drawn chunks change, the stream reports the changed footprints,
  and the Scatter renderers re-rest the copies inside them.

**Why not rest on the finest level built at the spot:** with the default
radii that is usually the drawn level, since drawn chunks are always
built. A high viewer, though, draws coarser ground than the radii build,
and the copy would then rest on ground that isn't drawn.

## Open choices

1. **The build radii and the tree cap**: defaults above, for the user to
   review. Until placement moves to the TIN (stage 3), the radii are a
   setting of the stream's plugin (`TerrainPlugin::with_build_radii`).
   Where they live after that, likely `WorldTerrainOptions` since Scatters
   clamp to them, is decided then.
2. **Edge and carve levels** per feature type: defaults above; later kinds
   (gullies, stamps, paths, cliffs, lakes) declare their own.
3. **Labels per placement level.** A label reads its level's TIN with a
   stencil in that level's units, so a "cliff" at level 3 is a big cliff.
   Ecotope and Biome rules need to say which level's labels they mean, or
   labels are defined at one level for all.
4. **Whether a parent must refine its children at shared edges**, so a
   coarser level's edges always lie on finer ground (exact containment,
   like a quadtree hierarchy). This is not required for placement; it would
   help drawing and geomorphing. Coordinator's default, for the user's
   review: no cross-level refinement constraint. Each level's edges are
   its own (`ChunkEdgeKey` carries the level), and seams between drawn
   levels are a drawing matter (skirts).
5. **GPU placement (GP) reading the level TINs:** upload each level's
   compact chunks, sample height by integer barycentrics in cm, and take
   slope from quantized smoothed normals, so the CPU and the GPU agree bit
   for bit.
6. **The doubling rule** (tolerance × 2^L) is a start. A per-level curve,
   or a looser bend limit for low-poly worlds, may follow measurement.

## How today's code migrates

- **TE7 chunk levels:** unchanged as the draw selection (`ChunkLevels`,
  size on screen). Each drawn level-`L` chunk becomes that level's
  canonical TIN instead of a regular heightfield chunk.
- **Feature tiers** (`FeatureTier`, `WorldTerrain::terrain_tier`, `Rough`,
  `Mid`, `Fine`): superseded by each feature type's edge and carve levels
  (TF). A region still plans its features once; a level simply omits the
  kinds above their levels. Until then, `ChunkSource` reads by
  `FeatureLevel`, and a world maps levels onto today's tiers through
  `TierLevels`. `level_changes` and `level_change_known` go, with
  promotion (see "No promotion").
- **CT-A** (adaptive sampling, contract cache): applies to every level as
  is; the lattice step and tolerance scale with the level.
- **CT5** (products and streaming): `TinIndex`, `GroundSurface`,
  `render_mesh` and `TinCollider` work per level. The stream builds each
  level to its build radius and draws each level's TIN. `CanonicalGround`
  holds level 0 for collision and rest height, and each placement level
  for its Scatters.
- **D6b:** revised from "`H*` decides spots, masks, slope and labels; the
  TIN gives collision and near rest heights" to "the TIN is the ground;
  `H*` is only its generator".
