# Ecotope draw time, and the order of the GPU work

Measured 2026-11-24 on an M1 Max (10 cores) running the studio's Ecotope
tab: the pine forest floor over 40 m of rolling hills. The long-term goal
is settled: **GPU everywhere**, meaning placement, masks and building for
every Scatter. This document uses the profile to order the GPU milestones,
and names the CPU-side and Bevy-side wins worth doing alongside them.

**Every absolute time here is provisional.** The machine was shared with
other sessions' builds and studios, at a load average of 150–400, which
inflates times by up to 3× from one run to the next. The **shares, counts
and orderings** held in every run. A quiet rerun is owed (question 6).

## Built so far (2026-11-24)

Three pieces are built:

- ED's stone-cache fix;
- ED's "re-place only from the edited Scatter down";
- R3a: the studio's Ecotope stones come from stone's GPU mesher through
  one pool per class, with an impostor level.

**Before and after.** These were measured back to back on the same
machine, at a load of 80–130. Each run started with an empty impostor
cache. The "before" build is master at `94f835a`.

| | Before | After |
|---|---:|---:|
| Cold load, until the last bake is done | 33.5 s | 12.7 s |
| Edit porcini: placing / everything drawn | 1,355 ms / 1.64 s | 5 ms / 0.22 s |
| Edit logs: placing / everything drawn | 1,327 ms / 1.92 s | 152 ms / 0.62 s |
| Edit pines: placing / everything drawn | 1,440 ms / 6.41 s | 52 ms / 0.85 s |

- **Cold placement is unchanged** (about 1.2 s, mostly measuring stone
  shapes on the CPU).
- **The logs edit still builds dead wood once per copy.** That is ED's
  next item.
- **"Everything drawn" for pooled stones counts their impostor bakes.**
  The GPU meshes themselves are not timed separately; they take
  milliseconds.

**Look:** pooled stones now cast a shadow on the ground, which the old
per-copy rocks didn't show, and their shading differs slightly. Other
copies are unchanged: 0.18% of the viewport changed, all of it at
stones. The crops (`stone_look_before_after.png`) are in the session
notes folder. This look change waits for the user's OK.

## Bottom line

**Where the time goes.** Placing copies is almost free; building stones is
not. On a cold load, the time grows with the number of unique specimens
and the number of new render pipelines, not with the number of copies.

| Stage | Share of the time | Notes |
|-------|-------------------|-------|
| Stones | about 94% of all CPU work (provisional) | Meshing costs about 1.2–2.7 s of CPU for each of the 7 stone variants. Measuring their shapes during placement costs about 2–3 s more. |
| Placement itself | under 20 ms of CPU | The lattice, `K_d` spacing, masks and exclusion, for all five Scatters together. |
| Long frames | none to tens of seconds | Once per process, on the first view. In the one sampled run they were render-pipeline compiles: Bevy 0.20 compiles them synchronously on macOS. The size varied widely between runs. |
| Impostor bakes | about 3–4 s when the impostor disk cache is cold | None when it is warm. |

**Edits are slow for different reasons:**

- An edit re-places the whole Ecotope from the top (1.5–2 s, mostly
  measuring stone shapes again), even when only the mushrooms changed.
- An edit high in the stack throws away the stone meshes it should have
  kept, through a cache-pruning bug (about 6 s).
- Dead wood is built once per copy instead of once per variant.

**What fixes it, in order:**

1. **ED: Ecotope draw speed (new, now).** Bevy-side and CPU-side wins, each
   worth doing in its own right:
   - re-place only from the edited Scatter down, keeping each Scatter's
     prepared shapes;
   - keep specimen caches across a redraw (fixes the stone rebuild);
   - build dead wood once per variant;
   - warm up the render pipelines ahead of time;
   - bake impostors only when one could be on screen.

   Together these should take a typical edit from 2–8 s to well under a
   second, and remove most of the first view's long frames.
2. **R3a: stone GPU building in the Ecotope (new, pulled out of R3).** The
   GPU mesher already exists in `procgena_stone` (feature `gpu`). Its pool
   is about 25–55× faster per rock than the CPU at the resolutions stones
   use (provisional). The studio and the Ecotope don't use it yet. Using it
   needs an impostor level in the pool and
   Ecotope wiring. This is the largest single win in the profile, and the
   first piece of "building everywhere on the GPU".
