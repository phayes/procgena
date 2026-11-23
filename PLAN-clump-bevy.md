# Tagged generation results and procgena_clump_bevy

## Bottom line

- **New shared crate `procgena_mesh`** (folder `mesh/crates/procgena_mesh`,
  depends on `glam` only, plus an optional `bevy` feature). It defines what a
  finished generation is, so generators can feed each other:
  - `Object { levels: Vec<Level> }`, most detailed level first.
  - `Level { start: f32, parts: Vec<Part> }`. `start` is the camera distance
    where the level begins, in metres (0 for level 0).
  - `Part { tags: TagSet, mesh: Arc<SurfaceMesh>, closed: bool, shells: Vec<Range<u32>> }`.
    `SurfaceMesh` is positions, normals and `u32` indices in metres. `shells`
    lists the part's separate pieces as triangle ranges, in order and
    covering every triangle once; it is empty when unknown.
  - `Tag` is an open string. The crate exports well-known constants (`ROCK`,
    `GROUND`, `BARK`, `TRUNK`, `LIMB`, `BRANCH`, `LEAF`, `CARD`, `FRUIT`,
    `FLOWER`, `CLUMP`, `SNOW`, `MOSS`, `MUD`, `LEAVES`). A part has several
    tags: what it is made of (`bark`) and what it is (`trunk`). User meshes
    and future generators can add their own tags.
  - `TagFilter { any_of, none_of }`, and `Object::merged(level, &filter)`,
    which joins the matching parts into one `Part`, with the union of their
    tags. It is closed only if all of them are, and its shells are known
    only if all of theirs are.
  - These move here from `procgena_clump`: `Surface`, `SurfaceMesh`,
    `SurfaceError`, position welding and `is_closed`.
  - Behind `bevy`: a `GeneratedObject(Arc<Object>)` component (bevy_ecs
    only) that stone, tree and clump put on their entities.
- **The facts are plain data.** A part carries tags, closedness and shells
  only. It has no query trait objects, so clump keeps its own ray-based
  inside tests.
- **The prepared-surface plan is superseded** (its file was deleted on
  2026-11-22; it is in git history). No `PreparedSurface` and no
  `MeshFacts` trait. Its phase 1 (moving `Surface` and welding into a shared
  crate) survives here. Its tree-shell idea becomes `Part::shells`.
- **Generators emit `Object`s.**
  - `Stone::object()`: one part per level, tagged `rock`, closed, one shell.
  - `Tree::object(...)`: bark becomes up to three parts, split by
    `BranchSpan::level`:
    - `bark + trunk` (level 0);
    - `bark + limb` (level 1);
    - `bark + branch` (level 2 and deeper).

    Each part takes `closed` from `LodDetail::closed`, with one shell per
    branch. Leaves are tagged `leaf` + `card`, open. Fruit and flowers get
    `fruit` and `flower`.
  - `SurfaceMesh::ground` gives an open part tagged `ground`.
  - Billboard-only geometry (ribbons, `billboard_cull`) is not a part:
    clumps can't grow on it.
- **Clump grows on tags.** `ClumpOptions` gains:
  - `grows_on: TagFilter`;
  - `tags: TagSet`, the tags its own output carries;
  - `material: MaterialOptions` (colour, texture set, texture size,
    roughness).

  Each preset declares all three. For example:
  - `SnowCap` grows on `rock | bark | ground | moss`, excludes `card`, and
    outputs `clump + snow`;
  - a trunk moss preset grows on `trunk | limb` only.

  `Clump::on(&Object, &options)` takes `merged(0, grows_on)`. Using
  `shells` for its "inside another part" checks is phase 3b. `Clump::object()` returns
  an `Object` too, so a clump can itself be grown on later.
- **New crate `procgena_clump_bevy`**, in the house pattern of the stone and
  tree Bevy crates:
  - `ClumpPlugin`, `ClumpParams`, `ClumpPart`, `ClumpGenerating` and
    `ClumpFinished`;
  - mesh conversion with `fade` in the vertex colour's alpha;
  - `ClumpMaterial`: triplanar textures, plus a rim that breaks up along
    the texture height where `fade` is low.

  A `ClumpParams` is spawned as a **child of its host**. It waits for the
  host's `GeneratedObject`, builds in the background and spawns the clump
  mesh. It doesn't depend on the stone or tree Bevy crates, and it never
  reads meshes back from the GPU.
