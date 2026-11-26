# Terrain load profile

> **Ground paint removed 2026-11-26** (the user): the paint costs measured
> below no longer exist; the terrain draws flat grey.

Where a cold terrain load spends its time, measured 2026-11-26 on the M1 Max
(10 cores), in release, with the timings in the Data section below. Milestones
and items link to [ROADMAP.md](../ROADMAP.md); their status lives there.

## Ranked: what to do

**0. Fix the cold-load deadlock first (correctness).**

- **What we saw:** 6 of 8 cold runs in release hung for good. The other
  two were still making progress when they timed out.
- **Which build:** every one contained the code of 5fad02d ("Fix a
  deadlock in WorldTerrain's caches").
- **Two kinds, by the stage left unfinished in `stages.csv`:**
  - **A road tier:** a Mid or Fine build never finished, in 3 runs.
  - **A region plan:** a plan inside a Rough build never finished, in 3
    runs. These include both studio replicas. One drew nothing in 360 s;
    the other stuck at 148 of 194 chunks.
- **Stacks were sampled for one road-tier hang only:**
  - the region's builder thread waits in `road::route`'s Rayon `collect`
    for its queued job;
  - one Rayon worker sleeps in `Cache::get`, waiting for that tier;
  - the other nine Rayon workers are asleep.
- **The plan hangs:** they look like the same pattern, Rayon work inside a
  region build, but this is not confirmed. A later studio-replica run, at
  load ~124, did not hang within 75 s.
- **Why it ranks first:** when it hangs, "everything drawn" never happens,
  so no speed-up reaches the user. **Time to everything drawn, with the
  road tiers built while streaming, is pending this fix.** The world
  worker is moving terrain reads onto one worker thread without Rayon; the
  full cold loads will be rerun once that lands.
- **Evidence:** in the shared notes folder, `load-profile/hang.md`.

| # | Speed-up | Expected saving (from the data below) | Effort | ROADMAP item |
|---|---|---|---|---|
| 1 | **Cheaper ground sites for paint.** Every chunk read samples a 129 × 129 grid of `GroundSite`s with analytic slopes, at `SampleDetail::Measures` (`GroundGrid::chunk`). The bare stack's riverbed layer uses `HeightAboveWater`, which counts as Measures, so each site also reads 4 curvature heights that no bare layer uses. That is 4× the mesh's 65 × 65 points, and each slope costs several heights near borders. On one thread the unused curvature stencil alone is 32% of the terrain worker's CPU (see the single-thread section). Options: heights only on the 129² grid, with slope by differences on the grid; one sampling pass for mesh and paint (the mesh points are a subset); or the site evaluation on the GPU. | **Site sampling is 75% of all CPU on the mixed world and 82% on mountains.** The mesh heights are 2.2–2.4%. **Gradient-only frames are 41–43% of all CPU: the measured saving of heights-only sites** (before the cost of differencing, which is small). `ground_cost` agrees: on a warm world a chunk's gradient sites cost 5–6× its mesh samples on hills and 6–20× on mountains. The read phase is CPU-bound (all 10 Rayon workers ~97% busy), so the first picture and the full view should shorten about in proportion. Estimated: a Rough view of 107 s → about 65 s; on mountains 450 s → about 270 s (both at load ~30). | Medium on the CPU: the grid-difference slope needs its visual error measured, as `gpu_progress.md` asks. Large on the GPU: it needs GPU3's f32 border and carving parity. | [GPU2](../ROADMAP.md#gpu2-ground-weights-on-the-gpu) (its site and measure evaluation is still on the CPU). A CPU step before it needs a new item. |
| 2 | **Skip Rough → Fine re-reads that change nothing.** With `coarse_first`, every chunk that wants a later tier is read twice. Only roads differ between the tiers. `ChunkSource::lines_within(box, Fine)` already says whether a road reaches a chunk. Where none does, keep the Rough read and mark it Fine. | Roads reach 111 of 252 chunks on the mixed world and 80 of 252 on mountains. So **56% and 68% of the upgrade reads are wasted.** An upgrade read costs as much as a Rough read: the default run used 4.47 T instructions in 400 s and was still refining, against 2.32 T for the whole view at Rough. So this saves about **28–34% of a default-view load's CPU.** That is approximate, and an upper bound until a test shows Rough and Fine reads bit-identical where no road reaches. The two instruction counts also come from different builds, and the default run's count includes render spin while it was hung. Near level-0 chunks need no upgrade at all (0 of 64; 3 of 64 on mountains), so they are final at the first picture. | Small to medium: a footprint check in `procgena_terrain_bevy`'s stream, plus a test that Rough and Fine reads are bit-identical where no road reaches. | [TE3b](../ROADMAP.md#te3b-chunk-reuse-by-feature-footprint) (the same footprint idea, used for tier upgrades rather than edits) |
| 3 | **Upgrade the nearest chunks first.** Today upgrades start only when no chunk is missing (`stream_chunks`), so the near chunks wait for the whole far view's Rough reads. | Near chunks reached their final tier at 155–159 s on the mixed world, 155 s with `--tin` and 352 s on mountains, against a first picture at 54–63 s and 167 s. Interleaving the near upgrades (few, after #2) would bring "near chunks final" to about the first picture plus a few reads: **about 90–185 s sooner** at the measured load. | Small: the order of reads in `stream_chunks`. | [TE2](../ROADMAP.md#te2-first-picture-fast) (nearest first) |
| 4 | **Cheaper border pulls and convergence fade per point.** Inside the site sampling, border pulls (with their gradients by differences, `pull_gradients` over four offsets) and the fade's blended gradients dominate. | Border pulls are **28% of all CPU on the mixed world and 40% on mountains**; the fade is 18% and 23%. Analytic pull gradients alone would remove most of the four-offset work (`[DVec2; 4]::try_map`, 27% inclusive on the mixed world). Much of this saving overlaps with #1: heights-only sites drop the gradient half. | Medium: border and convergence maths. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled) |
| 5 | **Plan regions ahead, in parallel.** Region plans and builds run lazily inside chunk reads, about one at a time (average concurrency ≤ 1). The first region builds its 9 plans one after another on the main thread while the camera is placed (2–7 s). | Region stages are now only **2–4% of CPU**. Before the first picture they run for 6.2 s of 43 s (mixed world, Rough only), 10.2 s of 63 s (default) and 17.6 s of 167 s (mountains). So building the ring of plans in parallel up front saves **at most about 5–15 s** of the first picture. | Small to medium: a plan-ahead pass over the regions the view needs. | [TE2](../ROADMAP.md#te2-first-picture-fast) ("centre region … waiting on about 7 plans") |

**Not worth it for load time now:**

- **[RB2](../ROADMAP.md#rb2-shared-reverse-cost-field):** roads are 0.0–0.2%
  of CPU.
- **[TE6](../ROADMAP.md#te6-river-speed-to-investigate):** river carving is
  0.6–1.6% of a whole load, and the river trace 0.3–0.4%. TE6's own profile
  agrees: along a river, carving is about 9% of a chunk read, and the
  border fade and pulls about 88% (rivers on, mountains). The border is the
  lever (#4), not the rivers.
- **[TE3](../ROADMAP.md#te3-content-keyed-stage-cache):** no duplicate region
  builds in any run, so it helps edits only.
- **A CT contract cache:** TIN builds take milliseconds per chunk.
- **[GPU1](../ROADMAP.md#gpu1-relief-preview-on-the-gpu) and GPU3:** GPU1 is
  for edits. GPU3 is the long-term form of #1 and #4.
- **Bevy's main thread:** frames that swap chunks take a median of 20 ms
  against 8–12 ms otherwise, with a worst case of 230 ms.
- **The async compute pool:** it has 3 threads for 4 reads, but it only hands
  points to Rayon, so it is not a limit today.

**Why it feels slower than the 33 s recorded in `edit_cache_plan.md`:**

- **That figure:**
  - 86 chunks;
  - CPU work without ground weights;
  - weights on the 24 near chunks only, as about 42 s more.
- **Today:**
  - The studio's view wants 194 chunks: the levels now switch by size on
    screen ([TE7](../ROADMAP.md#te7-intermediate-terrain-detail-levels)).
  - Every chunk samples ground sites with slopes.
  - A chunk read costs about 1.8 CPU-s on the mixed world, region builds
    included (445 CPU-s for 252 chunks at load ~30).
- **The result, a rough estimate:** a cold studio view is on the order of
  **350 CPU-s at Rough, plus the upgrades**. The 33 s baseline was scaled
  to an idle machine; these runs were not, and the studio config never
  finished here. So read "several times the baseline", not an exact ratio.
  Then the deadlock can keep it from finishing at all.

## Post-TB: one worker thread, paint off

Measured 2026-11-26 on master at b84789a. What changed since the sections
below:

- one terrain worker thread (TS);
- disabled features cost nothing (TO);
- paint off by default;
- no second read where the tiers can't differ;
- the adaptive TIN in centimetres, with a contract cache (CT-A);
- cheap borders (TB).

How it was measured:

- **Example:** `load_timeline` with the default viewer camera and view
  4000 m, on 252 chunks.
- **Features on:** `--roads --rivers 1.0` in every region. The presets
  themselves ship with both off.
- **Load:** 15–30. Read the worker's busy time as the solid figure.

### Cold loads

| World | Features | Mesh | First picture | Finest level final | Settled | Chunks read / promoted | Worker busy | Worker idle |
|---|---|---|---|---|---|---|---|---|
| mixed | off | regular | 1.38 s | 2.19 s | **2.67 s** | 252 / 252 | 1.3 s | 38% |
| mixed | off | `--tin` | 1.29 s | 2.14 s | 2.56 s | 316 / 188 | 0.9 s | 43% |
| mixed | on | regular | 2.00 s | 3.88 s | **22.2 s** | 366 / 138 | 17.2 s | 4% |
| mixed | on | `--tin` | 3.37 s | 7.25 s | 24.8 s | 430 / 74 | 18.8 s | 13% |
| hills | off | regular | 1.99 s | 3.35 s | **5.24 s** | 252 / 252 | 1.8 s | 71% |
| hills | off | `--tin` | 1.93 s | 3.46 s | 5.89 s | 316 / 188 | 1.8 s | 74% |
| hills | on | regular | 2.51 s | 5.70 s | **21.2 s** | 390 / 114 | 14.7 s | 26% |
| hills | on | `--tin` | 3.01 s | 6.44 s | 21.3 s | 454 / 50 | 14.7 s | 15% |
| mountains | off | regular | 1.42 s | 2.25 s | **2.62 s** | 258 / 252 | 1.3 s | 28% |
| mountains | off | `--tin` | 1.44 s | 2.75 s | 3.16 s | 322 / 188 | 1.8 s | 19% |
| mountains | on | regular | 2.36 s | 5.96 s | **29.4 s** | 432 / 78 | 24.0 s | 2% |
| mountains | on | `--tin` | 3.39 s | 6.43 s | 30.0 s | 464 / 46 | 24.3 s | 2% |

- **Region stages, features off:** each region's plan takes about 1 ms (53 in
  0.03–0.07 s), and each Rough build about 2 ms (31 in 0.05–0.08 s). The
  camera is placed at 0.01 s.
- **Region stages, features on:**
  - Rough builds take 0.8–1.7 s for 31 regions.
  - The road tiers take Mid 4–7 s and Fine 13–22 s for 31 regions, about
    0.13–0.22 s and 0.4–0.7 s a region.
  - Region 0's roads are built before the first chunk: the camera is placed
    at 0.28–0.74 s.
- **Instructions:** with features off, 36–50 G for the whole run, render loop
  included. Before TB the features-off Rough view took 2.65 T.
- **The first ranking's #0 (the hang):** no run hung.

### Height evaluations per consumer

| Consumer | Per chunk | Now |
|---|---|---|
| Mesh samples | 4,489 heights at level 0; 6,061 at levels ≥ 1 | all of the features-off chunk work |
| Paint sites | 0 (paint off) | none |
| TIN | level 0 only, read again on its own (+64 reads) | 6% (mixed), 11% (hills), 27% (mountains) of the features-off worker |
| Plans | about 1 ms a region | ~0 |
| Road planning (Mid, Fine) | height reads along routes and sections, with river and road carving | 61–76% of the worker with features on |

### Nanoseconds per height evaluation (one thread, warm)

| Layer | Mixed: all / border-touched | Hills | Mountains |
|---|---|---|---|
| Relief noise and E | 339 / 344 | 919 (756 untouched) | 951 |
| + scaled relief (`natural_height`) | 400 / 491 | 601 | 1,156 |
| + border field (uncarved height) | 434 / 568 | 652 | 1,332 |
| + carving (none, features off) | 432 / 578 | 798 | 934 |
| + owner lookup, point by point (`WorldTerrain::height`) | 1,062 / 1,228 | 2,021 | 2,318 |
| Query with slope / measures | 1,186 / 2,949 | 2,225 / 4,155 | 3,254 / 6,927 |

- **The border:** points it changes are 14.3% in the mixed world, and 0% in a
  one-preset world. Same-settings edges no longer move heights at all
  (0.000 m), and the border field costs 0.03–0.2 µs a height.
- **Owner lookup:** finding the region that owns a point is now the largest
  part of a point-by-point height, 0.6–1.4 µs. Chunk reads batch it, but it
  is still **37–53% of the features-off worker's busy time**: `owner_using`,
  sorting regions by distance, hashing. Relief noise is 25–40%, and the
  border field 0–4%.
- **The probe's noise:** load ~25, hence hills' relief row over its
  untouched one. Read the rows as orders of magnitude.

### Ranked after TB (one thread)

| # | Speed-up | Expected saving | Effort | ROADMAP item |
|---|---|---|---|---|
| 1 | **Cheaper road tiers.** With features on, Mid and Fine road builds are 61–76% of the worker. The promotion check (`tier_changes`) asks every region in view for its Fine tier, so all 31 regions plan roads, far ones included. Inside road planning, about half the time goes to river and road carving in the heights the routes and sections read; the search itself is about 2%. | Settled 21–30 s → about 6–10 s if far regions stop at Mid or Rough (as the studio's `rough_from` already does) and planning reads cheaper heights. The road tiers are 17–28 s of the 22–30 s. | Medium | Canonical ground and roads track ([RB2](../ROADMAP.md#rb2-shared-reverse-cost-field) is not it: the search is small). Needs a new item for road-planning height reads. |
| 2 | **Keep the worker fed.** Done: each stream's reads now wait in a queue the worker takes the nearest from as soon as it finishes one, and drawn chunks known to be unchanged by their later tier are promoted on the main thread without a read. Measured after paint was removed (medians of 8–9 runs before, 3 after, load 5–12): features off settled 1.89 → 1.16 s (1.70 → 1.10 s with `--tin`); roads and rivers on 12.7 → 12.4 s (12.6 → 12.1 s with `--tin`), where region builds are most of the load. | — | — | [TE2](../ROADMAP.md#te2-first-picture-fast) |
| 3 | **Resolve region ownership once per chunk.** A chunk wholly inside one region needs no per-point owner search. | Owner lookup is 37–53% of the features-off worker, so about 35–50% of chunk-read time. | Small to medium | (new) |
| 4 | **River carving cost.** With rivers on, river carving and gullies are 5–8% of chunk reads and 21–23% inside road planning, which reads carved heights. | 5–15% of a features-on load; overlaps with #1. | Medium | [TE6](../ROADMAP.md#te6-river-speed-to-investigate) (bake carving per region) |
| 5 | **TIN builds on rough ground.** With features off the TIN is 27% of the worker on mountains (11% hills, 6% mixed). Its 64 canonical chunks are also read again on their own. | Up to about 0.5 s on mountains. | Medium | CT follow-ups |

- **Below these:** relief noise (25–40% of the features-off worker) is the
  floor of what a height costs.
- **Main thread:** frames have a median of 2.4–5.5 ms and a worst case of
  101–165 ms. No multi-second stalls at this load.

## One worker thread, roads and rivers off (before TB)

The user's design goal is now one terrain worker thread with no Rayon, so
this section ranks by CPU work per stage, not by wall time on 10 cores. The
full table, with counts and budgets, is in the shared notes folder at
`load-profile/single-thread-table.md`.

### Ranked for one thread

| # | Speed-up | Saving of the worker's CPU (mixed world, Rough view) | Effort | ROADMAP item |
|---|---|---|---|---|
| 1 | **Stop reading the curvature stencil nobody uses.** Let `HeightAboveWater` need nearby water without the 4 curvature heights, or sample at the detail each layer really needs. | **32%** | Small | [GPU2](../ROADMAP.md#gpu2-ground-weights-on-the-gpu) (step 1a of `terrain/gpu_plan.md`) |
| 2 | **Paint from a grid of heights only.** Take slope (and curvature, if a layer wants it) by differences on the 129² grid, and let the mesh reuse those points. | The analytic slopes are about **53%**. With #1, per chunk drops from about 197k to 18.5k height evaluations (about **10×**). | Medium: measure the visual error of grid slopes first | [GPU2](../ROADMAP.md#gpu2-ground-weights-on-the-gpu) |
| 3 | **Skip border maths where both regions share their settings.** Relief seeds come from the world seed, so both sides already compute the same relief. 19 of 24 edges round the origin are like this, and all of them in a one-preset world such as the studio. Today the border still runs there, and bends heights by up to 13.6 m (hills) and 152 m (mountains). | The fade and pulls are **81% of a height evaluation** on hills (5.8 of 7.2 µs) and about 62% on mountains. Points the border touches cost 23–40 µs more than the 0.5–0.9 µs relief. | Small to medium for same-settings edges: skip the edge. A design decision for the rest: a plain blend plus TIN seams. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled) |
| 4 | **Cheaper plans.** The 9 plans before the first chunk take 0.18–0.27 s each, 1.9 s in all. Most of a plan is the offers, inside them the fade and pulls again. At river density 0 they still build the 25 m grid, run the trace and bend it (about 0.8%). | Plans are 3.1% of the whole load, but **~90% of the time before the first chunk**. The 2 s first-picture budget leaves about 55 ms per plan. | Medium; much of it comes free with #3 | [TE2](../ROADMAP.md#te2-first-picture-fast) |
| 5 | **No second read for identical ground.** With features off, every Rough → Fine (or Mid) re-read gives the same ground, yet all 252 chunks are read twice in the viewer default. With features on, see #2 of the first ranking. | **About 50% of a default (coarse-first) load**: the default view did the Rough pass, then re-read all 252 chunks. Mid builds still cost 0.14 s each. | Small: no upgrade when the tiers can't differ | [TE3b](../ROADMAP.md#te3b-chunk-reuse-by-feature-footprint) |

**Not levers on one thread:**

- **Mesh samples:** 3.4%.
- **Rough builds without their plans:** 0.4%.
- **Owner lookup and region geometry:** 1.5% of the worker in the profile,
  though 0.85 µs (hills) to 10 µs (mountains, noisy) per height in the
  probe.
- **TIN builds:** milliseconds per chunk.

### What one thread costs today, and the budget

- **Today:** a chunk costs about 197k height evaluations: 16,641 sites ×
  (1 height + a slope of ~6.5 heights + 4 curvature heights), plus 6,061
  mesh heights. At 7.2 µs per height on hills, the 252-chunk view is about
  **360 s on one thread**, plus about 24 s of region plans.
- **Observed:** 627 s to settle at load ~90, of which the worker was busy
  97% of the time.
- **Budget for a full load of about 10 s (252 chunks):** ≤ 2.1 µs per height,
  all in, after #1 and #2.
- **Budget for a first picture of about 2 s (40 chunks):** about 1.5 s of
  heights plus about 0.5 s for the 9 plans.
- **What that needs:** heights 3.4× cheaper on hills and about 14× on
  mountains. The relief and E alone cost 0.5–0.9 µs, so only near-free
  borders (#3) meet it.

| Per height evaluation, one thread (µs) | Hills: all / border-touched / untouched | Mountains: all / touched / untouched |
|---|---|---|
| Relief noise and E alone | 0.50 / 0.49 / 0.50 | 0.91 / 0.90 / 0.94 |
| + convergence fade | 4.3 / 15.3 / 2.3 | 13.5 / 26.8 / 4.5 |
| + border pulls | 6.3 / 23.4 / 3.0 | 19.4 / 40.8 / 6.2 |
| + owner lookup (`WorldTerrain::height`) | 7.2 / 24.7 / 3.9 | 29.9 / 62.2 / 13.7 |
| Site query at Measures (what paint pays) | 68 / 228 / 35 | 256 / 698 / 91 |

- **Where the numbers come from:** the probe `procgena_terrain --example
  height_parts`, best of 3, warm, load 37–100.
- **Untouched points:** they still pay 6–7× the relief, because no
  per-chunk skip exists for chunks wholly inside a region.

**Mountains, one thread, features off, Rough view:**

- **Timings:** first picture 1,455 s, settled 2,092 s, at load 12–64.
- **Stages:** 55 plans in 42 s, and 31 Rough builds in 60 s, inclusive.
- **The worker's CPU:**
  - ground sites 93.5%: border pulls 33.5%, convergence fade 33.4%, relief
    noise 20.9%;
  - mesh samples 3.4%;
  - plans 2.0%;
  - Rough builds 0.3%.
- **What it means:** the same ranking holds, with the border share higher
  than on the mixed world.

### Roads and rivers on against off

- **Single thread:** not measured yet. Since 55b5638 the built-in presets
  have roads and rivers switched off, so those runs had none
  (`--footprint` counted 0 of 252). A comparison needs the features
  switched on explicitly in the measuring example: river densities
  0.6/1.0/1.2 and `roads.enabled = true`, as
  `tests/shipped.rs::rivers_and_roads_carry_on_across_borders` does.
- **From the profiles with features on (10 cores):** the feature-only work
  is small:
  - river carving 0.6–1.6%;
  - gullies 1.1–1.3%;
  - road carving 0–0.5%;
  - the river trace 0.3–0.4%;
  - road planning 0–0.2%.

  That is about 3–4% of CPU in all. The per-point border maths dominates
  either way.
- **What features cost besides:** the Rough → Fine re-reads of chunks a
  road reaches (44% of chunks on the mixed world).

## Data

### How it was measured

- **Tool:** a new example,
  `procgena_terrain_bevy --example load_timeline -- <dir>`. It takes the
  viewer's flags (`--preset`, `--seed`, `--eye`, `--target`, `--view`,
  `--rough-from`, `--mid-from`, `--tin`, `--reads`), plus:
  - `--pool <n>`: async pool threads;
  - `--footprint`: count the chunks a road reaches;
  - `--timeout <s>`.
- **What it does:** it streams headless and records:
  - every region stage, through the progress hook, in `stages.csv`;
  - every frame's chunk counts and frame time, in `frames.csv`.
- **Milestones:**
  - **first picture:** the first frame with chunks drawn;
  - **near final:** every chunk at the finest drawn level is at its final
    tier (and canonical, with `--tin`);
  - **settled:** `TerrainProgress::settled`.
- **Ignore its main-thread busy share:** it renders uncapped, like the
  viewer's `--measure` (4–13% of CPU).
- **CPU split:**
  - `/usr/bin/sample` over whole runs (5–20 ms intervals);
  - demangled with `c++filt`;
  - each busy sample credited to its innermost stage (region plan, Rough
    build, roads, ground sites, mesh samples, TIN) and its innermost
    per-point part;
  - idle and waiting frames left out.
- **Which tools:** `samply` is not installed; `xctrace` was not used.
- **Code measured, three builds of a tree other workers were changing:**
  - **23:38:** c446839, plus the cache-deadlock change committed later as
    5fad02d. Used by the CPU-split profile of the mixed world (prof2) and
    two more default runs.
  - **00:07:** 83df794, plus TE6's uncommitted `plan.rs` and `grid.rs`. Used
    by the default, `--tin`, mountains and both studio-replica runs.
  - **00:40:** the same, slightly later. Used by both Rough-only runs and
    the mountains profile.
  - **Comparing across builds:** compare shares within a build. Counts
    across builds are approximate.
- **Load:** other agents shared the machine (load average 25–110 during the
  runs, noted per run). Read the CPU shares and instruction counts; wall
  times run long, by up to 2–4×.

### Cold loads

Default world (a mix by elevation), default camera, view 4000 m: 252 chunks
(levels 0–4: 64, 48, 48, 48, 44).

| Run | Load | First picture | Near chunks final | Everything drawn | CPU (user) | Instructions |
|---|---|---|---|---|---|---|
| default, 4 reads | 35–50 | 62.7 s | 158.9 s | hung at ~260 s (41 refining) | 1,011 s in 404 s | 4.47 T |
| `--tin` | 40–70 | 53.6 s | 154.8 s | hung at ~280 s (34 refining) | 900 s in 360 s | 4.44 T |
| `--preset mountains` | 40–70 | 166.9 s | 351.8 s | not within 360 s (170 refining) | 1,237 s in 360 s | 7.81 T |
| mixed, Rough only (`--rough-from 0`) | 25–34 | 43.1 s | 47.2 s | **106.9 s** | 445 s | 2.32 T |
| mountains, Rough only | 30–110 | 207.9 s | 216.6 s | **450.5 s** | 1,249 s | 6.75 T |
| studio replica: hills, view 3000, Rough from 4, Mid from 2, 8 reads, 5 pool threads | 32–60 | never | never | hung from ~23 s, nothing drawn | — | — |
| `edit_cache_plan` setup: hills, view 3000, Rough from 3, 4 reads | 26–47 | 36.9 s | — | hung at ~90 s (148 of 194 drawn) | — | — |

- **The studio replica's camera:** it places the camera where the studio's
  opening orbit does, at (477, 309, 697) looking at the origin.
- **Not tried:** the studio itself headless (debug builds only, with a 120 s
  limit, and its binary was stale).
- **Earlier runs:** two more default runs hung at 58 s and 178 s.

### Where the CPU goes

Shares of busy samples, by innermost stage:

| Stage | Mixed world, default (prof2) | Mountains, Rough only |
|---|---|---|
| Chunk: ground sites for paint | **74.4%** | **82.4%** |
| Chunk: mesh height samples | 2.4% | 2.2% |
| Chunk: TIN build, CPU paint, meshing | ~0% | ~0% |
| Region plan: offers | 1.3% | 1.9% |
| Region plan: river trace | 0.4% | 0.3% |
| Region build: Rough (negotiation, bend, hydrology) | 0.3% | 1.8% |
| Roads (Mid and Fine tiers) | 0.0% | 0.2% |
| Stream bookkeeping (main thread) | 2.0% | 0.8% |
| Bevy main and render (uncapped headless frames) | 14.4% | 4.9% |
| Other | 4.6% | 5.3% |

Inside the ground sites, by part of each point:

| Part | Mixed world (share of all CPU) | Mountains |
|---|---|---|
| Border pulls and bend | 28.2% | 39.5% |
| Relief noise | 22.9% | 16.9% |
| Convergence fade | 17.8% | 22.6% |
| Carving: rivers and ponds | 1.6% | 0.6% |
| Gullies | 1.3% | 1.1% |
| Carving: roads | 0.5% | ~0% |
| Gradient-only frames (any part) | 41.0% | 43.2% |

- **Region stages by the hook:** plans take 0.2–1.6 s each on hills
  (mountains up to 6 s, loaded). A region's Rough build takes 0.5–3 s. A
  region's Mid or Fine (roads) build takes 0.1–2 s.
- **Sums for the default run:** 55 plans in 39 s, 31 Rough builds in 18 s
  without their nested plans, and 13–15 Mid and Fine builds in 8–16 s.
- **Mountains Rough only:** 55 plans in 90 s and 31 Rough builds in 142 s
  (inclusive, at load up to 110).

### One chunk's read, per level (warm world)

- **Source:** `procgena_terrain --example ground_cost --chunks 4`, release,
  with Rayon.
- **What it times:** each chunk's mesh samples (65², fine tier) and its
  ground sites (129²) at increasing detail.
- **Load:** 120–165, so read the ratios within a row.

| Level | Hills: samples ms | sites ms, gradient / measures / labels | Mountains: samples ms | sites ms, gradient / measures / labels |
|---|---|---|---|---|
| 0 | 31 | 176 / 317 / 425 | 77 | 243 / 395 / 500 |
| 1 | 55 | 266 / 615 / 1,064 | 16 | 286 / 447 / 739 |
| 2 | 44 | 248 / 554 / 926 | 85 | 532 / 683 / 1,034 |
| 3 | 116 | 630 / 884 / 1,991 | 37 | 721 / 1,372 / 1,446 |
| 4 | 69 | 758 / 1,176 / 2,044 | 29 | 470 / 647 / 1,090 |
| 5 | 85 | 654 / 1,057 / 1,867 | 32 | 664 / 1,091 / 1,743 |

The bare stack used by the viewer and studio asks for gradient sites.
Painters that read measures or labels cost 1.5–3× more again.

### Parallelism

- **The read phase:** the 3 async compute threads (Bevy's default quarter of
  the cores; `max_reads` is 4) each sit in one chunk read. They hand its
  points to Rayon and wait, and all 10 Rayon workers were 97% busy in a 10 s
  sample. The phase is CPU-bound, not starved.
- **Region stages:** they start lazily, when a chunk read first touches a
  region, and run about one at a time:
  - average concurrency ≤ 1 per 2 s bin;
  - in the default run, the 9th Rough region finished at 77 s and the last
    at 131 s.
- **Camera placement:** placing the camera builds region 0's 9 plans one
  after another on the main thread, in 1.8–7.4 s.
- **Waiting for neighbours:** no region plan waited on another region's
  plan in a way that showed as idle cores. The cores were busy with chunk
  sites meanwhile.

### Wasted work

- **Duplicate builds:** none. Every (region, stage) was built once in every
  run.
- **Tier re-reads:** with `coarse_first`, every chunk wanting a later tier
  is read twice. Wasted re-reads: 56% (mixed) and 68% (mountains) of them
  (see #2). Roads reach these chunks, per level 0–4:
  - mixed world: 0, 17, 19, 32, 43;
  - mountains: 3, 7, 7, 27, 36.
- **Mesh and paint sample separately:** the mesh's 65² points are a subset
  of the paint's 129² sites, about 2% of CPU.
- **Cancelled reads:** not measured, since no camera moved.
- **Rebuilds after edits:** not measured here. From the code, an edit still
  makes a new `WorldTerrain`, as `edit_cache_plan.md` describes
  ([TE1](../ROADMAP.md#te1-stop-wasted-work),
  [TE3](../ROADMAP.md#te3-content-keyed-stage-cache)).