3. **The rest of R3, in this order: deadwood, mushroom, tree, then
   plant's CPU path.** Each is cheap per variant today. They matter at
   World scale, where a pool can hold up to 256 specimens.
4. **GP: GPU placement and masks (new).** This builds D2. It buys almost
   nothing in the studio's 40 m patch, because placement already costs
   under 20 ms there. It is what makes streaming possible: D2's spike
   placed a 64 m chunk in 0.74 ms on the GPU, against about 10 ms for the
   unoptimised CPU twin.

   It follows C3 and plant P3/P4, and gates S1.
5. **IX and I2 run alongside.** IX's "level 1 by default" also cuts build
   time: a stone's level 0 is about 70% of its build cost. I2 moves bakes
   off the first view.

## Measurements

### Method

- **A throwaway headless timeline in the studio** printed a line whenever
  anything changed, plus every frame over 100 ms. It tracked placement,
  each kind's `*Generating` markers, impostors waiting and being written,
  and `Mesh3d` entities.
- **A throwaway engine-free harness** timed each stage the way the Bevy
  crates run it, in process CPU time.
- **`sample`** profiled the process during one cold load.
- **Stone's own examples** were run: `bench` and `bench_gpu`.

None of this code is committed. In the session notes folder,
`ecotope-gpu-profile/` holds:

- `ecotope_cpu_harness.rs`, the harness;
- `studio_timeline_plugin.rs` and `throwaway_hooks.diff`, the studio
  hooks;
- every raw output.

**Headless isn't the windowed studio.** Headless turns off pipelined
rendering, so a stall shows up as one long frame. In the window, the
render thread stalls instead, and the user sees the same thing. Release
builds were no faster than dev builds at this load, so they don't fix the
noise.

### What runs where today

- **`Ecotope::place`** runs as one background task, serially, Scatter by
  Scatter, and runs again on every edit. Stone's shape measuring inside it
  fans out over rayon.
- **Specimen builds** (tree, stone and mushroom per unique specimen, dead
  wood per copy) run on Bevy's `AsyncComputeTaskPool`. The sample shows
  only **3 such threads on 10 cores**, Bevy's default share. Stone's own
  meshing fans out further over rayon.
- **Render pipelines** are compiled inside the render schedule. On macOS,
  Bevy 0.20 always compiles with `block_on` (`create_pipeline_task`'s
  macOS branch ignores `synchronous_pipeline_compilation`), so each new
  specialization stalls a frame.
- **Impostor bakes** draw on the GPU, then read back, build mip chains and
  write to disk on background tasks.
- **Mesh and material uploads** and entity spawning happen on the main
  thread. They never showed up in the sample next to the items above.

### What one Ecotope is

| Scatter | Kind | Copies | Unique specimens |
|---------|------|-------:|-----------------:|
| pines | tree | 44 | 10 |
| boulders | stone (two classes) | 51 | 7 |
| logs | deadwood | 17 | 3 |
| porcini | mushroom | 171 | 3 bodies |
| fly agarics | mushroom | 44 | 2 bodies |

Trees, stones and logs are drawn as **one entity per copy**, with child
mesh entities for each level and part. They don't use R2's instanced
renderer: about 1,400 `Mesh3d` entities at the peak, for 327 copies.
Mushrooms go through the instanced renderer, and are placed a second time
by `procgena_mushroom_bevy` (about 2 ms).

### CPU cost by stage (dev profile, process CPU including rayon helpers)

These are CPU times, not wall times, from two runs at loads of about 240
and 320. The release run, at a load of 330–400, measured about 1.5–1.7×
these values, which shows how much the load inflates CPU time too.

**Placement** (`Ecotope::place`, one background task):

| Scatter | Total | Of which, building specimens | Of which, placing (lattice, `K_d`, masks) |
|---------|------:|------------------------------|------------------------------------------:|
| pines | 0.4 ms | none | 0.4 ms |
| boulders | 1,815–1,889 ms | **1,814–1,888 ms** (`StoneScatter::prepare` measures 7 shapes) | 0.6–1.2 ms |
| logs | 74–77 ms | 58–63 ms (`Piece::new` × 3) | about 15 ms |
| porcini | 0.9 ms | none | 0.9 ms |
| fly agarics | 0.5 ms | none | 0.5 ms |
| **Ecotope** | **1.9–2.2 s** | **about 99%** | **under 20 ms** |

