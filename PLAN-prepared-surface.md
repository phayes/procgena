# Prepared surfaces and mesh facts (procgena_mesh + procgena_clump)

## Bottom line

- **New shared crate `procgena_mesh`** (folder `mesh/`, depends only on
  `glam`). It holds what every generator and consumer needs to talk about a
  triangle mesh:
  - `Surface<'a>`: the borrowed positions, normals and `u32` indices view.
    It moves out of `procgena_clump`.
  - Position welding: vertices at bitwise-equal positions count as one, with
    −0.0 equal to +0.0.
  - `Bvh`: ray, closest-point and box queries. It moves out of
    `procgena_clump`.
  - A `MeshFacts` trait, described below.
- **`MeshFacts`: facts a generator already knows, all optional.** Every method
  has a default that computes the answer from the mesh, so outside meshes work
  unchanged.
  - `is_closed() -> bool`. Default: the welded edge count.
  - `shells() -> Option<Vec<Range<u32>>>`: separate closed parts, as triangle
    ranges. Default: `None`, meaning unknown.
  - `inside(p) -> Option<bool>`. Default: `None`, meaning use rays.
  - `signed_distance(p) -> Option<f32>`. Default: `None`.
- **`PreparedSurface` lives in `procgena_clump`.** It is built once per input
  and reused for every clump pass on it (snow, then moss, then mud):
  `PreparedSurface::new(surface)` or `PreparedSurface::with_facts(surface, &facts)`.
  It owns the work that depends only on the input:
  - the BVH;
  - closedness and shells;
  - per-vertex input thickness ("room") and "inside another part" flags.
  - It also caches the costly exposure rays (formerly "sky visibility") per
    (direction, ray count), because those depend on the clump pass's
    settings.
- **`Clump::new(&PreparedSurface, &ClumpOptions)`** replaces
  `Clump::new(Surface, ...)`. There is no compatibility shim: callers make a
  `PreparedSurface` first, which is one line.
- **Generators implement `MeshFacts`; they never build a `PreparedSurface`.**
  - `procgena_stone` supplies an exact inside test and signed distance from
    its `StoneField`, which makes clump burial and room checks exact and
    ray-free.
  - `procgena_tree` supplies closedness (from `LodDetail::closed`) and its
    per-branch shells (from `TreeMesh::branch_spans`). This makes "inside
    another part" checks correct where shells overlap, such as a branch base
    inside its trunk.
  - Stone and tree depend only on `procgena_mesh`, not on `procgena_clump`.

**Why this shape (decided 2026-11-22):**
- If stone and tree built the prepared surface, they would have to depend
  on clump, which runs backwards. The exposure cache also depends on clump
  settings that a generator doesn't know.
- Keeping preparation in clump means one path serves both ecosystem meshes
  and outside meshes. Generators only add facts, which make the result more
  exact and cheaper.
- No hidden global memoization: a cache keyed on mesh content costs about as
  much to hash as to recompute, and pointer keys are unsafe.

**Expected gains:**
- Snow plus moss on one rock costs roughly half of today's two independent
  builds. The BVH, room and closedness work happens once, and the exposure
  rays are shared when the directions match.
- On ze_stone rocks, the exact signed distance replaces the ray-parity and
  `inside_run` heuristics. These caused the "rim vertices outside the rock"
  bug fixed on 2026-11-22.

## Current state (2026-11-22)

- **`ze_clump` (/opt/ze_clump) and `procgena_clump` are in sync.** The files
  match apart from the crate renames and rustfmt. The clump crate holds:
  - welded `Surface::is_closed`, and position welding in the clump itself
    (`Surface::welded`, a merged normal per position,
    `ClumpField::position_id`);
  - the thickness cap and `ShapeOptions::min_support`, plus the
    junction / inside-another-part skips (room 0);
  - always-on fold removal, which drops slivers it can't lower;
  - `MeshOptions::strict`, closed clumps, directions and presets.

  See `clump/plan.md`, "Thin input, junctions and seams".
- **Phase 1 note:** `Surface::welded()` (in `surface.rs`) moves to
  `procgena_mesh` together with `is_closed`. The clump field uses it for
  `position_id` and its merged normals.
