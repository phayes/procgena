# Canonical ground and roads: decisions so far (for workers)

These are the user's decisions and the choices already built into the road
builder (RB) and canonical TIN (CT) work, up to 2026-11-25. Read this before
you design anything in the CT or RB milestones. The status of each milestone
lives in ROADMAP.md ("Canonical ground and roads track").

## Sources

- **The implementation plan** for RB0–RB3 and CT0–CT2, with notes on the later
  milestones: `/Users/phayes/.claude/plans/ok-plan-mode-make-merry-shannon-of-9f175e1b-e2c2-4b43-beb7-f98fd4697123.md`.
- **The design documents:**
  - `docs/procgena-canonical-tin-design.md`. Its section 21 is the review
    against the code, with the D6 decisions.
  - `docs/procgena-road-builder-redesign.md`. Its section 17 is the review.
- **The RB4 handoff:** `docs/coordination/rb4-handoff.md`.
- **The full coordinator transcript:**
  `/Users/phayes/.claude/projects/-opt-procgena/9f175e1b-e2c2-4b43-beb7-f98fd4697123.jsonl`.
  It is large (14 MB), so search it rather than reading it whole.

## Gate D6 (decided by the user, 2026-11-25)

- **D6a, determinism.** Results are bitwise identical on one platform first.
  `std` maths stays. Pinned maths, for identity across platforms, is a later
  milestone.
- **D6b, which surface decides what.** The analytic composed surface `H*` (the
  terrain's height at `FeatureTier::Fine`) decides spots, masks, slope and
  labels, at every distance. The realized TIN gives collision and the rest
  height of near copies. Far copies rest on `H*`. A copy's identity never
  includes its rest height. A TIN query (`GroundSurface`) sits beside
  `Ground`, not in place of it. Slope for placement never comes from raw face
  gradients.
- **D6c, constraints.** Only level-0 chunk borders are mesh constraints. Region
  borders are not, so the `R ∩ C` patch work is superseded, and region
  ownership comes from the region graph. A region border is constrained only
  where something needs an edge there.
- **D6d, coordinates.** Canonical integer centimetres: `i64` x/z and `i32`
  heights (`Cm`, `CmHeight`, `NumericProfile::SamePlatformV2`). Decided by
  the user 2026-11-26: cm, superseding mm. spade works in exact
  chunk-local `f64`.
- **Feature tiers.** Only the Fine tier is canonical. Rough and Mid stay
  provisional, with today's skirts and stand-ins, so edge contracts exist for
  Fine chunks only.

## Other choices by the user

- **Triangulator:** spade 2.15, already in Cargo.lock through parry3d. spade's
  own `refine()` is not used, because it would split locked boundary edges.
- **Hashes:** the core `ContentHasher`, not BLAKE3.
- **Where the code lives:** the TIN is a module, `procgena_terrain::tin`, not a
  new crate.
- **Road effort (after RB0):** aimed at sampling. RB1 became parallel, batched
  sampling plus compact active cells. RB2 (the shared reverse cost field) is
  deferred.
- **Bridges are not road work.** The user's words: "All the roads produce are
  two linked nodes (like a POI) that later passes fill." A river crossing is
  two linked `RiverBank` nodes, plus a pair of `PoiKind::RiverCrossing` points
  linked through `partner`. Earthworks end at the two nodes, and the river
  channel and banks between them stay untouched.
- **RB4:**
  - `RoadOptions::min_radius` is a new option, 12 m by default.
  - The ground is carved from the fitted network.
  - A bend too tight for the radius is kept. It is counted in
    `RouteStats::sharp_bends` and recorded as a `PoiKind::SharpBend` point of
    interest ("could be useful to later passes").
  - Roads stop at least 6 m from rivers and ponds.
- **Gullies:** crossing gullies the way roads cross rivers is only a possible
  follow-up (ROADMAP, "Later in the track"), not scheduled.
- **The project rule:** terrain stays analytic, or bounded per region. There is
  no simulation creep.
- **Codex comment rewrites:** deferred ("Later"). Log each lane's new and
  changed comments in `docs/coordination/codex-comment-backlog.md`.

## How CT0–CT2 were built (so later work fits)

- **Sampling:** each chunk samples `H*` on the 1 m lattice through
  `ChunkSource::surfaces(.., FeatureTier::Fine)`. Since CT-A (ROADMAP) the
  inside is sampled coarse to fine (`src/tin/sampling.rs`): every 8 m
  where the ground is close to flat, down to every metre where it bends,
  and every metre near water and roads, in one batch per level. Edges are
  still read every metre and handed to the inside. `TinSampling::EveryMetre`
  (`TinTolerance::DENSE`) restores CT2's full lattice.
- **Refinement:** Garland–Heckbert over that lattice. Each step splits the worst
  triangle at its worst sample, using exact integer orientation tests and
  `i128` interpolation. Ties go to the sample nearest the centre, then by row
  and column.
- **Tolerance:** since CT3, `TinTolerance { general, water, water_band, chord,
  vertex_budget }`. `general` and `water` are each a
  `DomainTolerance { height, gradient, max_edge }`. CT2's `GENERAL` was 0.5 m,
  32 m and 4,096 vertices. `min_spacing` was removed. Since CT-A it also has
  `sampling` (`TinSampling::Adaptive` or `EveryMetre`), and named levels
  `DENSE`, `GENERAL` (the default), `LOW_POLY` and `VERY_LOW_POLY`; one
  tolerance serves a whole world (`WorldTerrainOptions::tin`, read through
  `ChunkSource::tin_tolerance`), so chunk edges match (see ROADMAP CT-A).
- **Edge contracts:** `EdgeContract::build` samples every 1 m along the edge and
  splits at the worst sample, or at the midpoint when a segment is too long.
  Since CT3 a contract also carries `crossings`. It is a pure function of the
  edge key, `H*` and the feature lines from `lines_within` over the edge.
- **Winding:** triangles face up, with negative orientation in the (x, z) plane.
  spade's counter-clockwise faces are flipped to `[a, c, b]`.
- **Pinned hash:** `GOLDEN_PLANE` in `tests/tin.rs`, for chunk (-1, 2) at
  `GENERAL`.
- **Built in CT3:** `ChunkSource::lines_within`, gradient error, and tolerance
  profiles per domain.
- **River banks (the user, 2026-11-25):** there is no bank-top line. Banks are
  the ground within `water_band` (5 m) of the water's edge, held to the water
  tolerance. The kept lines are each river's lowest line, its water's edges,
  and pond shores.
- **No slope check on open land (the user, 2026-11-25):** gradient error is
  checked only in the water band. At 0.05 on general terrain, rolling hills
  would go from 66 to 2,923 triangles per chunk. Placement, labels and shading
  read the exact ground, so they don't need it.
- **Contract cache:** built in CT-A (`ContractCache`, `build_chunk_with`).