**Specimen meshes**, built by the Bevy crates after placement:

| Kind | Jobs | CPU each | Total |
|------|-----:|---------:|------:|
| Stone (`Stone::new`, levels 96/48/24) | 7 | 1.2–1.6 s | **10.0–10.5 s** |
| Dead wood, **built once per copy** | 17 | 34–38 ms | 646–828 ms (3 builds would do: about 110 ms) |
| Mushroom (placed again, then 5 bodies meshed) | 2 | 9–13 ms | 21–25 ms |
| Tree (pines, 4 levels each) | 10 | 3.5–4.6 ms | 33–39 ms |
| Broadleaf trees, for comparison (oak, ash, big-leaf maple, large) | each | 0.6–5 ms | |

**Stone by detail level.** Each level was built alone, in process CPU:

| Stone | All three levels | Level 0 (96) alone | Level 1 (48) alone | Level 2 (24) alone |
|-------|-----------------:|-------------------:|-------------------:|-------------------:|
| lichen boulder | 1,483 ms | 1,320 ms | 425 ms | 299 ms |
| river stone | 1,210 ms | 1,052 ms | 343 ms | 250 ms |

Building the field costs about 250 ms whatever the level, and level 0 adds
about 1 s on top. Building from level 1 down cuts a stone by about 3×.

**Stone on the GPU against the CPU** (stone's `bench_gpu` and `bench`, in
ms per rock):

| Resolution | GPU, batches of 16 | CPU, dev | CPU, release (load 330+) |
|-----------:|-------------------:|---------:|-------------------------:|
| 64 | 21–24 | 270–420 | 730–1,020 |
| 100 | 28–36 | 900–1,130 | 2,540–2,750 |
| 128 | 36–50 | 1,740–2,030 | 4,490–4,880 |

The GPU numbers include preparing the batch on the CPU. Against the dev
CPU, the GPU is about 13–17× faster at resolution 64 and 25–55× faster at
100–128. That ratio is provisional, since the load inflated the CPU side
more than the GPU side.

### Studio timeline, cold load

These are wall times from selecting the Ecotope. "Last work done" is the
end of the last bake or long frame, including any frame that ran past the
timeline's own "done" mark.

| Run | Load | Placement done | Specimens drawn | Last work done | Longest frames |
|-----|-----:|---------------:|----------------:|---------------:|----------------|
| Cold impostor cache | 180 | 0.9 s | 5.4 s | 14.3 s | 5.0, 3.0, 1.3 s |
| Cold impostor cache, sampled | about 180 | 2.2 s | about 11 s | 40.7 s | 17.7, 8.7, 6.3 s |
| Warm impostor cache | 225 | 3.4 s | 21.3 s | 47.9 s | 25.6, 9.1, 8.4 s |
| Cold impostor cache | 247 | 3.0 s | 15.6 s | about 16 s | 2.3, 1.2, 1.2 s |
| Warm impostor cache | 273 | 1.5 s | 8.3 s | 8.3 s | 0.5 s (none long) |

**What the long frames were, in the sampled run.** The main thread spent
about 70% of its time waiting inside the render schedule. The work it
waited on was `PipelineCache::process_queue` → `create_render_pipeline` →
Metal's `newLibraryWithSource` through `MTLCompilerService`. That is about
6,000 of 13,500 samples, or 44% of the window. Sampling itself slows the
process, so that run's times are the longest.

The other runs' long frames have the same shape: they come once, while
new materials and passes appear, and never during an edit. They are very
likely pipeline compiles too, but only the sampled run proves it. Their
size swung from none to 25 s.

That may depend on Metal's shader cache and on other sessions using the
same compiler service. This is a guess. The last run had no long frame.

The rest of the time in the sampled run went to:

- impostor readbacks, mip chains and writes (`procgena_impostor::cache::save`,
  about 3,300 samples on background threads);
- command-buffer finishing (about 900 samples).