- **`procgena_stone_bevy` and `procgena_tree_bevy`** keep the core result
  and insert `GeneratedObject` on the host when generation finishes.
- **The studio moves onto the crate** in the last phase.
  `procgena/src/scene/clump.rs` (`ClumpJob`) is replaced by a `ClumpParams`
  child.

### Data flow

```
procgena_stone / procgena_tree / SurfaceMesh::ground / user Mesh3d + ObjectTags
        │  Object { levels[ Level { start, parts[ Part { tags, mesh, closed, shells } ] } ] }
        ▼
stone_bevy / tree_bevy insert GeneratedObject(Arc<Object>) on the host entity
        │
        ▼
procgena_clump_bevy: ClumpParams child of the host
   1. wait for the host's GeneratedObject
   2. directions world → host-local (host GlobalTransform rotation);
      lengths ÷ host uniform scale
   3. background task: Clump::on(object, options) → ClumpMesh + BuildStats
   4. drop the result if options or the host's object/transform changed meanwhile
   5. spawn ClumpPart child: Mesh3d (positions, normals, fade) + ClumpMaterial
   6. insert GeneratedObject(clump.object()) on the ClumpParams entity
   7. trigger ClumpFinished { entity, result: Ok(ClumpBuilt { stats, build_time }) | Err }
```

### Decisions made in this plan (flag any you disagree with)

- **Duplicate geometry for now.** A part's `SurfaceMesh` copies positions,
  normals and indices out of `StoneMesh` and the tree's `MeshBuffers`. That
  costs memory for every level kept on the host. A follow-up could have
  generators store their geometry as `Arc<SurfaceMesh>` and share it. That is a wide refactor of
  stone and tree, so it is not in this plan.
- **Host transform.**
  - Directions are world-space and are turned into the host's frame at
    build time. A clump rebuilds when the host's global rotation or scale
    changes. Translation is ignored.
  - A host with non-uniform scale gets `ClumpBuildError::NonUniformScale`.
  - Uniform scale divides `options.scale`, so lengths stay in world metres.
- **Host detail levels.** v1 grows on host level 0 and draws the clump at
  every distance. When the host switches to a coarser level, the clump can
  float or clip. Growing one clump level per host level waits for core
  clump LOD (`clump/plan.md` phase 5).
- **No cross-host cache in v1.** Clump output depends on the surface as well
  as the options, and `ClumpOptions` has no `Hash` or serde. Each
  `ClumpParams` builds its own mesh. A `ClumpMaterial` is shared per equal
  `MaterialOptions`.
- **Unsupported hosts.**
  - `StonePool` / `PooledStone`: their meshes live only on the GPU.
  - A user `Mesh3d` whose asset isn't kept in the main world
    (`RenderAssetUsages::MAIN_WORLD` missing): its clumps keep waiting. A
    mesh that is readable but has no triangles, positions or normals gets
    one warning in the log.
- **No wind.** A clump on a tree doesn't sway. Copying the tree's `wind`
  attribute onto the clump comes later.
- **Rim breakup is opaque and view-independent.** `fade` travels as the
  alpha of a white vertex colour. The material uses `AlphaMode::Mask`, so
  Bevy runs its fragment shader in shadow passes too. It discards where
  `fade < 0.5 × (1 − texture height)`, with the height read using the
  triangle's flat normal, so every pass removes exactly the same surface.
  Screen-space noise was tried first. It cast stippled shadows onto the
  host and would crawl with the camera, so it was dropped. Without textures
  nothing is discarded.
- **`ClumpMesh::fade` now means coverage, not height ÷ nominal thickness**
  (changed 2026-11-22). Layer clumps take it from the coverage weight
  before any height limits. Blobs take it from each blob's own height.
  Before, snow capped thin on tree branches had fade ≈ 0.1 everywhere, and
  the textured rim test discarded nearly all of it.
