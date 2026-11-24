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

## D2. Border distance and mixing (option A, distance power 16)

Borders now pull on nearby ground along their whole length (an integral
in closed form) instead of once per straight piece, so cutting an edge into
pieces changes nothing. That's the fix W3's warped borders need. The cost is
2.0–2.4× the old time per height. Results move most near border corners and
bends, because the old soft distance was short there.

20. **The distance to the border, soft, against the true distance.** The
    old method (per-piece soft minimum) came out about 8% short near any
    bent edge, because a corner counted once for each piece meeting there.
    A comes out long outside convex corners and short inside concave ones.
    You chose power 16 (from the sweep below).

    | Point (bent test border) | True | Old | A, power 8 | A, power 16 |
    |---|---|---|---|---|
    | (825, −525), outside a convex bend | 697 m | 639 m | 746 m | 730 m |
    | (600, 0) | 716 m | 652 m | 683 m | 701 m |
    | (−500, 0) | 300 m | 275 m | 326 m | 317 m |
    | (1000, 1000), near the border's open end | 919 m | 843 m | 954 m | 968 m |

    Swept over the distance power, A's largest error against the true
    distance, 10–500 m out (open ends of the test border left out), with
    how sharp the crease along a concave bend's bisector gets (the slope
    change across ±10 m; the true distance has a kink of 1.83 there):

    | Power | Bent border, long | Bent border, short | Region cell, short | Crease at 100 m / 400 m |
    |---|---|---|---|---|
    | 8 | 9.0% | 6.7% | 8.2% | 0.33 / 0.09 |
    | 12 | 7.0% | 5.0% | 5.6% | 0.43 / 0.12 |
    | 16 | 5.8% | 3.9% | 4.2% | 0.51 / 0.15 |
    | 24 | 4.5% | 2.8% | 2.8% | 0.64 / 0.21 |

    No power keeps within 3% outside convex corners (power 24 is 4.5%).
    Height time doesn't depend on the power. The power also sets the
    bending's soft distance (`Terrain` and the fade share it), so it moves
    the margins too.

    **Later option:** a correction for the overshoot outside convex corners
    (the 4.7% at (825, −525)). Revisit once warped borders exist, since the
    warp changes how many corners there are and how sharp they get.
21. **Biggest height changes A makes** (uncarved ground, against before A):

    | World | Power 8 | Power 16 | Where (power 16) | Away from any corner or bend |
    |---|---|---|---|---|
    | Two terrains | 142 m | 121 m | (825, −525), 697 m outside the convex bend at (150, −700) | 0.27 m |
    | Triple junction | 40 m | 31 m | (1185, 745), 1.2 km from the border | 2.7 m |
    | Hills into plains, seed 102 | 3 m | 2.6 m | (315, −685), 166 m from a bend | 0.17 m |

    Most of the size comes from the relief fade, which reaches kilometres;
    about half of each change is the old 8% shortfall going away.
22. **Seeds after A at power 16:** `RIVER_SEEDS` 102, 173 still cross in
    all 12 configurations, so they stay. The neighbouring-rivers test moves
    from seed 30 (its two rivers no longer both cross) to seed 217, the only
    seed of 300 whose closest pair of mountain rivers is under 195 m apart
    (159 m) with both joining a plains river; the test is unchanged.

## E. Housekeeping

18. `spike_erosion.rs` (Phase 2.8 erosion spikes, test-only, ignored) is
    committed on master. Delete it, or keep it?
19. The Codex comment rewrites for all of this are still pending (blocked
    on credits); see [codex-comment-backlog.md](codex-comment-backlog.md).
