# Terrain edits: caching and incremental rebuilds

A decision document, not a build log. It answers three questions the user
asked: will a cache make terrain editing fast, what are its limits, and
should we use something like `salsa`. Measured 2026-11-24 on master at
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
3. **Key each stage by what it reads (stage S3).** Give each `WorldTerrain`
   stage a content-hash key made from exactly the settings it reads, and
   keep the caches across edits. Edits to rivers, roads, borders and
   ground then rebuild only the later stages. Edits to the land's shape
   still rebuild nearly everything.

**Will it work?** In part, and the answer depends on the setting.

Today a cold build of the studio's opening view takes about 40 s on one
thread in release: 23 region plans, 9 region builds, roads in 2 regions
and 86 chunks. A one-setting edit costs exactly the same.

- **Ground colours and debug drawing:** yes, nearly all the time is
  saved. Neither changes heights.
- **Roads, rivers and border settings:** partly.
  - The probe shows most rebuilt work changes nothing. A road edit moved
    no Rough height and only a quarter of the near chunks. A river-width
    edit moved 4 chunks in 177.
  - But roads (27% of the time) are re-routed on every road edit.
  - The plans (51%) are kept only if road and river port wishes move out
    of the border offers (stage S4).
  - Expect 20–70% saved on these edits, depending on how far S3 and S4
    go.
- **Land shape (`base.*`) and the seed:** no. Every region's relief
  changes, so every later stage must run again. No cache, salsa
  included, can skip work whose inputs really changed. Here only less
  work helps: a preview tier, fewer regions to wait for, a faster plan.
- **The optimisation level is not the answer.** Release is only about
  1.2× faster than the dev profile here.

In the studio, the land settings are the ones people move most. So the
preview and cancelling come first, and the cache second.

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
- Memory is small next to what is drawn. Floats are bit-exact today
  because each stage is pure. A cache must never key on rounded values.
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
  dependencies at 3) and release. One thread, one cold run, then one edit
  (`features.roads.width` + 1 m).
- **The machine was loaded by other agents.** Load average was 84–245 on
  10 cores.
  - `/usr/bin/time` shows what share of a core each run got. The release
    run got about 93% (81 s user in 87 s), so its wall times are close to
    an idle machine's.
  - The dev run got about 39% (94.6 s user in 244 s). Its wall times are
    about 2.6× too long.
  - The counts (regions, chunks, builds) don't depend on load.

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

Time by stage, one thread. Release is the main column; dev is shown
beside it for scale (loaded, so its wall times run long):

| Stage | Release (s) | Share | Dev, loaded (s) | What it is |
|---|---|---|---|---|
| World, region graph, E, border warp | < 0.005 per region | ~0 | < 0.005 | `WorldTerrain::new`, `RegionGraph::region` |
| Plan: settings, convergence, river trace, offers | 20.2 (23 × 0.88) | 51% | 73.4 | `build_plan` |
| Rough: negotiate, bend, hydrology (rivers, ponds) | 4.6 (9 × 0.51) | 12% | 15.2 | `build_roughest`, without the nested plans |
| Fine: roads | 10.8 (2 × 5.4) | 27% | 28.1 | `Terrain::with_tier(Fine)` |
| Chunk sampling (warm, regions built) | 3.7 (86 × 43 ms) | 9% | 8.5 | `ChunkSamples::sample` |
| Meshing | 0.02 | < 0.1% | 0.04 | `ChunkSamples::mesh` |
| Ground weights (GM0) | not measured | — | — | `ChunkWeights::build` |
| Upload to the GPU | not measured | — | — | Bevy `Mesh` creation and upload |
| **Total, Rough pass + Fine pass** | **39.7** | | **124** | |

Notes:

- **Release is only about 1.2× faster than dev in CPU time.** Release
  used 81 s of CPU for two builds and two warm re-reads; dev used 94.6 s
  for the same. The dev profile already optimises our crates at level 1
  and every dependency at level 3. So **the optimisation level is not
  the fix: the work is.**
