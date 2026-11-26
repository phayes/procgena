# RB4 handoff (2026-11-25, done)

RB4 (canonical road network) is built and committed in
`terrain/crates/procgena_terrain`. The ROADMAP's RB4 status line has the
summary; this note keeps the details until the DS clean-up folds them in.
Decisions taken with the user:

- **Minimum radius:** a new option `RoadOptions::min_radius`, 12 m by default.
- **Carving:** the ground is carved from the fitted network.
- **Bends too tight for the radius:** the bend is kept, counted in
  `RouteStats::sharp_bends` and recorded as a `PoiKind::SharpBend` point of
  interest.
- **Bridges:** not road work. A river crossing is a pair of linked
  `RiverBank` nodes and a pair of `PoiKind::RiverCrossing` points of
  interest, linked through `partner`.

## What is in place

New files in `src/road/`:

- **`network.rs`:**
  - Types: `RoadNetwork`, `RoadNode`, `RoadNodeId`, `RoadNodeKind`, `RoadSegment`, `RoadId`, `RoadStation`, `Alignment` and `AlignmentPiece` (line or arc), `RiverCrossing`.
  - Lookup: `node()` and `segment()`.
  - `content_hash()`, which rounds to the millimetre.
- **`align.rs`:**
  - Douglas–Peucker simplification of the routed path.
  - Each corner is rounded with an arc. The radius is at least `min_radius` where the legs allow it, wider when that stays within tolerance, and the widest that fits otherwise; a corner that can't reach `min_radius` is reported as sharp.
  - `stations()` places a station every 2 m, with the ends exact.
- **`assemble.rs`:**
  - River gaps along each road: the road stops on dry ground at least 6 m from the water.
  - `nearest_on`, which finds where a road joins the network.
  - Junctions split the road they join.
  - Builds the segments, the lines to carve (with open ends at river banks) and the points of interest.

Changes to existing files:

- **`road/mod.rs` (`route`):**
  - Fit, then stations, then gaps, then levels.
  - Over a gap the levels only follow the grade, toward a straight target from bank to bank.
  - Roads can't join over a gap.
  - New `Hub` parameter.
  - `RoadStart` gains `node` and `min_radius`.
  - New fields: `Routes.{network, carved, points_of_interest}` and `RouteStats.{sharp_bends, river_crossings}`.
- **`carve.rs`:**
  - `Features::new` takes `road_open`.
  - Past an open end, the road's hold on the ground fades out within about 4 m (`OPEN_END`, `past_open_end`, with its gradient).
  - `PoiKind` gains `RiverCrossing` and `SharpBend`, and `PointOfInterest` gains `partner`.
- **`plan.rs`:**
  - Node IDs derived from the region (hub) and from the edge and port (border-piece ends, the same in both regions).
  - Crossing pieces are added to the network as segments.
  - `Roads` gains `open` and `network`.
- **`terrain.rs`:** `Terrain::road_network()`.
- **Exports:** in `lib.rs`.
- **Option metadata:** `presets/terrain.meta.yml`.

## Tests

- **All passing (2026-11-25):** doctests, `--lib` (157), `chunks`,
  `features`, `labels`, `measures`, `seams` and `shipped`, plus
  `procgena_biome --test empty_biomes`, `procgena_world --test world`,
  and `cargo check` of `procgena_terrain_bevy` and `procgena_world` (all
  targets). Clippy and `cargo doc` are clean for the road files.
- **New tests:**
  - `road::` tests: a crossing stops on both banks; a road keeps clear of a pond in its way; a junction splits a road; the network comes out the same every time; grade and radius limits hold.
  - `align` tests.
  - Open-end carving.
  - `features::roads_leave_the_rivers_they_cross_open`: no carved road runs over a river or through a pond, and each crossing's banks straddle its river or the pond it ends in.
  - `features::road_networks_are_the_same_on_any_number_of_threads`: the same hashes on 1, 3 and 8 threads, and (checked by hand with `--nocapture`) without `rayon`.
- **Tests moved, intent kept:**
  - **`chunks::mid_tier_roads_outline_the_fine_ones`:** now compares the Border and Hub node IDs instead of first points.
  - **`features::roads_cross_a_river_again_only_to_save_a_long_way_round`:** now uses the network's crossings, grouped by `origin`.
  - **`carve::carving_slopes_match_differences`:** one end of its road is now open, so the open-end gradient is checked too.
  - **`features::roads_leave_the_rivers_they_cross_open`:** a crossing may straddle the pond its river ends in, not only the river line (a road through a pond stops at its shore since the pond fix).
- **Bugs found by the full run and fixed:**
  - A road could end its crossing inside the pond a river ends in, so its embankment filled the pond (`labels`). Ponds now count as water when gaps are placed.
  - `Measures::road_distance` used the carving distance, which is stretched past an open road end; it now uses the true distance to the road's edge (`measures`).

## Corpus timing

`cargo run --release -p procgena_terrain --example road_cost -- --corpus --stats`,
now also printing sharp bends and crossings. Run at a load average of
about 8 to 11 (other agents busy), so times are not comparable with
`terrain/roads_baseline.md`'s quiet runs:

| Measure | After RB1 + RB3 (quiet) | After RB4 (load ~10) |
|---|---|---|
| Fine tier ("roads ms"), median / max | 72 / 145 ms | 148 / 275 ms |
| Routing (outline + fine), median / max | 49 / 92 ms | 84 / 157 ms |
| Route searches alone, median / max | 14 / 34 ms | 18 / 44 ms |
| Ports connected | 117 / 117 | 117 / 117 |
| Sharp bends, river crossings | | 22, 11 |
| Worst cut / fill | 12.5 / 15.8 m | 17.4 / 15.0 m |
| Smallest radius | | 1.4 m (a recorded sharp bend) |
| Carved roads over a river | 11 | 1 (mountains 3/2; see shortcuts) |

Rerun on a quiet machine before comparing times.

## Visual check

`--example seam_spike -- <dir> 0.5 shipped` draws smooth roads with
junctions and roads stopping at rivers. At 2 m a pixel it is too coarse to
judge arc radii or the bank gaps; a closer view is for the user. In seed 1
a junction sits right at a river bend, with one road running close beside
the river, worth a closer look.

## Known shortcuts

- A crossing whose gap would reach a road's first or last station (or
  finds no dry ground within 300 m) is skipped, and the road fills across
  that river. One case in the corpus: mountains seed 3, region 2.
- The road's level over a gap is grade-limited only; there is no
  clearance above the water.
- The fitted line may stray up to half a cell (4 m) from the routed
  cells, which can bring it slightly inside the border clearance.
- The route search doesn't see ponds; only the gap finder does.

## Follow-ups

- Roads over gullies: "Maybe: roads over gullies" in the ROADMAP's "Later
  in the track".
- The skipped crossing above: handle a gap at a road's end by moving the
  end (or the hub) instead of filling the river.
