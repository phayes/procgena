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
    commands.spawn(TreeParams::preset(TreePreset::OakSmall));
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
        let mut options = TreePreset::OakSmall.options();
        options.seed = i;
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

## Load settings from JSON

Save full or partial ez-tree settings to `assets/trees/oak.tree.json`. Missing
fields use defaults. For example:

```json
{
  "seed": 42,
  "branch": { "length": { "0": 12.0 } }
}
```

Load the file through Bevy's asset server in a startup system:

```no_run
use bevy::prelude::*;
use procgena_tree_bevy::TreeParams;

fn load_tree(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        TreeParams::Asset(asset_server.load("trees/oak.tree.json")),
        Transform::from_xyz(20.0, 0.0, 0.0),
    ));
}
```

The filename must end in `.tree.json`. The tree waits for the asset to load,
then generates in the background. It rebuilds when Bevy reports that the asset
has changed; detecting live file edits requires Bevy asset watching to be enabled.

## Plant a grove

Spawn a `TreeGroveParams` to spread a mix of trees over the ground around an
entity. Start from a grove preset, or build a `procgena_tree::TreeGrove` of
your own:

```no_run
use bevy::prelude::*;
use procgena_tree::GrovePreset;
use procgena_tree_bevy::TreeGroveParams;

fn plant_grove(mut commands: Commands) {
    commands.spawn((
        TreeGroveParams::preset(GrovePreset::OakWoodland, Vec2::new(60.0, 40.0)),
        Transform::from_xyz(0.0, 0.0, -30.0),
    ));
}
```

The grove draws all its trees itself, without an entity per tree, so one
grove can hold thousands of trees (up to 20,000). Trees of the same species
and shape variant share meshes and materials, so even a large grove generates
only a handful of meshes. The grove entity gets:

- `TreeGroveInstances`: every tree's species, shape variant, spot id and
  transform;
- `TreeGroveAssets`: each variant's meshes, materials and tagged specimen;
- `TreeGroveStats`: the tree count, and vertices and triangles per detail
  level.

Change the component to change the grove. Only the trees that appear,
disappear or change are affected, so lowering the density leaves the
remaining trees exactly as they were. The grove has a `TreeGroveGenerating`
component until every tree is ready, and `TreeGroveFinished` is triggered on
it when the build ends, with an error if the grove could not be built.

### Promote single trees

To give one tree an entity of its own, for a collider or to interact with
it, spawn a `PromotedTree` with whatever the tree needs:

```rust,ignore
commands.spawn((PromotedTree::new(grove, tree.id), MyCollider));
```

The entity becomes an ordinary tree, drawn exactly like the grove's copy,
which stops drawing it. Despawn the entity to give the tree back.
`GroveTrees` finds trees near a point (`within`) or under a ray
(`ray_cast`). The `grove_promote` example promotes the trees near the camera,
or the tree you click:

```sh
cargo run -p procgena_tree_bevy --example grove_promote -- --mode distance
cargo run -p procgena_tree_bevy --example grove_promote -- --mode click
```

To stand the trees on terrain or another generated surface, spawn the grove as
a child of an entity with a `GeneratedSpecimen`, and say which parts of it count
as ground:

```no_run
use bevy::prelude::*;
use procgena_specimen::{TagFilter, tags};
use procgena_tree::GrovePreset;
use procgena_tree_bevy::TreeGroveParams;

fn plant_on(terrain: Entity, mut commands: Commands) {
    commands.spawn((
        TreeGroveParams::preset(GrovePreset::PineStand, Vec2::splat(50.0))
            .on_host(TagFilter::any_of([tags::GROUND]), 0.5),
        ChildOf(terrain),
    ));
}
```

The ground is sampled every 0.5 m here. Trees are left out where the host has
no surface or the slope is too steep.

Groves also load from files. Save a grove as `assets/groves/woods.grove.json`
(see [the grove guide](../../docs/groves.md) for the format), and load it with
`GroveSource::Asset`:

```no_run
use bevy::prelude::*;
use procgena_tree_bevy::{GroveGround, GroveSource, TreeGroveParams};

fn load_grove(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(TreeGroveParams {
        grove: GroveSource::Asset(asset_server.load("groves/woods.grove.json")),
        size: Vec2::splat(80.0),
        ground: GroveGround::Flat,
    });
}
```

