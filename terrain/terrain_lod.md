# Terrain levels: every chunk level is a canonical TIN

Decided by the user, 2026-11-26. This is the design; ROADMAP's CT-T block
schedules it. It revises D6b: the TIN is the ground at every distance, and
`H*` is only the generator the TINs sample.

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
- **A level is built out to the longest place range of the Scatters on
  it.** That is separate from what is drawn: TE7 still picks the levels
  to draw by size on screen.
- **Features keep their lines up to a highest level per feature type.**
  This replaces the Rough, Mid and Fine feature tiers. Keep-off rules such
  as "no trees on roads" read the feature lines themselves, so they hold at
  every placement level.
- **It's affordable because cost per chunk is flat across levels.** At
  `GENERAL` × 2^L, a level-6 chunk (4 km wide) costs about what a level-0
  chunk does: 0.4 ms on plains and 4.5–7.7 ms on mountains, on one thread.
  So even an uncapped 20 km forest is a few thousand cheap builds, not the
  307,000 level-0 chunks a single canonical level would need. Frame-time
  E4 found that triangle count, not chunk count, drives render time.

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
  settings (including `WorldTerrainOptions::tin`) and its key, with the
  same integer-centimetre profile, hashing and edge contracts as level 0
  today. The water band (rivers' and ponds' banks) keeps its own
  tolerance on every level that carries water lines.
- **No cross-level refinement constraint by default.** A level-`L` chunk
  doesn't have to contain its children's vertices or edges. Whether a
  parent must refine its children at shared edges is an open choice
  (below).
- **One level per Scatter.** A Scatter reads only its placement level's
  TIN, for spots, height and slope (normals smoothed from the TIN's own
  faces, see `tin/product.rs`). Masks pass down an Ecotope as today; a
  mask is ground-independent once placed, so Scatters on different levels
  still mask each other.
- **Rest height settles.** A copy's identity never includes its rest
  height. Near the player, where level 0 is built, a copy rests on level
  0, so a tree placed on level 4 stands on the finest ground up close.
- **`H*` is only the generator.** Nothing reads `H*` but the TIN builder,
  and the tests that measure it.

## Default placement levels

A Scatter setting, with these defaults:

| Placement level | Scatters |
|---|---|
| 0 | grass and undergrowth (plant), mushrooms, small stones, sticks and twigs |
| 1 | boulders, logs and other dead wood |
| 2 | bushes and small trees |
| 3–4 | big trees |

## Build radius against draw selection

- **Build radius (canonical).** Level `L` is built out to the longest place
  range of the Scatters on it, and around any viewer that collides with or
  rests on it (level 0 near the player). A Scatter's place range is, as
  today, the largest draw range of itself and every Scatter it masks. That
  is computed per Scatter from the cull of its largest copy.
- **Draw selection (not canonical).** TE7 picks which level to draw where,
  by size on screen, and draws that level's TIN. Seams between drawn levels
  are a drawing matter only: skirts or transitions, never a change to any
  canonical chunk.
- **A cap on how far trees place.** Beyond it, the far field (FF) tints the
  ground instead. Today's code computes about 20 km for trees, from the
  cull at 0.002 of the screen's height (about 1.4 px at 720p); the cap is
  still to be chosen.

**Worked counts.** A disc of radius R holds about πR² / (4,096 × 4^L)
chunks at level `L`. Taking the pine and oak–maple Ecotopes' computed draw
ranges (`--example tin_cost` study, `terrain/tin_canonical.md`), and
assuming 200 m for level 0's undergrowth:

| Level | Longest place range on it | Uncapped chunks | With a 2 km tree and bush cap and a 1 km log cap |
|---|---|---|---|
| 0 | about 200 m (undergrowth, mushrooms) | 31 | 31 |
| 1 | 3.3–4.6 km (logs) | 2,090–4,060 | 192 (1 km) |
| 2 | 10.8 km (bushes) | 5,590 | 192 |
| 4 | 20–21 km (canopy trees) | 1,200–1,330 | 12 |
| **Total** | | **about 8,900–10,800** | **about 430** |

A single canonical level 0 out to 20 km would be about 307,000 chunks.

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

- **Cost per chunk stays about flat.** Plains go from 0.3 to 0.4 ms, hills
  from 1.7 to 2.6–3.6 ms, mountains from 4.5 to 7.7 ms. Reads stay under
  about 4,100, and triangles stay under the vertex budget.
