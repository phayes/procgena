# procgena_clump performance

## Bottom line

- **Exposure rays are almost all of the build time on real inputs.** These
  are the rays that measure how exposed each input vertex is toward each
  direction's source.
  - Boulder snow: 1.85 s of 2.05 s (90%).
  - Boulder moss: 1.41 s of 1.78 s (79%).
  - Oak snow: 128 ms of 187 ms (69%).

  Everything else in a build is in the tens of milliseconds.
- **The ray count is a user setting: `MaskOptions::exposure_rays`** (default
  64 in both presets). Users trade look for speed per pass. Time in this
  stage scales linearly with the count: 24 rays is about 2.7× faster than 64.
  Fewer rays measure exposure in coarser steps, so the edges of sheltered
  areas get blockier. 0 turns the test off and treats everything as fully
  exposed. Lowering the presets' value changes the snow look Patrick
  approved, so the presets keep 64 until a before-and-after render is
  signed off. Callers can lower it freely.
- **The biggest wins at a given ray count leave the output unchanged:**
  - skip exposure rays when they can't affect the result: MossPatches has
    `exposure_power` 0, so all of its 1.4 s is wasted;
  - cast them only for vertices that could be covered: 30% of the boulder's
    vertices face up enough for snow;
  - cast once per position, not once per seam copy: 23% of the oak's
    vertices are copies;
  - run the per-vertex rays in parallel.

  Together, an estimated 2 s boulder build becomes roughly 0.2–0.4 s on an
  8-core machine.
- **Strict mode wastes half its time:** crossings stop falling after about
  9 of its 24 thinning passes.
- **Reuse across passes** (snow then moss on one rock) is covered by
  `PLAN-prepared-surface.md`, not repeated here.

Goal (Patrick, 2026-11-22): speed over perfection.

**Naming:** these used to be called "sky" rays (`sky_rays`, `sky_power`).
They were renamed to "exposure" (`exposure_rays`, `exposure_power`) on
2026-11-22, because directions can point anywhere, not just down from the
sky.

## Trading look for speed: `exposure_rays`

| `exposure_rays` | Exposure stage on boulder snow (estimated from the measured 1.85 s at 64) | Exposure steps | Look |
|---|---|---|---|
| 64 (preset default) | 1.85 s | 1/64 | The approved snow look |
| 32 | about 0.93 s | 1/32 | Expected close to 64 |
| 24 | about 0.69 s | 1/24 | Sheltered edges slightly blockier |
| 16 | about 0.46 s | 1/16 | Visibly coarser shelter edges |
| 0 | 0 | none | No shelter at all; facing alone decides |

- Per call, set `options.mask.exposure_rays` before `Clump::new`.
- The cost is per input vertex per direction, so two directions cost twice
  as much.
- `exposure_power` 0 means exposure has no effect, so `exposure_rays` should
  be 0 too. Until win 1 lands, that has to be set by hand to get the
  speed-up.
- Before changing the presets' value, render the stone galleries at 16, 24,
  32 and 64 with the same camera (`render.html` in the clump spike) and get
  Patrick's sign-off.

## Measurements (2026-11-22)

Measured with temporary stage timers in `ze_clump` (identical code to
`procgena_clump`, since removed), in a release build on one thread, on a
machine shared with other agents' builds. Expect ±30% noise between runs.
Default presets; the oak uses `scale` 10.

| Stage | Boulder snow (48k input tris) | Boulder moss | 1-quad ground snow | Oak snow (3.4k input tris) |
|---|---|---|---|---|
| BVH build | 23 ms | 22 ms | 0 | 0.8 ms |
| Closedness + welding | 20 ms | 16 ms | 0 | 0.9 ms |
| Room rays (2 per vertex) | 84 ms | 88 ms | 0 | 5.4 ms |
| **Exposure rays (64 per vertex per direction)** | **1,850 ms** | **1,413 ms** | 0.03 ms | **128 ms** |
| Covered triangles + refine | 7 ms | 18 ms | 2.6 ms | 2.6 ms |
| Blur push directions | 5.6 ms | 15 ms | 5.5 ms | 4.8 ms |
| Weights + gap rays | 11 ms | 39 ms | 1.4 ms | 11 ms |
| Slope limit | 3 ms | 8 ms | 1.5 ms | 1.8 ms |
| Rim sink + underside rays | 33 ms | 114 ms | 1 ms | 17 ms |
| Fold removal | 1.9 ms | 8 ms | 0.3 ms | 5.7 ms |
| Assemble (closed solid) | 9.8 ms | 35 ms | 16.6 ms | 7 ms |
| **Total** | **2,049 ms** | **1,778 ms** | **30 ms** | **187 ms** |
| Output triangles | 33k | 134k | 67k | 32k |

