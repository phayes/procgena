# Decisions taken while the user was away (2026-11-26)

The user said: "I will be away when you start so go to the advisor with any
questions. Dont ask me." These decisions were taken by the coordinator,
with the advisor where needed, for the user to review.

1. **The deadlock fix in the `WorldTerrain` cache is accepted** (`5fad02d`).
   - Each key is still built exactly once, and no lock is held while it
     builds.
   - A build claimed on a Rayon worker runs on its own thread.
   - Threads that wait help with queued Rayon work, or sleep between
     checks.
   - Possible later refinement: replace the sleeping (20 µs up to 1 ms)
     with a wake-up signal.
2. **No river carving bake (TE6).**
   - The profile found that river carving is a small share of the time:
     about 9% of chunk reads along a river and much less over a whole
     view.
   - A bake would make `H*` piecewise at region borders, and it would leak
     relief detail into river beds.
   - `H*` is unchanged. TE6 lands two fixes that leave the output bitwise
     identical:
     - one faded-relief read per cell in `own_rivers`, instead of two;
     - a parallel `with_heights` pass.
   - The real cost per point is the border fade and pulls, about 88% of
     the chunk reads along a river.
3. **Adaptive TIN sampling's default tolerance (CT-A)** is chosen by the
   coordinator from the worker's trade-off curve. The user allowed "a
   slightly looser mesh if we get major speedups" and asked the coordinator
   to choose. The rule: take the knee of the curve. About 2× today's max
   error is a guide, not a cap: the user "might accept more if the tradeoff
   is worth it". The river band, determinism and identical chunk
   edges are not loosened. The curve and the choice are in ROADMAP's CT-A
   block, for review.
4. **How the speed-up options are being run**
   (`terrain/speedup_options.md`):
   - **Option 3** (classify each chunk against borders) is folded into the
     cheap-borders worker's design.
   - **Option 1** (disabled features cost nothing, lazy plans) goes to the
     TE6 worker once the Rayon removal (TS) lands. Its output must stay
     bitwise identical.
   - **Built-in presets with roads and rivers off:** committed `55b5638`,
     with tests kept on intent. Cold build on one thread: about 45–48%
     fewer instructions.
   - **Option 4** (sharing noise evaluations) waits until the borders land,
     because both edit `relief.rs`.
   - **The GPU plan's 6 open questions** wait for the user. Nothing that
     depends on them has started.
5. **Superseded by the user (2026-11-26): terrain paint is off by default**
   until adaptive TIN sampling lands. Then painting from the TIN is to be
   explored. Step 1b is cancelled. Step 1a lands only if it's already done,
   since it's bit-identical. The original entry follows.
   **Paint slope from height differences (GPU plan open questions 1 and
   2): yes, for paint only.** The load profile found paint sites are 92% of
   the terrain worker's CPU. Step 1b gives paint sites heights only, takes
   slope from differences on the CPU and GPU alike, and shares one height
   grid between mesh and paint. Acceptance: slope error p95 under 1°, and
   material edges within one texel. Scatters and queries keep exact slopes.
   The GPU plan's open questions 3–6 still wait for the user.
6. **The canonical unit becomes integer centimetres (decided by the user,
   2026-11-26),** superseding D6d's millimetres. The adaptive TIN worker
   makes the change in `src/tin/` along with its rework.
   - Water-band tolerance stays 0.10 m.
   - The design doc's road tolerance becomes 0.10 m (from 0.05 m), for CT4.
   - General terrain stays 0.5 m.
   - The coordinator's earlier "river beds need about 1 cm" was overstated:
     the water-band tolerance in force is 10 cm.
7. **The double chunk read** (Rough, then the wanted tier, for identical
   ground) goes to the world worker after TS. It must be bitwise identical.
7. **Cheap borders (TB):**
   - Its "before" numbers are taken after the Rayon removal (TS), so the
     comparison is like for like.
   - `Convergence::blend` and `EdgeOptions::roughness` stay as settings
     that have no effect, documented as such, until a separate removal
     step.
   - Border stencils are **not** changed to read the neighbour's height.
     The architecture decision "Labels at region borders" (2026-11-25)
     stands. The code review had called this a bug; it's a decision. The
     user may revisit it.
6. **CT4 (roads in the TIN) is held** while the focus is loading speed.
   CT3 and RB5 have unblocked it.
