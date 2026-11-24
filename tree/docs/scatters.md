# Groves

A grove is a mix of tree species spread over a stretch of ground: the tree
counterpart of a plant field. Two types describe it:

- A **`GroveTree`** is one species. It holds the tree's shape (`TreeOptions`),
  how many trees of it grow per hectare, the distance its trunks keep from
  their neighbours, how much it gathers into stands, and how many shape
  variants it is drawn from.
- A **`TreeGrove`** is the whole mix: its species, a seed, a scale on every
  species' density, and how individual trees vary in size, lean and footing.

A grove says nothing about where it grows. `GrovePlacement::new` in
`procgena_tree` lays it out over a `ScatterInput`: the ground, an area (a World
centre and a size), a seed and an optional mask that thins it. Positions come
back in metres from the area's centre. The `TreeGroveParams` component in
`procgena_tree_bevy` does the same for a Bevy entity, with the entity at the
centre and the grove's own seed, and draws the trees.

## Start from a preset

```rust
use glam::{DVec2, Vec2};
use procgena_tree::{FlatGround, GrovePlacement, GrovePreset, ScatterArea, ScatterInput};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grove = GrovePreset::PineStand.grove();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::new(60.0, 40.0));
    let input = ScatterInput::new(&FlatGround, area, grove.seed);
    let placement = GrovePlacement::new(&grove, &input)?;
    println!("{} trees", placement.instances().len());
    Ok(())
}
```

Each `GroveInstance` names its species (`tree`, an index into
`TreeGrove::trees`), its shape variant, and a `transform` that sizes, leans,
turns and stands the tree on the ground. Build each species' variant once
with `GroveTree::variant` and draw it at every instance that uses it.

In Bevy, spawn the component instead:

```rust
use bevy::prelude::*;
use procgena_tree::GrovePreset;
use procgena_tree_bevy::TreeGroveParams;

fn plant_grove(mut commands: Commands) {
    commands.spawn((
        TreeGroveParams::preset(GrovePreset::PineStand, Vec2::new(60.0, 40.0)),
        Transform::from_xyz(0.0, 0.0, -30.0),
    ));
}
```

The grove draws its trees itself, without an entity per tree, so one grove
can hold thousands of trees. Each tree switches detail level by its distance
from the camera and sways in the wind, exactly like a single tree. The grove
entity gets a `TreeGroveInstances` listing every tree (its species, shape
variant, spot id and transform), a `TreeGroveAssets` with each variant's
meshes and materials, and a `TreeGroveStats` with the tree count and sizes.

## Give single trees an entity

A game often needs a few trees as entities: those near the player get
colliders, or the one under the mouse gets selected. Spawn a `PromotedTree`
naming the grove and the tree's spot id, with whatever the tree needs:

```rust
use bevy::prelude::*;
use procgena_tree_bevy::{GroveTrees, PromotedTree};

#[derive(Component)]
struct Collider;

/// Gives every tree within 10 m of the camera a collider.
fn promote_nearby(
    mut commands: Commands,
    trees: GroveTrees,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    promoted: Query<&PromotedTree>,
) {
    for (grove, tree) in trees.within(camera.translation(), 10.0) {
        let promotion = PromotedTree::new(grove, tree.id);
        if !promoted.iter().any(|p| *p == promotion) {
            commands.spawn((promotion, Collider));
        }
    }
}
```

- The entity becomes an ordinary tree with a `TreeParams`, a child of the
  grove standing where the grove drew the tree. It is drawn with the same
  meshes, materials, detail levels and wind, and the grove stops drawing its
  own copy, so nothing visibly changes.
- Despawn the entity to give the tree back. Removing only `PromotedTree`
  removes the tree but keeps the entity and your components.
- The entity follows the grove: if an edit leaves no tree on its spot, it is
  despawned; if the spot's tree changes, so does the entity's.
- `GroveTrees::within` finds trees whose base is near a point, and
  `GroveTrees::ray_cast` finds the tree a ray hits first, tested against the
  triangles of its nearest detail level. Use it with
  `Camera::viewport_to_world` to pick trees with the mouse.
- `GroveVariant::specimen` gives a variant's tree as a tagged `Specimen`, to
  build colliders from.

The `grove_promote` example shows both: `--mode distance` promotes the trees
near the camera, and `--mode click` the tree you click.

## Far trees as impostors

