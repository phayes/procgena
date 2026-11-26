# Decisions taken while the user was away (2026-11-26)

The user said: "I will be away when you start so go to the advisor with any
questions. Dont ask me." The coordinator took these decisions, using the
advisor where needed, for the user to review. The items marked **Review**
are the ones most worth a look.

## Terrain speed

1. **Review: the adaptive TIN default is GENERAL (the RELAXED rule).** You
   decided that the allowed error becomes a world setting. The coordinator
   picked the default from the worker's trade-off curve, using your
   guidance: "a slightly looser mesh if we get major speedups", and 2× as a
   guide, not a cap.
   - **Reads per chunk:** plains need about 12× fewer `H*` reads, hills
     1.8–3× fewer, mountains 1.0–1.2× fewer.
   - **Error:** max error stays within the dense build's max. The worst
     overshoot on the 1 m lattice is 131 mm, 1.26× the tolerance.
   - **Named levels:** DENSE, GENERAL, LOW_POLY (1 m) and VERY_LOW_POLY
     (3 m). They are set in `WorldOptions::ground_mesh`.
   - **River band:** stays at 0.10 m at every level.
   - **Where to look:** the curve is in ROADMAP's CT-A block.
2. **Review: cheap borders (TB), the decisions inside your approval:**
   - **Plans build their own border profiles,** from settings alone, so
     rivers are traced on the final ground. Plans no longer read
     neighbours' plans.
   - **The test whose scenario vanished** (`neighbouring_mountain_rivers…`)
     now runs on hand-made land inside the test. It was not retired.
   - **A border limit loosened:** the curvature limit at borders goes from
     0.01 to 0.02 (0.011 measured). It follows from the slope creases you
     allowed.
   - **Unused settings:** `Convergence::blend` and `EdgeOptions::roughness`
     now have no effect, and their docs say so. They will be removed in a
     separate step.
   - **Known issue:** faint star-shaped slope creases near sharp border
     bends and corners. A close-up is in `terrain/spike/tb_creases.png`, and
     a follow-up is listed in the TB block.
   - **Kept as is:** border stencils still don't read the neighbour's
     height. ARCHITECTURE's "Labels at region borders" stands. The code
     review had called it a bug; it is a decision you may revisit.
3. **No river carving bake (TE6).** River carving was only about 9% of
   chunk reads along a river. A bake would have made `H*` piecewise at
   borders. Two fixes that leave the output bitwise identical landed
   instead.
4. **The elevation field E's precision fix.** E had rounding steps of up to
   1.7 mm. It now sums in higher precision (its own commit). The crack test
   was not loosened, and E got about 25% faster.
5. **Option 1 (TO), and the work order.** Disabled features now cost
   nothing, and the output is bitwise identical. Speed-up option 3 (each
   chunk classified against borders once) is folded into TB. Option 4
   (sharing noise work) waits until TB lands.
6. **The double chunk read** (`d43b824`, `9329dec`, `8a64669`). A chunk is
   no longer read twice when a higher tier can't change its ground. The
   result is bitwise identical and tested.
7. **Main-thread stutter.**
   - Chunks are no longer re-chosen every frame (`2c25ab9`).
   - No per-frame budget for paint uploads yet, because paint will return
     differently.
   - Bevy's own start-up stalls are left to ED's warm-up.

## Superseded or overtaken

8. **The first deadlock fix** (`5fad02d`, with its claim flags and helper
   threads) is superseded by your decision to remove Rayon (TS). The caches
   are now plain build-once on one thread.
9. **Paint slope from height differences** (GPU plan step 1b) is
   cancelled. You turned paint off until the cheaper TIN lands, then paint
   on the GPU from the TIN. Step 1a (skipping unused curvature) did land,
   bit-identical.

## Demo

10. **Review: the play area.**
    - **Area:** the plan's 1.5 km area (7×7 tiles of 220 m) stays.
    - **Start:** play starts once the 3×3 tiles around the start are
      placed, and the rest load in the background.
    - **Porcini:** halved, in the demo's own Ecotope preset only.
    - **The keep-off mask:** a cached per-tile label raster in the engine,
      if one is needed, waits for re-measuring after TB.

## Waiting for the user

- **The GPU plan's questions 3–6** (`terrain/gpu_plan.md`):
  - canonical ground on the GPU only after a fixed-point `H*`;
  - GPU labels for paint;
  - 1 cm parity on far levels;
  - a 2 ms per-frame GPU budget.
- **The docs clean-up's questions Q6–Q8**, in the DS session.
- **CT4 (roads in the TIN)** is held while the focus is loading speed.