- **The triplanar shader is copied** from `procgena_stone_bevy` into
  `procgena_clump_bevy`. Sharing one shader library between the two is a
  follow-up refactor.

## Phases

| Phase | Scope | Parallel? |
|---|---|---|
| 1 ✅ | `procgena_mesh`: `Tag`, `TagSet`, `TagFilter`, `Part`, `Level`, `Object`, `Object::merged`. Move `Surface`, `SurfaceMesh`, welding and `is_closed` from clump; clump re-imports them. `bevy` feature with `GeneratedObject`. | — (edits root `Cargo.toml`) |
| 2a ✅ | `procgena_stone`: `Stone::object()`. `procgena_stone_bevy`: insert `GeneratedObject` on finish, remove it on reset. | after 1, parallel with 2b, 3 |
| 2b ✅ | `procgena_tree`: `Tree::object(...)` / `Skeleton::object` / `TreeMesh::parts`. `procgena_tree_bevy`: insert `GeneratedObject`. | after 1, parallel with 2a, 3 |
| 3 ✅ | `procgena_clump`: `grows_on`, `tags`, `MaterialOptions`, `TEXTURE_SETS` + `TextureSet::path`, `Clump::on(&Object, ..)`, `Clump::object()`. Presets declare tags and textures. | after 1 |
| 3b | Clump uses `Part::shells` for its "inside another part" checks (needs per-shell ray queries in the BVH; measure folds and crossings on the oak and the rocks before and after). | after 3 |
| 4 ✅ | `procgena_clump_bevy`: plugin, `ClumpParams`, lifecycle, mesh conversion, `ClumpMaterial` + shaders, `ObjectTags` for user meshes, viewer example, tests. | after 2a, 2b, 3 |
| 5 ✅ | Studio: replace `ClumpJob` with `ClumpParams`; map `ClumpFinished` to `ViewerEvent::ClumpBuilt`; colour → `MaterialOptions::color`. `ClumpSettings::color` is gone; the Tint control edits `clump.material.color` directly. The plugin swaps the material for material-only changes instead of rebuilding. The studio's render-test scene (`HeadlessScene`) draws clumps untextured (`ScenePlugin::without_clump_textures`), so the tests recognise snow by its flat colour. | after 4 |
| 6 ✅ | Docs and housekeeping: `clump/README.md` and `mesh/README.md`, root README table, `CLAUDE.md` package rows for `mesh/` and `procgena_clump_bevy`, `clump/plan.md` "Later" updated, `PLAN-prepared-surface.md` deleted. | last |

## Step-by-step

### Phase 1: `procgena_mesh`

1. Create `mesh/crates/procgena_mesh` with `glam` (workspace) and optional
   `bevy_ecs` behind `bevy`. Pin `bevy_ecs` to the version Bevy 0.20.1
   uses, so no new Bevy build configuration appears. **Tell the
   coordinator first**: this edits the root `Cargo.toml` and `Cargo.lock`.
2. `tag.rs`: `Tag(Cow<'static, str>)` with `const fn` constants, `TagSet`
   (a small sorted `Vec<Tag>`), and `TagFilter { any_of, none_of }` with
   `matches(&TagSet)`. An empty `any_of` matches every part.
3. `object.rs`: `Part`, `Level`, `Object`, with a `validate()` that checks
   buffer lengths, indices in range, level starts and shell coverage.
4. `surface.rs`: move `Surface`, `SurfaceMesh`, `SurfaceError`, welding and
   `is_closed` from `procgena_clump/src/surface.rs`, with their tests.
   `procgena_clump` depends on `procgena_mesh` and uses the types directly,
   with no re-export shim.
5. `Object::merged(level, &TagFilter) -> Option<Part>`: appends the matching
   parts, offsets indices and shell ranges, unions tags, and reports
   `closed = all matching parts closed`.
6. Tests: filter semantics, merged index and shell offsets, and a validation
   error for each kind of bad buffer.

### Phase 2a: stone

1. `Stone::object() -> Object`: one level per detail level, with
   `start = DetailLevel::distance` scaled to the rock's size, as the Bevy
   plugin scales it. Each level has one part tagged `ROCK`, or none if its
   mesh is empty. `closed` is checked on the mesh, and shells are left
   unknown, because a carved rock can fall into several pieces.