Beyond 120 m, trees are drawn as impostors: one flat quad per tree showing a
picture of the tree baked from the nearest of 64 directions, lit by the scene.
This applies to groves and to single trees alike, and makes large groves
several times cheaper to draw. Set the distance, or turn impostors off, with
`TreePlugin::impostor`:

```rust
use bevy::prelude::*;
use procgena_tree_bevy::{TreeImpostor, TreePlugin};

App::new().add_plugins(TreePlugin {
    impostor: Some(TreeImpostor { distance: 200.0, ..default() }),
    ..default()
});
```

Each distinct tree is baked once, the first time it is generated, which takes
a few frames; until then its farthest mesh level is drawn at every distance.
Impostors don't sway in the wind.

Baked impostors are kept in memory. To keep them between runs, add
`procgena_impostor::ImpostorPlugin` yourself, before `TreePlugin`, with a
`cache_dir`. To ship them with a game, fill a folder with
`cargo run -p procgena_tree_bevy --example bake_impostors -- --out <dir> --compress <shipped>`
and point `shipped_dir` at `<shipped>`. `--compress` writes the impostors
compressed (BC7 in KTX2 files, with mip levels), which take a quarter of the
GPU memory and look smoother far away; every desktop GPU can draw them.

While impostors bake, the app has extra 3D cameras, marked
`ImpostorBakeCamera`. Leave them out of queries for your own camera:
`Single<&Transform, (With<Camera3d>, Without<ImpostorBakeCamera>)>`.

## Prepare a grove behind a loading screen

`PrepareGrove` generates every tree a grove can use, and bakes their
impostors, without placing any. `GrovePreparing` on its entity shows the
progress, and `GroveReady` is triggered on it when everything is ready. A
`TreeGroveParams` with the same grove then appears within a few frames.

```rust
use bevy::prelude::*;
use procgena_tree::GrovePreset;
use procgena_tree_bevy::{GroveReady, PrepareGrove, TreeGroveFinished, TreeGroveParams};

fn start_loading(mut commands: Commands) {
    commands
        .spawn(PrepareGrove::preset(GrovePreset::PineStand))
        .observe(|ready: On<GroveReady>, mut commands: Commands| {
            let preparer = ready.entity;
            commands
                .spawn(TreeGroveParams::preset(GrovePreset::PineStand, Vec2::splat(200.0)))
                // The grove holds its own trees once it is built.
                .observe(move |_: On<TreeGroveFinished>, mut commands: Commands| {
                    commands.entity(preparer).despawn();
                });
        });
}
```

Keep the `PrepareGrove` entity until the grove has finished building, or its
trees may be freed and generated again. `grove_variants` lists the trees of a
grove, for loading code of your own. See
`cargo run -p procgena_tree_bevy --example grove_loading`.

## Mix species

The species of a grove share it like this:

- **Densities add up.** The grove grows the sum of its species' densities,
  and each species' share is its density over that sum.
- **`density_scale` on the grove** multiplies every species' density at once
  and keeps the mix. `TreeGrove::with_density` sets it from a total in trees
  per hectare.
- **`density_scale` on a species** changes only that species' density, and so
  its share.
- **The last species fills the gaps.** Species are chosen in list order, and
  the last one with a density above 0 takes whatever the others leave. Its
  `clustering` has no effect, so put the species that should spread evenly
  last.
- **Spacing thins the grove.** A tree closer to a neighbour than the larger
  of the two species' `spacing` is left out. The preset spacings leave about
  80 to 95% of the trees a density asks for.

```rust
use procgena_tree::{GrovePreset, GroveTree, TreeGrove, TreePreset};

// Oaks with clumps of bushes and a few large ashes in stands.
let grove = TreeGrove::new(vec![
    GroveTree::new(TreePreset::Bush2).scaled(0.1).clustered(0.7),
    GroveTree::new(TreePreset::AshLarge).scaled(0.2).clustered(0.4),
    GroveTree::new(TreePreset::OakMedium).scaled(0.6),
])
.with_density(250.0);

// Or start from a preset and change one species.
let mut pines = GrovePreset::PineStand.grove();
pines.trees[0].clustering = 0.9;
```

## Change a grove without moving its trees

The same grove, size and ground always give the same trees. Edits keep as
many of them as they can:

- Lowering a density only removes trees, and raising it only adds them. The
  others stay exactly where they were, with the same species and shape.
- Raising `patchiness` only removes trees, and lowering it only brings them
  back.
- Changing the grove's `seed` moves every tree, but never changes a shape
  variant's mesh.