**Critical path without long frames** (last row): about 1.5 s of
placement, mostly stone shapes, then about 7 s of stone meshes. Every tree
is drawn by 1.7 s, every log by 3.4 s, and the last stone at 8.3 s.

### Studio timeline, one-setting edits

These are from the first run (load 180); the stone-cache run (load 273)
agrees.

| Edit | Placement | Redrawn | Done | What filled the time |
|------|----------:|---------|-----:|----------------------|
| porcini density × 0.5 | 1.5–2.1 s | porcini, fly agarics | 1.8–2.2 s | Re-placing the whole Ecotope, mostly measuring stone shapes again. Mushrooms themselves take about 50 ms. |
| logs density × 0.8 | 1.6–1.7 s | logs and below | 2.3–2.5 s | Re-placing, then 17 log builds (one per copy) |
| pines density_scale × 0.9 | 1.5–1.8 s | everything | 7.8–8.3 s | Re-placing, then **all 7 stones built again** |

**The stone rebuild is confirmed** with a log line in stone's
`prune_cache`: `kept 0 of 7, 0 keyed entities`, followed by 7 cache misses.

The Ecotope's redraw despawns the old layer and spawns the new one in one
command flush. Stone's prune then ran at a moment when no stone entity
had a `StoneKey`. The trees kept their cache through the same edit; why
they differ is not confirmed. It may be when each crate's marker and key
systems run relative to `procgena_biome_bevy`'s.

Frames during an edit ran at 60–250 ms on this loaded machine. The
studio's own UI and the 636 `Mesh3d` entities set that floor; this
profile doesn't separate the two.

### Not measured

- **Grass.** Plant P2 hasn't landed. Plant already places and builds on
  the GPU.
- **Upload times** for individual meshes and textures.
- **GPU frame time** for the drawn Ecotope.
- **The windowed studio.** Everything above ran headless.
- **More async compute threads.** Stone's meshing already fans out over
  rayon, so a larger `AsyncComputeTaskPool` is likely a small win. Not
  tried.

## Proposed milestones

All of them serve the GPU-everywhere goal. ED and R3a come first because
they fix most of what the user feels in the studio today. None of them
replaces GP or the rest of R3.

### ED: Ecotope draw speed

- **Covers:**
  1. **Edits re-place only from the edited Scatter down.** Keep each
     placed layer whose settings, seed, ground, area and coverage are
     unchanged and that has nothing changed above it. Keep each Scatter's
     `prepare` (stone shapes, dead wood pieces) by settings and seed.
     `procgena_biome_bevy` already works out which layers it may keep
     drawing; this keeps their placement too.
     - *Payoff:* the porcini and logs edits spend 1.5–2.1 s placing, and
       about 99% of that is measuring stone shapes again. With this,
       re-placing costs milliseconds.
  2. **Specimen caches survive a redraw.** Stone, and check dead wood
     and mushroom, must not prune entries that a layer swap is about to
     reuse. Hold keys across the swap, or prune a frame late.
     - *Payoff:* about 6 s of the 8 s pines edit.
  3. **Dead wood is built once per variant**, not once per copy, and
     reuses the pieces placement already built.
     - *Payoff:* 17 builds become 3, saving about 0.5–0.8 s of CPU per
       log edit or cold load.
  4. **Warm up the pipelines.** When an Ecotope document opens, request
     the material, mesh-layout and pass specializations an Ecotope uses,
     impostor bake pipelines included, before the copies arrive. This
     keeps the stalls off the moment the copies appear, since macOS
     compiles them synchronously.
     - *Payoff:* the long first-view frames: none to tens of seconds
       (provisional). Check it against a windowed run.
  5. **Bake impostors only when needed.** In the studio, bake a
     specimen's impostor once its impostor level could be on screen at
     the current orbit limits (the 40 m patch never reaches the trees'
     800 m), and keep the disk cache.
     - *Payoff:* about 3–4 s of bakes and readbacks on a cold cache, plus
       their pipeline compiles. I2 later prepares them ahead of time.
  6. **Optional:** a larger `AsyncComputeTaskPool` share in the studio.
     Unmeasured, and likely small.