2. `procgena_stone_bevy`:
   - Add `procgena_mesh` with its `bevy` feature.
   - Keep the `Arc<Object>` in the generated cache entry.
   - Insert `GeneratedObject` when the parts spawn, next to the existing
     `StoneEmbedDepth` handling, and remove it in `reset_changed_stones`.
3. Tests: the object matches `Stone::meshes()` level by level, and in the
   Bevy crate the component appears with `StoneFinished`.

### Phase 2b: tree

1. Build the object from each LOD's `TreeMesh`:
   - bark: split `branches` by role. A branch that carries on from its
     parent's tip (`Attachment` at the last section, `t == 0`) takes its
     parent's role. So `BARK + TRUNK` is the trunk plus its continuations,
     `BARK + LIMB` is the side branches of a trunk chain plus their
     continuations, and `BARK + BRANCH` is everything else. Each
     continuation chain is copied as one block and becomes one shell. With
     closed bark every chain is watertight on its own, which the tests
     check. Each part takes `closed` from `LodDetail::closed` with tube bark.
     `Object::merged` joins them again when a filter selects several;
   - leaves: `LEAF` + `CARD`;
   - fruit: `FRUIT`;
   - flowers: `FLOWER`.

   Empty buffers and buffers without normals are skipped: ribbon bark and
   camera-facing fruit.
2. `procgena_tree_bevy`: insert `GeneratedObject` the same way as stone,
   alongside `TreeFinished`.
3. Tests:
   - the three bark parts together cover every bark triangle exactly once;
   - the trunk part is larger than the level-0 branch alone;
   - every part and every shell is watertight at LOD0;
   - the tags are as listed.

### Phase 3: clump core

1. `ClumpOptions`:
   - add `grows_on: TagFilter`, `tags: TagSet` and
     `material: MaterialOptions { color, texture, texture_size, roughness }`;
   - `validate()` checks the texture name against `TEXTURE_SETS`;
   - add a `TextureSize` and `texture_path` mirroring `procgena_stone`, plus
     the `TEXTURE_SETS` list taken from `clump/textures.yml`.
2. `Clump::on(object: &Object, options) -> Result<Clump, ClumpError>`:
   - takes `object.merged(0, &options.grows_on)`;
   - returns an empty mesh when nothing matches.

   Keep `Clump::new(Surface, ..)` for plain meshes. Using the shells moved
   to phase 3b.
3. `Clump::object()`: one level, one part with `options.tags`, `closed` from
   `MeshOptions::closed`, and unknown shells. Clump groups aren't tracked
   per triangle.
4. Presets set `grows_on`, `tags` and a default texture set (snow → `snow_02`,
   moss → `moss_002`).
5. Tests:
   - a snow preset on an object with bark and leaf parts never covers the
     leaves;
   - a filter that matches nothing gives an empty clump;
   - shells stop a branch-in-trunk junction from being covered, using the
     spike's open-tube fixture rebuilt with explicit shells.

### Phase 4: `procgena_clump_bevy`

1. The crate, at `clump/crates/procgena_clump_bevy`, with **exactly**
   `procgena_stone_bevy`'s Bevy feature list. Dev-dependencies:
   `procgena_stone_bevy` and `procgena_tree_bevy`, for the example and tests
   only.
2. `mesh.rs`: `clump_mesh(&ClumpMesh) -> Mesh`, with `fade` as the alpha of
   a white `ATTRIBUTE_COLOR`. That avoids three custom vertex shaders.
3. `material.rs`:
   - `ClumpMaterial = ExtendedMaterial<StandardMaterial, ClumpExtension>`,
     with `AlphaMode::Mask`;
   - `ClumpTextures::load` and `clump_material(&MaterialOptions, ...)`.
4. Shaders:
   - `clump_triplanar.wesl`, copied unchanged from stone;
   - `clump_material.wesl`: bindings, `clump_surface`, and the rim test;
   - `clump.wesl` and `clump_prepass.wesl`, the fragment shaders.
