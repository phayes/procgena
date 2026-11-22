# procgena_stone — port plan for Unity-Procedural-Rock-Generation

## Status (2026-11-20)

| Phase | State |
|---|---|
| 0 — Scaffolding | ✅ Done (commit `1cd3392`) |
| 1 — Field (CPU) | ✅ Done (commit `1cd3392`) |
| 2 — Meshers + Stone wrapper | ✅ Done (commit `1cd3392`) |
| 3 — Exports | ✅ Done (commit `1cd3392`) |
| 4 — WESL twin | ✅ Done (commit `1cd3392`; Codex comment pass pending) |
| 5 — `gpu` feature | ✅ Done (commit `6be79e2`; Codex comment pass skipped by user) |
| 6 — `procgena_stone_bevy` per-entity rocks | ✅ Done (commit `7da4bfb`; Codex comment pass skipped by user) |
| 7 — Textures | ✅ Done (commit `7da4bfb`); files in GCS `gs://ze_textures/stone/`. Follow-up (uncommitted): roughness KTX2 replaced by packed `orh` (R occlusion, G roughness, B height stretched to 0–1); export JPEG roughness replaced by glTF-style `orm`; AO feeds `diffuse_occlusion`, height drives triplanar blending. Uploaded to GCS; old roughness files removed. |
| 8 — `procgena_stone_bevy` GPU variant pool | ✅ Done (uncommitted). Per-light-view shadow culling; motion vectors assume still rocks; visible lists bound as instance-rate vertex buffers (no `first_instance` feature needed) |
| 9 — Polish | Not started |

Reference source: `prior_art/Unity-Procedural-Rock-Generation` (git-ignored). The
algorithm lives in `Assets/Code/ScalarField.compute` (field) and
`Assets/Code/Triangulation.compute` (marching cubes); `ProceduralRockGenerator.cs`
holds the parameters and defaults (scene values in `Assets/Scenes/Main.unity`).

## Agent working notes

- Port the algorithm, not the HLSL syntax. Keep Unity's constants (FBM rotation,
  octave count, carving radii, the 0.025 surface threshold) unless a decision says
  otherwise, and record every deliberate change in "Decisions".
- CPU and WESL code are twins: when one changes, change the other in the same step,
  and keep the `include_str!` agreement tests passing.
- Never widen a parity tolerance to make a test pass; a topology mismatch means a bug
  or a documented on-surface sample, which must be explained.