- **Phase 2 note:** the per-input work that moves into `PreparedSurface` is
  the BVH, closedness, welding with merged normals, and the per-vertex room.
  The ray offset ("epsilon") is 1e-6 × the mesh diagonal.
- **Trees, solved without facts:** the oak's folds were mostly seams, not
  overlapping shells. On OakSmall LOD0 bark (closed, 3,350 triangles), snow
  at `scale` 10:
  - dropped folds: 1,587 still inside out → 143 removed, 0 inside out in
    the mesh;
  - crossing triangles: 20,458 → 8,998;
  - strict: 0 crossings, 5.4 s → 2.3 s.

  The fixes were welding the tree's repeated seam positions in the clump, a
  smaller ray offset (the old one was larger than branch tips), and skipping
  junction vertices: where a ray straight in first meets another part's
  outside. Per-shell facts from the tree (phase 4) would make "inside
  another part" exact, rather than relying on the junction ray test. That
  phase is now about exactness and speed, not a known fold bug.
- **Repro:** a throwaway crate with path dependencies on `procgena_tree` and
  `procgena_clump`. Use `Tree::from_preset(TreePreset::OakSmall)`,
  `mesh_with_detail(&LodDetail::default())`, its `branches` buffers as the
  `Surface`, `ClumpOptions::default()` with `scale = 10.0`, and
  `diagnostics::crossing_count`.
- **Units note:** the oak numbers above were measured before procgena_tree
  switched to metres (1 old tree unit = 0.1 m). The tree now comes out 10×
  smaller, so the same snow is `scale` 1 instead of `scale` 10.

## Phases

| Phase | Scope | Status |
|---|---|---|
| 1 | Create `procgena_mesh`: move `Surface`, welding, `is_closed` and `Bvh` out of `procgena_clump`. Add `MeshFacts` with defaults. | ⬜ |
| 2 | `PreparedSurface` in `procgena_clump`; `Clump::new(&PreparedSurface, …)`; per-input caches; exposure cache per (directions, rays) | ⬜ |
| 3 | `procgena_stone` implements `MeshFacts` (closed, exact inside, signed distance). The clump field uses them when present. | ⬜ |
| 4 | `procgena_tree` implements `MeshFacts` for bark (closed, shells). The clump field uses shells for the "inside another part" checks. | ⬜ |
| 5 | Measure and document (see Verification) | ⬜ |

### Phase 1: `procgena_mesh`

- New workspace member `mesh/crates/procgena_mesh`, with `glam` only.
- **Move:**
  - `Surface`, `SurfaceMesh` (owned) and `SurfaceError` from
    `clump/crates/procgena_clump/src/surface.rs`;
  - `Bvh`, `ClosestPoint`, `closest_on_triangle`, `ray_triangle`,
    `triangles_cross` and `overlapping` from `.../src/bvh.rs`.
- **Keep** the welded `is_closed` exactly as it is (bitwise positions, −0.0
  folded into +0.0), and keep its test
  `closedness_counts_repeated_positions_as_one_vertex`.
- **Add the trait:**
  ```rust
  pub trait MeshFacts {
      /// Whether the mesh encloses one or more solids. Default: welded edge count.
      fn is_closed(&self, surface: &Surface) -> bool { surface.is_closed() }
      /// Separate closed parts, as triangle index ranges. Default: unknown.
      fn shells(&self) -> Option<Vec<std::ops::Range<u32>>> { None }
      /// Whether a world point is inside the solid. Default: unknown.
      fn inside(&self, _p: glam::Vec3) -> Option<bool> { None }
      /// Signed distance, negative inside. Default: unknown.
      fn signed_distance(&self, _p: glam::Vec3) -> Option<f32> { None }
  }
  /// Facts for a mesh nobody knows anything about.
  pub struct NoFacts;
  impl MeshFacts for NoFacts {}
  ```
  Open question: pass `surface` to `is_closed`, or have a generator's facts
  object own its own knowledge? The sketch above passes it, so the default
  can compute.
- **`procgena_clump` re-exports `Surface` and `SurfaceMesh`,** so its public
  API keeps the same names.

### Phase 2: `PreparedSurface`

