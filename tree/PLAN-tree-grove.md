# TreeGrove: a mix of trees laid out on land

The tree equivalent of `PlantField`. A grove is a list of tree species, each
with its own density, spacing and clustering, spread over a rectangle of
ground.

## Status

- **Phase 0 (shared ground): done.** `Ground`, `FlatGround` and `HeightGrid`
  live in `procgena_mesh::ground`. Deadwood re-exports them, so its public
  API is unchanged.
- **Phase 1 (core grove in `procgena_tree`): done.** See "Core API (built)".
- **Phase 2 (Bevy component): done**, in
  `tree/crates/procgena_tree_bevy/src/grove.rs` and `examples/grove.rs`. It
  differs from the Phase 2 section below in these ways:
  - The child marker is `GroveMember { tree, variant, id }`.
  - Placement runs in a **background task**, not synchronously. This is
    the same pattern as deadwood's `Building`/`Attempted`, here called
    `Placing`/`Placed`.
  - Each frame, every grove's input (grove, size and ground) is compared
    with its last one, so file edits and host changes need no event
    system.
  - The `.grove.json` loader is registered but has no test that loads a
    real file. The asset test adds and edits the asset in memory.
- **Phase 4 (docs): done.** `tree/docs/groves.md` is the guide and
  reference, and both READMEs have a "Plant a grove" section. Two tests in
  `tests/grove.rs` check the guide's grove file and preset tables against
  the code.
- **Phase 3 (studio tab): done.** The Tree Grove tab (after Trees) edits a
  grove in `procgena/src/grove/`. The scene shows it in `SceneMode::Grove`,
  updated in place through `ViewerCommand::SetGrove`. The status bar shows
  the tree count and every tree's size at full detail. Headless tests cover
  building, edits, presets and save/load. Differences from the plan below:
  - The grove has its own **Grove Size** control (10–100 m, 40 m to start)
    instead of the environment's Field Size, which suits grass.
  - `GroveSettings { grove, size }` carries the size.
  - Rows also show **Spacing**, and a row's trees per hectare count the
    trees a flat-ground placement actually grows.
  - There is no Level of Detail picker. Each grove tree switches detail by
    its own camera distance, and grove mode ignores the Trees tab's choice.
  - The status bar shows the grove's size at full detail, not the variants
    generating.

## Bottom line

- **The core is in `procgena_tree`**: `src/grove.rs`, `src/grove/presets.rs`,
  `src/grove/file.rs` and the preset files in `groves/*.grove.json`. It has
  no Bevy and no new dependencies.
- **A new Bevy component in `procgena_tree_bevy`**, `TreeGroveParams`.
  - It reuses the existing tree pipeline. Each placed tree is a child entity
    with `TreeParams` and a `Transform`.
  - Trees of the same variant share one `TreeKey`, and through `TreeCache`
    the same meshes and materials. LOD, wind and materials therefore work
    unchanged.
  - The target is hundreds of trees per grove, at about 8–16 part entities
    per tree.
  - Ground is `Flat` or `Host`, the same as deadwood's `ScatterGround`.
- **A studio "Tree Grove" tab** that mirrors Plant Field, with Save and Load
  as `.grove.json`.
- **Placement is stable under edits.**
  - A density change only adds or removes trees.
  - The grove's seed moves trees but never changes a mesh.
  - Children are matched up by `GroveInstance::id`, so an edit respawns only
    the trees that changed.

## Core API (built)

| Item | What it is |
| --- | --- |
| `GroveTree { tree, density, spacing, density_scale, clustering, variants }` | One species, the counterpart of `FieldPlant`. `density` is trees per hectare; `spacing` is the smallest trunk-to-trunk distance in metres (the larger of the two applies between species); `variants` runs from 1 to 8. |
| `GroveTree::new(TreePreset)`, `custom(options, density, spacing)`, `.scaled()`, `.clustered()`, `.with_variants()`, `density()`, `variant(v)` | `variant(v)` returns the options of shape variant `v`. Variant 0 is `tree` itself; the others change only the seed, derived from `tree.seed` and `v`. |
| `TreeGrove { seed, trees, density_scale, variation }` | The grove definition: what a `.grove.json` holds. There is **no size and no ground**. |
| `TreeGrove::new(trees)`, `from_preset`, `density()`, `with_density(per_ha)`, `with_trees`, `validate()`, `from_json`, `to_json` | `Default` is `GrovePreset::OakWoodland`. |
| `GroveVariation { scale_jitter, lean, max_slope, sink }` | Changes only a tree's transform: its size, lean (degrees), the steepest ground it stands on (degrees), and how far it sinks into the ground (metres). |
| `GrovePlacement::new(&grove, size, &dyn Ground, Option<&dyn Fn(f32, f32) -> f32>)` | Lays the grove out on a `size` rectangle centred on the origin. `instances()` returns `GroveInstance { tree, variant, id, position, transform }`. |
| `GroveError` | `Options`, `Tree { index, error }`, `TooManyTrees`, `TooMany`. The limits are `MAX_GROVE_TREES` (16), `MAX_GROVE_VARIANTS` (8), `MAX_GROVE_INSTANCES` (4000) and `MAX_GROVE_SIZE` (2048 m). |
| `GrovePreset` (an enum, like `TreePreset`) | `OakWoodland`, `AspenStand`, `MixedDeciduous`, `PineStand`, `CedarGrove`, `Orchard`, `SpringBlossom` and `WinterCopse`, with `ALL`, `key`, `from_key`, `name`, `description`, `group`, `json` and `grove`. Each preset is an embedded `groves/<key>.grove.json`. |
| `TreePreset::key()`, `from_key()`, `grove_density()`, `grove_spacing()` | Keys are snake_case (`"oak_small"`). Density is 10,000 / (π r²), about one tree per crown area. Spacing is 0.55 r, where r is the preset's measured crown radius. |
| Re-exports | `procgena_tree::{Ground, FlatGround, HeightGrid}`. |

