# procgena_tree_bevy

Display procedural trees in Bevy with background generation, textured materials,
wind animation, and distance-based mesh detail. Built on `procgena_tree`, this crate
targets Bevy 0.20.1.

## Display a tree

Add `TreePlugin` after Bevy's default plugins, then spawn a `TreeParams` component.
This complete example includes a camera and light and runs without texture files:

```no_run
use bevy::prelude::*;
use procgena_tree::TreePreset;
use procgena_tree_bevy::{TreeParams, TreePlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            TreePlugin {
                texture_root: None,
                ..default()
            },
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(TreeParams::preset(TreePreset::oak_small()));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(3.5, 2.5, 5.5)
            .looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 50.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
```

The tree appears when background generation finishes. Bark, leaves, and any
visible trellis support wires are spawned as child entities. Trees with identical
options share meshes and materials.

With `texture_root: None`, trees use material colours and leaves are solid
rectangles. Add textures to give leaves their outline and bark its surface detail.

## Add textures

Bark and leaf textures are not stored in the repository. Download them first:

```sh
uv run --project tools textures download tree
```

This fetches every leaf into `assets/textures/leaves/` and the 512 and 1k bark
sizes into `assets/textures/bark/`; add `--sizes 512,1k,2k` to include 2k bark.
Then copy the repository's `assets/textures`
directory to `assets/procgena_tree/textures` in your app and use
`TreePlugin::default()`. The default texture root is `procgena_tree/textures`,
relative to Bevy's asset directory. Set `texture_root` to another path if you
store textures elsewhere. Missing textures cause asset-loading errors.

Every texture comes in two formats. The default, BC7, requires GPU support for
that compression format. For other GPUs, set `texture_compression` to
`TextureCompression::Uastc` and enable Bevy's `basis-universal` feature.

Bark normal maps add lighting detail without extra geometry. Keep the default
`BarkUvMode::Continuous` to use them. With `BarkUvMode::Mirrored`, Bevy omits
the bark normal map to avoid reversed lighting detail on every other branch
section.

Leaf normal maps are used
automatically for leaf kinds that have one, such as `white_oak`; see
`procgena_tree::leaf_has_normal_map`.

## Customize and position trees

Edit a preset's options and wrap them with `TreeParams::new`. Add a `Transform` to
position each tree. Register this function as a startup system in your app:

```no_run
use bevy::prelude::*;
use procgena_tree::TreePreset;
use procgena_tree_bevy::TreeParams;

fn plant_trees(mut commands: Commands) {
    for i in 0..5 {
        let mut options = TreePreset::oak_small().tree.clone();
        options.default_seed = i;
        options.branch.length[0] = 12.0; // Trunk length.

        commands.spawn((
            TreeParams::new(options),
            Transform::from_xyz(i as f32 * 20.0, 0.0, 0.0),
        ));
    }
}
```

The same options and seed produce the same shape. Changing a tree's `TreeParams`
component rebuilds its generated parts; removing the component removes those
children. Moving the parent entity moves the whole tree.

## Load a preset file

Save a tree preset to `assets/trees/oak.tree.yml`. It names a built-in preset
as its `base` and gives only the settings that differ. For example:

```yaml
base: oak_small
tree:
  default_seed: 42
  branch:
    length: { "0": 12.0 }
```

Load the file through Bevy's asset server in a startup system:

```no_run
use bevy::prelude::*;
use procgena_tree_bevy::TreeParams;

fn load_tree(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        TreeParams::Asset(asset_server.load("trees/oak.tree.yml")),
        Transform::from_xyz(20.0, 0.0, 0.0),
    ));
}
```

The filename must end in `.tree.yml`; the name before that is the preset's
key. A file with an invalid setting fails to load. The tree waits for the asset to load,
then generates in the background. It rebuilds when Bevy reports that the asset
has changed; detecting live file edits requires Bevy asset watching to be enabled.

## Plant a grove

Spawn a `TreeScatterParams` to spread a mix of trees over the ground around an
entity. Start from a `procgena_tree::TreeScatterPreset`, or build a
`procgena_tree::TreeScatter` of your own:

```no_run
use bevy::prelude::*;
use procgena_tree::TreeScatterPreset;
use procgena_tree_bevy::TreeScatterParams;

fn plant_grove(mut commands: Commands) {
    commands.spawn((
        TreeScatterParams::preset(TreeScatterPreset::oak_woodland(), Vec2::new(60.0, 40.0)),
        Transform::from_xyz(0.0, 0.0, -30.0),
    ));
}
```

The grove draws all its trees itself, without an entity per tree, so one
grove can hold thousands of trees (up to 20,000). Trees of the same species
and shape variant share meshes and materials, so even a large grove generates
only a handful of meshes. The grove entity gets:

- `TreeScatterInstances`: every tree's species, shape variant, spot id and
  transform;
- `TreeScatterAssets`: each variant's meshes, materials and tagged specimen;
- `TreeScatterStats`: the tree count, and vertices and triangles for the
  game-level and lower meshes.

Change the component to change the grove. Only the trees that appear,
disappear or change are affected, so lowering the density leaves the
remaining trees exactly as they were. The grove has a `TreeScatterGenerating`
component until every tree is ready, and `TreeScatterFinished` is triggered on
it when the build ends, with an error if the grove could not be built.

### Promote single trees

To give one tree an entity of its own, for a collider or to interact with
it, spawn a `PromotedTree` with whatever the tree needs:

```rust,ignore
commands.spawn((PromotedTree::new(grove, tree.address), MyCollider));
```

The entity becomes an ordinary tree, drawn exactly like the grove's copy,
which stops drawing it. Despawn the entity to give the tree back.
`TreeScatterTrees` finds trees near a point (`within`) or under a ray
(`ray_cast`). The `scatter_promote` example promotes the trees near the camera,
or the tree you click:

```sh
cargo run -p procgena_tree_bevy --example scatter_promote -- --mode distance
cargo run -p procgena_tree_bevy --example scatter_promote -- --mode click
```

To stand the trees on terrain or another generated surface, spawn the grove as
a child of an entity with a `GeneratedSpecimen`, and say which parts of it count
as ground:

```no_run
use bevy::prelude::*;
use procgena_specimen::{TagFilter, tags};
use procgena_tree::TreeScatterPreset;
use procgena_tree_bevy::TreeScatterParams;

fn plant_on(terrain: Entity, mut commands: Commands) {
    commands.spawn((
        TreeScatterParams::preset(TreeScatterPreset::pine_stand(), Vec2::splat(50.0))
            .on_host(TagFilter::any_of([tags::GROUND]), 0.5),
        ChildOf(terrain),
    ));
}
```

The ground is sampled every 0.5 m here. Trees are left out where the host has
no surface or the slope is too steep.

Groves also load from files. Save a tree Scatter preset as
`assets/scatters/woods.tree_scatter.yml` (see
[the tree Scatter guide](../../docs/scatters.md) for the format), and load it
with `TreeScatterSource::Asset`:

```no_run
use bevy::prelude::*;
use procgena_tree_bevy::{TreeScatterGround, TreeScatterParams, TreeScatterSource};

fn load_grove(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(TreeScatterParams {
        tree_scatter: TreeScatterSource::Asset(
            asset_server.load("scatters/woods.tree_scatter.yml"),
        ),
        size: Vec2::splat(80.0),
        ground: TreeScatterGround::Flat,
    });
}
```

The filename must end in `.tree_scatter.yml`. A file with an invalid setting
fails to load. The grove updates when the file changes if Bevy is watching
asset files.

## Control wind

By default, trunks bend, branches sway, and leaves flutter. Change the global
`Wind` resource to update all tree materials. For example, register this startup
system to keep trunks and branches still while leaves flutter:

```no_run
use bevy::prelude::*;
use procgena_tree::WindParams;
use procgena_tree_bevy::Wind;

fn set_wind(mut wind: ResMut<Wind>) {
    wind.0 = WindParams::LEAVES_ONLY;
}
```

Wind animation runs on the GPU and is included in shadows and motion vectors.
Each tree's position varies the timing so copies do not all move together.
The plugin synchronizes every `TreeMaterial` with the global resource, including
materials you create yourself.

## Choose mesh detail

Each tree's `TreeOptions::game_level` sets its real shape: the branch,
leaf and fruit detail drawn while the tree is large on screen. A coarser
lower mesh is derived from it, with fewer bark sections and leaf cards
and open branch ends, and an impostor is baked from it. Which one is
drawn depends on how large the tree looks on screen, the same for every
kind of object (see `procgena_core::chain`); Bevy fades between them, and
stops drawing the tree once it is a few pixels tall. Fruit and flowers
switch by their own size until their tree's impostor takes over.

For a lighter tree that keeps every second leaf card, and every second
bark section, even when it is close:

```rust
use procgena_tree::TreePreset;
use procgena_tree_bevy::TreeParams;

let mut options = TreePreset::oak_small().tree.clone();
options.game_level.leaf_stride = 2;
options.game_level.section_stride = 2;
let tree = TreeParams::new(options);
```

With a texture folder configured, the game-level mesh uses 1k bark
textures and the lower mesh 512 ones. Generated children have a
`TreePart` component naming the step they draw and whether they draw
bark, leaves, fruit and flowers, the impostor, or a trellis wire.

## Try the viewer

From the repository, run:

```sh
cargo run -p procgena_tree_bevy --example viewer
```

The viewer uses the repository's textures; run `uv run --project tools textures download tree`
first to fetch the bark textures. Press Left/Right to switch presets,
F to toggle a forest, Space to toggle trunk and branch movement, and N to toggle
bark normal maps. Hold the left mouse button to look around and use WASD and Q/E
to fly. Press J to toggle shape variation and R to generate new variations.

Use `--headless` with `--screenshot` to save an image without opening a window.
This is useful when running the viewer from a script or on a remote machine:

```sh
cargo run -p procgena_tree_bevy --example viewer -- --headless --screenshot tree.png \
    --preset oak_medium --eye 0 3 11 --target 0 2.8 0 --resolution 1280 720
```

This saves `tree.png` at 1280 × 720 pixels and closes the viewer. Change
`--resolution <width> <height>` to choose a different image size. The viewer waits
until the tree and its textures (the images that give bark and leaves their
appearance) are ready before taking the screenshot. With `--headless`, trees and
leaves stay still so wind does not change their appearance between screenshots.
If loading or saving fails, the viewer exits with an error. It also exits with an
error if the scene is not ready within two minutes after the trees finish
generating; tree generation itself has no time limit.
See the top of `examples/viewer.rs` for all available options.

To try groves, run:

```sh
cargo run -p procgena_tree_bevy --example scatter -- --textures
```

Press Left/Right to switch tree Scatter presets, Up/Down to make the grove
denser or sparser, and F to switch between flat ground and hills. Leave out
`--textures` to use plain colours instead of the downloaded bark and leaf
textures. See the top of `examples/scatter.rs` for all options.

## Build your own entities

For direct control, convert a `procgena_tree::TreeMesh` with `bark_mesh` and `leaf_mesh`,
create materials with `bark_material` and `leaf_material`, and attach the mesh
handles and `MeshMaterial3d<TreeMaterial>` to your entities. Keep `TreePlugin`
installed to register shaders and synchronize wind settings.
