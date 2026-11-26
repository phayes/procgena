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
3. **CT4 (roads in the TIN) is held** while the focus is loading speed.
   CT3 and RB5 have unblocked it.
