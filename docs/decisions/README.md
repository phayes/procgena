# Decision records

Why procgena is built the way it is. Each record keeps the reasoning, the
options weighed and the user's answers, and opens with a status banner. The
current design is in [ARCHITECTURE.md](../../ARCHITECTURE.md), and what is
built in [ROADMAP.md](../../ROADMAP.md).

- **Decision gates** (named in ROADMAP's "Decision gates"): [D1](D1.md)
  crate names, YAML crate and coordinates; [D2](D2.md) masks on the GPU and
  chunk reach, with its prototypes in `d2/`; [D3](D3.md) stable keys, seeds
  and slot choice; [D4](D4.md) region size and shape; [D5](D5.md) World and
  Biome scale, with its research in `d5/`. Gate D6 is recorded in ROADMAP
  and in the canonical ground and road plans.
- **Milestone plans** (named after the milestone they planned): [C1](C1.md)
  the same inputs for every Scatter; [C2-plan](C2-plan.md) the `Scatter`
  trait; [IX-plan](IX-plan.md) the impostor experiment.
- **Design records** for a track: [ecotope-gpu](ecotope-gpu.md) the order
  of the GPU work; [ground-materials](ground-materials.md) ground layers
  (GM0–GM5; superseded 2026-11-26, paint removed); [saved-file-format](saved-file-format.md) preset bundles
  (PB1–PB4).