- **Error stays within each level's tolerance at p95** (0.5 m, 4 m and
  32 m). The worst case exceeds it on rough ground, by up to 3×, because a
  level can't see relief finer than its lattice step. That is the price of
  sampling at 1 m × 2^L, and is why placement on a coarse level must
  accept coarse ground.
- **Reads cost a little more per read on coarse levels:** 0.34 µs at level
  0 and 0.8–0.9 µs at level 6, since a batch spans more regions.
- **Cold build, uncapped** (about 9,000–11,000 chunks): about 4 s on plains
  and 45–85 s on mountains, plus region builds. **Capped** as in the table
  (about 430 chunks): under 0.2 s on plains and 2–3 s on mountains.
- **Travel.** At radius R, level `L` adds about 2R / (4,096 × 4^L) chunks
  per metre. Gliding at 15 m/s, uncapped, adds about 14 chunks a second
  across all levels: level 1's 4.6 km logs about 8.4, level 2's bushes
  about 4.9, level 0 about 1.5, and level 4's trees about 0.6. That is about
  110 ms a second on mountains (11% of a thread) and 6 ms on plains.
  Capped as in the table, it is about 4 chunks a second: about 30 ms a
  second on mountains (3%).
- **Memory.** Today's form takes 4–18 KB a chunk; a compact form (8 bytes a
  vertex, 6 a triangle) takes 1–8 KB. About 10,000 chunks is 40–180 MB
  today, 10–80 MB compact.

## Features per level

- **Each feature type has a highest level** where its lines are kept as
  TIN edges and its carving is sampled; above it, the feature is invisible
  to the TIN. For example: roads up to level 1–2, rivers up to level 3–4.
- **This replaces the Rough, Mid and Fine feature tiers.** A chunk at a
  level above a feature's cut-off simply doesn't carry it. The tier
  machinery's job (draw far ground before costly features are planned)
  becomes "a coarse level doesn't need the costly feature".
- **Promotion** becomes "this level adds nothing to its children here".
  A coarse chunk with no feature of a finer level's types can stand for
  its children in drawing, as `tier_change_known` does across tiers today.
- **Keep-off from vector data.** Rules such as "no trees on roads", "keep
  off rivers" and "clear of a road's edge by `d`" read the feature lines
  (`lines_within`) directly, never the TIN. So they hold at every
  placement level, including levels above the feature's cut-off.

## Open choices

1. **The tree cap:** how far big trees place, beyond which FF tints. Today
   about 20 km, from the 1.4 px cull. The cap sets the canonical build
   radius of level 3–4.
2. **Cut-off levels** per feature type (roads 1 or 2, rivers 3 or 4,
   ponds, later cliffs and lakes).
3. **Labels per placement level.** A label reads its level's TIN with a
   stencil in that level's units, so a "cliff" at level 4 is a big cliff.
   Ecotope and Biome rules need to say which level's labels they mean, or
   labels are defined at one level for all.
4. **Whether a parent must refine its children at shared edges**, so a
   coarser level's edges always lie on the finer ground (exact containment,
   like a quadtree hierarchy). This is not required for placement; it would
   help drawing and geomorphing.
5. **GPU placement (GP) reading the level TINs:** upload each level's
   compact chunks, sample height by integer barycentrics in cm, and take
   slope from quantized smoothed normals, so the CPU and the GPU agree bit
   for bit.
6. **The doubling rule** (tolerance × 2^L) is a start. A per-level curve, or
   a looser bend limit for low-poly worlds, may follow measurement.

## How today's code migrates

- **TE7 chunk levels:** unchanged as the draw selection (`ChunkLevels`,
  size on screen). Each drawn level-`L` chunk becomes that level's
  canonical TIN instead of a regular heightfield chunk.
- **Feature tiers** (`FeatureTier`, `WorldTerrain::terrain_tier`, `Rough`,
  `Mid`, `Fine`): superseded by per-type cut-off levels. A region still
  plans its features once; a level simply omits the types above its
  cut-off. `tier_changes` and `tier_change_known` become "does this level
  add anything to its children".
- **CT-A** (adaptive sampling, contract cache): applies to every level as
  is; the lattice step and tolerance scale with the level.
- **CT5** (products and streaming): `TinIndex`, `GroundSurface`,
  `render_mesh` and `TinCollider` work per level. The stream draws each
  level's TIN; `CanonicalGround` holds level 0 for collision and rest
  height, and each placement level for its Scatters.
- **D6b:** revised from "`H*` decides spots, masks, slope and labels; the
  TIN gives collision and near rest heights" to "the TIN is the ground;
  `H*` is only its generator".