5. `spawn.rs`:
   - `ClumpParams(Arc<ClumpOptions>)` with `new` and `preset`, and
     `#[require(Transform, Visibility)]`;
   - `ClumpGenerating`;
   - `ClumpFinished { entity, result: Result<ClumpBuilt, ClumpBuildError> }`;
   - `ClumpBuilt { stats, build_time, vertices, triangles }`;
   - `ClumpBuildError { Clump(ClumpError), NonUniformScale }`;
   - `ClumpPart` on the spawned mesh child.

   The systems run chained in `Update`:
   - `describe_tagged_meshes`: `ObjectTags` on a `Mesh3d` entity gives
     that same entity a `GeneratedObject`;
   - `remove_clumps`;
   - `start_generation`: compares the options, host object (by `Arc`
     identity) and host world rotation and scale with the running or last
     build. The host transform comes from `TransformHelper`, because it
     may not be propagated yet on its first frame. A changed input replaces
     the running task, so stale builds never finish;
   - `finish_generation`: replaces the part, which stays visible until then;
   - `prune_materials`.
6. `lib.rs`:
   - `ClumpPlugin { texture_root, texture_compression }`, with stone's
     defaults (`procgena_clump/textures`, BC7);
   - crate docs in the stone style: display a clump on a rock, on a tree,
     on your own mesh, textures, the host rules above.
7. `examples/viewer.rs`: a rock with snow, a tree with moss, and a ground
   patch with a leaf pile; keys to reroll seeds and switch presets.
8. Tests (headless `App` with `MinimalPlugins` + `AssetPlugin`):
   - waits for the host;
   - builds once the host object arrives;
   - rebuilds and drops the stale build when options change mid-build;
   - rotating the host rebuilds, translating it doesn't;
   - non-uniform scale gives an error;
   - removing `ClumpParams` despawns the part;
   - `clump_mesh` keeps `fade`.

### Phase 5: studio

1. Replace `ClumpJob` and `SurfaceBuffers` with one `ClumpParams` child of
   the subject. Settings changes replace the component.
2. Observe `ClumpFinished` and write `ViewerEvent::ClumpBuilt` with the same
   `BuildStats` and error mapping as today, so the studio tests in
   `studio/editing.rs` still pass unchanged.
3. Route `ClumpSettings::color` into `options.material.color`, and drop
   `CLUMP_ROUGHNESS`. Add `ClumpPlugin` to the app.
4. `procgena` depends on `procgena_clump_bevy`.

### Phase 6: docs and housekeeping

Done:
- `clump/README.md` (the group had none) and `mesh/README.md`, and rows in
  the root README's crate table;
- `CLAUDE.md` package rows for `procgena_clump_bevy` and `procgena_mesh`;
- `clump/plan.md`: the Bevy integration moved out of "Later", with what is
  still open;
- `PLAN-prepared-surface.md` deleted.

## Verification (scoped, per phase)

```bash
cargo test -p procgena_mesh --all-features
cargo test -p procgena_stone --lib && cargo test -p procgena_stone_bevy --test convert
cargo test -p procgena_tree && cargo test -p procgena_tree_bevy
cargo test -p procgena_clump
cargo test -p procgena_clump_bevy
cargo clippy -p <package> --all-targets -- -D warnings
cargo doc -p <package> --no-deps --all-features
cargo test -p <package> --doc --all-features
cargo run -p procgena_clump_bevy --example viewer
cargo test -p procgena --lib studio::editing
```

After the last phase, the coordinator runs the full suite from `CLAUDE.md`.

## Later (not in this plan)

- Clump on clump (snow on moss): gather the `GeneratedObject`s of sibling
  `ClumpParams`, with an explicit order.
- One clump level per host level, once core clump LOD lands.
- Wind: interpolate the tree's `wind` attribute onto clump vertices.
- `.clump.json` loader, once `procgena_clump` gains serde. Tags already
  serialize as strings.
- Stalk cards (`clump/plan.md` phase 4) as a second `ClumpPart` kind.
- Sharing geometry: generators store `Arc<SurfaceMesh>` instead of copying.
- A shared triplanar shader library for stone and clump.
