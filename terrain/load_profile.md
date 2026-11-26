# Terrain load profile

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
  so no speed-up reaches the user.
- **Evidence:** in the shared notes folder, `load-profile/hang.md`.

| # | Speed-up | Expected saving (from the data below) | Effort | ROADMAP item |
|---|---|---|---|---|
| 1 | **Cheaper ground sites for paint.** Every chunk read samples a 129 × 129 grid of `GroundSite`s with analytic slopes (`GroundGrid::chunk`, bare stack at `SampleDetail::Gradient`). That is 4× the mesh's 65 × 65 points, and each gradient costs several heights near borders. Options: heights only on the 129² grid, with slope by differences on the grid; one sampling pass for mesh and paint (the mesh points are a subset); or the site evaluation on the GPU. | **Site sampling is 75% of all CPU on the mixed world and 82% on mountains.** The mesh heights are 2.2–2.4%. **Gradient-only frames are 41–43% of all CPU: the measured saving of heights-only sites** (before the cost of differencing, which is small). `ground_cost` agrees: on a warm world a chunk's gradient sites cost 5–6× its mesh samples on hills and 6–20× on mountains. The read phase is CPU-bound (all 10 Rayon workers ~97% busy), so the first picture and the full view should shorten about in proportion. Estimated: a Rough view of 107 s → about 65 s; on mountains 450 s → about 270 s (both at load ~30). | Medium on the CPU: the grid-difference slope needs its visual error measured, as `gpu_progress.md` asks. Large on the GPU: it needs GPU3's f32 border and carving parity. | [GPU2](../ROADMAP.md#gpu2-ground-weights-on-the-gpu) (its site and measure evaluation is still on the CPU). A CPU step before it needs a new item. |
| 2 | **Skip Rough → Fine re-reads that change nothing.** With `coarse_first`, every chunk that wants a later tier is read twice. Only roads differ between the tiers. `ChunkSource::lines_within(box, Fine)` already says whether a road reaches a chunk. Where none does, keep the Rough read and mark it Fine. | Roads reach 111 of 252 chunks on the mixed world and 80 of 252 on mountains. So **56% and 68% of the upgrade reads are wasted.** An upgrade read costs as much as a Rough read: the default run used 4.47 T instructions in 400 s and was still refining, against 2.32 T for the whole view at Rough. So this saves about **28–34% of a default-view load's CPU.** That is approximate, and an upper bound until a test shows Rough and Fine reads bit-identical where no road reaches. The two instruction counts also come from different builds, and the default run's count includes render spin while it was hung. Near level-0 chunks need no upgrade at all (0 of 64; 3 of 64 on mountains), so they are final at the first picture. | Small to medium: a footprint check in `procgena_terrain_bevy`'s stream, plus a test that Rough and Fine reads are bit-identical where no road reaches. | [TE3b](../ROADMAP.md#te3b-chunk-reuse-by-feature-footprint) (the same footprint idea, used for tier upgrades rather than edits) |
| 3 | **Upgrade the nearest chunks first.** Today upgrades start only when no chunk is missing (`stream_chunks`), so the near chunks wait for the whole far view's Rough reads. | Near chunks reached their final tier at 155–159 s on the mixed world, 155 s with `--tin` and 352 s on mountains, against a first picture at 54–63 s and 167 s. Interleaving the near upgrades (few, after #2) would bring "near chunks final" to about the first picture plus a few reads: **about 90–185 s sooner** at the measured load. | Small: the order of reads in `stream_chunks`. | [TE2](../ROADMAP.md#te2-first-picture-fast) (nearest first) |
| 4 | **Cheaper border pulls and convergence fade per point.** Inside the site sampling, border pulls (with their gradients by differences, `pull_gradients` over four offsets) and the fade's blended gradients dominate. | Border pulls are **28% of all CPU on the mixed world and 40% on mountains**; the fade is 18% and 23%. Analytic pull gradients alone would remove most of the four-offset work (`[DVec2; 4]::try_map`, 27% inclusive on the mixed world). Much of this saving overlaps with #1: heights-only sites drop the gradient half. | Medium: border and convergence maths. | [Border-pull cost (unscheduled)](../ROADMAP.md#border-pull-cost-unscheduled) |
| 5 | **Plan regions ahead, in parallel.** Region plans and builds run lazily inside chunk reads, about one at a time (average concurrency ≤ 1). The first region builds its 9 plans one after another on the main thread while the camera is placed (2–7 s). | Region stages are now only **2–4% of CPU**. Before the first picture they run for 6.2 s of 43 s (mixed world, Rough only), 10.2 s of 63 s (default) and 17.6 s of 167 s (mountains). So building the ring of plans in parallel up front saves **at most about 5–15 s** of the first picture. | Small to medium: a plan-ahead pass over the regions the view needs. | [TE2](../ROADMAP.md#te2-first-picture-fast) ("centre region … waiting on about 7 plans") |

**Not worth it for load time now:**

- **[RB2](../ROADMAP.md#rb2-shared-reverse-cost-field):** roads are 0.0–0.2%
  of CPU.
- **[TE6](../ROADMAP.md#te6-river-speed-to-investigate):** river carving is
  0.6–1.6%, and the river trace 0.3–0.4%.
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
