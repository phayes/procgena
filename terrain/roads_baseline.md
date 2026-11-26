# Road builder baseline (RB0)

The road plan's Stage 0 baseline, recorded 2026-11-25 on the road builder
as TE5 left it (coarse 16 m outline at the Mid tier, then 8 m refined
routes in a 48 m corridor at the Fine tier). Each RB milestone compares
against these numbers.

```bash
cargo run --release -p procgena_terrain --example road_cost -- --corpus --stats
```

- **Corpus:** every shipped preset (plains, rolling hills, mountains), world
  seeds 1 to 3, the region at the origin and its two nearest neighbours: 27
  regions at the default `medium_small` scale class (2 km regions). Rows are
  numbered 0 to 2 per seed, nearest first.
- **Machine:** Apple silicon laptop, release build, `rayon` on, load
  average about 2 (other agents idle).
- **Times** vary by about 10% from run to run. Counts are exact.

## Summary

| Measure | Value |
|---|---|
| Fine tier on its own ("roads ms"), median / max | 360 ms / 963 ms |
| Of which routing (outline + fine), median / max | 203 ms / 505 ms |
| Of which route searches alone, median / max | 22 ms / 48 ms |
| Ports connected | 117 / 117 |
| Searches, states popped (all regions, both tiers) | 235, 2.07 M |
| Crowded routes kept | 0 |
| Peak search state memory | 44.6 MB |
| Worst max grade / p95 grade | 0.250 / 0.250 (mountains' `max_grade`) |
| Worst cut / fill against the ground before roads | 12.5 m / 15.8 m |
| River crossings, smallest crossing angle | 11, 67° |
| Crowded points | 0 |

## Where the time goes

Per region, in ms. "Routing" is `RouteStats::total_time` for the outline
and the fine roads. "Other" is the rest of the fine tier: carving the
roads into the ground, their banks, and measuring how far the tier moved
the ground. "Outline grid" is sampling the ground on the 16 m outline grid
over the whole region.

| Region | Roads | Routing | Outline | Fine | Other | Outline grid |
|---|---:|---:|---:|---:|---:|---:|
| plains 1/0 | 322 | 177 | 118 | 59 | 145 | 94 |
| plains 1/1 | 382 | 214 | 150 | 64 | 168 | 109 |
| plains 1/2 | 353 | 190 | 124 | 66 | 163 | 91 |
| plains 2/0 | 510 | 225 | 140 | 85 | 285 | 94 |
| plains 2/1 | 319 | 181 | 130 | 51 | 138 | 103 |
| plains 2/2 | 337 | 175 | 109 | 66 | 162 | 79 |
| plains 3/0 | 360 | 165 | 99 | 66 | 195 | 74 |
| plains 3/1 | 296 | 124 | 76 | 48 | 172 | 59 |
| plains 3/2 | 391 | 189 | 109 | 80 | 202 | 77 |
| rolling_hills 1/0 | 347 | 191 | 127 | 64 | 156 | 96 |
| rolling_hills 1/1 | 358 | 210 | 141 | 69 | 148 | 109 |
| rolling_hills 1/2 | 344 | 186 | 118 | 68 | 158 | 92 |
| rolling_hills 2/0 | 345 | 192 | 125 | 67 | 153 | 93 |
| rolling_hills 2/1 | 385 | 212 | 138 | 74 | 173 | 105 |
| rolling_hills 2/2 | 339 | 177 | 107 | 70 | 162 | 78 |
| rolling_hills 3/0 | 288 | 152 | 98 | 54 | 136 | 74 |
| rolling_hills 3/1 | 324 | 146 | 81 | 65 | 178 | 59 |
| rolling_hills 3/2 | 431 | 203 | 112 | 91 | 228 | 79 |
| mountains 1/0 | 393 | 313 | 282 | 31 | 80 | 247 |
| mountains 1/1 | 963 | 505 | 337 | 168 | 458 | 257 |
| mountains 1/2 | 568 | 376 | 300 | 76 | 192 | 235 |
| mountains 2/0 | 491 | 332 | 280 | 52 | 159 | 238 |
| mountains 2/1 | 302 | 302 | 302 | 0 | 0 | 271 |
| mountains 2/2 | 813 | 411 | 256 | 155 | 402 | 198 |
| mountains 3/0 | 858 | 441 | 262 | 179 | 417 | 194 |
| mountains 3/1 | 890 | 386 | 209 | 177 | 504 | 154 |
| mountains 3/2 | 532 | 335 | 256 | 79 | 197 | 200 |

(mountains 2/1 has no road crossings, so only its outline grid is built.)

## After RB1 and RB3 (2026-11-25)

RB1 was re-aimed at what the baseline showed (decided with the user):

- **Parallel sampling.** The routing grid's cells, the per-cell grades,
  the levels' ground and grade samples, road banks and the tier
  difference (`moved`) sample through the crate's ordered `batch::map`,
  so they use every core with `rayon` and give the same values without
  it.
- **Compact search state.** Search state is held only for the cells a
  road may pass through, in a scratch reused by every search of a region
  (generation-stamped, never cleared). States keep their order, so ties
  break as before.
- **Turn table and join levels.** The turn table is built once per run;
  the network's level at a cell is looked up once per start instead of
  per visit.

RB3 replaced the 40-step bisection in `grade_limited` with the
closed-form smallest gap (pairwise interval conditions, one pass with
running maxima). A randomized test checks it against bisection on more
than 8,000 random roads, with levels agreeing to 0.1 mm.

| Measure | Baseline | After RB1 + RB3 |
|---|---|---|
| Fine tier ("roads ms"), median / max | 360 / 963 ms | 72 / 145 ms |
| Routing (outline + fine), median / max | 203 / 505 ms | 49 / 92 ms |
| Route searches alone, median / max | 22 / 48 ms | 14 / 34 ms |
| Peak search state memory | 44.6 MB | 6.4 MB |
| Searches, states popped | 235, 2,065,842 | 235, 2,065,842 |
| Quality (every measure above) | | unchanged |

Routes are identical (same searches, same states popped, same quality
figures). The remaining time is spread roughly evenly across the
mountains' outline heights (about 25 ms, already parallel), the searches,
and carving the roads into the ground. The shared reverse cost field
(RB2) is deferred until the larger World classes show the search
dominating there.

## What this says about the redesign

- **The route search is not the bottleneck at 2 km.** Searches take 2 to
  31 ms per tier, about 6% of the fine tier. The road plan's premise
  (section 4: repeated full-grid searches dominate) holds for its
  asymptotic model, not for today's 2 km regions: TE5's coarse outline and
  corridor already cut the search down. The shared cost field (RB2) can
  save at most the search time here. It matters more at the 8 and 16 km
  classes, which this corpus doesn't cover yet (`--scale large`).
- **Ground sampling dominates.** The 16 m outline grid's heights (one
  `base_height` call per cell over the whole region, on one thread) take 60
  to 270 ms. The fine tier's "other" work (carving and measuring the
  ground) takes 80 to 500 ms. Together they are about 80% of the tier.
- **The fine grid is mostly empty.** Only 2 to 10% of the 8 m grid's cells
  lie in the corridor, but search state is allocated for the whole box
  (up to 38 MB per search). Compact active cells (RB1) pay off in memory
  first.
- **Quality to keep:** no crowded legs, every port connected, crossings no
  sharper than 67°. The 0.4 m minimum radius comes from Chaikin smoothing
  of knight moves next to diagonal moves, not from the route.