The filename must end in `.grove.json`. A file with an invalid setting fails
to load. The grove updates when the file changes if Bevy is watching asset
files.

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

The plugin generates four mesh detail levels. Bevy fades to simpler meshes at
camera distances of 10, 25 and 50 metres. With a texture folder configured,
the nearest level uses 1k bark textures and the next two use 512 textures.
At the farthest level (LOD3), thin branches are omitted and the remaining branches
use flat strips. With textures configured, those strips use a single colour
approximating the bark texture, and fewer, larger leaf cards show clumps of leaves.
Fruit above the size threshold become flat hexagons without stalks. These flat
shapes, called billboards, turn toward the camera to help preserve the tree’s
outline as the view changes. For shadow rendering, they turn toward the light.
To see shadows at this distance, extend the directional light’s shadow cascades
far enough to cover the trees.
Set `TreePlugin::lod` when adding the plugin to choose mesh detail,
switching distances, bark appearance and leaf images. For a single full-detail
mesh with 2k bark textures, no distance switching and no impostor:

```rust
use procgena_core::{Lod, LodLevel};
use procgena_tree::{LOD_REFERENCE_SIZE, LodDetail, TextureSize};
use procgena_tree_bevy::{BarkTexture, LeafTexture, TreeLevel, TreePlugin};

let level = TreeLevel {
    detail: LodDetail::default(),
    bark: BarkTexture::Textured(TextureSize::Size2k),
    leaves: LeafTexture::Leaf,
};
let plugin = TreePlugin {
    lod: Lod::new(LOD_REFERENCE_SIZE, vec![LodLevel::new(0.0, 0.0, level)]),
    impostor: None,
    ..Default::default()
};
```

The 2k bark textures are downloaded only with `--sizes 512,1k,2k`.

No levels at all produce no bark or leaf meshes. Generated children have a
`TreePart` component identifying their detail level and whether they draw bark,
leaves, or a trellis wire.

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
cargo run -p procgena_tree_bevy --example grove -- --textures
```

Press Left/Right to switch grove presets, Up/Down to make the grove denser or
sparser, and F to switch between flat ground and hills. Leave out `--textures`
to use plain colours instead of the downloaded bark and leaf textures. See the
top of `examples/grove.rs` for all options.

## Build your own entities

For direct control, convert a `procgena_tree::TreeMesh` with `bark_mesh` and `leaf_mesh`,
create materials with `bark_material` and `leaf_material`, and attach the mesh
handles and `MeshMaterial3d<TreeMaterial>` to your entities. Keep `TreePlugin`
installed to register shaders and synchronize wind settings.

## Load exported glTF trees

Enable this crate's `gltf` feature to animate billboards in trees saved with
`procgena_tree::export::gltf`. Billboards are flat shapes used for distant tree parts;
this integration lets them turn toward the camera and move with the wind.
Export with `GltfOptions::wind` enabled (the default).

Register the tree's custom vertex attributes with Bevy's `GltfPlugin`, then add
`TreePlugin` after Bevy's default plugins:

```rust,ignore
use bevy::gltf::GltfPlugin;
use bevy::prelude::*;
use procgena_tree_bevy::{TreePlugin, add_tree_gltf_attributes};

App::new().add_plugins((
    DefaultPlugins.set(add_tree_gltf_attributes(GltfPlugin::default())),
    TreePlugin::default(),
));
```

Load and spawn the file's scene with Bevy's glTF loader. Eligible meshes under
nodes tagged as billboards use `TreeMaterial` for wind and camera-facing rotation.
Their crossed fallback shapes are removed once all meshes under the billboard
node are converted. Bounds are updated to keep the rotating shapes visible;
these bounds do not include wind movement. Other meshes keep their original
materials, and converted billboard materials do not retain normal maps.

Meshes keep their original shapes and materials if wind or billboard attributes
are missing or unregistered, or if they use animation joints. Their fallback
partners remain in the scene. Missing or unregistered attributes produce a
warning; check both the export's wind setting and the plugin setup above.

Try the example, which exports and loads a distant Apple tree by default:

```sh
cargo run -p procgena_tree_bevy --example gltf_tree --features gltf
```

To load your own file, append `-- path/to/tree.glb`. Hold the left mouse button
to look around, and use WASD and Q/E to fly.