There are two regimes:
- **Dense real inputs** (rocks, trees): per-input-vertex rays dominate,
  above all the exposure rays.
- **Coarse inputs with a big clump** (the one-quad ground): per-clump-vertex
  work dominates (refine, blur, assemble), and all of it is cheap.

Strict mode on the boulder adds 1,987 ms (3.35 s in total). Crossing
triangles per pass:
835, 566, 314, 132, 79, 53, 21, 17, 14, 13, 13, 13, 13, 13, 13, 12, 13, 13,
12, 17, 12, 12, 13, 13, 13. The cutting step after pass 24 brings it to 5,
then 0.

Coverable share: of the boulder's 24,126 vertices, 7,223 (30%) face up
past SnowCap's `facing_min` of 0.3. On the oak, 464 of 2,344 (20%) do, and 549
(23%) repeat an earlier vertex's position (seams and joins).

## Wins that keep the output identical

Verify each one with a mesh-equality check: same `positions`, `fade` and
`indices`, bit for bit, on every spike input (`cargo run --release -p
procgena_clump --example spike`) before and after. Rank is by measured
impact.

| # | Win | Saves (measured / estimated) | Notes |
|---|---|---|---|
| 1 | **Skip all exposure rays when `exposure_power == 0`.** Exposure to the power 0 is always 1, so the rays are discarded. | Measured: 1.41 s of 1.78 s on boulder moss (79%). Every MossPatches build. | Trivial: a check in `vertex_exposure`. |
| 2 | **Cast exposure rays only where they can matter.** That means every corner of a triangle where some corner could be covered on facing, `room`/`min_support` and `radius` alone. It must be every corner, not just covered ones: midpoints blend exposure from their corners, so a facing-0 corner's exposure still affects its neighbours. | Estimated: about 2–3× less exposure time on rocks (30% of boulder vertices face up, plus their neighbours). | Do the per-triangle coverage pre-check before `vertex_exposure`; needs room first, which is already computed first. |
| 3 | **Compute room and exposure once per position id, not per vertex index.** Seam copies cast identical rays. | Measured share: 23% of oak vertices are copies, so about 23% of room and exposure time on trees. 0% on stones. | `ClumpField` already has `position_id` and per-position normals. |
| 4 | **Run per-vertex ray work in parallel** with optional `rayon` (exposure, room, gap, sink). Each vertex is independent. | Estimated: close to linear in cores for the ray stages (about 6–8× on 8 cores). | Adds a dependency, which changes the shared `Cargo.lock`, so ask the coordinator first. The `thinned` counter must be counted afterwards, not mutated in a closure. Keep results in vertex order so output stays deterministic. `rayon` is already a workspace dependency (stone uses it). |
| 5 | **BVH query overhead.** Every query allocates its traversal stack on the heap (`vec![0usize]`); use a fixed-size stack array. `ray_hit` doesn't visit the nearer child first; ordering by entry distance lets the nearest-hit search prune more. | Estimated: 1.2–2× on every ray stage. | `bvh.rs`. A surface-area-heuristic (SAH) build is a later, larger option. |
| 6 | **Gap-limit rays:** don't cast when the height is ≤ 0, where the cap can't apply. Cap the ray at `height / GAP_FILL` instead of infinity, so the BVH can stop early. | Measured stage: 11–36 ms per build (≤ 2%). | `shell.rs`, the `tops` map. |
| 7 | **Minor duplicates:** `Surface::welded()` runs twice per build (`is_closed` and the field), and the rim sink and underside sink can cast the same inward ray. | Measured stage: closedness + welding is 16–20 ms on the boulder (about 1%). | Compute welding once and pass it on. |