- **The studio, with 4 reads at once, needs roughly 10–20 s to the full
  view** in release on an idle machine. That is an estimate, not
  measured.
  - Once-locks make readers wait on one region's build.
  - The ring of plans must finish before the first Rough region.
  - The roads of a near region take 5 s on one thread whatever else
    runs.
  - The earlier "0.3 s per 10 km region" in `world_plan.md` predates
    border warp, convergence and the road rework.
- **The Rough figure** is the hook's Rough total less the plans built
  inside it: 24.8 − 20.2 s in release, 88.5 − 73.4 s in dev. Every plan
  was built inside a Rough build.
- **Inside one plan** (the centre region, re-run on its converged
  generator; dev, under heavy load, so read as shares):
  - river trace 1.22 s;
  - offers 1.01 s;
  - settings (`TerrainGenerator::new`) under 0.01 ms;
  - region graph and warp 4 ms.

  That leaves convergence and generator cloning as the rest of the plan,
  about a third. So the plan is roughly a third convergence, a third
  trace and a third offers. A cached lookup costs 1 µs.
- **Ground weights: not measured.** The harness timed weights only at
  levels 0–1, and the opening view has no chunks there.
  - By point count, a weights chunk reads 129² = 16,641 points with
    gradients through the same terrain query. A height read takes about
    6,000 points without gradients.
  - So weights cost roughly 3× a chunk read, on top of it: an estimate of
    20–25% of a cold build if every drawn chunk gets weights.
  - They need the built regions but nothing else. A ground-only edit is
    therefore pure weights work.
  - GM1, which draws the colours, is in progress, and how many levels
    get weights is its call.
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

- Studio world, centre region, dev build.
- Its level-2 chunks (256 m) that lie wholly inside the region: 177 of
  them, up to the region's borders.
- Each chunk's vertex heights are hashed at Rough and at Fine.
- Relief is hashed on a 41 × 41 grid; rivers and roads by their
  polylines.

| Setting changed | Relief | River lines | Road lines | Rough chunks changed | Fine chunks changed |
|---|---|---|---|---|---|
| `base.hills.height` +5 m | changed | changed | changed | 177 / 177 | 177 / 177 |
| `features.rivers.width` +1 m | same | same | same | 4 / 177 | 4 / 177 |
| `features.roads.width` +1 m | same | same | same | **0** / 177 | 47 / 177 |
| `features.roads.grade` +0.01 | same | same | changed | **0** / 177 | 29 / 177 |
| `features.roads.enabled` toggled | same | same | changed | **0** / 177 | 47 / 177 |
| `edges.margin` +20 m | same | same | same | 42 / 177 | 42 / 177 |

"Relief" is `natural_height` of the built terrain's generator. It did not
change with `edges.margin`, which the code says moves the border fade.
So `natural_height` probably reads the relief before the fade, and the
column is only a check that the relief did not change. The chunk columns
are the ground truth.

**Most of the work an edit redoes today changes nothing.**

- A road edit reruns every plan and every Rough build (63% of the time),
  yet no Rough height moves. Only the roads (27%) and about a quarter of
  the near chunks change.
- A river-width edit moves 2% of the chunks.
- A border-margin edit moves the border bands, about a quarter of the
  chunks.

This is the evidence that a stage cache keyed finely enough, with early
cutoff on outputs, would pay off for feature and border edits.

Reading the table:

- **`base.*`** changes the unconverged relief. Everything after the
  Settings stage changes, in every region.
- **`edges.*`** goes into the offers and the convergence fade. Every
  plan changes, but the rivers did not move. The heights change only in
  the border bands: 42 of 177 chunks.
- **Roads.**
  - By the code, `roads.width` and `roads.enabled` go into the offers'
    port wishes. `roads.grade` and `roads.max_grade` go into the
    convergence targets and the offers.
  - So an input-keyed cache must rebuild every plan on a road edit, the
    most expensive stage.
  - By the probe, no Rough height moved. A cache that keys the Rough
    tier on the **agreed heights**, not on the whole offers, would keep
    it.
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