### Placement

1. **Sites.**
   - The candidate sites lie on a fixed lattice of 0.5 m cells anchored to
     the origin, one jittered site per cell. Each site's position, rank and
     `id` depend only on the grove's seed and the cell.
   - A site is kept when `rank / local_density < total_density × cell²`, so
     a lower density keeps a subset of a higher one's sites. This also holds
     under a density function.
2. **Species.**
   - Species are picked in list order, and each takes its share of what the
     species before it left. The last species with a density above 0 takes
     the rest.
   - A clustered species decides a fraction `clustering` of its sites by a
     patch value instead of an independent draw. Patches are jittered
     Voronoi cells about `max(6 × spacing, 4 m)` across. The patch values
     are spread evenly from 0 to 1, so a clustered species **keeps its
     share** (tested at 15% for clustering 0, 0.6 and 1).
3. **Ground.**
   - The ground is sampled at the trunk and at 4 points around it, at the
     larger of the trunk radius and 0.25 m.
   - A tree is left out where any sample is NaN or the slope exceeds
     `max_slope`.
   - The tree stands at the lowest sample minus `sink`. It stays upright,
     apart from its random lean.
4. **Spacing.**
   - Sites are visited in rank order, using a spatial hash, and a site is
     dropped if it is too close to a tree already kept.
   - With the preset values, spacing leaves 79–94% of the stated density
     (tested at above 75%).
5. **Transform.** Each tree gets a random yaw, a lean, and a uniform scale of
   `1 ± scale_jitter`.

**Size changes** keep trees away from the edges. A tree near the old edge
can make way for a new neighbour beyond it.

### File format (`.grove.json`, camelCase)

```json
{
  "base": "pine_stand",
  "seed": 4,
  "densityScale": 0.5,
  "variation": { "scaleJitter": 0.15, "lean": 3, "maxSlope": 35, "sink": 0.1 },
  "trees": [
    { "tree": "cedar", "densityScale": 0.2, "clustering": 0.5 },
    { "tree": { "base": "pine_medium", "seed": 9 }, "density": 300, "spacing": 1.0, "variants": 3 }
  ]
}
```

- **Everything is optional.** Missing settings come from `base`, a
  `GrovePreset` key. With no `base`, the file starts from an empty
  `TreeGrove::new(vec![])`.
- **A `trees` list, when given, replaces the base's trees.**
- **`tree`** is a `TreePreset` key, or an object of ez-tree settings with an
  optional `base` tree key.
- **`density` and `spacing`** default to that tree preset's grove values. A
  tree with no `base` must set both.
- **Unknown fields are errors.** Inside a tree object, unknown fields are
  ignored, as ez-tree does.
- **Writing:**
  - Every setting is written.
  - A tree equal to a preset is written as the preset's key.
  - Numbers are written as `f32` (`0.6`).

## Rebuild cost (as built)

| Edit | What happens |
| --- | --- |
| Move or rotate the grove entity | Nothing; the children follow. With Host ground, the grove is a child of its host, as in deadwood. |
| `density_scale` (grove or species), `clustering`, `spacing`, `variation`, size | Placement runs again **in a background task**, which takes milliseconds for hundreds of trees. When it lands, children are matched up by `id`: new trees spawn, dropped ones despawn, and moved or rescaled trees update their `Transform`. No mesh is regenerated. |
| `seed` | Positions and species picks are new. **Variant meshes stay the same** and come from `TreeCache`, because variant seeds don't depend on the grove's seed. |
| One species' `tree` options or `variants` | Only that species' children get new `TreeParams`. `TreeCache` builds those variants in the background, and other species keep their parts. |
| Add or remove a species | Shares change, so some sites switch species. Positions stay put, apart from spacing differences. |
| Host object changes, or the host's scale | The height grid is resampled and the grove is placed again. |
| The same grove set again (an equal value, or `set_changed`) | Nothing. |
| An edit every frame (a slider drag) | The running placement finishes and is applied, then the newest input starts. The grove keeps up, one placement behind. |

## Phase 2: Bevy component (as built)

All of this is in `tree/crates/procgena_tree_bevy/src/grove.rs` and is
exported from the crate root.

- **The component:**
  `TreeGroveParams { grove: GroveSource, size: Vec2, ground: GroveGround }`.
  Build one with `TreeGroveParams::new(grove, size)`,
  `TreeGroveParams::preset(preset, size)` and `.on_host(filter, cell)`.
