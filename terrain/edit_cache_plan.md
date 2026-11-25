# Terrain edits: caching and incremental rebuilds

A decision document, not a build log. It answers the user's questions:

- will a cache make terrain editing fast;
- what are its limits;
- should we use something like `salsa`;
- what would moving terrain work to the GPU gain. Measured 2026-11-24 on master at
`6fbb193` (before G4a and GM1 merged).

## Bottom line

**Recommendation.** Do the cheap, cache-free work first. Then add a small
hand-written stage cache on top of the cache `WorldTerrain` already has.
Do not adopt `salsa` or `comemo`.

1. **Stop throwing the cache away.** `WorldTerrain` already caches
   every region in stages: settings, then plan, then the Rough tier, then
   the Fine tier. The studio builds a new `WorldTerrain` on every settled
   edit (`procgena/src/scene/terrain.rs`, `TerrainSubject::update`), so
   every edit starts cold. A cold build and a one-setting edit took the
   same time (see "Measurements").
2. **Cheap wins, no cache needed (stages S1 and S2 below):**
   - Show a preview of the edited land within about a second: the
     region's own relief, with no borders or features.
   - Cancel stale builds.
   - Start with the region under the camera.
   - Leave meshes alone when only ground colours change.

   These change what you wait for, not how much work there is.
3. **Key each stage by what it reads (stage S3).**
   - Give each `WorldTerrain` stage a content-hash key made from exactly
     the settings it reads, and keep the caches across edits.
   - On its own, this makes ground edits, undo and drag-back free.
   - Making road, river and border edits cheaper also needs S4:
     - split the plan so port wishes are negotiated apart from the
       heights;
     - give the Fine tier its own road settings.
   - Edits to the land's shape still rebuild everything.

**Will it work?** In part, and the answer depends on the setting.

Today a cold build of the studio's opening view is about 33 s of CPU work
in release (an idle-machine estimate): 23 region plans, 9 region builds,
roads in 2 regions and 86 chunks. Ground weights for the near chunks
would add about as much again. A one-setting edit costs exactly the same.

- **Ground colours and debug drawing:** yes, nearly all the time is
  saved. Neither changes heights.
- **Roads, rivers and border settings:** partly.
  - The probe shows most rebuilt work changes nothing. A road edit moved
    no Rough height and only a quarter of the near chunks, on hills and
    on mountains alike. A river-width edit moved 4 chunks in 177 on
    hills, and 38 on mountains.
  - But a cache keyed on stage inputs alone (S3) saves nothing on these
    edits. Each setting reaches the region plans (40% of the work)
    through the border offers or the convergence.
  - Keeping the plans takes two more steps:
    - Move road and river port wishes into their own negotiation (S4).
    - Key chunks by the features near them (S3b).

    With both, expect up to about 25% saved on river edits and 65% on
    road edits (estimates).
  - Roads (29% of the work) are re-routed on every road edit whatever
    the cache.
- **Land shape (`base.*`) and the seed:** no. Every region's relief
  changes, so every later stage must run again. No cache, salsa
  included, can skip work whose inputs really changed. Here only less
  work helps: a preview tier, fewer regions to wait for, a faster plan.
- **The optimisation level is not the answer.** Release is only about
  1.2× faster than the dev profile here.

In the studio, the land settings are the ones people move most. So the
preview and cancelling come first, and the cache second.

**And the GPU?**

- It can take the per-point work:
  - a relief preview of the whole view, estimated at milliseconds;
  - chunk heights (23% of the CPU work);
  - ground weights (about as much again as the rest of the build, for
    the near chunks alone);
  - measures.
- It can't take the plans: the river trace, road routing and
  negotiation, about 76% of a cold build without weights. They are sequential and
  graph-shaped, and the CPU needs them for queries anyway.
- GPU drawing would make per-chunk caching unnecessary, but not the
  region-stage cache.
- Its biggest cost is rewriting the f64 border and carving maths in f32
  for WGSL (see "Terrain on the GPU").

**Main limits.**

- The seed, the world settings (region size, warp, E) and every `base.*`
  setting invalidate everything.
- Borders couple neighbours. Building one region reads the plans of
  every region touching its corners. Each of those plans reads the
  settings one ring further out. So in a Biome world, changing one
  region's settings invalidates two rings of regions around it.
- A cache key that leaves out something a stage reads gives stale
  terrain. Nothing fails; the land is quietly wrong. The keys must be
  derived from what each stage reads, and tested by changing settings and
  comparing outputs.
- Memory: one view's regions took about 200 MB. Keyed by content, a
  slider drag could pile up old versions to about 1 GB (an estimate), so
  the cache must keep only the last one or two versions per stage.
- Floats are bit-exact today because each stage is pure. A cache must
  never key on rounded values.
- The rule that terrain stays analytic is untouched. Caching stores the
  results of pure functions and changes no math.

## Measurements

### Setup

- Throwaway harness (not committed; source in the shared notes folder,
  `edit-cache-harness/`):
  - It builds the studio's world: the `rolling_hills` preset (the
    studio's start preset) in every region, the world seed from
    `default_seed`, and default world settings.
  - It picks chunks for the studio's opening camera: focus at the origin,
    yaw 0.6, pitch 0.35, 900 m out, view distance 3000 m, `ROUGH_FROM` 3.
  - It reads them as the studio does. First every chunk at the Rough tier
    (`coarse_first`), then levels below 3 again at Fine.
  - Region stage times come from `WorldTerrain::with_progress`.
- Run in both the dev profile (`procgena_terrain` at opt-level 1,
  dependencies at 3) and release. One cold run, then one edit
  (`features.roads.width` + 1 m).
- **Threads.** The harness calls stages one at a time.
  - But `procgena_terrain`'s default `rayon` feature samples a chunk's
    rows in parallel, and region builds that start inside a chunk read
    run inside those rows.
  - So the first runs were partly parallel. A last release run with
    `RAYON_NUM_THREADS=1` gives the true single-thread split of the work.
    The shares below come from it.
- **The machine was loaded by other agents.** Load average was 84–340 on
  10 cores, so absolute wall times swing by up to 4× between runs.
  - The shares within one run, and the counts (regions, chunks, builds),
    are what to read.
  - Absolute seconds below are scaled to an idle machine. The release
    run that got 93% of a core (81 s of CPU for two builds and two warm
    re-reads) sets the scale.

### Where a cold build's time goes

What the studio's opening view needs:

| Item | Count |
|---|---|
| Chunks: level 2 (Fine) / 3 / 4 / 5 (Rough) | 24 / 30 / 27 / 5 = 86 |
| Regions planned (the Plan stage) | 23 |
| Regions built at Rough | 9 |
| Regions built at Fine (roads) | 2 |
| Mesh data (positions, normals, indices) | 18.4 MB |

The 9 regions under the view need plans from 14 more regions round them:
each build reads the plan of every region touching its corners. **Only 9
of the 23 plans are for land that is drawn.**

CPU work by stage, release, one thread (`RAYON_NUM_THREADS=1`):

- "Loaded" is that run's measured seconds; its load was 290–340, so
  they run long.
- "Idle" scales the run to the release run that had a nearly free core.
  It is an estimate.

| Stage | Loaded (s) | Share | Idle, est. (s) | What it is |
|---|---|---|---|---|
| World, region graph, E, border warp | < 0.005 per region | ~0 | ~0 | `WorldTerrain::new`, `RegionGraph::region` |
| Plan: settings, convergence, river trace, offers | 55.4 (23 plans) | 40% | ~13 | `build_plan` |
| Rough: negotiate, bend, hydrology (rivers, ponds) | 10.0 (9 regions) | 7% | ~2.3 | `build_roughest`, without the nested plans |
| Fine: roads | 39.8 (2 regions) | 29% | ~9.5 | `Terrain::with_tier(Fine)` |
| Chunk sampling (86 chunks) | 32.0 | 23% | ~7.8 | `ChunkSamples::sample` |
| Meshing | 0.02 | < 0.1% | ~0 | `ChunkSamples::mesh` |
| **Total, without ground weights** | **137.8** | | **~33** | |
| Ground weights, 24 level-2 chunks | 176.6 (7.4 s a chunk) | +128% | ~42 | `ChunkWeights::build` |
| Ground weights, all 86 chunks (est.) | ~630 | +~460% | ~150 | if every drawn chunk had them |
| Upload to the GPU | not measured | — | — | Bevy `Mesh` creation and upload |

Cross-check: the passes' wall time (85.5 + 52.2 s) equals the region
stages (105.2 s) plus sampling (32.0 s) to within 0.6 s, so the split
adds up.

With rayon on (the default, and what the studio runs), the earlier
release run spread sampling over cores: 39.7 s wall for the same build,
with sampling only 9% of the **wall** time. The plans and roads did not
get faster.

Notes:

- **Release is only about 1.1–1.2× faster than dev.** The load-proof
  counters from `/usr/bin/time -l` over the same work (two builds and
  two warm re-reads): release 432 G instructions and 148 G cycles, dev
  509 G and 166 G. That is 1.18× fewer instructions and 1.12× fewer
  cycles in release. The dev profile already optimises our crates at level 1
  and every dependency at level 3. So **the optimisation level is not
  the fix: the work is.**
- **The studio, with 4 reads at once, needs roughly 10–20 s to the full
  view** in release on an idle machine. That is an estimate, not
  measured.
  - Once-locks make readers wait on one region's build.
  - The ring of plans must finish before the first Rough region.
  - The roads of a near region take about 5 s whatever else runs.
  - The earlier "0.3 s per 10 km region" in `world_plan.md` predates
    border warp, convergence and the road rework.
- **The Rough figure** is the hook's Rough total less the plans built
  inside it (65.4 − 55.4 s). Every plan was built inside a Rough build.
- **Inside one plan** (the centre region, re-run on its converged
  generator, release, under heavy load):

  | Step | Rolling hills | Mountains |
  |---|---|---|
  | river trace | 0.74 s | 5.06 s |
  | offers | 0.67 s | 2.31 s |
  | settings (`TerrainGenerator::new`) | < 0.01 ms | < 0.01 ms |
  | region graph and warp | 3 ms | 0.4 ms |

  - Trace and offers dominate, and both grow steeply on mountains.
  - **Convergence was not isolated.** Its cost is inside the plan's total,
    which this run didn't time on its own.
  - A cached lookup costs 1 µs.
  - A whole cold Fine build of the centre region, with its ring of
    plans, took 26 s on hills and 87 s on mountains (both loaded). So
    mountains cost about 3× as much as the hills these numbers come from.