- **Needs:** C3a (built). Items 2 and 3 touch only the asset Bevy crates'
  caches. Items 1, 4 and 5 edit `procgena_biome_bevy` and the studio, so
  they wait for GM1 and plant P2 to leave `procgena_biome_bevy` and agree
  with ST1.
- **Done when:** in the studio's Ecotope tab, on a quiet machine, with
  timings recorded in this document:
  - a mushroom-only edit shows its result in under 0.3 s;
  - an edit to the top Scatter rebuilds no stone, tree or log whose
    settings didn't change;
  - a second open of an Ecotope in the same session has no frame over
    100 ms.

### R3a: Stone GPU building in the Ecotope

- **Covers:**
  - Ecotope stones are built by `procgena_stone`'s GPU mesher and drawn
    through `procgena_stone_bevy`'s pool (`StonePool`, feature `gpu`) on
    R2's renderer, instead of one `StoneParams` entity per copy.
  - **One pool per stone class.** A `StonePool` made of the class's stone
    settings, variant count and stretch jitter, in the Scatter's World,
    already gives exactly the class's variants (both call
    `StoneOptions::variant`), so the pool needs no per-variant settings.
    A test pins this.
  - **The pool gains an impostor level** (I1 left impostors in the pool
    unbuilt). Pool meshes never reach the CPU, so each variant's impostor
    is baked from its baked level built on the CPU, the reference path,
    in the background.
  - `procgena_biome_bevy`'s stone layer spawns pooled stones.
  - The studio enables `procgena_stone_bevy/gpu`.
  - The CPU path stays the reference for export, colliders and parity
    (rule 1).
  - The footprint measure in `StoneScatter::prepare` stays on the CPU,
    because placement depends on it. It is kept across edits (ED item 1).
- **Payoff (estimate):** 10–16 s of stone CPU becomes about 0.5 s of GPU
  time for 7 variants at three levels each. Stone meshing is the critical
  path of a cold load once the long frames are gone.
- **Needs:** R2 (built). It edits `stone/` and `procgena_biome_bevy`'s
  stone layer, so the latter part comes after GM1 and P2 there, like ED.
- **Done when:** the pine forest floor's stones come from the GPU in the
  studio, matching the CPU meshes within stone's existing parity
  tolerance, with impostors at their distance.

### R3, ordered by the profile

R3's worker per asset stays. The profile only sets the order: **stone
(R3a) → deadwood → mushroom → tree**, with plant's part following
`plant/alignment_plan.md`.

- **Today's costs per variant:**
  - dead wood: 34–58 ms;
  - a mushroom body: 4–7 ms;
  - a tree with all four levels: 1–6 ms.
- **At World scale:** a pool may hold up to 256 specimens (D3), so dead
  wood reaches about 9–15 s of CPU per pool and trees about 1.5 s. The
  GPU paths are what keep a streamed World's first view and its edits
  bounded.

### GP: GPU placement and masks