Items 1, 2, 3 and 6 (and 7's welding) change `field.rs` or the start of
`displaced_shell`. Those are the same areas that `PLAN-prepared-surface.md`
phase 2 restructures, so land them with phase 2 or right after it, not in
parallel. Item 5 is in `bvh.rs`, which phase 1 moves to `procgena_mesh`, so
do it after phase 1. Item 4 touches both, and needs the coordinator's OK
for `Cargo.lock`.

**Estimated combined effect on boulder snow at 64 rays:** exposure rays
1.85 s → about 0.7 s (item 2) → about 0.1 s (item 4 on 8 cores), with
smaller cuts elsewhere. Total about 2 s → about 0.2–0.4 s. Boulder moss:
about 0.4 s after item 1 alone. Lowering `exposure_rays` multiplies on top of
these.

## Other wins that change the look (need a render comparison and Patrick's sign-off)

Compare renders with `render.html` in the clump spike. Use the stone
galleries for snow and moss, wide and close, with the same camera, before
and after.

| # | Change | Saves (estimated) | What changes |
|---|---|---|---|
| A | **Lower `exposure_rays` in the presets** (see "Trading look for speed" above). It is already a user setting; this is only about the presets' value. | About 2.7× less exposure time at 24 rays. | Coarser exposure steps; the approved speckled snow look may shift. |
| B | **Sample exposure at about `edge_length` spacing** instead of per input vertex on inputs much denser than the clump mesh, then blend. | Estimated: up to 2–4× fewer exposure samples on 48k-triangle rocks. | Fine shelter detail below the clump's own resolution is smoothed away. |
| C | **Cap exposure ray length** (for example a few clump radii, or a fixed world distance), so distant occluders don't count. | Estimated: modest. The BVH stops early, mostly on big scenes. | Large overhangs far away stop sheltering the surface below. |

The current recommendation is to do the identical-output wins first,
re-measure, and only then decide whether A is still needed.

## Strict mode

| # | Win | Saves | Output |
|---|---|---|---|
| S1 | **Stop thinning when crossings stop falling**, for example after 2 passes without a drop, then go straight to cutting. On the boulder, passes 9–24 sit at 12–13 crossings. | Estimated: about 55% of strict's 2 s on the boulder. | Probably identical, since the plateau passes lower vertices that are already at their floor. Verify with mesh equality. |
| S2 | **Re-test only what changed.** Each pass rebuilds the whole mesh, the weld map and a fresh BVH, then tests every triangle, although only a few triangles moved. Track the vertices changed in the last pass and re-test only triangles near them. Refit the BVH instead of rebuilding it. | Estimated: most of the remaining strict time. | Identical if the neighbourhood is chosen conservatively. |

Strict is off by default, so these matter only to clients that turn it on
(CSG, signed distance fields).

## Across passes and detail levels

See `PLAN-prepared-surface.md`: build the BVH, welding, closedness, room and
the exposure rays per direction set once per input, and reuse them for snow,
moss and mud passes and for every level of detail. Snow plus moss on one
rock is expected to cost about half of two independent builds, and much less
once wins 1–4 land.

## Repro

- **Stage timings:** add temporary `Instant` marks around the stages listed
  above in `ClumpField::new` and `displaced_shell`, and gate the printing on
  an environment variable. Don't commit them.
- **Inputs:**
  - `procgena_stone::Stone::from_preset(StonePreset::Boulder).mesh()`;
  - `SurfaceMesh::ground(3.0, 1)`;
  - `procgena_tree::Tree::from_preset(TreePreset::OakSmall)` with
    `mesh_with_detail(&LodDetail::default())`, using its `branches` buffers,
    with `scale = 10.0`.

  Build them in a throwaway crate outside the workspace (its own
  `CARGO_TARGET_DIR`) so the shared `Cargo.lock` isn't touched.