- **Ground weights** (GM0's bare stack, level 2, the 24 near chunks):
  - One thread: 7.4 s a chunk against 0.52 s to read the same chunk's
    heights, in the same run. **About 14× a chunk read**, on top of it.
  - With rayon the ratio was 4–5×, because weights spread over cores
    better.
  - A weights chunk reads 129² = 16,641 points, each with a gradient (more
    height evaluations per point). A height read is about 6,000 points
    without gradients.
  - So weights for only the 24 near chunks cost about as much CPU as the
    whole rest of the build. For every drawn chunk they would cost about
    4.6× the rest of the build (an estimate).
  - **Weights are the largest per-point cost in sight**, and the best
    GPU candidate.
  - Weights need the built regions but nothing else, so a ground-only
    edit is pure weights work.
  - GM1, which draws the colours, is in progress; how many levels get
    weights is its call.
- **Uploads.** These were not measured, to avoid building Bevy. 18.4 MB
  for 86 chunks is small next to the region stages; Bevy's mesh upload
  costs about a memcpy per chunk.
- **Memory.** Peak resident memory of the dev run (two worlds, one after
  the other) was 213 MB.

### A one-setting edit

An edit of `features.roads.width` (+1 m) rebuilt exactly what the cold
build did:

| Run | Plans | Rough builds | Fine builds | Rough pass (s, release) | Fine pass (s, release) |
|---|---|---|---|---|---|
| Cold | 23 | 9 | 2 | 28.0 | 11.7 |
| Edit | 23 | 9 | 2 | 27.4 | 10.3 |

**The edit reused nothing.** This is expected: `TerrainSubject::update`
makes a new `WorldTerrain`, and nothing carries over.

### What the studio adds on top

The harness reads chunks one at a time. The studio differs in four ways:

- It reads up to 4 chunks at once (`TerrainPlugin::max_reads`) on Bevy's
  async compute pool.
- It waits 400 ms after the last change before it builds
  (`procgena/src/terrain/editor.rs`, `SETTLE`).
- It swaps in the new chunks only when **every** chunk of the view has been
  read at Rough. The old land stays drawn until then.
- Region builds are once-locked. Four reads that need the same region
  wait on one build. The first picture after an edit therefore waits for
  all 9 Rough regions and their 23 plans, spread over at most 4 threads.

**Stale builds are not stopped.** `reset_streams` clears the task list,
and dropping a Bevy `Task` cancels it only if it has not started.
`ChunkSamples::sample` never yields, so a read already running finishes,
along with any region plan it started on the old world. After an edit,
up to 4 threads can keep building land nobody will see. They hold CPU
time and battery, for up to a full region build each. On a slider drag
that settles twice, the second build competes with the first.

## What depends on what

Read from the code (`world.rs`, `terrain.rs`, `plan.rs`, `converge.rs`),
not from the stage names. Then checked with a probe:

- Build the centre region of the studio world.
- Change one setting per group.
- Compare hashes of each stage's output, and the heights of the level-2
  chunks inside the region, with the original.

### The stages and what they read

| Stage (cached today in `WorldTerrain`) | Reads |
|---|---|
| Region graph and warp | world seed, `RegionGraphOptions` (size, jitter, warp) |
| E (`Elevation`) | world seed, `ElevationOptions` |
| Settings (per region) | the source's `TerrainOptions` for the region, E. Gives the generator and its relief amplitude (from `base.*`). |
| Plan (per region) | Own settings; for **every region on its edges and corners**: amplitude, road grades (`roads.grade`, `roads.max_grade`) and whole `TerrainOptions` (convergence blends neighbours' relief). Then the river trace (`rivers.*`, relief). Then offers: natural border samples, `edges.stiffness/margin/roughness`, river port wishes (`rivers.width`, trace), road port wishes (`roads.enabled`, `roads.width`), road grades. |
| Rough tier (per region) | own plan and the plans of **every region touching its corners** (corner and edge negotiation), E; then bend and hydrology (`rivers.*`, `edges.margin`). |
| Fine tier (per region) | Rough tier, `roads.*`, agreements, rivers. |
| Chunk samples | the Rough or Fine terrain of every region the chunk and its ring touch. |
| Chunk mesh | the samples, the transitions, the skirt depth. |
| Ground weights | the samples' measures, the ground stack (GM0 layers, seed). |
| Debug tint and borders | nothing above (already applied at once). |

### Which settings invalidate what

The probe:

- Studio world, centre region.
- Two presets: rolling hills (the studio's start), and mountains. The
  same world seed. Hills ran in dev and again in release, with the same
  counts of changed chunks.
- Its level-2 chunks (256 m) that lie wholly inside the region: 177 of
  them, up to the region's borders.
- Each chunk's vertex heights are hashed at Rough and at Fine.
- Relief is hashed on a 41 × 41 grid; rivers and roads by their
  polylines.
- Each setting moves by one small step. That is one seed and one step
  size, so treat "0 chunks" as strong evidence, not proof.

Changed chunks are Rough / Fine, out of 177:

| Setting changed | Hills: relief, rivers, roads | Hills: changed chunks | Mountains: relief, rivers, roads | Mountains: changed chunks |
|---|---|---|---|---|
| `base.hills.height` +5 m | all changed | 177 / 177 | all changed | 177 / 177 |
| `features.rivers.width` +1 m | same, same, same | 4 / 4 | same, same, same | 38 / 38 |
| `features.roads.width` +1 m | same, same, same | **0** / 47 | same, same, changed | **0** / 44 |
| `features.roads.grade` +0.01 | same, same, changed | **0** / 29 | same, same, changed | **0** / 12 |
| `features.roads.enabled` toggled | same, same, changed | **0** / 47 | same, same, changed | **0** / 40 |
| `edges.margin` +20 m | same, same, same | 42 / 42 | same, **rivers and roads changed** | 94 / 95 |

On mountains, a border-margin edit moved the rivers. The margin changes
the converged relief the rivers are traced over, so an `edges.*` edit
must invalidate the trace too.

"Relief" is `natural_height` of the built terrain's generator. It did not
change with `edges.margin`, which the code says moves the border fade.
So `natural_height` probably reads the relief before the fade, and the
column is only a check that the relief did not change. The chunk columns
are the ground truth.

**Most of the work an edit redoes today changes nothing.**

- A road edit reruns every plan and every Rough build (47% of the work),
  yet no Rough height moves. Only the roads (29%) and about a quarter of
  the near chunks change.
- A river-width edit moves 2% of the chunks on hills and 21% on
  mountains.
- A border-margin edit moves the border bands: about a quarter of the
  chunks on hills, and half on mountains, where it moves the rivers
  too.

This is the evidence that a stage cache keyed finely enough, with early
cutoff on outputs, would pay off for feature and border edits.

Reading the table:

- **`base.*`** changes the unconverged relief. Everything after the
  Settings stage changes, in every region.
- **`edges.*`** goes into the offers and the convergence fade. Every
  plan changes. On hills the rivers did not move and only the border
  bands changed (42 of 177 chunks). On mountains the rivers moved too
  (95 chunks): the margin reaches the trace through the converged
  relief.
- **Roads.**
  - By the code, `roads.width` and `roads.enabled` go into the offers'
    port wishes. `roads.grade` and `roads.max_grade` go into the
    convergence targets and the offers.
  - So an input-keyed cache must rebuild every plan on a road edit, the
    most expensive stage.
  - By the probe, no Rough height moved. A cache that keys the Rough
    tier on the **agreed heights**, not on the whole offers, could keep
    it.
    - But only once the Fine tier stops reading road settings from the
      Rough terrain it clones (see option (a)).
  - Splitting port wishes out of the plan (stage S4) would keep the plans
    too.
- **`rivers.*`** reads the trace, the offers and hydrology. Width changed
  no river line and only 4 chunks. The relief and convergence before it
  could be kept, but today they share one Plan stage with the trace.
- **Ground (GM0/GM1) and debug drawing** never touch heights. Only
  weights and colours need to be redone.

### Across regions

- **Studio.** Every region gets the same `TerrainOptions`, so every edit
  touches every region. A per-region key saves nothing here. The savings
  must come from per-stage keys (keeping the stages an edit doesn't
  reach) and per-chunk reuse (keeping chunks whose heights didn't move).
- **A World of Biomes.** Each region's options come from its Biome.
  Changing one Biome's Terrain preset invalidates:
  - the Settings of the regions using it;
  - the Plans of those regions and every region touching them (one ring);
  - the Rough and Fine builds of every region touching any of those
    (two rings).

  The rest of the world is kept. That is where a per-region key pays off.
- **Global inputs** (the world seed, region size and warp, E, the
  convergence settings) invalidate the whole world.

## Fit with the architecture and the feature plans

**ARCHITECTURE.md, "Rules that hold at every level".** Every stage is
pure and deterministic, so a cached result equals a recomputed one. That
is what makes any of this sound. Nothing here changes the rules.

**ARCHITECTURE.md, "Loading by chunk".**

- **Story 5** asks that "builds for places I've already left are
  dropped". Today they aren't (see "Stale builds are not stopped"). W1
  is a requirement the architecture already sets, not an extra.
- **The Plan stage** is budgeted at "up to about a second" per region.
  - Measured here in release on rolling hills (idle-machine estimate):
    a plan takes about 0.6–0.9 s, but the roads of one near region take
    about 5 s.
  - That breaks the budget. It is the step-2 benchmark that
    [features_plan_review.md](features_plan_review.md), Conflict 4, set
    as the gate for macro cells.
- **Provisional far tiles** may be used for drawing only. W2's preview
  tier is that idea applied near the camera, for a few seconds, in the
  studio. Placement and queries must never read it.
- **Plans "cached on disk"** are named there. This plan keeps caches in
  memory only. A disk cache adds the version and target to every key
  (see "Float determinism").

**features_plan_review.md, step 2** ("planning stages cached on their
own", with cold and warm benchmarks) is where this work belongs.

- The review notes that "W4's region cache is where the staged plans
  live". S3 and S4 are the rest of step 2, and the measurements above
  are its first cold benchmarks for the studio's view.

**features_plan.md §9** says "exact seam agreements need the port
offers". S4 keeps that rule:

- Ports are still negotiated before any tier that carves them.
- They are only negotiated apart from the heights, so a road setting
  that moves no height can leave the height agreement, and everything
  built on it, alone.

## Options

What each one saves, and what it costs. "Saves" is measured against the
cold build's CPU work above (single thread, without ground weights),
whose shares are:

- plans 40%;
- Rough builds 7%;
- roads 29%;
- chunk sampling 23%.

### Cheap wins (no cache)

| Win | Saves | Costs |
|---|---|---|
| **W1. Cancel stale builds.** Pass a generation token or `Arc<AtomicBool>` into `WorldTerrain`. Check it between region stages and between chunk rows; a cancelled build returns an error that is never cached. | Wasted CPU after each edit: up to 4 threads × one region build. Mostly battery, plus a faster second edit during a drag. | Small. One flag check per stage. Results are unchanged, because a cancelled build is dropped, never stored. |
| **W2. Relief-only preview tier.** A `FeatureTier::Preview` (or a separate preview source) that draws each region's **unconverged** relief plus E from its Settings alone, without plans or borders. Show it right after an edit, then swap in Rough and Fine as they land. | Time to the first picture of a `base.*` edit: from all 23 plans to sampling only: under 23% of the work (~8 s of CPU, spread over the cores by rayon), and much less for relief alone, without borders and carving, or less if previewed at a coarser level. | Borders show seams for a few seconds, and rivers and roads are missing. A new tier for chunks and skirts. Must be marked provisional and never queried (features_plan §9, "Canonical terrain versus display LOD"). |
| **W3. Nearest region first.** Order region builds and reads by distance to the camera, and swap per region rather than waiting for the whole view. | The land under the camera arrives first. Today the swap waits for all 9 Rough regions. | Medium. `Stream` swaps the whole set on purpose, so cracks never show between old and new land. Per-region swaps need skirts or a fade at the boundary. |
| **W4. Keep meshes on non-height edits.** Route ground-colour (GM) and debug changes to weights and materials only. Don't make a new world. | All of the land time for ground edits. | Small, once GM1's settings are in the editor. Separate the height settings from the paint settings in `TerrainSettings`. |
| **W5. Faster plan.** Profile `build_plan` (convergence, trace, offers). The warped-border cost (W3b: 2.7× straight) and the per-region ring overhead are known. | Every edit, cold builds and the game too. | A performance sweep. Already a TODO in `world_plan.md`. |

### (a) A hand-written stage cache, keyed by content hashes

**What.** Keep the stage caches that `WorldTerrain` already has, and keep
them across edits:

- Key each entry by a 64-bit content hash of exactly what that stage
  reads (the table above), not by `RegionKey` alone.
- On an edit, the studio hands `WorldTerrain` a new source. Entries whose
  key is unchanged are kept.
- Chunks get a key too: the keys of the region builds they read, the
  tier, the chunk and its transitions. The stream keeps a drawn chunk
  whose key is unchanged.
  - With that key, any change to a region's Fine build re-reads every
    Fine chunk of the region.
  - Keeping only the chunks whose ground did not move needs a finer key
    (stage S3b): the region's relief and agreement key plus a hash of the
    features within reach of the chunk (`Terrain::features_within` over
    the chunk grown by the features' reach).
  - That is footprint diffing. It is its own piece of work, and its key
    can drift too: it must cover every feature's reach, banks included.

**Building blocks already in the tree.**

- `procgena_core::ContentHash` and `ContentHasher` (F2): stable, with
  `-0.0` and NaN normalised, and no derived `Hash`.
- No terrain type implements `ContentHash` yet. `TerrainOptions`,
  `ElevationOptions`, `RegionGraphOptions`, `Convergence` and the ground
  layers need it.
- F2's rule is that an options hash **leaves out the seed**. A cache key
  must put it in, so stage keys add the world seed explicitly.

**Early cutoff, by hand.** A plan's key covers its inputs. A Rough build's
key covers the **output hashes** of the plans it reads (the offers and
the converged relief), not their input keys. A setting that changes a
plan's inputs but not its offers then rebuilds the plan and keeps
everything downstream. This is salsa's "backdating", done by hand where
it pays.

**What S3 alone saves** (stage keys only, no change to what the stages
read, chunks keyed by their region builds):

| Edit | Rebuilt | Saved |
|---|---|---|
| Ground colours, debug drawing | weights or nothing | ~100% of the land time |
| `roads.*`, `rivers.*`, `edges.*` | everything: each reaches the plans through the offers or convergence | ~0% |
| `base.*`, seed | everything | 0% |
| Undo, redo, or dragging back to an earlier value | nothing, while the old entries are still kept | ~100% |

**Why a road edit can't even keep the Rough tier without a change.**

- The probe moved no Rough height on a road edit, so it is tempting to
  key the Rough tier on the agreed heights and keep it.
- But `Terrain::with_tier(Fine)` routes the roads from the options
  **inside the Rough terrain it clones** (`self.generator.options`). A
  Rough terrain kept across a road edit would route the new Fine tier
  with the old road settings.
- That is a stale-result bug of exactly the kind "Limits" warns about.
  The Fine stage must take the road settings as its own input before the
  Rough tier can be kept.

**With the changes in S3b and S4.**

- S3b: chunk keys by feature footprint.
- S4:
  - The Fine stage takes the road settings as its own input.
  - Port wishes are negotiated apart from the height offers, so the
    relief, convergence and trace are stages of their own.
  - The Rough tier is keyed on the agreed heights.

| Edit | Rebuilt | Saved |
|---|---|---|
| `roads.*` | road port wishes and their negotiation, roads (Fine tier), the chunks near changed roads (about a quarter of the near ones in the probe) | plans, Rough and most sampling: up to ~65% |
| `rivers.*` | trace, offers, hydrology, roads, the few chunks that moved | convergence and most sampling: not estimated, since convergence was not timed on its own |
| `edges.*` | offers, negotiation, Rough, roads; the border-band chunks | relief, convergence, trace and the inner chunks: hard to estimate until the plan is split |

These "up to" figures assume each split stage costs what its share of
today's plan costs. They are estimates, not measurements.

Roads (29%) are rebuilt on every road edit under every option. Making a
road edit truly quick needs the road search itself to be cheaper (W5),
or to be redone only near the roads that changed.

**Costs.**

- Small and in our own code: about one `ContentHash` impl per options
  type, a key per cache, and tests.
- The real cost is **keeping the keys honest** (see "Limits").

### (b) `salsa`

| | |
|---|---|
| Version | 0.28.6 (2026-11-22). Pre-1.0, described as experimental. |
| Downloads | 11.2 M total, 3.6 M recent (mostly rust-analyzer and ty). |
| Licence | Apache-2.0 OR MIT. |
| In our tree | No. New crates: `salsa`, `salsa-macros`, `salsa-macro-rules`, `boxcar`, `hashlink`, `intrusive-collections`, `thin-vec`. Already present: `crossbeam-queue`, `crossbeam-utils`, `hashbrown`, `indexmap`, `parking_lot`, `portable-atomic`, `rustc-hash` 2, `smallvec`, `tracing`. |
| MSRV | 1.95 by crates.io metadata; ours is 1.97.1. |
| API stability | High churn. Breaking changes in 0.22, 0.25 (the experimental parallel feature removed), 0.26 and 0.28 (`Update` replaced by `SalsaValue` + `PartialEq`, references returned by default). Six point releases since July; 0.28.5 fixed a use-after-free advisory. |

**How it would fit.**

- Inputs (`#[salsa::input]`) would be the world options and each region's
  `TerrainOptions`. Tracked functions would be the stages.
- Tracked-function arguments must be `Eq + Hash` (salsa docs). Our
  options are f32 throughout, and f32 is neither, so they would most
  likely need a bit-wrapper, the same key discipline as (a). How salsa
  treats f32 input fields was not confirmed.
- Salsa's real gift is **automatic dependency tracking**. A tracked
  function records which inputs and queries it read, so a key can't
  drift. But our stages read options through plain struct fields. To
  benefit, every field read would have to become a query (or a tracked
  struct field), which means restructuring the generator around salsa.
  Without that, salsa tracks only "read the whole `TerrainOptions`", and
  we are back to whole-region invalidation.
- **Backdating** (equal outputs stop invalidation) comes free. That is
  the one thing it does better than a hand cache.

**Threads and Bevy.**

- Salsa's docs describe a local `Storage` with handles to shared state.
  Its threading model changed in 0.25, when the experimental parallel
  feature was removed. How several threads share one database in 0.28
  was not confirmed and would need a spike.
- Cancellation is **by unwinding** (a `Cancelled` panic) at each query
  call. Long loops must call `unwind_if_revision_cancelled` themselves.
- Bevy tasks, and anything holding locks across that panic, would have
  to be unwind-safe.
- On `wasm32-unknown-unknown` (the plant web demo; terrain on the web
  later), panics abort by default, so cancellation would kill the app.
  Not confirmed either way; a spike would have to check.
- Our determinism is unaffected (salsa caches; it doesn't reorder our
  math). But its cancellation would bring panics into normal control
  flow.

**Memory.** Memos are kept forever by default. An `lru = N` per tracked
function evicts values, but not dependency metadata.

**Verdict: no.**

- It fits a compiler: thousands of small queries over text, with many
  edits.
- We have about six coarse stages per region and one input struct per
  region. The bookkeeping would cost more than the hashing in (a).
- It would fight our once-locked, thread-pool build, and its churn would
  be a tax on every upgrade.

### (c) `comemo`

| | |
|---|---|
| Version | 0.5.1 (2026-01). |
| Downloads | 4.9 M total, 2.3 M recent (Typst). |
| Licence | MIT OR Apache-2.0. |
| In our tree | No. New: `comemo`, `comemo-macros`, `siphasher`. Already present: `parking_lot`, `rustc-hash`, `slab`. |

**How it works.**

- `#[memoize]` on a function hashes its plain arguments (128-bit). It
  replays recorded calls on `Tracked<T>` arguments: a cache hit needs
  only the **methods that were called** to return the same values.
- That is exactly the "key drift" protection we want, at
  method-call granularity. A `#[track] impl TerrainOptionsView` with
  `fn roads_width(&self) -> u32` (bits) would let a stage read only what
  it needs, and stay valid when other fields change.
- The cache is global and is emptied by calling `comemo::evict`. That it
  evicts by age is from the README; the parameter was not confirmed.
- Outputs must be `Send + Sync`, since they live in the global cache.
  Tracked methods' arguments must be `ToOwned`, and their returns
  `Hash`. f32 would most likely need wrapping again; how comemo hashes
  f32 was not confirmed.

**Fit.**

- Better than salsa for us: no database, no cancellation by panic, and
  plain functions.
- But a global cache is wrong for us. A studio with several open
  documents and a game with its own world would share one cache with no
  owner, and eviction by age doesn't match "drop the regions out of
  view".
- Tracked impls can't be generic, and every option read becomes a method
  call.
- The per-call bookkeeping is meant for coarse functions. Our stages are
  coarse, so the cost is fine.

**Verdict: no for now.** If key drift turns out to be a real source of
bugs after S3, comemo's tracked-view idea is worth copying by hand (see
"Limits").

### (d) Others

- **`adapton`**: MPL-2.0, last release 2019, 325 recent downloads.
  Unmaintained. No.
- **`moka`** (169 M downloads, MIT/Apache) and **`quick_cache`** (36 M,
  MIT) are bounded concurrent caches, not incremental engines.
  - They could replace `world.rs`'s `Cache<T>` (a mutex around an LRU,
    once-locked per key).
  - Our `Cache` is about 60 lines and does what we need, including the
    per-key once-lock that `quick_cache` also offers.
  - Only worth it if lock contention shows up in profiles (noted as a
    risk in W4 "Not done").
- **A custom query graph** (our own small salsa: stages declare their
  inputs, and a scheduler tracks revisions). This is (a) with a generic
  layer. Not worth it for six stages. Revisit if Ecotopes, Scatters and
  Terrain all want the same machinery (ARCHITECTURE.md "Loading by
  chunk" has plan, place and draw stages that could share it).

## Terrain on the GPU

**Short answer.**

- The GPU can take everything that is evaluated **per point**:
  - chunk heights, normals and meshes;
  - ground weights;
  - G4a's measures;
  - a relief-only preview.
- It cannot take the **planning**, which is about 76% of a cold build
  without weights
  today:
  - the river trace, road routing and border negotiation;
  - the convergence and offers that feed them.
- So the GPU makes the per-chunk tail and every preview nearly free, and
  makes a per-chunk cache unnecessary for drawing. It does not replace
  the region-stage cache, and it does not help the plan.

### Which stages could move

Shares are of the cold build's single-thread CPU work, without ground
weights (see "Measurements").

| Stage | Share today | GPU? | What it would save | Why |
|---|---|---|---|---|
| Relief noise and E | inside plans and sampling | **Yes** | The preview tier (W2) for the whole view, estimated at milliseconds instead of seconds on the CPU (not measured) | Pure per-point noise over an integer lattice. This is the best fit of all. |
| Chunk height sampling (relief, E, border pulls, carving) | 23% | Yes, with work | ~23% of a cold build's CPU work, and most of a feature edit's re-read | Per point and analytic. But it reads per-region data (agreements, warped border pieces, the feature index), and the border and carving maths is written in f64 (see "Parity"). |
| Normals and gradients | inside sampling | Yes | Part of the 23% | Analytic (G2) or central differences on the sampled grid. |
| Meshing | < 0.1% | Yes, but pointless | ~0 | Could become a fixed grid mesh displaced by a height texture in the vertex shader. That removes the 18.4 MB of mesh uploads (a 67² f32 height texture is ~18 KB a chunk, against ~210 KB of mesh), which matters more than the meshing time. |
| Ground weights (GM0/GM1) | about 14× a chunk read; for the 24 near chunks alone, as much CPU as the rest of the build (measured at level 2) | **Yes, the biggest win** | The largest per-point cost in sight. A ground-only edit becomes a few dispatches, estimated near instant. | Per texel, a layer stack over local measures, 8-bit output. |
| G4a measures (slope, curvature, water and road distance, height above water) | inside weights | Yes | With weights | Per point. Distances read the feature index as a buffer. |
| Border negotiation | inside Rough (7%) | No | — | Small, per edge, and feeds the CPU's own builds. |
| River trace | a large part of each plan (the split within a plan was not timed in one run) | No | — | Downhill tracing is sequential and graph-shaped. |
| Road routing | 29% | No | — | A* over a cost grid. Its cost-grid heights (80–170 ms of it, measured earlier) could be batched on the GPU, but that needs readback into a CPU plan (see below). |
| Convergence and offers | the rest of each plan | No | — | They sample heights along edges, but only to feed negotiation on the CPU. |
| Building the feature index | inside Rough | No | — | Built from the plans on the CPU. **Reading** it on the GPU is fine: upload it as a bucketed buffer. |

### Why the plans stay on the CPU

- ARCHITECTURE rule 1 makes the CPU path the reference for queries,
  colliders, export and tests.
- D2's rule is **no readback for queries**: the CPU recomputes what it
  needs itself.
- Region plans are inputs to everything the CPU must answer: heights
  for colliders, measures for placement, features for queries. They
  must exist on the CPU anyway.
- Computing them on the GPU and reading them back would break the no-
  readback rule and make the CPU depend on the GPU.
- Doing them on both would double the work.

So the GPU's share is the **drawing side**: what the view needs, not what
queries need.

The CPU still samples the chunks it needs itself:

- around the player for colliders;
- for placement and queries.

That is a small set next to the drawn view, as for Scatters under D2.

### Parity: the CPU twin

Every GPU stage keeps its CPU twin, and parity tests hold the GPU to it
(rule 1, as for plant and stone). What parity can mean here:

- **Bit-for-bit parity is not on offer for float noise.**
  - WGSL doesn't promise IEEE-exact results: fused multiply-add,
    reassociation, and the precision of `sqrt`, `exp`, `pow` and the
    trig functions vary by GPU.
  - Our noise and border maths are float. So heights match to a
    tolerance, not to the bit.
  - A start: a few millimetres. f32 height noise is already about
    0.5 mm at 109 m (world_plan W3).
  - Stone's CPU/GPU parity tests work the same way: placement is held
    "within rounding tolerance", not to the bit.
- **Integer parts stay exact.**
  - Lattice cells come from integer coordinates, and the hash (`pcg3d`)
    is 32-bit integer maths. Both port exactly.
  - Anything **discrete** derived from heights must be quantised before
    it is decided, as D2 does for slopes: which ground layer wins, a
    label, keep or drop.
  - Otherwise a last-bit difference flips a decision and the GPU's
    picture disagrees with the CPU's queries.
- **Seams stay exact within one path.**
  - Neighbouring GPU chunks evaluate the same shader on the same inputs,
    so their shared border samples are equal on one device.
  - Where a GPU chunk meets a CPU chunk, or tiers differ, the existing
    skirts hide the tolerance.
- **No f64 on the GPU.**
  - WGSL on Metal and WebGPU has no f64.
  - Today `border.rs` is written entirely in f64, and `carve.rs`,
    `converge.rs` and `relief.rs` mostly so. World_plan W3 even asks to
    keep the height path in f64 until the final cast.
  - A GPU port must rewrite the border pulls and carving in f32 relative
    to the chunk's integer origin (the split integer-plus-fraction
    coordinates `plan.md` already prescribes), and prove the precision
    holds on warped borders far from the origin.
  - **This is the biggest cost and risk of GPU terrain.**
- **Determinism across machines.** Two GPUs may draw last-bit-different
  land. That is fine for drawing, since nothing is decided from the GPU's
  heights.

### How the GPU and the cache combine

**Caching decides what to recompute; the GPU makes recomputing cheap.**
They cover different stages:

| Work | Answer |
|---|---|
| Region plans: Plan, Rough, Fine (76%) | Cache (S3, S4) and a faster plan (S5). The GPU can't help. |
| Chunk heights for drawing | GPU, once GPU3 lands. **S3b, chunk reuse by feature footprint, then becomes unnecessary for drawing.** Re-evaluating all 86 chunks on a GPU should take a few dispatches (an estimate), so diffing footprints wouldn't be worth its key-drift risk. Until GPU3, S3b is how feature edits avoid re-reading every chunk (question 7). |
| Chunk heights for colliders and queries (CPU) | A small CPU cache near the player. |
| Ground weights and measures | GPU; ground edits need no cache at all. |
| The preview after a land edit (W2) | GPU: the whole view, estimated at milliseconds against seconds on the CPU. |

So the GPU would let us drop S3b and make W2 and W4 nearly free. The
region-stage cache is still needed: with the GPU, the plans become an
even larger share of what an edit waits for.

### What it would take

GPU terrain is a large project, not a step of this plan. In order of
value per effort:

(Named GPU1–GPU3 to keep clear of the G milestones.)

1. **GPU1. GPU preview tier.** Relief noise plus E only, no borders or
   features. It is the smallest port (noise, the integer hash, E's
   bumps), the biggest win for land edits, and needs no per-region data.
   Parity: heights within tolerance of the CPU's unconverged relief.
2. **GPU2. GPU ground weights and measures from CPU heights.** Upload each
   chunk's sampled heights (or keep them as a texture), and evaluate
   measures and layers on the GPU. Ground edits become instant. Measures
   need the feature index as a buffer.
3. **GPU3. GPU height evaluation.** Border pulls, convergence fade and
   carving, from per-region buffers. This is the f64-to-f32 rewrite, the
   largest and riskiest step. It removes the 23% sampling share and most
   chunk uploads.

**WebGPU and wasm.** Compute shaders need WebGPU. WebGL2 has none, so
the web demo would keep the CPU path there.

## Limits, stated honestly

- **Some settings invalidate everything.**
  - World seed, region size, jitter and warp, E's settings, convergence
    meeting and blend: every region.
  - `base.*` and `edges.*`: every region whose options change, which in
    the studio is all of them.
  - No cache helps here. Only less work does: W2's preview, a faster
    plan (W5), and fewer regions in view.
- **Memory.**
  - A built region holds its relief, river trace, offers and feature
    index. The default caches hold up to 64 Fine, 64 Rough, 256 plans and
    1024 settings.
  - **Measured:** the whole process peaked at 213 MB while building one
    opening view (23 plans, 9 Rough, 2 Fine, 86 chunks of samples and
    meshes). Its mesh data alone was 18.4 MB.
  - **Estimated:** that is a few MB per cached stage entry, an upper
    bound since the peak includes the chunks. If the caches filled to
    their bounds (384 region entries), they would hold roughly 1–2 GB.
  - Today a view uses a tenth of those bounds, so the bound never bites.
  - With content keys, each settled edit of a drag adds a new set of
    every stage it touches (about 200 MB for a land edit) until the LRU
    evicts. A drag that settles five times could hold about 1 GB of old
    versions.
  - Recommendation: cap versions, not just entries. Keep the current
    version plus the previous one or two per stage, and drop older
    versions when a new one lands. That bounds a drag at 2–3× one view
    (400–600 MB in this view), and still makes undo of the last edit
    free. These are estimates; measure the per-entry size when S3 is
    built.
- **Stale results from drifted keys.** This is the main risk of (a).
  - A key that leaves out a field the stage reads (a new option, a
    neighbour's road grade, a constant moved into options) gives terrain
    that is silently wrong after an edit, yet right after a restart.
  - Three guards:
    1. Each options type's `ContentHash` impl destructures `Self` (F2's
       rule), so a new field fails to compile until it is hashed.
    2. A test per stage changes each settings field one at a time, as
       the probe above does. It checks that a stage's key changes
       **whenever its output changes**, and, for the stages S4 splits,
       that its output stays the same when the key does.
    3. A debug-build check recomputes a sample of cache hits and compares
       them bit for bit, off by default.
  - Stages read a narrowed view of the options (for example a
    `RoadSettings` struct passed in), not the whole `TerrainOptions`. The
    type then shows what the key must cover. comemo automates this
    narrowing; we would do it with types.
- **Float determinism.**
  - Every stage is a pure function of its inputs, and the tests already
    require bit-identical rebuilds and order-free threads (W4: dropped
    terrains rebuild bit-identically). So a cached result equals a
    recomputed one bit for bit, on one build of one binary.
  - Keys hash floats by bits (F2 normalises `-0.0` and NaN). They never
    round, because two settings that round alike can give different
    land.
  - Results are not guaranteed identical across compilers, opt-levels or
    targets (fused multiply-add, libm). So a **disk** cache must also key
    on the crate version and target, or be dropped. This plan keeps the
    cache in memory only.
- **Analytic, never a simulation.**
  - Caching changes no math. Every stage stays closed-form or bounded per
    region.
  - The risk runs the other way: a cache can make a slow, unbounded step
    feel acceptable. The per-stage budgets in features_plan §9 still
    apply.
- **Undo and dragging.** A slider drag settles at 400 ms, and each
  settled value starts a build. W1 makes the abandoned build stop. A
  content-keyed cache makes dragging back free, but only while the
  entries survive the LRU.

## Staged build plan

Each stage stands alone and is worth doing even if the next never comes.

- **S1. Stop wasted work (W1 cancel, W4 ground-only edits).**
  - `WorldTerrain` takes a cancellation flag. The stream sets the flag on
    reset.
  - The studio sends ground and debug changes to weights and materials
    without making a new world.
  - Tests: a cancelled world caches nothing and its rebuild is
    bit-identical; a ground edit triggers no region build.
  - Exit: after an edit, no thread works on the old world beyond one
    chunk row.
- **S2. First picture fast (W2 preview, W3 nearest first).**
  - A provisional relief-only tier drawn on edit, nearest regions first,
    then Rough and Fine.
  - While editing, only the centre region is brought to full quality
    first (question 4).
  - Exit: a `base.*` edit shows new land in under 1 s on an idle machine
    in release, and the full view as today.
  - Needs a user decision on how a provisional seam may look (question
    2).
- **S3. Content-keyed stage cache, kept across edits (option a).**
  - `ContentHash` for the terrain and world options types.
  - Key every `WorldTerrain` cache by its inputs, and add the
    `WorldTerrain::with_source` swap. The studio calls it instead of
    `new`.
  - Chunk keys in the stream.
  - Tests: the per-field change test above; cold and cached results
    bit-identical.
  - Exit: undo and drag-back rebuild nothing; a ground edit rebuilds
    nothing.
- **S3b. Chunk keys by feature footprint.**
  - A chunk's key covers the relief and agreements it reads, plus a hash
    of the features within reach of it.
  - Only chunks whose key changed are re-read.
  - Tests: for every probe setting, the chunks the key keeps are
    bit-identical to a fresh read.
  - Exit: a river-width edit re-reads only the chunks along the rivers.
- **S4. Split the plan so feature edits are cheap.**
  - The Fine stage takes the road settings as its own input, not from
    the Rough terrain it clones.
  - Move road and river port wishes out of the height offers, if the
    agreed heights can stay independent of them: a separate port
    negotiation. The ports are still negotiated before any tier that
    carves them (features_plan §9: "exact seam agreements need the port
    offers").
  - Split the Plan stage into convergence, trace and offers, each cached
    on its own.
  - Hash plan outputs for early cutoff.
  - This changes the seam protocol, so it needs the user's agreement
    (question 3), and the seam, crack and feature suites must hold.
  - Exit: a `roads.width` edit keeps every plan, and a `rivers.width`
    edit keeps the convergence.
- **S5. Faster plan (W5).** A profiling pass on `build_plan` and roads,
  in release. Independent of S1–S4.

## Questions for the user

1. **Which edits must feel fast?**
   - Context: the cache helps feature, ground and undo edits. It doesn't
     help the land settings (`base.*`), which only the preview (S2) and
     a faster plan (S5) help.
   - Options: (a) land shape first: S1, S2, S5 before S3; (b) features
     and ground first: S1, S3, S4; (c) both, in the order of the staged
     plan.
   - Recommendation: (c), S1 to S3 in order, with S4 and S5 decided
     after S3 is measured.
2. **May the first picture after an edit be provisional?**
   - Context: W2 shows each region's own relief within a second. Borders
     then show steps, and rivers and roads are missing, until the real
     build lands a few seconds later.
   - Options: (a) yes, with seams and no features for a moment; (b) yes,
     but blurred or faded at borders; (c) no, keep the old land until the
     real build is ready (today).
   - Recommendation: (a) in the studio only. The game never shows
     provisional land near the player.
3. **May S4 change the seam protocol?**
   - Context:
     - Roads and rivers sit inside the border offers today, so a road
       edit rebuilds every plan, and S3 alone saves nothing on it.
     - Moving port wishes into their own negotiation, still settled
       before any tier that carves them (features_plan §9), makes road
       and river edits cheaper.
     - But it reopens the protocol that the seam, crack and feature
       suites guard.
   - Options: (a) yes, after S3; (b) only after S3b, if feature edits
     are still too slow then; (c) no.
   - Recommendation: (a). S3 alone gives feature edits nothing, so
     without S4 the cache helps only ground edits and undo.
4. **How much land should the studio wait for?**
   - Context: the opening view (3000 m view distance) touches 9 regions
     and needs 23 region plans, because each region reads its
     neighbours' plans. Only 9 are drawn.
     - A preset is about its own land, so the studio could show the
       centre region at full quality and the rest at the preview tier
       (W2).
     - Or it could draw out to a shorter distance while editing.
   - Options: (a) centre region at full quality, the rest preview, while
     editing; (b) a shorter view distance while editing; (c) keep the
     full view.
   - Recommendation: (a). The first full picture then waits for 1 region
     and its ring of plans (about 7) instead of 9 regions and 23 plans,
     and the horizon stays.
   - A release build is no shortcut: it was only 1.2× faster than dev.
5. **`salsa` or `comemo`?**
   - Context: neither is in the tree.
     - salsa: 7 new crates, pre-1.0 with frequent breaking releases,
       cancellation by panic, unclear on wasm.
     - comemo: 3 new crates, a global cache.
     - Our stages are few and coarse.
   - Options: (a) neither, hand-written keys (this plan); (b) a comemo
     spike for key tracking; (c) a salsa spike.
   - Recommendation: (a). Revisit comemo's tracked views only if
     key-drift bugs appear.
6. **Road routing is over the plan budget. What next?**
   - Context:
     - In release, one region's roads took about 5.4 s on rolling hills.
       ARCHITECTURE.md budgets a whole region plan at "up to about a
       second".
     - features_plan_review.md (Conflict 4) made macro cells wait for
       exactly this benchmark.
     - Every road edit re-routes the roads whatever the cache, so this
       sets the floor for road edits.
   - Options:
     - (a) profile and speed up the road search first (S5);
     - (b) a fixed search budget per region with a deterministic
       fallback (features_plan §9);
     - (c) macro cells now.
   - Recommendation: (a), then (b) if it is still over. Decide (c) with
     D4's region sizes.
7. **Where should GPU terrain start, and does S3b wait for it?**
   - Context:
     - The GPU can take per-point work (preview relief, chunk heights,
       ground weights, measures) but not the plans, which are about 76%
       of a cold build.
     - Ground weights are the largest per-point cost measured. On one
       thread, the near chunks' weights alone cost as much as the rest
       of the build. GM1 decides how many chunks get them.
     - The preview (GPU1) is the smallest port and the biggest win for
       land edits.
     - Full height evaluation (GPU3) needs the f64 border and carving
       maths rewritten in f32. Only GPU3 makes S3b (chunk reuse by
       footprint) unnecessary.
   - Options:
     - (a) GPU2 (weights) and GPU1 (preview) early, each after its CPU
       twin; S3b on the CPU now; GPU3 later.
     - (b) Skip S3b and go to GPU3 for all drawn terrain.
     - (c) Keep terrain on the CPU for now.
   - Recommendation: (a). Do GPU2 first if GM1 puts weights on every
     drawn chunk. S2's CPU preview is the twin GPU1 needs anyway.
8. **What parity should GPU terrain promise?**
   - Context:
     - Float noise can't match bit for bit across CPU and GPU.
     - D2 settled this for placement with integer maths and quantised
       decisions.
   - Options:
     - (a) Heights within a tolerance (a few millimetres), every discrete
       decision quantised, seams exact within one path, skirts between
       paths.
     - (b) Bit-exact, by moving the noise and border maths to fixed point
       on both sides: a rewrite, slower on the CPU.
   - Recommendation: (a). Nothing is decided from GPU heights under D2's
     no-readback rule, so a tolerance is enough.
9. **Does the CPU still sample the drawn view once the GPU draws it?**
   - Context: under D2 the CPU computes its own answers for queries and
     colliders.
   - Options:
     - (a) The CPU samples only what queries and colliders need: around
       players and on request, with a small cache.
     - (b) The CPU keeps sampling the whole view too, for tools and
       export.
   - Recommendation: (a), with export and tools asking the CPU path
     explicitly.