- Changing `variation` changes only each tree's transform.
- Each `GroveInstance` has an `id` naming the spot it stands on, so a renderer
  can match trees between two placements and update only those that changed.
  `TreeGroveParams` does this for you.

## Stand trees on uneven ground

The input's ground can be any `Ground`: `FlatGround`, a closure from a World
position (`DVec2`, X and Z) to a height, or a `HeightGrid` sampled from another
generator's specimen with `Specimen::height_grid`. A height of NaN means there
is no ground there.

A tree is left out where the ground at its trunk is steeper than its species'
`max_slope` (the grove's `variation.max_slope` when the species sets none), so
shrubs can grow on slopes large trees can't. The ground is then sampled at the
trunk and at eight points around it, one trunk radius away, and the tree is
left out where any of that is missing. Each tree stands upright at the lowest
of those samples, sunk by `variation.sink` so its roots never show on a slope.
This is the shared `place()` with the tree's `ground_placement()`: upright,
resting on the lowest ground.

The last argument thins the grove in places: a function from X and Z to a
value from 0 (no trees) to 1 (the full density).

In Bevy, spawn the grove as a child of an entity with a `GeneratedSpecimen` and
call `TreeGroveParams::on_host` with the tags of the parts to stand on.

## Load and save `.grove.json` files

`TreeGrove::to_json` writes a grove and `TreeGrove::from_json` reads it back.
Every setting in a file is optional, and settings the format does not know
are errors. Names are camelCase.

```json
{
  "base": "oak_woodland",
  "seed": 12,
  "densityScale": 0.8,
  "variation": { "lean": 6 },
  "trees": [
    { "tree": "bush_2", "densityScale": 0.1, "clustering": 0.7, "variants": 2 },
    { "tree": { "base": "oak_medium", "seed": 9 }, "densityScale": 0.6 },
    { "tree": { "seed": 3, "branch": { "levels": 2 } }, "density": 300, "spacing": 1.5 }
  ]
}
```

- `base` names a grove preset that missing settings come from. Without one,
  missing settings take the defaults in the tables below, and the grove has
  no species.
- A `trees` list replaces the base's species.
- A species' `tree` is either a tree preset key, or an object of tree
  settings with ez-tree field names. The object may name a tree preset as
  its own `base`.
- A species' `density` and `spacing` default to its tree preset's. A tree
  written out without a `base` has no preset to take them from, so it must
  give both.

In Bevy, load a file with `GroveSource::Asset(asset_server.load("woods.grove.json"))`.
The filename must end in `.grove.json`.

## Reference

### `TreeGrove`

| Setting | File name | Default | Meaning |
| --- | --- | --- | --- |
| `seed` | `seed` | 0 | Picks where trees stand, which species each is, and how it is turned and sized. Changing it moves every tree but keeps the shapes |
| `trees` | `trees` | none | The species, at most 16 (`MAX_GROVE_TREES`) |
| `density_scale` | `densityScale` | 1 | Multiplies every species' density, keeping the mix. 0 or more |
| `patchiness` | `patchiness` | 0 | How much ground is left open as treeless glades, 0 to 1. Around 0.5 the woods hold separate glades; from about 0.7 the glades join up and the trees stand in islands. Glades are about 12 m across, larger where they join, and take trees away, so the grove grows fewer than its density asks for |
| `variation` | `variation` | below | How individual trees differ |

### `GroveTree`

| Setting | File name | Default | Meaning |
| --- | --- | --- | --- |
| `tree` | `tree` | required | The tree's shape. Its seed is the seed of variant 0 |
| `density` | `density` | the tree preset's | Trees per hectare that suit the species on its own. 0 or more |
| `spacing` | `spacing` | the tree preset's | Smallest distance from this species' trunks to any neighbour's, in metres. Between two species the larger applies. 0 or more |
| `density_scale` | `densityScale` | 1 | Multiplies `density` in this grove. 0 removes the species |
| `clustering` | `clustering` | 0 | 0 mixes the species evenly among the others; 1 gathers it into stands several spacings across. Changes where it grows, not how much |
| `variants` | `variants` | 4 | Shape variants the species is drawn from, 1 to 8 (`MAX_GROVE_VARIANTS`). Each costs a mesh |
| `max_slope` | `maxSlope` | the tree preset's | Steepest ground this species grows on, in degrees, 0 to 90. `None` (`null` in a file) uses the grove's `variation.max_slope`. Shrub and sapling presets (Bush 1–3, Aspen Small, Pine Small, Spring Pussy Willow) allow 45°; other presets use the grove's |

### `GroveVariation`

| Setting | File name | Default | Meaning |
| --- | --- | --- | --- |
| `scale_jitter` | `scaleJitter` | 0.15 | How much tree sizes vary, 0 to 0.9. Most trees are scaled by `1 ± 0.5 × scale_jitter`, and a few grow into giants up to `1 + 3 × scale_jitter` times their normal size. At 0.9, about one tree in thirteen is more than twice its normal size |
| `lean` | `lean` | 3 | Largest lean of a trunk from vertical, in degrees, in a random direction. 0 to 45 |
| `max_slope` | `maxSlope` | 35 | Steepest ground a tree grows on, in degrees. 0 to 90; 90 allows any slope |
| `sink` | `sink` | 0.1 | How far each tree is sunk below the lowest ground around its trunk, in metres. 0 or more |

### Limits

| Limit | Value | What happens past it |
| --- | --- | --- |
| `MAX_GROVE_TREES` | 16 species | `GroveError::TooManyTrees` |
| `MAX_GROVE_VARIANTS` | 8 variants per species | `GroveError::Options` |
| `MAX_GROVE_INSTANCES` | 20,000 trees, before spacing | `GroveError::TooMany` |
| `MAX_GROVE_SIZE` | 2048 m on a side | `GroveError::Options` |

A grove is never denser than 4 trees per m², because each 0.5 m square of
ground holds at most one tree.

### Grove presets

| Preset | Key | Species, from first to the one that fills the gaps |
| --- | --- | --- |
| Oak Woodland | `oak_woodland` | Bush 1 in clumps, Ash Medium in loose stands, Oak Large, Oak Medium |
| Aspen Stand | `aspen_stand` | Aspen Small in clumps, Aspen Large, Aspen Medium |
| Mixed Deciduous | `mixed_deciduous` | Big Leaf Maple Large and Ash Large in loose stands, Aspen Medium in stands, Oak Medium |
| Pine Stand | `pine_stand` | Pine Small in clumps, Pine Large, Pine Medium |
| Cedar Grove | `cedar_grove` | Bush 3 in clumps, Pine Medium in loose stands, Cedar |
| Orchard | `orchard` | Apple, 150 per hectare and 3 m apart, standing straight and of similar size |
| Spring Blossom | `spring_blossom` | Spring Pussy Willow in clumps, Flowering Dogwood |
| Winter Copse | `winter_copse` | Spring Pussy Willow in clumps, Winter Thicket |

`GrovePreset::ALL` lists them, and each preset's file is in
`crates/procgena_tree/groves/`.

### Tree preset densities and spacings

These are the `density` and `spacing` a species takes from its tree preset.
The density is about one tree per crown area, so a species alone at
`density_scale` 1 makes a closed canopy.

| Tree preset | Key | Trees per hectare | Spacing (m) |
| --- | --- | --- | --- |
| Ash Small | `ash_small` | 510 | 1.4 |
| Ash Medium | `ash_medium` | 260 | 1.9 |
| Ash Large | `ash_large` | 160 | 2.5 |
| Aspen Small | `aspen_small` | 2600 | 0.6 |
| Aspen Medium | `aspen_medium` | 980 | 1 |
| Aspen Large | `aspen_large` | 550 | 1.3 |
| Bush 1 | `bush_1` | 880 | 1.05 |
| Bush 2 | `bush_2` | 1100 | 0.95 |
| Bush 3 | `bush_3` | 800 | 1.1 |
| Oak Small | `oak_small` | 720 | 1.15 |
| Oak Medium | `oak_medium` | 400 | 1.55 |
| Oak Large | `oak_large` | 130 | 2.7 |
| Pine Small | `pine_small` | 2600 | 0.6 |
| Pine Medium | `pine_medium` | 1100 | 0.95 |
| Pine Large | `pine_large` | 550 | 1.3 |
| Trellis | `trellis` | 0 | 0.8 |
| Big Leaf Maple Medium | `big_leaf_maple_medium` | 310 | 1.75 |
| Big Leaf Maple Large | `big_leaf_maple_large` | 140 | 2.6 |
| Apple | `apple` | 650 | 1.2 |
| Flowering Dogwood | `flowering_dogwood` | 470 | 1.45 |
| Spring Pussy Willow | `spring_pussy_willow` | 2600 | 0.6 |
| Winter Thicket | `winter_thicket` | 1250 | 0.9 |
| Cedar | `cedar` | 480 | 1.4 |

The trellis has a density of 0, so it grows in a grove only when given a
density of its own.