- **Covers:** building D2's decided design ("Masks and placement" in
  ARCHITECTURE.md, rule 1):
  - one placement model for every Scatter, on a World-anchored lattice
    with `K_d` spacing (`d` = 5 by default, per Scatter);
  - 8-bit mask textures per chunk on the shared chunk grid, chained down
    the Ecotope by priority, with exact integer disc tests for hard
    exclusion;
  - integer maths throughout (32-bit hashes, fixed-point positions at
    1/256 m relative to the chunk, ranks compared by cross-multiplying,
    and the quantised slope byte), so the CPU and the GPU agree bit for
    bit;
  - the CPU twin (C3's CPU placement) answers queries, colliders and
    promotion, and never reads GPU placement back;
  - window B streaming, with each chunk's Scatters encoded in one compute
    pass, so many Scatters and chunks run concurrently on the GPU;
  - the GPU path that C2 left out;
  - the timing runs D2 still owes, on a low-end integrated GPU and on a
    discrete GPU, as its first step.
- **Payoff:**
  - In the studio's 40 m patch, placement costs under 20 ms of CPU, so
    GP saves almost nothing there.
  - Its payoff is at scale. D2's spike put the whole stack, down to logs,
    at 0.74 ms of GPU per 64 m chunk in window B. The unoptimised CPU twin
    took about 10 ms per chunk in window A. A 1 km view radius holds about
    770 chunks.
  - Grass, already placed on the GPU, then shares the Ecotope's masks
    with no CPU bake.
- **Needs:**
  - C3, whose CPU Ecotope on the `Scatter` trait is the twin;
  - C2 and R2 (built);
  - plant P3 (World cells and integer placement) and P4 (the sibling
    trait for GPU-placed Scatters), so that grass joins the same chain;
  - D2's owed timing runs.
- **Done when:**
  - The pine forest floor, with grass, is placed on the GPU chunk by
    chunk.
  - It is bit-identical to the CPU twin in any build order, with
    readback used only in tests.
  - It stays within D2's budget of 2 ms per chunk on an integrated GPU.
  - The studio draws it with no CPU placement on the draw path.
- **Gates:** S1 (streaming). Whether it also gates S1's early
  single-Biome start is question 4.

### How these fit R3, C3, P3/P4, I2 and IX

| Milestone | Fit |
|-----------|-----|
| ED, R3a | Start now, beside wave 8, with their `procgena_biome_bevy` parts after GM1 and P2. |
| C3 | GP's twin, as above. ED's "re-place from the edited Scatter down" carries over to C3's Ecotope. Drawing every Ecotope kind through R2's renderer, ending the entity per copy for trees and logs, has no owner yet (question 3). |
| P3 / P4 | GP needs both. Grass already places on the GPU, so GP's mask chain is what joins it to the rest. |
| I2 | Moves bakes off the first view, a step past ED item 5. Independent of GP. |
| IX | Its "level 1 by default" has a measured build payoff: a stone's level 0 is about 70% of its CPU build. Build level 0 only on demand, as an input to IX's planning. IX's measurements should include build time, not only frame time. |
| S1 | Needs GP, besides I2 and R2. |

## Decided (user, 2026-11-24)

1. **ED is split, and its first parts start now.** ED item 2 (stone
   caches survive a redraw) and item 1 (re-place only from the edited
   Scatter down) were built first. The rest of ED (dead wood once per
   variant, pipeline warm-up, impostors only when needed) follows.
2. **R3a goes ahead of the rest of R3,** which then runs deadwood →
   mushroom → tree.

## Questions for the user

1. *(Decided above.)*
2. *(Decided above.)*
3. **Who moves Ecotope trees and logs onto R2's renderer?**
   - *Context:* the Ecotope draws them as one entity per copy (about
     1,400 mesh entities for 327 copies). R3a moves stones; mushrooms are
     already instanced.
   - *Options:*
     - (a) C3, as part of replacing the adapters with the `Scatter` trait;
     - (b) ED, as a later item;
     - (c) GP, since its GPU-placed copies must feed the renderer anyway.
   - *Recommendation:* (a). C3 rewrites that code path anyway, and GP
     then inherits it.
4. **Does GP gate S1's single-Biome start?**
   - *Context:* the roadmap lets S1 start early on one Biome with B1,
     G6, R2 and I2. Streaming on CPU placement would work for one small
     Biome. The CPU twin costs about 10 ms per chunk (unoptimised),
     against D2's 5 ms budget for it.
   - *Options:*
     - (a) S1 needs GP from the start;
     - (b) single-Biome S1 may start on CPU placement, and World-wide S1
       needs GP.
   - *Recommendation:* (b). It keeps streaming work unblocked and
     exercises the twin that GP is tested against.
5. **When to run D2's owed timing runs.**
   - *Context:* GP needs C3's CPU Ecotope as its twin and P4's trait for
     grass. The spike ran only on an M1 Max; an integrated GPU may change
     `d` or the window choice, which C3 would want to know.
   - *Options:*
     - (a) with GP, after C3 and P4;
     - (b) now, on the spike's code, on a low-end integrated GPU and a
       discrete GPU.
   - *Recommendation:* (b). The kernels themselves still go after C3.
6. **A quiet-machine rerun.**
   - *Context:* every wall time here was taken at a load of 150–400, and
     ED's done-when needs real numbers.
   - *Options:*
     - (a) one window with no other sessions building, to rerun the
       timeline and harness from the notes folder before ED starts;
     - (b) let ED measure its own before and after on whatever machine
       state it finds.
   - *Recommendation:* (a). It is a few minutes, and it fixes the
     baseline for every later milestone here.