- `PreparedSurface<'a> { surface: Surface<'a>, bvh: Bvh, closed: bool, shells: Option<…>, room: Vec<f32>, exposure: Mutex<HashMap<ExposureKey, Arc<Vec<[f32; MAX_DIRECTIONS]>>>>, facts: &'a dyn MeshFacts }`.
  - `ExposureKey` is the direction vector's bits plus the ray count
    (`MaskOptions::exposure_rays`), so passes
    with the same directions share results.
  - The cache uses a `Mutex`, so several passes can run in parallel.
- `ClumpField::new` takes `&PreparedSurface` and stops building its own BVH,
  room and closedness.
- `Clump::new(&PreparedSurface, &ClumpOptions)`. Update the spike, the doc
  example, the tests and `clump/plan.md`.
- **Test:** building snow and then moss from one `PreparedSurface` gives
  bit-identical meshes to building each from a fresh one. The second pass
  must not rebuild the BVH or the room data.

### Phase 3: stone facts

- `procgena_stone` implements `MeshFacts` for `Stone`, or for a small view
  type it returns:
  - `is_closed`: `true`, since marching cubes and dual contouring meshes are
    closed.
  - `signed_distance(p)`: `field.distance(placement.field_position(p / scale))`.
    Follow the conversion that `Stone::field`'s docs describe, and account for
    `embed_depth` if the caller buried the rock.
  - `inside(p)`: `signed_distance(p) < 0`.
- **In clump**, when `signed_distance` is available:
  - **Room:** step inward along the normal, or sphere-trace, until the
    distance changes sign.
  - **Sinking:** check `inside()` on the sunk position instead of
    `inside_run`'s face-orientation test. The ray versions remain the
    fallback.
- **Test:** on all 8 stone presets, the spike's `out` column stays 0, and
  rim and underside placement uses no rays when facts are supplied (count
  BVH ray calls in a test build).

### Phase 4: tree facts

- `procgena_tree` implements `MeshFacts` for bark from a `TreeMesh` built
  with `LodDetail::closed = true`:
  - `is_closed`: `true`;
  - `shells()`: one range per branch shell from `branch_spans`. A
    continuation that joins its parent counts as the same shell.
- **In clump:** "inside another part" means inside any shell other than the
  vertex's own. Test it per shell with parity, or with exact cylinders from
  the skeleton if the tree exposes them. This replaces today's single
  outward-ray test.
- **Test:**
  - On OakSmall LOD0 at `scale` 1 (metres), no vertex of a branch's clump lies inside
    the trunk shell, by a per-shell parity check.
  - Folds drop well below today's 1,600. Find where the remaining ones are
    first.

### Phase 5: verification

- `cargo test -p procgena_mesh`, `-p procgena_clump`, `-p procgena_stone`,
  `-p procgena_tree`.
- `cargo clippy -p <each> --all-targets -- -D warnings`.
- `cargo doc -p <each> --no-deps --all-features`.
- The clump spike (`cargo run --release -p procgena_clump --example spike`):
  the `out` and `cross` columns are unchanged or better, and build times for
  a snow-then-moss pair are about halved.

## Coordination (procgena rules)

- Read `/opt/procgena/CLAUDE.md` first. Agents share this checkout. Run only
  `-p <package>` checks. Never run git stash, checkout, reset, clean or
  rebase. Stage by path.
- **Phase 1 changes shared files:** a new workspace member in the root
  `Cargo.toml`, and `Cargo.lock`. Ask the coordinator (Patrick) before
  editing them. Every agent rebuilds afterwards.
- Phases 3 and 4 touch `stone/` and `tree/`. Check with whoever owns those
  folders before editing them.
- Commit only when asked.

## Open questions

1. Should `MeshFacts::shells` also carry whether each shell is closed (some
   tree LODs are open)? Proposed: no. Closedness is per mesh, and open LODs
   aren't for physics.
2. Should `PreparedSurface` own its `Surface` data, or borrow it? Proposed:
   borrow it (`'a`), as `Surface` does today.
3. Should the stone facts account for `embed_depth`? The stored mesh isn't
   lowered, so the facts should match the mesh as given and document that.