- Field conventions (Phase 1): stone space is the cube `[-0.5,0.5]³`; carve space is
  stone space × `carve_scale` (2 / normalized stretch), Unity's `Map(p) = RockSDF(2p)`.
  `StoneField::distance = max(0.5·(rock + disp·|triplanar| − 0.025), |p|∞ − 0.5)`.
  The 0.025 offset moves the surface *outward* (Unity's inside is `d < 0.025`).
  The rock gradient for triplanar weights is analytic (`(1−h)·ga − h·gb` per cut),
  not Unity's 1e-4 finite difference. The noise hash is `pcg3d([ix, iy, seed])`;
  carving uses `pcg3d([i, seed, salt])`.
- Displacement only removes material, so the mean surface radius at defaults is
  ~0.30 (0.35 without displacement); the max is ≤ 0.4875.
- Mesh conventions (Phase 2):
  - Grid coordinate `i / R − 0.5`, so boundary samples are exactly on the cube and
    have values ≥ 0 (outside); inside means `value < 0`.
  - MC case bit `i` is set when corner `i` is **outside** (Unity's
    low-density bit). With Unity's corner order (`mesh/tables.rs`) the table then
    winds CCW outward as-is.
  - Crossing normals are the normalized `StoneField::gradient` with step `0.5/R`,
    falling back to the inside→outside edge axis.
  - DC vertex normals fall back to the normalized sum of crossing normals.
  - DC quads go round the edge CCW in the `(axis+1, axis+2)` plane and are
    reversed when the edge's lower sample is outside. They split along the shorter
    diagonal, with ties going to `0–2`.
  - Measured at R=48: DC has 4–14 non-manifold edges per ~11k triangles; MC has
    none. Face/normal agreement is ≥ 99.97% for MC and ≥ 99.6% for DC.
  - Bench (M1 Max, heavily loaded machine, ms/rock): MC R=100 ≈ 113 ms with rayon
    and ≈ 826 ms serial; R=128 ≈ 215 ms with rayon.
- Export conventions (Phase 3):
  - Box projection picks one of 6 faces per triangle from its geometric normal
    (ties go to x, then y); vertices are split per (vertex, face). u/v are chosen so
    images are not mirrored seen from outside; side faces keep v along +y.
  - Tangent = u direction projected onto the vertex normal plane; w makes
    `cross(n, t)·w` point along +v. glTF flips v (`1 − v`), tangents unchanged.
  - Measured: DC has ~0.4% of vertices with w = −1 (normal faces away from its
    projected face), matching its face/normal disagreement; MC has fewer.
  - Khronos glTF-Validator (npm `gltf-validator` 2.0.0-dev.3.10, 2026-11-20): the
    six example GLBs (3 seeds × MC/DC, 3 LODs) have 0 errors and warnings; untextured
    ones get only the infos `UNUSED_MESH_TANGENT` and `UNUSED_OBJECT` (TEXCOORD_0),
    and with color/normal/roughness images embedded they are fully clean. The
    `.gltf` + `.bin` path is only checked by the `gltf` crate tests. Not automated.
- WESL conventions (Phase 4):
  - Library modules (`hash`, `noise`, `field`, `tables`) have no bindings. `generate`
    holds the bindings, the `Request` uniform (carve spheres inline, 1232 bytes) and
    `sample_field`; `mc`, `dc` and `scan` hold their mesher's passes. Modules import
    each other as `package::<name>`.
  - Pass order per request: `sample_field` → `count_mc|count_dc` (N+1 items; the
    extra zero item makes the scanned last entry the totals) → exclusive scan of
    `vec2<u32>` → emit passes → `finish_mc|finish_dc`. MC prefix = (crossings, mc
    indices); DC prefix = (crossings, crossed-cell flag), DC index base = 6 ×
    crossing prefix.
  - The 256-case triangle table is a uniform packed from Rust (`packed_tri_table`),
    not a WGSL const, to avoid per-invocation copies of a dynamically indexed array.
  - Shader code mirrors the CPU operation order (glam `normalize_or_zero`,
    `try_normalize`, `Mat3::inverse` × vec, `a + (b − a)·t` lerp), so the WESL
    interpreter reproduces the CPU bit for bit. Real GPUs may still differ by fused
    multiply-adds and transcendental precision (checked in Phase 5).
  - Known parity risk for Phase 5: DC's shorter-diagonal split compares float
    distances, so a rounding difference can swap a quad's diagonal without changing
    counts. Samples exactly on the surface can flip inside/outside.
  - Tests: `src/shaders/tests.rs`. `slow_kernels_reproduce_cpu_meshes` (ignored,
    ~4 min) runs every pass except the scan in the interpreter at R=8 and matches
    the CPU meshes exactly; run with `cargo test -p procgena_stone --lib -- --ignored slow`.
    The scan uses workgroup barriers, which the interpreter cannot run; Phase 5 GPU
    tests cover it.
- GPU conventions (Phase 5):
  - `procgena_stone::gpu` (feature `gpu`): `GpuStoneGenerator::new(&Device)`, then
    `encode(&Device, &mut CommandEncoder, &[GpuStoneRequest]) -> GpuStoneBatch`
    (no `Queue`: uniforms use `create_buffer_init`). Outputs are new buffers per
    call; every output also has `COPY_SRC`. One compute pass per batch; requests
    run in order and share the scratch buffers (samples, prefix, crossings, scan
    sums). Uniform slots use dynamic offsets: one `Request` per request, one
    `ScanParams` per (request, scan level).
  - DC `crossing_capacity = index_capacity / 6` (exact: DC indices = 6 ×
    crossings), so requests carry only vertex/index capacities.
  - `scan_blocks` now skips the block-total write for workgroups past the end,
    which row dispatches (> 65 535 workgroups) can create.
  - `estimate_capacity(resolution)` = 4.5·R² vertices, 6× that for indices. Measured
    max per R² over 12 seeds (R=16–128): plain sphere 4.15, unrounded 3.34,
    `spread` 30 3.14, default 2.78, stretched 1.26. Mesher-independent (MC and DC
    counts agree within 1%).
  - **GPU vs CPU parity (Metal, M1 Max, 2026-11-20).** wgpu-hal builds Metal
    libraries with Apple's default `MTLCompileOptions`, i.e. fast math on, with no
    wgpu setting to disable it. 94% of samples differ from the CPU (max 2.3e-6);
    1 in ~2.4M samples flips sign at R=48. Exact buffer matches over 20 seeds:
    MC 20/19/17/8 of 20 at R=16/48/96/160, DC 20/18/8/0. Flipped samples renumber
    MC vertices for the rest of the mesh; DC also swaps quad diagonals.
    Surface comparison (20 seeds at R=16–160, 4 seeds at R=254): counts within
    2 vertices / 12 indices; nearest-vertex distance p99.9 ≤ 1.2e-3 cell and max
    ≤ 0.047 cell for MC; DC p99.9 ≤ 0.055 cell, max 0.68 cell; matched normals
    within 1.5e-2; GPU MC meshes closed and manifold, GPU DC meshes closed with
    non-manifold edges at the CPU's rate. Every vertex more than 0.1 cell from
    the other mesh (6 / 42 / 131 DC vertices at R=48/96/160, none for MC) has a
    CPU sample within 1e-5 of zero in its 4×4×4 sample neighbourhood, i.e. it
    comes from a flipped sample; on meshes with identical buffers the max is
    0.011 cell (DC) and 0.003 cell (MC).
    `assert_same_surface` (`src/gpu/tests.rs`) checks: counts within 16 vertices
    / 96 indices (8× observed); p99.9 ≤ 0.01 cell (MC) / 0.1 cell (DC); every
    vertex within 1 cell; every vertex further than 0.05 cell lies next to a
    sample within `SAMPLE_TOLERANCE` (1e-5, ~4× the observed 2.3e-6) of zero, and
    identical meshes have none; matched normals within 0.05; closed, outward,
    and (MC) manifold. Bit-exactness stays covered by the WESL-interpreter test.
  - Bench (M1 Max, batches of 16, ms/rock, GPU vs CPU with rayon): MC R=64
    3.0 vs 21.6, R=100 8.2 vs 73.4, R=128 14.6 vs 150; DC R=100 12.1 vs 97.5,
    R=128 21.4 vs 174. `cargo run --release -p procgena_stone --example bench_gpu --features gpu`.
  - GPU tests: `cargo test --release -p procgena_stone --features gpu --lib -- --ignored gpu`
    (5 tests, ~40 s).
- Bevy conventions (Phase 6):
  - `StoneMaterial = ExtendedMaterial<StandardMaterial, TriplanarExtension>`. The
    extension owns the textures (bindings 100–106: settings uniform, then
    colour/normal/roughness texture + sampler pairs); the base material's texture
    slots stay empty, so `pbr_input_from_standard_material` samples nothing.
    Shaders: `stone_triplanar.wesl` (pure functions, textures as parameters, for
    reuse by the Phase 8 pool), `stone_material.wesl` (bindings +
    `stone_surface`), `stone.wesl` (main + deferred fragment, based on bevy_pbr's
    `pbr.wesl`), `stone_prepass.wesl` (normal/motion-vector prepass fragment).
    Default vertex shaders are kept.
  - Triplanar runs in mesh space: `get_local_from_world(instance_index)` on
    `world_position` (Bevy always defines `VERTEX_OUTPUT_INSTANCE_INDEX` in the
    main and prepass pipelines). Projection `(a, b)` planes are x:(z,y), y:(x,z),
    z:(x,y) with image coordinates `(a, −b) / texture_size`, so OpenGL green
    points along +b; normals use whiteout blending; weights are `|n|⁴`
    normalized; roughness is the red channel.
  - Per-entity generation builds `StoneField` once, then `mesh_field` per plugin
    LOD level and `StoneMesh::scale(options.scale)`; `options.mesh.resolution` is
    ignored (still validated). Cache key is `options.to_json()`, so rocks that
    differ only in `mesh.resolution` generate twice.
  - LOD distances are multiplied by `scale / 2.5`; the fade band is
    `scaled distance ± scaled distance × hysteresis` (procgena_tree's meaning; core
    default 0.05).
  - Viewer: `FreeCamera` uses the right mouse button to look and M to toggle
    cursor grab, so the mesher toggle is Tab, not M. The plan's "± for resolution"
    became `[`/`]` rock size, since the plugin's LOD levels set resolution.
    `--prepass` and `--deferred` exercise the other shader variants; all three
    render correctly on Metal (M1 Max). Unexplained, not reproduced: the first
    `--prepass` and `--deferred` screenshots (run back to back right after the
    first forward run) came back fully black, clear colour included, with no
    errors logged. Later reruns of both, and a control scene with the same
    ingredients, rendered correctly. Cause not found; if a black frame shows
    up again in Phase 8, investigate it rather than assuming a capture glitch.
    A later all-black batch (every mode) coincided with the display being off
    (`pmset -g log`: "Display is turned off"); check that first. After the
    alpha-discard, decal and unlit steps were restored to `stone.wesl`, forward,
    `--prepass` and `--deferred` were re-captured with the display on, with
    textures loaded from the flat `rock/` layout; all three render correctly.
  - Release-only viewer runs are practical: debug generation at R=96 is slow.
    `cargo run --release -p procgena_stone_bevy --example viewer -- --screenshot out.png`.

## Context

ze_stone is a procedural rock generator for Rust/Bevy, the sibling of `/opt/ze_tree`.
After reviewing four prior-art generators, we picked
`prior_art/Unity-Procedural-Rock-Generation` (Zaworski, MIT) as the base. It has the
best shapes and is the fastest. Its method: subtract N smoothly blended spheres from a
sphere's signed distance field (SDF), add triplanar FBM displacement, and mesh the
result with marching cubes on the GPU. Its weaknesses are a non-portable `sin` hash,
an unwelded triangle soup, normals sampled from the grid, and no CPU path.

Goals:
1. A pure-Rust core with a CPU generator and a **twin GPU generator** behind a `gpu`
   feature. Both run the same field and the same two meshers, marching cubes (MC) and
   dual contouring (DC).
2. A `procgena_stone_bevy` crate. It renders rocks generated per entity on the CPU, and a
   GPU-resident **variant pool** whose instances are drawn indirectly.

Landscape scatter (grass-style per-view chunk placement) is a **follow-up plan**. It
will build on the pool and the per-view cull pass from this plan.

Physics uses the **CPU twin only**: colliders are regenerated on the CPU from the same
options and seed. No meshes are read back from the GPU. The only readback is a tiny
status buffer that reports when a capacity was too small.

---

## Bottom line up front

1. **Virtual workspace** with `crates/procgena_stone` (core) and `crates/procgena_stone_bevy`,
   the same layout as procgena_tree. The placeholder `src/lib.rs` (`add()`) is removed.
   Shared workspace dependencies: `glam 0.33` (bevy 0.20's version), `serde`,
   `serde_json`, and `wgpu = "30.0.2"` (bevy 0.20.1's version; optional, core only).
2. **Algorithmically faithful, not bit-identical to Unity.** The carving, smooth
   subtraction, FBM constants, triplanar blend and surface threshold
   (`smoothstep(0.05, 0, d) = 0.5` → d = 0.025) are kept. Changed on purpose:
   - Unity's `frac(sin(x)*43758)` hash is replaced by an integer hash (PCG3D) that
     gives the same result on CPU and GPU.
   - The integer `seed: u32` replaces Unity's float seed.
   - Meshing works directly on the SDF instead of a smoothstep density grid.
   - Normals come from the continuous field's gradient, not from the grid.
   - Boundary cells are forced outside the surface, so every mesh is closed.
3. **The carving list is precomputed.** The `steps` carving spheres (direction and
   radius from the seed) are computed once per rock, in `CarveSet`, and uploaded to the
   GPU. Unity rehashes them for every voxel at every step. The twin then only has to
   agree on the noise hash.
4. **One `Mesher` enum, `{ MarchingCubes, DualContouring }`, on both backends.**
   - Both produce welded, indexed meshes (`Vec<[f32;3]>` positions and normals,
     `Vec<u32>` indices).
   - Both use a **cell-owned emission order**: each cell owns the three grid edges at
     its minimum corner. Vertices and indices are emitted in cell-major order. Because
     the CPU follows exactly the same order as the GPU's prefix sums, the two outputs
     compare element by element.
5. **LOD = re-mesh at a lower resolution** (`LodLevel { distance, resolution, hysteresis }`).
   There is no decimation dependency.
6. **WESL shaders live in core** (`crates/procgena_stone/shaders/`):
   - `field.wesl` (hash, noise, carving, field, gradient), `scan.wesl`, `mc.wesl`,
     `dc.wesl`, `tables.wesl`.
   - They are exported as `&str` constants for Bevy (procgena_tree's `WIND_WESL` pattern).
   - With the `gpu` feature, `build.rs` compiles them to WGSL using `wesl 0.6`.
   - Tests validate them with naga 30.
   - CPU constants are checked against the shader source with `include_str!` tests
     (bevy-grass's "twin" pattern).
7. **`gpu` feature in core:** `GpuStoneGenerator` drives raw `wgpu` from a
   caller-supplied `Device` and `Queue` (Bevy passes its own). It generates a batch of
   (variant × LOD) meshes into **resident** buffers:
   - Vertex/index buffers created with `VERTEX|STORAGE` and `INDEX|STORAGE` usage.
   - Per-mesh `DrawIndexedIndirectArgs` written on the GPU.
   - A status buffer that flags any capacity overflow.
   Parity tests against the CPU are `#[ignore]` GPU tests.
8. **CPU parallelism** through an optional `rayon` feature (default on): z-slabs of
   the sample grid. Without it, sampling is serial.
9. **Rendering uses triplanar texturing; files get UVs.**
   - `StoneMaterial = ExtendedMaterial<StandardMaterial, TriplanarExtension>` samples
     color, normal and roughness in object space, so meshes need no UVs.
   - The `export-obj` and `export-gltf` features (as in procgena_tree) generate
     box-projected UVs, splitting vertices at projection seams.
10. **Bevy pool rendering follows bevy-grass.**
    - `NonMesh` binned phase items are queued in the Opaque3d, Opaque3dPrepass,
      Opaque3dDeferred and Shadow phases.
    - A pipeline is specialized on top of `MeshPipeline`.
    - A per-view compute pass handles culling and LOD selection, then
      `draw_indexed_indirect` is called for each (variant, LOD).
    - The overflow status is read back with bevy-grass's non-blocking `map_async`
      state machine, to grow capacities and regenerate.
    - All of Bevy's render internals are isolated in `render/bevy_glue.rs`.
11. **Textures:** Poly Haven rock sets by Rob Tuytel, starting with `rock_01`, plus
    other popular CC0 rock sets. They are converted to BC7 and UASTC KTX2 with
    procgena_tree's `tools/textures/convert.sh` and stored in repo `assets/`, not in the
    crate. The resolution and git storage are decided with size numbers in Phase 7.

---

## Target public API (sketch)

```rust
// options.rs — serde; from_json merges onto defaults like procgena_tree
pub struct StoneOptions {
    seed: u32,                 // Unity default 880
    scale: f32,                // world size of the [-0.5,0.5]³ domain; Unity 2.5
    stretch: [f32; 3],         // non-uniform scale for variety; default [1,1,1]
    carve: CarveOptions,       // steps 20, smoothness 0.05, base_radius 0.95,
                               // radius_min 2.5, radius_jitter 1.0, depth 0.8
    surface: SurfaceOptions,   // displacement_scale 0.15, displacement_spread 10, octaves 6
    mesh: MeshOptions,         // mesher: Mesher, resolution: u32 (cells per axis; Unity 100)
}
pub enum Mesher { MarchingCubes, DualContouring }   // "marching_cubes" | "dual_contouring"
impl StoneOptions { fn from_json(&str); fn to_json(&self); fn validate(&self) -> Result<(), OptionsError>; }

// field.rs
pub struct CarveSphere { center: Vec3, radius: f32 }
pub struct CarveSet(Vec<CarveSphere>);              // from (seed, CarveOptions); ≤ MAX_CARVE_STEPS (72)
pub struct StoneField { carve: CarveSet, surface: SurfaceOptions }
impl StoneField { fn new(&StoneOptions); fn distance(&self, p: Vec3) -> f32; fn gradient(&self, p: Vec3) -> Vec3; }

// mesh.rs
pub struct StoneMesh { positions: Vec<[f32;3]>, normals: Vec<[f32;3]>, indices: Vec<u32> } // + bounds, triangle_count
pub struct LodLevel { distance: f32, resolution: u32 }
pub fn default_lod_levels() -> [LodLevel; 3];        // e.g. 96 / 48 / 24
pub fn mesh_field(field: &StoneField, mesher: Mesher, resolution: u32) -> StoneMesh;

// stone.rs — immutable wrapper, like procgena_tree::Tree
pub struct Stone { options, field, mesh }
impl Stone { fn new(StoneOptions) -> Result<Self, OptionsError>; fn from_json(&str) -> Result<Self, StoneError>;
             fn mesh(&self); fn mesh_with(&self, Mesher, resolution: u32) -> StoneMesh;
             fn lod_meshes(&self, &[LodLevel]) -> Vec<(LodLevel, StoneMesh)>; }

// gpu/ (feature "gpu")
pub struct GpuStoneGenerator;   // new(&wgpu::Device) builds pipelines from build.rs WGSL
pub struct GpuStoneRequest { options: StoneOptions /* mesher+resolution inside */, vertex_capacity: u32, index_capacity: u32 }
pub struct GpuStoneBatch { vertices: wgpu::Buffer, indices: wgpu::Buffer, draw_args: wgpu::Buffer /*one DrawIndexedIndirectArgs per request*/,
                           status: wgpu::Buffer /*per-request overflow flags + true counts*/, ranges: Vec<GpuMeshRange> }
impl GpuStoneGenerator { fn encode(&self, &wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder, &[GpuStoneRequest]) -> GpuStoneBatch; }
pub fn estimate_capacity(resolution: u32, mesher: Mesher) -> (u32, u32);   // surface-area heuristic + margin

// shaders
pub const FIELD_WESL: &str; pub const SCAN_WESL: &str; pub const MC_WESL: &str; pub const DC_WESL: &str; pub const TABLES_WESL: &str;
```

`procgena_stone_bevy` (sketch):
- `ZeStonePlugin { texture_root: Option<String>, lod_levels: Vec<LodLevel> }`.
- `ZeStone::{Options(Arc<StoneOptions>), Asset(Handle<StoneOptionsAsset>)}` with a
  `.stone.json` loader that supports hot reload.
- Generation runs on `AsyncComputeTaskPool` and results are cached by options JSON.
- Child entities are tagged `StonePart { lod }` and switch with `VisibilityRange`.
- Helpers `stone_mesh(&StoneMesh) -> Mesh` and `stone_material(...)`.
- With the `gpu` feature:
  - A `StonePool` asset: a base `StoneOptions`, `variants: u32` (seeds
    `seed..seed+variants`, each with its own `stretch` jitter) and LOD levels.
  - `PooledStone { pool: Handle<StonePool>, variant: u32 }` entities with a
    `Transform`, drawn through the pool path.
  - For a collider, call `pool.variant_options(variant)` → `Stone::new` → mesh at the
    collider resolution (the CPU twin).

---

## Field and mesher details

- **Field** (unit domain `[-0.5,0.5]³`):
  - `rock(p) = RockSDF(2p)`: a sphere of radius 0.95, then for each `CarveSphere`
    a smooth subtraction `d = mix(d, -b, h) + k·h·(1-h)`, with
    `b = |p + v·r| - 0.8r`.
  - `field(p) = rock(p) + disp·|triplanar_fbm(p, ∇rock(p))| - 0.025`.
  - Then `max(field, box_sdf(p))`, so the domain boundary is always outside the rock.
  - The gradient uses central differences on the full field, with ε = ½ cell.
  - Note the Unity quirk that is kept: displacement only ever shrinks the rock.
- **Value noise / FBM:**
  - Corner values come from PCG3D on the integer lattice, mapped to `[0,1)`.
  - Interpolation and the result squared (`res*res`) follow Unity.
  - Six octaves with Unity's rotation `[[0.87,0.48],[-0.48,0.87]]`, ×2 and +100.
  - Triplanar weights are `max(n²,0.001)` normalised, as in Unity.
- **Grid:** `R` cells per axis, `(R+1)³` samples, with sample spacing `1/R`. This fixes
  Unity's mismatch between `/R` and `/(R-1)`.
- **MC:**
  - Paul Bourke's edge and triangle tables, ported from `Triangulation.compute`.
  - Each cell owns its edges along +x, +y and +z.
  - A vertex's ID is the cell's vertex base plus the edge's rank among the cell's
    owned crossing edges.
  - The ID is derivable from the sample signs alone, which is what lets the GPU
    compute indices after the prefix sum.
- **DC:**
  - One vertex per cell whose corner signs differ. Its position comes from a QEF over
    the edge crossings and gradient normals, regularized towards the mass point and
    clamped to the cell. On the GPU it is solved with a 3×3 Jacobi eigen-solve.
  - One quad per owned crossing edge, connecting the four cells around it. Each quad
    is split along its shorter diagonal, with orientation taken from the sign.
  - DC can create non-manifold vertices in ambiguous cells. This is documented and
    the tests measure it.
- **GPU passes** (per batch; requests are concatenated with base offsets):
  1. `sample`
  2. `classify`: vertex and index counts per cell
  3. `scan`: multi-level exclusive scan, 256 wide, up to 3 levels for ≤ 16M cells
  4. `emit_vertices`
  5. `emit_indices`
  6. `write_args`: indirect arguments and status, clamped to capacity
  Bindings stay at ≤ 8 storage buffers per stage, so the shaders fit WebGPU limits.

---

## Implementation phases

Each phase ends with these passing: `cargo test --all-features`, `cargo clippy
--all-targets --all-features` (and once per feature on its own),
`cargo doc --all-features` and the doctests. After that comes the Codex
comment-rewrite pass from the user's CLAUDE.md, then a report. Commit only when asked.

### Phase 0 — Scaffolding
- Root `Cargo.toml` becomes a virtual workspace (resolver 3, edition 2024 in
  `[workspace.package]`, `[workspace.dependencies]`).
- Create `crates/procgena_stone` and `crates/procgena_stone_bevy`. Delete `src/lib.rs`.
- `.gitignore`: `/target`, `/prior_art`.
- Copy this plan to `/opt/ze_stone/port_plan.md`. It gets the Status, Decisions and
  Agent-notes sections, kept up to date the way procgena_tree's is.
- Credit the source in the README (Zaworski, MIT; references iq's Shadertoy `ldSSzV`
  and Paul Bourke).
- Empty modules: `options`, `hash`, `noise`, `field`, `grid`, `mesh/{mod,mc,dc}`,
  `stone`, `export`, `gpu`.

### Phase 1 — Field (CPU)
- `options` + validation: resolution between 8 and 512, steps between 1 and 72,
  everything finite. Also `from_json`/`to_json` with merge semantics copied from
  `/opt/ze_tree/crates/ze_tree/src/options.rs`.
- `hash` (PCG3D, plus fixed test vectors), `noise` (value noise, FBM, triplanar),
  `field` (`CarveSet`, `StoneField::distance`/`gradient`).
- Tests:
  - Hash vectors.
  - The field is positive on the boundary and negative at the centre for many seeds.
  - The same seed gives the same field values, and different seeds give different ones.
  - The surface radius at default settings is in the expected band, about
    0.40–0.475 of the unit domain.

### Phase 2 — Meshers + Stone wrapper (CPU)
- `grid` sampling, serial or with rayon z-slabs (feature `rayon`, default on). The
  output must be identical either way, and a test checks it.
- `mc` (welded) and `dc`, `StoneMesh`, `Stone`, `LodLevel`/`default_lod_levels`.
- Tests:
  - MC is closed and manifold: every edge is used by exactly two triangles, with
    consistent orientation.
  - Orientation: every normal points outward (it agrees with the triangle's face
    normal).
  - DC is closed on the test seeds; any non-manifold vertices are counted and
    reported.
  - Determinism, and the triangle count grows roughly as R².
  - Islands: the largest connected component holds ≥ 99% of the triangles. A test
    records what it finds; if real floating fragments show up, ask whether to add
    island removal.
- `examples/bench.rs`: times CPU generation at R = 32/64/100/128 for both meshers.

### Phase 3 — Exports
- `export-obj` (no dependencies) and `export-gltf` (`gltf-json 1.4.1`, re-imported in
  tests with `gltf`), modelled on `/opt/ze_tree/crates/ze_tree/src/export/`.
- Box-projected UVs with seam splitting, written only by the exporters.
- `examples/export.rs`.

### Phase 4 — WESL twin
- Port the field, hash, noise, scan, MC, DC and tables to `shaders/*.wesl`, with no
  bindings in the library modules.
- Exported constants, a naga validation test (the pattern of procgena_tree's
  `tests/wind.rs`), and `include_str!` constant-agreement tests (MC tables,
  `MAX_CARVE_STEPS`, FBM constants).
- `build.rs` (gated on `gpu`) compiles the entry points to WGSL in `OUT_DIR`.

### Phase 5 — `gpu` feature
- `GpuStoneGenerator`, `GpuStoneRequest`, `GpuStoneBatch`, `estimate_capacity`.
- A status buffer reports, per request, an overflow flag and the true counts, so the
  caller can grow the capacity and retry.
- `wgpu` 30.0.2 with default features off; tests turn on the default backends.
- Ignored GPU tests (`cargo test --features gpu -- --ignored gpu`): a headless device
  plus a test-only readback, compared with the CPU twin:
  - Same vertex and index counts and identical index buffers, over about 20 seeds ×
    both meshers × 3 resolutions.
  - Positions and normals within ε.
  - Any seed whose topology flips because a sample sits exactly on the surface is
    documented, not tolerated silently.
  - Overflow and retry, and a batch of many requests.
- Bench: GPU timing next to the CPU numbers.

### Phase 6 — `procgena_stone_bevy`: per-entity rocks
- Bevy 0.20.1 with `default-features = false`. Its features are kept to a subset of
  procgena_tree_bevy's and bevy-grass's, adding `bevy_render` and `bevy_core_pipeline` only
  when the pool needs them.
- Plugin, component, `.stone.json` loader, cache, LOD children, the `stone_mesh`
  conversion, and `StoneMaterial` (triplanar main and prepass shaders, with
  whiteout-blended normal maps).
- Patterned on `/opt/ze_tree/crates/ze_tree_bevy/src/{lib,spawn,mesh,material}.rs`.
- `examples/viewer.rs`:
  - Seed stepping, M to toggle the mesher, ± for resolution, F for a rock field to
    check LOD, T to cycle texture sets.
  - A free camera, and `--screenshot` for headless checks.
- Tests: conversion tests without a renderer.

### Phase 7 — Textures
- Use the Poly Haven API to list the CC0 texture sets in the rock category, filtered
  to Rob Tuytel plus the most-downloaded sets.
- **Show the user the list, with sizes at 1k and 2k, before downloading.**
- Then fetch color, normal (GL) and roughness maps, convert them with
  `/opt/ze_tree/tools/textures/convert.sh` into `assets/textures/{bc7,uastc}/stone/<set>/`,
  and add a `LICENSE.md` listing each set and author.
- Decide with the user between committing the files and a fetch script (git size).

**Outcome (user decisions, 2026-11-20):**
- 17 sets from the Poly Haven rock category, listed in `tools/textures/sets.txt`
  (authors in `tools/textures/LICENSE.md`). Ground, aerial, wall, tile, floor, gravel
  and marble sets were excluded, and so was `gray_rocks` (a ground texture of many
  small rocks, not a single rock surface).
- Three sizes per set: 512, 1k and 2k. Poly Haven's smallest download is 1k, so
  `tools/textures/fetch.sh` downloads the 2k JPGs (checked against the API's MD5) into
  the git-ignored `prior_art/polyhaven/<id>/{color,normal,roughness}.jpg`, and
  `tools/textures/convert.sh` downscales them with Lanczos (colour in linear light).
- The encoder settings are the same as procgena_tree's.
- ~~Storage: Git LFS~~, superseded by the GCS sidequest below.

**GCS sidequest (2026-11-20, after Phase 6), mirroring procgena_tree's uncommitted setup:**
- Textures live in the public `gs://ze_textures` bucket under the **`stone/`
  prefix**. The bucket root holds procgena_tree's own manifest and licence, so procgena_stone
  must not write there. Download base URL:
  `https://storage.googleapis.com/ze_textures/stone/`.
- Flat layout `assets/textures/rock/<id>-<size>-<map>-<format>.ktx2` (procgena_tree uses
  `bark/…`), built by `procgena_stone::texture_path`. JPEGs for exports are at
  `assets/textures/images/rock/<id>-<size>-<map>.jpg`.
- Scripts:
  - `fetch.sh` also writes `prior_art/polyhaven/<id>/info.json`.
  - `convert.sh` writes the flat layout.
  - `manifest.sh` writes `assets/textures/assets_manifest.json`.
  - `images.sh` writes the JPEGs and `images/images_manifest.json`.
  - `download.sh` fetches the 512 and 1k sizes, both formats, checked by SHA-256.
- Git tracks the two manifests and `LICENSE.md`. `rock/` and `images/*` are
  ignored. The `.gitattributes` LFS rule is now unused.
- Tests: `crates/procgena_stone/tests/textures.rs` checks that the manifests match
  `TEXTURE_SETS` and `texture_path`. Its ignored test checks the downloaded KTX2
  headers.
- Sizes: 306 KTX2 files totalling 595 MB, and 153 JPEGs totalling 143 MB.
- Upload is done by the user, because `gcloud-claude` is read-only. The commands
  are in the headers of `manifest.sh` and `images.sh`.
- Verified 2026-11-20: `download.sh --sizes 512,1k,2k` fetched all 306 public files
  and every SHA-256 matched. The public `assets_manifest.json` is byte-identical to
  the local one, and all 153 JPEGs and both manifests and licences are served.
- Tidy-up: `.gitattributes` (the unused LFS rule) removed; `download.sh --images`
  fetches the JPEGs from `images_manifest.json`; core `image_path()` builds JPEG
  paths for the export example and the manifest test.

### Phase 8 — `procgena_stone_bevy`: GPU variant pool (`gpu` feature)
- Render world: when a `StonePool` changes, run `GpuStoneGenerator` on Bevy's
  `RenderDevice`/`RenderQueue` into persistent per-pool buffers. Overflow status is
  read back without blocking (bevy-grass `view::check_readback` pattern), and then the
  pool grows and regenerates.
- Instances: extract the `PooledStone` transforms into an instance buffer. A per-view
  compute pass runs frustum culling and LOD selection with dithered bands, compacting
  instance indices per (variant, LOD) with atomics. It then fills in the instance
  counts in indirect arguments copied from the pool's.
- Drawing: `NonMesh` phase items in Opaque3d, Opaque3dPrepass, Opaque3dDeferred and
  Shadow, using a specialized `MeshPipeline` derivative that pulls vertices from the
  pool buffers and shares the triplanar WESL with `StoneMaterial`. Everything is
  confined to `render/bevy_glue.rs` (the bevy-grass structure: `extract.rs`,
  `resources.rs`, `view.rs`, `compute.rs`).
- Viewer: P switches to pool mode (for example 64 variants × 3 LODs, 10k instances).
- GPU tests (`#[ignore]`) use a headless Bevy app, as in
  `/opt/bevy-grass/src/render/gpu_test.rs`.

### Phase 9 — Polish
- README and crate docs, a small set of presets (boulder = Unity defaults, pebble,
  slab, shard, using `stretch` and carve settings), and the feature map against Unity.

---

## Verification

- Unit and integration tests per phase, as listed above. GPU parity and Bevy pool
  tests are run by hand with `-- --ignored gpu` on the M1 Max (Metal).
- Bench example: CPU vs GPU generation times at several resolutions.
- Visual checks: `cargo run -p procgena_stone_bevy --example viewer` (both meshers, LOD
  forest, pool mode). `--screenshot` captures frames to compare with Unity's
  `Image1.jpg`.
- Exports: GLB checked with the Khronos glTF-Validator, as in procgena_tree.

## Decisions (from planning Q&A)
1. Base: Unity-Procedural-Rock-Generation, with the improvements in BLUF 2–5.
2. Meshers: both MC and DC, on both CPU and GPU, behind one `Mesher` enum.
3. LOD: re-mesh at a lower resolution.
4. GPU: in core behind the `gpu` feature, using raw wgpu with a device supplied by
   the caller. WESL is compiled by `build.rs` using `wesl 0.6`.
5. GPU output: resident buffers only. Physics uses the CPU twin, never a mesh
   readback.
6. Parity: same topology, positions within ε. Exceptions are documented.
7. GPU model: variant pool plus instancing. Landscape scatter is a follow-up plan.
8. CPU parallelism: `rayon`, optional and on by default.
9. Material: triplanar in Bevy; OBJ and glTF exports with box-projected UVs.
10. Textures: Poly Haven CC0 (Rob Tuytel's `rock_01` and others), converted to
    BC7 and UASTC KTX2.
11. (Phase 1, agent choice, approved by user 2026-11-20) Surface noise is seeded: the noise
    hash includes `seed`, so different seeds also differ in roughness. Unity's noise
    ignores the seed.
12. (Phase 1, agent choice, approved by user 2026-11-20) `stretch` is applied inside the
    field (carving only), so roughness stays isotropic and the longest axis fills
    the cube.
13. (Phase 1, user decision 2026-11-20) JSON **ignores** unknown names (applies to
    `StoneOptions` and `LodLevel`), so users can store their own metadata next to
    the settings. Unknown names are not kept by `to_json`. Wrong value types are
    still errors. (Originally the agent chose to reject unknown names; reversed.)
14. (Phase 1, agent choice, approved by user 2026-11-20) `carve.steps` allows 0 (a plain
    sphere); the plan said 1–72.
15. (Phase 2, agent choice, approved by user 2026-11-20) DC vertices use a regularized
    least-squares solve, `(Σ n nᵀ + 0.05 I) y = Σ n (n·(p − mass))` in cell units,
    then clamp to the cell, instead of the planned Jacobi-eigen truncated SVD.
    It is simpler and easy to port to WGSL, which has no `inverse`, by inverting the
    3×3 matrix directly.
16. (Phase 2, agent choice, approved by user 2026-11-20) Edges are owned by *samples*
    (`(R+1)³` items), not cells. Edge ids are their rank in the sorted crossing list,
    found by binary search, so no per-voxel id array is kept. Because boundary
    samples are always outside, this order equals the cell-major order the GPU
    prefix sum will produce.
17. (Phase 2, agent choice, approved by user 2026-11-20) `mesh_field`, `Stone::mesh_with` and
    `Stone::lod_meshes` return `Result` for an out-of-range resolution instead of
    panicking. The `default_lod_levels` distances (0 / 15 / 40 world units for
    96 / 48 / 24 cells) are placeholders until they can be tuned in the Phase 6
    viewer.
18. (Phase 3, agent choice, approved by user 2026-11-20) Exporters take a shared
    `ExportMaterial { base_color, roughness, texture_size }` (default grey 0.35 linear,
    roughness 0.9, one texture tile per world unit) because `StoneOptions` has no
    material settings. Texture callbacks receive only `TextureMap {Color, Normal,
    Roughness}`; the caller picks the texture set.
19. (Phase 3, agent choice, approved by user 2026-11-20) The glTF export writes analytic
    box-projection tangents instead of leaving them to the viewer, as procgena_tree does.
    An empty mesh is an error (`GltfError::EmptyMesh(lod)`), since glTF accessors
    cannot be empty; OBJ writes an object with no faces.
20. (Phase 4, agent choice, approved by user 2026-11-20) Shader constants live in a public
    `procgena_stone::shaders` module (`HASH_WESL` … `SCAN_WESL`, `SHADER_MODULES`) instead
    of the crate root. Per-request settings use one uniform with a dynamic offset,
    and scratch buffers are shared across requests, so a batch is a sequence of
    per-request dispatches rather than one concatenated dispatch.
21. (Phase 4, agent choice, approved by user 2026-11-20) The GPU path is limited by the
    prefix buffer (8 bytes × `(R+1)³ + 1`) under WebGPU's default 128 MiB storage
    binding: R ≤ 254 (R = 255 is 8 bytes over). The scan recurses to any depth, so
    it is not the limit. Phase 5 rejects larger resolutions on the GPU path with an
    error (no raised device limits); the CPU still allows up to 512. User rationale:
    the GPU path is for fast procedural worlds; high-resolution assets for export
    come from the CPU.
22. (Phase 3 follow-up, user decision 2026-11-20) The exporters reject a
    `texture_size` that is not finite and greater than zero: `to_obj` returns
    `Result<ObjFiles, InvalidTextureSize>` and glTF returns
    `GltfError::InvalidTextureSize`.

23. (Phase 5, user decision 2026-11-20) GPU meshes stay in the unscaled unit
    cube, like `mesh_field`; each `GpuMeshRange` reports `scale`, and the renderer
    applies it through the instance transform. `Stone::mesh()` stays world-scaled.
24. (Phase 5, user decision 2026-11-20) GPU parity means the *same surface*, not
    identical buffers: GPU fast math (Metal default, FMA contraction elsewhere)
    makes exact topology impossible near the surface. The thresholds and
    measurements are in "Agent working notes"; Decision 6's "same topology" is
    replaced by this.
25. (Phase 5, user decision 2026-11-20) Dev-dependency `futures-lite` 2.6.1
    (`block_on` for wgpu futures in tests and the GPU bench); already in the lock
    file through bevy. Chosen over `pollster`.
26. (Phase 6, user decision 2026-11-20) The texture set lives in core:
    `StoneOptions::material: MaterialOptions { texture, color, roughness,
    texture_size }` (defaults `rock_01`, white, 1.0, 1.0; validated). Core also
    gains `TEXTURE_SETS`, `TextureSize`, `TextureCompression`, `TextureMap`
    (moved from `export`) and `texture_path` (`<compression>/stone/<set>/<size>/<map>.ktx2`).
27. (Phase 6, consequence of 26) `ExportMaterial` is removed: the exporters read
    `options.material`, `to_obj` now takes `&StoneOptions` like the glTF
    exporters, and `GltfOptions` keeps only `textures`. Untextured exports now
    default to white; the export example sets grey when it finds no images.
28. (Phase 6, user decision 2026-11-20) Bevy LOD switching distances scale with
    rock size (`options.scale / 2.5`).
29. (Phase 6, user decision 2026-11-20) Core `LodLevel` gains `hysteresis`
    with procgena_tree's meaning: switching margin on each side, as a fraction of
    `distance` (default 0.05).
30. (Phase 6, user decision 2026-11-20) `StoneMesh::scale` is public, so callers
    can mesh several resolutions from one `StoneField` without `Stone::new`'s
    extra full-resolution mesh.

## Open items to raise during implementation
- Islands after displacement (Phase 2): none found. Every test seed meshes as one
  piece at R=64 with both meshers, so island removal is not needed for now.
- Texture resolution and git storage (Phase 7): decided. See the Phase 7 outcome.
- Whether Bevy's `RenderDevice` gives direct access to the `wgpu::Device` in 0.20.1
  (check in Phase 8). If it doesn't, stop and discuss.