## Options

What each one saves, and what it costs. "Saves" is measured against the
39.7 s release cold build above, whose shares are:

- plans 51%;
- Rough builds 12%;
- roads 27%;
- chunk sampling 9%.

### Cheap wins (no cache)

| Win | Saves | Costs |
|---|---|---|
| **W1. Cancel stale builds.** Pass a generation token or `Arc<AtomicBool>` into `WorldTerrain`. Check it between region stages and between chunk rows; a cancelled build returns an error that is never cached. | Wasted CPU after each edit: up to 4 threads × one region build. Mostly battery, plus a faster second edit during a drag. | Small. One flag check per stage. Results are unchanged, because a cancelled build is dropped, never stored. |
| **W2. Relief-only preview tier.** A `FeatureTier::Preview` (or a separate preview source) that draws each region's **unconverged** relief plus E from its Settings alone, without plans or borders. Show it right after an edit, then swap in Rough and Fine as they land. | Time to the first picture of a `base.*` edit: from all 23 plans to sampling only, about 9% of the total (~4 s on one thread, ~1 s on four), or less if previewed at a coarser level. | Borders show seams for a few seconds, and rivers and roads are missing. A new tier for chunks and skirts. Must be marked provisional and never queried (features_plan §9, "Canonical terrain versus display LOD"). |
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

**What it saves** (no shape change to the stages):

| Edit | Rebuilt | Saved |
|---|---|---|
| Ground colours, debug drawing | weights or nothing | ~100% of the land time |
| `roads.*` | plans; Rough kept by early cutoff on agreed heights (the probe moved none); roads; the near chunks that changed (about a quarter) | Rough 12% + about 7% of sampling, so ~19% |
| `rivers.*` | plans, Rough, roads; only the chunks that moved | about 9% (sampling) |
| `edges.*` | everything; only the border-band chunks re-read | about 7% (sampling) |
| `base.*`, seed | everything | 0% |
| Undo, redo, or dragging back to an earlier value | nothing, if the old entries are still kept | ~100% |

With the stage split in S4 (road and river port wishes out of the height
offers, so the relief, convergence and trace are separate stages):

| Edit | Rebuilt | Saved |
|---|---|---|
| `roads.*` | offers' road ports, roads (Fine tier), the near chunks that changed | plans, Rough and most sampling: ~70% |
| `rivers.*` | trace, offers, hydrology, roads, the few chunks that moved | convergence (about a third of each plan) and most sampling: ~25% |

Roads (27%) are rebuilt on every road edit under every option. Making a
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
- Tracked-function arguments must be `Eq + Hash`. Our options are f32
  throughout, so every argument needs a bit-wrapper, which is the same
  key discipline as (a).
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

- Salsa wants a database handle per thread. Setting an input cancels
  running queries **by unwinding** (a `Cancelled` panic). Long loops
  must call `unwind_if_revision_cancelled` themselves.
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
- The cache is global and evicted by age (`comemo::evict`).
- Outputs must be `Send + Sync + Clone`. Tracked methods' arguments must
  be `ToOwned + Hash`, and their returns `Hash`, so f32 needs wrapping
  again.

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
  - Keeping one earlier version of each (for undo or drag-back) at most
    doubles that.
  - Chunk samples are about 18 MB for the opening view. Keeping the last
    set is cheap, and keeping many is not.
  - Recommendation: keep the current bounds, keyed by content, so old
    versions fall out by LRU naturally.
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
- **S4. Split the plan so feature edits are cheap.**
  - Move road and river port wishes out of the height offers, if the
    agreed heights can stay independent of them: a separate port
    negotiation.
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
   - Context: roads and rivers sit inside the border offers today, so a
     road edit rebuilds every plan. Moving port wishes into their own
     negotiation makes road and river edits cheap. But it reopens the
     protocol that the seam, crack and feature suites guard.
   - Options: (a) yes, after S3; (b) only if S3 shows feature edits are
     still too slow; (c) no.
   - Recommendation: (b).
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
