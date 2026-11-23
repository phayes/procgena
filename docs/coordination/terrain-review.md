# Terrain review (owed by the user, M0)

Everything in the terrain crate the user should look at or confirm, gathered
on 2026-11-23. Review the worlds in preview v9:
https://claude.ai/artifact/A34crTYs6dkUce3qx1QYLo

Tick an item, or note a change, as you go. Remove this file once the review
is done.

## A. Results to look at

1. **Phases 1–2.8** are built and marked "review pending" in the phase
   table of [terrain/plan.md](../../terrain/plan.md). The valleys in
   previews v4/v5 were approved, but every world has changed since (string
   seed keys), so they need a fresh look in v9.
2. **Mountain road test case** moved from seed 1 to seed 62 (a 546 m climb to
   572 m at (900, 16)); the old world's road now ends at a low hub.
   plan.md, "Road rework" table; v9 scene "Mountain road close-up, seed 62".
3. **River crossings on sloped land:** 20 of 300 worlds flat, 64 tilted
   towards the plains, 4 tilted away. world_plan.md, "W1 as built"; v9
   scenes "Hills into plains, seed 15", flat and tilted.
4. **Border gap** re-measured, for example a 50 m mean in the default world.
   plan.md, "Re-recorded after the string-key seed salts".

## B. Decisions already taken, worth confirming

5. **Roads remove a river's water wherever they cross it** (33 of 33
   crossings) until Phase 4 bridges. `feature_limits` flags one such case
   (seed 60, at a confluence); the probe is unchanged. plan.md, the bridge
   item under Phase 2.7.
6. **Bridge cost:** each road–river crossing costs 300 m of road, more if it
   slants. The repeat-crossing test allows a second crossing only with at
   least 600 m of river between (it started as "at most once"). plan.md,
   the same item.
7. **Rivers from one side may cross a border 130 m apart** (others keep
   195 m), in every world. plan.md, "Phase 2.8 as built".
8. **Deep river cuts** (from breaching hollow rims) wait for lakes in Phase
   4, as already decided. plan.md, the Phase 2.8 deep-cut section.
9. **Preset base offsets** (20 / 50 / 120 m) are still unequal; equalising
   them is deferred to W4 as its own commit with re-recorded figures.
   world_plan.md, W1 row.
10. **`tilt`** was added to `ElevationOptions` (not in the first plan). It
    grows without bound with distance, so it is for test worlds and bounded
    areas. world_plan.md, "What E looks like".
11. **E passes through borders exactly:** negotiation smooths only heights
    above E, so `negotiate()` takes `&Elevation`. Rivers start where relief,
    not E, is highest. world_plan.md, "Interface to the terrain crate" and
    "W1 as built".
12. **Default E is gentle** (400 m over 150 km for continents, 120 m over
    40 km for uplands): the real slope bound is about 16× stricter than
    amplitude ÷ wavelength. world_plan.md, "W1 as built".

## C. W2 region graph: departures from the plan sketch

world_plan.md, "W2 as built".

13. `IVec2` keys that pack exactly into `RegionId`; `f64` options; a
    `min_edge` field with a hard 390 m floor; `MAX_SIZE` 50 km, which keeps
    the exact arithmetic in range.
14. Sites snap to whole millimetres; corners are decided by exact integer
    Delaunay tests, and cocircular sites form one corner. `region_at` now
    tests nearby regions until one holds the point (fixing an old bug).
15. W4's public surface will key on `RegionId`; `RegionKey` is the
    lattice's own key.

## D. Seeds and calibration

plan.md, "Re-recorded after the string-key seed salts".

16. **Seed salts are frozen string keys**, so every world changed once.
    Re-picked test seeds: `RIVER_SEEDS` 81, 293 → 102, 173; `JOIN_SEEDS`
    64, 136 → 56, 65 (by a reconstructed criterion, the sharpest join bend,
    kept as the probe `join_bend_seeds`); the neighbouring-rivers test now
    uses seed 30.
17. **The stated relief amplitude now counts erosion** (it was 29% too high
    for rolling hills). The test checks 10 seeds per preset within 15%. This
    shifts the convergence targets of eroded presets.

## E. Housekeeping

18. `spike_erosion.rs` (Phase 2.8 erosion spikes, test-only, ignored) is
    committed on master. Delete it, or keep it?
19. The Codex comment rewrites for all of this are still pending (blocked
    on credits); see [codex-comment-backlog.md](codex-comment-backlog.md).
