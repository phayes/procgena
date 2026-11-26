# RB5 plan: cross-sections and earthworks (draft, 2026-11-25)

RB5 turns each road segment's stations into explicit cross-sections, the
ground they make, and semantic breaklines for CT4.

## Files

- **`src/road/section.rs` (new):**
  - `CrossSection` per station: centre level, crossfall, carriageway
    and shoulder edges, and on each side a side slope (cut or fill) that
    ends at a daylight point.
  - Daylight: walk outward along the station's normal in fixed steps
    against the pre-road ground (`H*` before roads), bounded by
    `BANK_REACH` (30 m), then refine by bisection. Fallback when the
    slope never meets the ground within reach: a retaining wall at the
    reach, recorded as a diagnostic.
  - Normals on tight curves: where the inner offset would fold
    (offset beyond the radius), the section is clipped to the arc centre
    side and the breakline vertex is merged with its neighbour, so no
    offset line self-intersects.
- **`src/road/earthworks.rs` (new):** cut and fill areas per station
  (exact for the piecewise-linear section against sampled ground),
  integrated to volumes per segment (average end area), totals in
  `RouteStats`.
- **`src/road/junction.rs` (new):** a junction footprint polygon per
  junction node (union of the meeting carriageways out to their shoulders
  over the join), normalized (counter-clockwise, no repeated or collinear
  points). The joining road's sections stop at the footprint's edge.
- **`src/road/network.rs`:** `RoadSegment` gains `sections` (one per
  station) and `earthworks` (cut and fill volumes); `RoadNetwork` gains
  `junctions: Vec<JunctionFootprint>` and `breaklines()`:
  `RoadBreakline { kind: Centre | CarriagewayEdge | ShoulderEdge | Crest | Toe,
  side, segment, points: Vec<DVec3> }`. Earthworks and breaklines stop at
  `RiverBank` nodes.
- **`src/carve.rs`:** depends on the answer to "carve from sections".
- **`src/options.rs` and `presets/terrain.meta.yml`:** depend on the
  answer to "section settings".
- **Tests:** `road::section` unit tests (flat ground, side hill, cut,
  fill, no daylight, tight curve); `tests/features.rs`: on every test
  world, footprints don't self-intersect, breaklines stay in the
  footprint, every crossing's river channel and banks between the nodes
  are unchanged (heights equal the uncarved terrain there); the corpus in
  `road_cost --stats` gains volumes and fallback counts.

## Done when

On the corpus, swept footprints are valid (no self-intersection, junctions
normalized) and between a crossing's two nodes the river channel and banks
are untouched.

## Decided with the user (2026-11-25)

- **Carve from sections:** the ground is carved from the cross-sections
  now, so `H*` and CT4's breaklines agree.
- **Settings:** new `RoadOptions` fields with defaults: shoulder 1 m,
  crossfall 2.5%, cut slope 1:1, fill slope 1:1.5 (in `terrain.meta.yml`).
- **Banking:** crown only; no superelevation on curves.
- **No daylight within reach:** a retaining wall at the reach, tagged
  `RETAINING_WALL`, counted in the road stats.

## As built (2026-11-25)

- **Carving (`src/carve.rs`, `src/road/section.rs`):** each road line is
  seen from a point through `RoadView` (the nearest segment's distance;
  half width, level and shape mixed between segments within 0.1 m of
  equally near, so the inside of a bend has no step). `RoadView::band`
  gives the heights the road holds the ground between: the road surface
  exactly on the carriageway and shoulders (crown rounded over 0.5 m),
  then the cut and fill slopes from the shoulder's edge, steepening into a
  retaining wall over the last 3 m of the reach (`BANK_REACH` less the
  shoulder). The hold is a smooth clamp whose softening grows from none at
  the shoulder to 0.2 m a metre out, so the surface is exact. Gradients come
  from the same arithmetic (`Grad`). Where roads overlap, the road listed
  first (planned first) wins. The old tolerance band (0.25 m), the
  ground-dependent bank slopes (`bank_for`) and `ROAD_BANK` are gone.
- **Open ends:** past an open end (a river bank node) every segment's
  distance is stretched, not only the last segment's (a bug in RB4 that the
  new crossing check found: the segment before the last held ground about
  7 m past the end).
- **Sections per point:** `SectionShape { shoulder, crossfall, cut, fill }`
  replaces the per-point bank steepness, eased from `SectionShape::PIECE`
  (the defaults, which every border crossing piece has, so both regions
  carve the same ground) to the region's own over the first 80 m.
- **Earthworks (`src/road/earthworks.rs`, `Terrain::road_earthworks`,
  lazily):** per station and side, the daylight found by a 1 m march from
  the shoulder's edge plus 12 halvings, against
  `Terrain::height_before_roads` (rivers and ponds cut in, roads not); cut
  and fill areas; volumes by mean end areas; walls counted. Fallback: if the
  march reaches the end of the reach, the daylight is put there and counted
  as a wall (the wall's rise to vertical makes this a rounding case only).
- **Footprints and breaklines:** offset points that fold back over the road
  are dropped, then loops are cut out of each line at its first crossing
  (`untangle`). Breaklines: centre, carriageway and shoulder edges, wall
  starts, crests and toes, as x/height/z points, broken inside junction
  footprints and where crest turns to toe.
- **Junction footprints:** the convex hull of the shoulder edges of every
  segment meeting the junction, within twice the formation width of the
  node.
