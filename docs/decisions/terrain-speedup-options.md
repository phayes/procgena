# Terrain loading: algorithmic speed-up options (2026-11-26)

> **Ground paint removed 2026-11-26** (the user): options that speed up
> paint no longer apply; the terrain draws flat grey.

Answers to the user's request for alternative architectures and coarser
constraints that give large algorithmic speed-ups on **one worker thread**,
with roads and rivers off. The sources are a code-only review by Codex and
the advisor. Every gain here is an estimate. `terrain/load_profile.md`
supplies the measured counts that confirm or rule out each one. Paths are
relative to `terrain/crates/procgena_terrain/src/`.

## Bottom line

About 10× comes from **fewer `H*` evaluations**, not from cheaper ones.
Ranked by gain for the effort:

| # | Option | Output | Effort | Estimated gain |
|---|--------|--------|--------|----------------|
| 1 | Make disabled features cost nothing. Split plans into lazy edge and corner products. | `H*` bitwise identical | low to medium | several-fold on plans |
| 2 | Paint from the mesh's own samples, or a sparse grid, not 16,641 full `H*` queries per chunk | visual only | medium | 10–30× on paint |
| 3 | Classify each chunk against borders once, so interior chunks skip all border maths. Reject in `blended_gradient` before sampling neighbours. | bitwise identical | medium | 2–5× on sampling |
| 4 | Share noise evaluations: own and scaled relief, gradient and Hessian, identical presets | bitwise identical if each accumulation keeps its order | medium | 2–4× on relief |
| 5 | Make Rough and Mid cheaper: sample their interiors at 4–8 m and drop octaves finer than a cell, with Fine-derived edge contracts | Rough and Mid change; Fine unchanged | medium | 16–64× fewer probes for provisional tiers |
| 6 | Cheap borders: one shared 1-D height profile per edge, plus a polynomial blend inside a bounded strip | `H*` changes near borders | medium to high | about 10× on border evaluation, 2–4× on chunk reads |
| 7 | Make homogeneous relief borders no-ops | `H*` changes | medium | over 10× in blended homogeneous areas |
| 8 | A world-anchored control lattice for low frequencies, plus detail per point | `H*` redefined | high | the most general option; only if 1–7 fall short |
| 9 | Adaptive TIN sampling under an analytic error bound | TIN only | medium | matters once TIN streaming is the default |

Options 1–4 leave `H*` bitwise identical or are visual only. Options 5–8
change the look or what `H*` is, so they are the user's decision.

## Where the work goes (from reading the code)

- **Mesh:** a level-0 chunk reads 67² = 4,489 heights. Higher levels add
  1,572 transition samples, for 6,061 in total. About 252 chunks give
  1.1–1.5 M heights (`chunk.rs:622`, `:635`).
- **Paint:** `GroundGrid` reads 129² = 16,641 sites per chunk at Gradient
  detail or more, about 4.2 M for 252 chunks. Measures add 4 height queries
  per site and labels 8 more (`ground/weights.rs:11`, `:269`;
  `ground/site.rs:205`).
- **Relief:** plains use 8 octaves, hills 9, mountains 12–19. Height plus
  slope costs about 29–60 lattice kernels. The default elevation adds 4
  octaves and always computes gradients (`relief.rs:235`, `noise.rs:209`,
  `elevation.rs:273`).
- **Blending:** blending can evaluate the region's own relief twice. Inside
  blend zones it evaluates every region any fade edge refers to.
  `blended_gradient` evaluates all neighbours before rejecting the point
  (`relief.rs:146`, `converge.rs:695`, `:770`).
- **Borders:** up to three pull systems run per height: relief scaling,
  relief blending and seam bending. They use `powf`, `sqrt`, `atan2`, series
  of up to 80 terms, and `asin`/`sin`. A gradient repeats the pulls at four
  offsets (`border.rs:120`, `:178`, `:204`, `:418`; `converge.rs:567`;
  `terrain.rs:873`).
- **Plans with features off still do the work:**
  - the 25 m river grid of about 6,400 cells, built before density 0 is
    checked;
  - the hydrology grid on Rough;
  - a 16 m road grid of about 15,600 queries on Mid;
  - loading one region builds its neighbours' full plans
    (`plan.rs:127`, `:426`; `river.rs:208`; `terrain.rs:339`, `:511`,
    `:588`; `road/mod.rs:508`; `world.rs:694`).

## "Close enough" borders, with the TIN papering over (user question)

- **Most of the saving needs no height step.** The cost isn't exactness
  itself. It's integrating the pulls and evaluating the neighbour's relief
  at runtime. Option 6 keeps the height exact at the edge (return the shared
  `P(s)` on the edge) and lets the slope crease. That removes most of the
  cost.
- **Allowing a real step of 0.1–0.5 m in `H*` adds little on top, and causes
  problems:**
  - placement and labels still read the stepped `H*`;
  - the TIN, which has no region-border constraints (D6c), shows a ramp
    while slope masks see flat ground on both sides;
  - objects float or sink at the border;
  - finite-difference slope and curvature see false features;
  - a continuous TIN must be off by at least δ/2 near a jump of δ.
- **Both sides must still compute the same edge definition,
  deterministically.**
- **Existing issue:** curvature and shape stencils sample the centre
  region's own height beyond its border, not the world's (`terrain.rs:1055`,
  `:1087`; `label.rs:234`). This is a separate bug to fix.

## How to decide

The load profiler measures `H*` evaluations per consumer, nanoseconds per
evaluation (split into noise, blend maths and border geometry), plan costs
before the first picture, and work that should be zero with features off.
Then:

1. Do options 1–4: no look change, and measured to matter.
2. Put options 5–8 to the user with their counts.
3. Work in `world.rs` waits until the Rayon removal (TS) lands.
