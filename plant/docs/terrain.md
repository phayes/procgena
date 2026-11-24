# Fields, terrain and visibility

A `PlantScatter` is a rectangle of grass centred on its `Transform`, lying in
the XZ plane. Its size is `size` times the transform's X and Z scale, so a
scaled field is bigger, like a scaled `Plane3d`. It turns with the
transform's rotation about Y, and its mask and heightmap turn with it. Grass
grows upright, so a tilt is left out, with one warning: for a slope, use a
heightmap or a mesh.

## Grow grass on a heightmap, with a density mask

```rust
fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    // Masks and heightmaps are data, not colour: load them as linear.
    let linear = |path: &'static str| {
        assets
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = false)
            .load(path)
    };
    commands.spawn(PlantScatter {
        ground: PlantGround::Heightmap {
            image: linear("terrain/height.png"),
            height_scale: 40.0,
        },
        density_map: Some(linear("terrain/plant_mask.png")),
        ..PlantScatter::from_preset(&PlantScatterPreset::SUMMER_TALL, Vec2::splat(512.0))
    });
}
```

Both images cover the whole field, and their first row is the field's −Z edge.
In the heightmap, red 0–1 spans `height_scale` metres above the field's
height. In the mask, red is density (0 bare, 1 full) and green multiplies the
blade height. The images are read on the CPU when the field changes, so keep
their CPU copy (Bevy's default for loaded images).
`ImageLoaderSettings` is `bevy::image::ImageLoaderSettings`.

## Grow grass on terrain built from code

Terrain made by a height function, cut into many chunk meshes, or kept only
on the GPU needs neither a heightmap file nor a mesh: give the field the
same function. A density function keeps grass out of the sea, off roads and
off cliffs, with no mask to paint:

```rust
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(
        PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(240.0))
            // Metres above the field's own height; NaN is no ground at all.
            .with_ground_from_fn(&mut images, 0.5, |point| island.height(point))
            // 0 bare to 1 full: nothing below the tide line or on cliffs.
            .with_density_from_fn(&mut images, 0.5, |point| {
                let dry = (island.height(point) - SEA_LEVEL).clamp(0.0, 1.0);
                if island.slope(point) > 0.85 { 0.0 } else { dry }
            }),
    );
}
```

Each function is called once at the centre of every cell of `cell_size`
metres (at most 4096 cells per side), when the field is made, and the
results become a heightmap and a mask in `Assets<Image>`: from then on the
field is an ordinary heightmap field, with no cost per frame and no copy of
your terrain kept. The point is in metres from the field's centre along its
X and Z, so for a field at the origin, unturned and unscaled, it is world X
and Z. Set the field's `size` before calling them. A height that is not
finite leaves no ground, and nothing grows within a cell of it, as past the
edge of a mesh. The mask can be edited afterwards like any other.

[`examples/code_terrain.rs`](../examples/code_terrain.rs) is an island in 16
chunk meshes with one field over all of it, bare on the beach and the cliff.

## Grow grass on a mesh

For terrain that is a mesh, put the field on the terrain's own entity and
give it `PlantGround::mesh`. The field then lies where the mesh is drawn,
turn and scale included:

```rust
fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let terrain: Handle<Mesh> =
        assets.load(GltfAssetLabel::Primitive { mesh: 0, primitive: 0 }.from_asset("terrain.glb"));
    commands.spawn((
        Mesh3d(terrain.clone()),
        MeshMaterial3d(materials.add(Color::srgb(0.2, 0.15, 0.09))),
        PlantScatter {
            ground: PlantGround::mesh(terrain),
            // The mesh's extent in X and Z.
            ..PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(256.0))
        },
        Transform::from_rotation(Quat::from_rotation_y(0.5)),
    ));
}
```

The mesh is baked once, on the CPU, into a heightmap of one height per half
metre (`PlantGround::Mesh { mesh, cell_size }` picks another cell size, at
most 4096 cells per side), and from then on it is drawn exactly like a
heightmap: there is no cost per frame. It is baked again when the mesh asset,
the field's size or the cell size changes, not when the field moves. The
field waits for the mesh to load, so it never appears at the wrong height.

- **Where blades grow.** On the highest surface over each point, so the
  underside of an overhang or a bridge gets none. Nowhere the mesh does not
  reach: a field larger than its mesh stops at the mesh's edge, within half
  a cell.
- **Scale.** Like the mesh, the ground follows the transform's scale,
  including Y. Blade heights do not.
- **What can be baked.** Triangle lists and strips with `Float32x3`
  positions, kept on the CPU (`RenderAssetUsages::MAIN_WORLD`, which loaded
  and built meshes have by default). A mesh that cannot be baked draws no
  grass and logs one warning. Games often drop the CPU copy of meshes they
  build to save memory (`RenderAssetUsages::RENDER_WORLD`); for those, grow
  the grass from the function that built them instead
  ([above](#grow-grass-on-terrain-built-from-code)).
- **Several meshes.** One field per mesh; a terrain cut into tiles gets a
  field on each tile, or, when the tiles come from code, one field over all
  of them from the same function.

[`examples/mesh_terrain.rs`](../examples/mesh_terrain.rs) builds rolling
hills and grows a meadow on them.


## Keep the grass colour past the last blade

Beyond the grass radius no blades are drawn. `FarPlantMaterial` is a
`StandardMaterial` for your terrain that fades to the grass colour with
distance, with the same gusts rolling across it, so there is no seam:

```rust
fn ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarPlantMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(1000.0, 1000.0))),
        MeshMaterial3d(materials.add(FarPlantMaterial {
            base: StandardMaterial::from_color(Color::srgb(0.20, 0.15, 0.09)),
            extension: FarPlants::new(&PlantScatterPreset::MEADOW.plant_scatter),
        })),
    ));
}
```

A game with a terrain shader of its own calls the same WESL function,
`procgena_plant::shaders::far::plant_far_color`, from it. Its material holds a
`FarPlantUniform` and the gust texture, and a system rewrites the uniform
each frame, since the gusts scroll:

```rust
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
struct Terrain {
    #[uniform(0)]
    far: FarPlantUniform,          // `var<uniform> far: FarPlants;` in the shader
    #[texture(1)]
    #[sampler(2)]
    gusts: Handle<Image>,          // FarPlants::GUSTS
    // ...the terrain's own bindings
}

fn follow_the_plants(
    terrain_plants: Res<TerrainPlants>, // your FarPlants, e.g. FarPlants::new(&field)
    wind: Res<PlantWindFrame>,
    settings: Res<PlantSettings>,
    mut terrains: ResMut<Assets<Terrain>>,
) {
    let far = terrain_plants.0.uniform(&wind, &settings);
    for (_, terrain) in terrains.iter_mut() {
        terrain.far = far;
    }
}
// app.add_systems(Update, follow_the_plants.after(PlantWindSystems::Sample));
```

and in the shader:

```wgsl
import procgena_plant::shaders::far::{FarPlants, plant_far_color};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> far: FarPlants;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var gust_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var gust_sampler: sampler;
// ...
let color = plant_far_color(world_position, view.world_position, soil, far, gust_texture, gust_sampler);
```

[`examples/custom_terrain.rs`](../examples/custom_terrain.rs) is the whole
thing, with its shader beside it.

| Item | What it does |
| --- | --- |
| `FarPlantMaterial` | `ExtendedMaterial<StandardMaterial, FarPlants>` for terrain, forward and deferred |
| `FarPlantPlugin` | Registers the material. Part of `PlantPlugins` |
| `FarPlants` | `color`, `amount` (0–1), `fade: Option<Vec2>` (where the tint starts and is full; `None` follows `PlantSettings::radius`), `sheen` (gusts brighten it, 0–1, where the ground is seen at a glancing angle; little from above), `area: Option<Rect>`. Its wind follows `PlantWindFrame`, the same frame the blades read |
| `FarPlants::new(&field)` | Colour from a field's plants, each by its share; the fade follows the grass radius, also when it changes |
| `.with_fade(Vec2)` | A fade of your own, whatever the radius |
| `.with_area(rect)` | Only inside this world-XZ rectangle |
| `.with_sheen(f32)` | Another sheen; 0 for far grass without gusts |
| `FarPlantUniform` | The material's GPU data, for a terrain material of your own |
| `FarPlants::uniform(&wind, &settings)` | That data with this frame's wind and grass radius |
| `FarPlants::GUSTS` | The gust texture `plant_far_color` reads |
| `plant_far_color` (WESL) | `procgena_plant::shaders::far::plant_far_color`, for your own terrain shader |

## Hide a field, or leave a camera out

Fields and cameras follow Bevy's usual rules for what is drawn where:

```rust
// Hide a field, or a whole level it is a child of, and show it again. Nothing
// is rebuilt either way.
commands.entity(field).insert(Visibility::Hidden);

// Split-screen, a minimap, a mirror: a camera draws the fields it shares a
// render layer with. A field or camera without `RenderLayers` is on layer 0.
commands.spawn((Camera3d::default(), RenderLayers::layer(1))); // a minimap
commands.spawn((PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(64.0)),
                RenderLayers::from_layers(&[0, 1])));          // seen by both

// Or keep a camera from drawing any grass at all.
commands.spawn((Camera3d::default(), NoPlants));
```

A field can be spawned at any time, not only at `Startup`. Behind a loading
screen, spawn it when the level is ready, with a state's `OnEnter` rather
than a system that checks a flag every frame, or spawn it with
`Visibility::Hidden` and show it when the screen lifts:

```rust
app.add_systems(OnEnter(GameState::Playing), spawn_plants);
```

Layers 0 to 31 are supported. Every other 3D camera draws its own grass, with
its own culling and level of detail; [`PlantCamera`](settings.md#draw-differently-on-one-camera)
changes how much.
