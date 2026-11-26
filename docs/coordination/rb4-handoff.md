# RB4 handoff (2026-11-25, uncommitted, in progress)

RB4 (canonical road network) is mostly built and uncommitted in
`terrain/crates/procgena_terrain`. Decisions taken with the user:

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

- **Passing:**
  - `--lib`: all 156.
  - `--test chunks` and `--test features`.
- **New tests:**
  - `road::` tests: a crossing stops on both banks; a junction splits a road; the network comes out the same every time; grade and radius limits hold.
  - `align` tests.
  - Open-end carving.
  - `features::roads_leave_the_rivers_they_cross_open`.
- **Tests moved, intent kept:**
  - **`chunks::mid_tier_roads_outline_the_fine_ones`:** now compares the Border and Hub node IDs instead of first points.
  - **`features::roads_cross_a_river_again_only_to_save_a_long_way_round`:** now uses the network's crossings, grouped by `origin`.
  - **`carve::carving_slopes_match_differences`:** one end of its road is now open, so the open-end gradient is checked too.
- **Not yet checked:** a full `cargo test -p procgena_terrain --tests` run was started; its results are in the session scratchpad. Those results, plus `seams`, `measures`, `labels` and `shipped`, need checking.

## Full test run results

- **Passing:** `--lib`, `batch_sampling`, `chunks`, `convergence`, `elevation`, `features`, `ground` and `ground_layers`.
- **Failing:** `labels::water_and_roads_get_their_labels` panicked at `tests/labels.rs:161`: it got `Shore` where it expected `Pondbed`. Not looked into yet. A likely cause is that the ground near a pond changed now that roads are arcs and stop at river banks.
- **Not reached:** cargo stopped at that failure, so `measures`, `probes`, `seams`, `shipped` and the rest still need running, with `--no-fail-fast`.

## Next steps

0. Look into the `labels` failure above, then rerun with `--no-fail-fast`.
1. Rerun `cargo test -p procgena_terrain --tests`. `shipped`, `labels` and `measures` take minutes, and their figures may move now that roads are arcs and have river gaps.
2. Run these checks:
   - `cargo clippy -p procgena_terrain --all-targets -- -D warnings`
   - `cargo doc -p procgena_terrain --no-deps`
   - `--doc` tests
3. Check the dependent crates: `procgena_world` (`tests/world.rs`), `procgena_biome` (`tests/empty_biomes.rs`), and `procgena_terrain_bevy` and the studio, which may match on the now `#[non_exhaustive]` `PoiKind`.
4. Measure with `cargo run --release -p procgena_terrain --example road_cost -- --corpus --stats`. Compare against `terrain/roads_baseline.md`, and add sharp bends and crossings to the example's output.
5. Add a test that a network hash is the same with and without `rayon`, and with different thread counts (RB4's "done when").
6. Look at it with `--example seam_spike` to judge the arc shapes and the gap at each crossing.
7. Update the ROADMAP's RB4 status line and the road plan's §10.2, then commit. Log the comment rewrite in `codex-comment-backlog.md`.

A possible follow-up, not scheduled: roads crossing gullies. It's in the
ROADMAP's "Later in the track" list as "Maybe: roads over gullies".

Known shortcuts:

- A crossing whose gap would reach a road's first or last station is skipped, and the road still fills across that river.
- The road's level over a gap is grade-limited only; there is no clearance above the water.
- The fitted line may stray up to half a cell (4 m) from the routed cells, which can bring it slightly inside the border clearance.
