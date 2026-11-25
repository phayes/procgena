# Road planning revamp (TE5)

## 1. Where road time goes (measured 2026-11-25)

### How it was measured

- `cargo run --release -p procgena_terrain --example road_cost` builds the
  rough tier of each region first, then times the fine tier on its own. The
  fine tier is the roads: routing, levels, gathering the features and
  `moved`.
- It used the shipped presets (`plains`, `rolling_hills`, `mountains`) on the
  default world: seeds 1–3, the region at the origin and the three nearest
  around it, 36 regions in all.
- The machine was shared with other agents (load average about 14), so the
  absolute times are noisy. The shares below come from a profile and hold
  better.
- The breakdown comes from an Instruments Time Profiler run on `mountains`,
  seeds 1–2, two regions each, about 6,300 one-millisecond samples in the
  fine tier.

### Totals per region (release)

| preset        | roads median | roads max |
|---------------|-------------:|----------:|
| plains        |       751 ms |   1038 ms |
| rolling_hills |      1128 ms |   1321 ms |
| mountains     |      1606 ms |   2093 ms |
| all 36        |      1128 ms |   2093 ms |

- These times are well below the "about 5 s per near region" in ROADMAP. That
  figure probably came from a dev build or from many regions building at once
  in the studio.
- They are still over the target of "well under a second".
- A mountains region with **no roads at all** took 1554 ms. The cost doesn't
  depend on how many roads a region has.

### Breakdown of the fine tier (mountains)

| part                                                           | share |
|----------------------------------------------------------------|------:|
| `Grid::with_cell`: the 8 m height grid, one `Terrain::base` read per cell | ~63% |
| per-cell `relief.road_grades` (`Fade::roads`)                  | ~18% |
| A* search, heap, levels, `across_road`, `banks`, `hub`         | ~10% |
| `moved` and `gather` (outside `plan::roads`)                   |  ~6% |
| `level_on_roads`                                               | below 0.5%, not visible |

Within both of the large items, most of the time is the border blending
rather than the relief noise:

- `border::power_integral` is 36% of all fine-tier time on its own (30% as the
  leaf function).
- `converge::Fade::blended` takes 28% and `Fade::scales` 12%.
- The relief noise (`fbm`, `ridged`, `gradient_noise`) takes about 10%.

### What this changes from the ROADMAP's guesses

- **Confirmed:** the grid's height and grade reads are the cost, at about 80%.
- **Not confirmed:**
  - The A* state space, the reruns for refused or crowded joins and the
    full-grid distance pass together are about 10%.
  - `level_on_roads` doesn't show up in the profile.
  - Removing reruns or switching to HPA* would save at most that 10%.
- **What helps, in order:**
  1. **Read fewer cells at full cost.** Coarse-to-fine routing reads a
     32–64 m grid (16–64× fewer cells), then reads 8 m cells only in a
     corridor around the route. This also gives TE7 the coarse route it
     needs for its middle tier.
  2. **Make each read cheaper.** Read the grid from a coarser height source
     and interpolate. For example, use the rough tier's samples, or
     evaluate the border blending once per coarse cell. Or take the
     border-pull speed-ups in the TE5 "Also" item (fewer border pieces,
     cheaper far-piece integrals). Those edit `converge.rs`/`border.rs`, and
     the user hasn't decided yet whether they belong in TE5.
  3. Read the grades at coarse resolution only, since they vary slowly.
  4. Search improvements last. They are small.

## 2. Candidates

| candidate | targets | expected saving | status |
|---|---|---|---|
| coarse-to-fine (outline grid, then an 8 m corridor) | the grid reads (~80%) | most of them | prototyped, below |
| cheaper reads (interpolate from a coarser height source) | each grid read | large, but heights near the roads become approximate | not tried |
| grades read only on the coarse grid | ~18% | a part of the grades | in effect, the corridor already does it |
| no reruns, HPA*, a faster `level_on_roads` | the search (~10%) | at most 10% | not worth it on its own |
| a spanning-tree skeleton, then fitting | the grid reads | similar to coarse-to-fine | not tried: it needs its own corridor reads, a new road look, and an all-new join and level logic |
| border-pull speed-ups (`converge.rs`) | every height read | large, workspace-wide | out of TE5 (the user decided on 2026-11-25: roads only) |

### Coarse-to-fine, as prototyped

1. `road::coarse_routes`: the existing `route` on a grid of `COARSE_CELL`
   metres, with the same starts, rivers and grades. This is the road network
   in outline. It is also the coarse route TE7 wants for a middle feature
   tier: same crossings and levels, planned without the fine stage.
2. `road::refined_routes`: the existing `route` on the 8 m grid, with
   `Grid::with_cell_where` counting only cells within `CORRIDOR` (48 m) of
   the outline as inside. Heights and grades are read only there. The hub is
   the outline's hub. Every refined road stays inside the corridor, so a road
   doesn't jump when a chunk switches from the outline to the refined route.
3. The search, joins, grade limits, crowding and bridges are unchanged.

### The outline's cell size

- With 32 m cells, `roads_cross_a_river_again_only_to_save_a_long_way_round`
  failed in one test world. A road crossed one river twice, 480 m apart
  along the river. The outline grid is too coarse to tell sides of a winding
  river apart, and the corridor then fixes the double crossing.
- With 16 m cells it passes, and the features, seams and shipped suites all
  pass.

### Timings, release, run back to back at the same load

Load average was about 26–30 because other agents were running, so the
absolute times are inflated. Before this run, a quieter run of the 32 m
outline gave a 214 ms median.

|               | old median | old max | 16 m outline median | 16 m outline max |
|---------------|-----------:|--------:|--------------------:|-----------------:|
| plains        |     839 ms | 1516 ms |              340 ms |           384 ms |
| rolling_hills |    1228 ms | 1579 ms |              406 ms |           562 ms |
| mountains     |    1886 ms | 2809 ms |              644 ms |          1431 ms |
| all 36        |    1210 ms | 2809 ms |              384 ms |          1431 ms |

That is about 3× faster at the median. The worst mountains region is still
around a second under heavy load.

### Determinism and look

- Both grids are fixed functions of the region, its crossings and its
  rivers, so roads stay deterministic per region. Crossings are unchanged
  (the seams suite passes).
- The networks change: the hub comes from the outline, so roads meet in
  different places and a few legs run straighter. The number of road lines
  per region is unchanged in all 36 timed regions.
- Old and new are side by side in `target/te5_roads_seed1.png` and
  `target/te5_roads_seed2.png` (`seam_spike shipped`, 4 km square).
- The user still has to accept the look.

### Decision

- On 2026-11-25 the user accepted the look, and coarse-to-fine with a 16 m
  outline landed.
- The hub is now picked on the outline grid, so every network ends
  somewhere new. A region with one crossing can get a much shorter or
  longer road: in mountains seed 1, region 0, a 3.9 km switchback road
  became 0.7 km. The user accepted this.
- A 24 m outline, between the 32 m that failed and the 16 m that passes,
  was not tried.
- If the worst regions must come down further, the grid reads are still the
  cost. There are two options:
  - read the corridor's heights in parallel;
  - the border speed-ups, which are out of TE5.