- **`GroveSource`** is `Options(Arc<TreeGrove>)` or
  `Asset(Handle<TreeGroveAsset>)`. `TreeGroveAsset(pub TreeGrove)` is
  loaded from `.grove.json` by a loader registered in `TreePlugin`, which
  checks the settings with `validate()`.
- **`GroveGround`** is `Flat` or `Host { filter, cell }`, with the same
  rules as deadwood's `ScatterGround::Host`.
- **The child marker** is `GroveMember { tree, variant, id }`. Each tree
  child is `(TreeParams::Options(arc), Transform, GroveMember, ChildOf(grove))`,
  and one `Arc<TreeOptions>` is shared per `(tree, variant)` in each
  placement.
- **Build state:**
  - `TreeGroveGenerating` stays on the grove until placement is done and
    no member has `TreeGenerating`.
  - `TreeGroveFinished { entity, result }` is an `EntityEvent`.
  - `TreeGroveBuildError` is `Grove(GroveError)`, `NoGround`, `BadCell` or
    `NonUniformScale`.
- **Systems** (crate-private), in the `TreePlugin` `Update` chain:
  1. `remove_groves`, `finish_placing` and `start_groves` run before the
     tree systems.
  2. `finish_groves` runs after `prune_cache`.
  3. `start_groves` compares each grove's input (grove, size and ground) with
     its last placement every frame. Host objects are compared by `Arc`
     identity.
  4. `start_groves` doesn't replace a placement that is still running.
  5. `finish_placing` updates a kept child's `TreeParams` only when the
     options differ, so unchanged trees keep their parts.
- **Example:** `examples/grove.rs`.
  - Left and Right switch the preset, Up and Down change the density, and
    F switches between flat ground and hills.
  - The options are `--preset <key>`, `--hills`, `--textures` and
    `--screenshot <png>`.
- **Headless tests** (10, in `grove::tests`):
  - variants share keys;
  - lowering density keeps the survivors' entities and parts;
  - changing one species rebuilds only that species;
  - setting the same grove again does nothing;
  - a grove edited every frame keeps updating;
  - host ground waits for its object, follows the host's scale, and is
    removed when the object goes;
  - a new host object re-places the trees;
  - `NoGround` and `BadCell` are reported;
  - removing the component removes the trees;
  - an asset change updates the trees.
- **Not tested:** loading a real `.grove.json` file through the asset
  server.

## Phase 3: studio "Tree Grove" tab (`procgena`)

This follows the `field/` module. The new files are
`procgena/src/grove/{mod.rs, editor.rs, params.rs}`.

1. **Tab and scene.**
   - `StudioTab::TreeGrove` and `SceneMode::Grove`.
   - `spawn_subject` spawns `TreeGroveParams` on flat ground, sized by the
     environment's field size.
   - A high orbit camera by default.
2. **Protocol.** `GroveSettings { grove: TreeGrove }`, `Start::grove` and
   `ViewerCommand::SetGrove`.
3. **Editor.**
   - A `GrovePreset` picker.
   - One row per species:
     - a `TreePreset` picker, or Custom when the options match no preset;
     - ✎, which opens an inline tree `ParamPanel` reusing `tree/params.rs`;
     - Density × (exponential), Clustering and Variants;
     - a status line, `N/ha · share%`.

     The status line should show the **placed** count. Spacing removes
     about 15%, and that loss should not be hidden.
   - `+ Add Tree`.
   - Grove and Variation sections.
4. **Save and Load Grove…** as `.grove.json`.
5. **Status bar:** the tree count, the variants generating, and the
   triangle total.
6. **Tests:**
   - row statuses;
   - `read_grove`;
   - a headless click on "Tree Grove", then a density edit that lowers the
     child count;
   - a Save/Load round trip.

## Phase 4: docs

- A "Groves" section in `tree/README.md` and
  `tree/crates/procgena_tree_bevy/README.md`.
- Reference tables in the style of `plant/docs/fields.md`.

## Out of scope / follow-ups

- An instanced or impostor far-field renderer for thousands of trees.
- An image density mask and heightmap ground on the Bevy component.
- A per-tree colour tint, which needs per-instance material data.
- Undergrowth: a `PlantField` under a grove.
- Wiring to `procgena_terrain`, which can pass its height function as a
  `Ground` closure.

## Scoped checks per phase (worker rules)

- **Phase 1:**
  - `cargo test -p procgena_tree --test grove`
  - `cargo test -p procgena_tree --lib`
  - `cargo test -p procgena_tree --doc`
  - `cargo clippy -p procgena_tree --all-targets`

  `-D warnings` fails on lints that were already in `skeleton.rs` and
  `tests/skeleton.rs`.
- **Phase 2:**
  - `cargo test -p procgena_tree_bevy --lib grove`
  - `cargo clippy -p procgena_tree_bevy --all-targets -- -D warnings`
  - the `grove` example with `--screenshot`
- **Phase 3:**
  - `cargo test -p procgena --bin procgena grove`
  - `cargo clippy -p procgena --all-targets --no-deps`
