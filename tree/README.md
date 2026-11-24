# procgena_tree

Generate procedural trees in Rust, from small bushes to branching oaks and pines.
A port of [ez-tree](https://github.com/dgreenheck/ez-tree), `procgena_tree` produces
branch and leaf meshes, repeatable shapes from a seed, and simpler meshes for
distant trees. Your application supplies the renderer.

## Generate a tree

Start with a built-in preset:

```rust
use procgena_tree::{Tree, TreePreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = Tree::from_preset(TreePreset::oak_small())?;
    let mesh = tree.mesh();

    println!("{} triangles", tree.triangle_count());
    println!("{} bark vertices", mesh.branches.positions.len());
    println!("{} leaf vertices", mesh.leaves.positions.len());
    Ok(())
}
```

`mesh.branches` and `mesh.leaves` contain positions, normals, texture coordinates,
and triangle indices. Upload these buffers to your renderer and give each mesh
its own material. Presets include ash, aspen, oak, pine, bushes, a tree trained
along a trellis, a cedar, and flowering and fruiting trees such as apple and
flowering dogwood. `TreePreset::all()` lists them all, and `TreePreset::named`
finds one by its key, such as `"oak_small"`.

## Customize the shape

Edit a preset's options, then generate a new tree. The same options and seed
produce the same shape; change the seed for another variation.

```rust
use procgena_tree::{Tree, TreePreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = TreePreset::oak_small().tree.clone();
    options.default_seed = 42;
    options.branch.length[0] = 1.2; // Trunk length, in metres.
    options.leaves.size = 0.15;

    let tree = Tree::new(options)?;
    println!("{} triangles", tree.triangle_count());
    Ok(())
}
```

Branch settings use index `0` for the trunk and indices `1` through `3` for
successive branch levels. You can also start with `TreeOptions::default()`.
`Tree::new` checks branch depth and section counts; it does not validate every
numeric setting, so use sensible dimensions and counts.

## Load and save preset files

Presets are saved as `.tree.yml` files. A file names a built-in preset as its
`base` and gives only the settings that differ, in snake case. Settings under
the `tree` key merge into the base's, so nested settings may be partial; lists
replace the base's list whole. Lengths are in metres. Settings a file doesn't
know are errors.

```rust
use procgena_tree::{Tree, TreePreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let preset = TreePreset::from_yaml(
        "tall_oak",
        "base: oak_small\n\
         name: Tall oak\n\
         tree:\n  \
           default_seed: 42\n  \
           branch:\n    \
             length: { \"0\": 1.2 }\n",
    )?;
    let tree = Tree::from_preset(&preset)?;

    // Write it back, as only what differs from the base.
    let text = preset.to_yaml(Some(TreePreset::oak_small()));
    assert_eq!(TreePreset::from_yaml("tall_oak", &text)?, preset);
    assert_eq!(tree.options(), &preset.tree);
    Ok(())
}
```

The key, here `tall_oak`, is the file's name without `.tree.yml`. Every
built-in preset is a file in `crates/procgena_tree/presets/` to copy from.
Deserializing `TreeOptions` directly with serde requires every field.

## Create simpler meshes

Generate several levels of detail (LOD) from the same tree. Your renderer chooses
which mesh to show based on camera distance.

```rust
use procgena_tree::{Tree, TreePreset, default_lod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = Tree::from_preset(TreePreset::ash_small())?;
    for (level, mesh) in tree.lod_meshes(&default_lod().levels) {
        println!(
            "From distance {}: {} triangles",
            level.distance,
            mesh.triangle_count(),
        );
    }
    Ok(())
}
```

The four default levels use progressively simpler meshes at distances of 0, 10,
25 and 50 metres for a tree 10 metres tall; a renderer scales these by each
tree's height, so a 5 m tree switches at 5, 12.5 and 25 metres. Nothing is
drawn beyond 1.5 km, scaled the same way. To choose your own mesh detail, pass a `LodDetail` to
`Tree::mesh_with_detail`.

Only LOD0 and LOD1 are meant for gameplay. Their trunk and branches are closed,
outward-facing solids (`LodDetail::closed`), suitable for physics, collision and
volumes. LOD2 and LOD3 are for drawing distant trees only: their bark is open
and simplified, so don't build colliders or volumes from them.

At 50 metres (LOD3), retained branches, leaves and fruit use flat shapes
called billboards. A compatible renderer turns them toward the camera to help
preserve the distant tree’s outline. `MeshBuffers::billboard` stores the rotation
data; `TreeMesh::billboard_cull` stores crossed fallback shapes. Draw the fallback
shapes only when the renderer does not rotate billboards. `procgena_tree_bevy` handles
rotation and skips the fallback shapes automatically. glTF exports the data and
`billboard` / `billboard_cull` node tags for compatible importers; ordinary viewers
show the fixed shapes. OBJ exports fixed shapes, naming fallback objects
`<part>_billboard_cull`.
If you only need branch shapes and leaf placements, use `Skeleton::generate`
without building a mesh.

## Export a model

Optional Cargo features enable file export:

- `export-obj`: Wavefront OBJ geometry and MTL materials.
- `export-gltf`: glTF (`.gltf` and `.bin`) or a single binary `.glb` file.

With `export-gltf` enabled, save a tree as GLB:

```no_run
use procgena_tree::{Tree, TreePreset};
use procgena_tree::export::gltf::{GltfOptions, to_glb};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = Tree::from_preset(TreePreset::pine_small())?;
    let bytes = to_glb(
        std::slice::from_ref(tree.mesh()),
        tree.options(),
        &GltfOptions::default(),
    )?;
    std::fs::write("pine.glb", bytes)?;
    Ok(())
}
```

Exporters return file contents for you to save. By default, exports use material
colours without images; leaves appear as rectangles without a texture supplying
their outline. Use `GltfOptions::textures` to reference or embed PNG/JPEG images.
An optional rig prepares a model for bending animation, but does not add animation
clips. Exported LOD meshes require the receiving tool to choose which to display.

To export every preset as OBJ and GLB from this repository:

```sh
cargo run -p procgena_tree --example export --features export-obj,export-gltf
```

Files are written to `target/procgena_tree_export` by default. The example accepts an
output directory and an optional source texture directory as positional arguments.

## Materials and animation

`BarkMaterial` and `LeafMaterial` describe the appearance settings for a renderer.
The repository's `assets/textures/` directory holds KTX2 textures, which are not
included in the crate. Download them with `uv run --project tools textures download tree`, which
fetches every leaf and the 512 and 1k bark sizes (add `--sizes 512,1k,2k` for
2k bark). Every texture comes in two formats:

- BC7 for GPUs that support BC7 compression, including most desktop GPUs.
- UASTC for a Basis Universal loader to transcode to a supported GPU format.

`bark_texture_path` and `leaf_texture_path` give each file's path inside that
directory, such as `bark/sakura_bark-1k-color-bc7.ktx2`. The options' `bark.kind`
and `leaves.kind` select the texture names; bark comes in three sizes.
`LEAF_KINDS` lists 26 leaf textures: four single leaves from ez-tree, and 22
leafy branch tips from Nobiax's CC0 packs, which also have normal maps
(`leaf_has_normal_map`). Two of these, `cedar` and `maple`, are recoloured
arborvitae and field maple branches in a bright green that presets can darken
or shift with a leaf tint. Every leaf kind also has a 256 × 256 clump image
(`LeafMap::Clump`), showing several leaves on one flat card. Use it with fewer,
larger cards for distant trees. For bark without texture images, multiply the
linear RGB colour from `bark_average_color` by the bark tint to approximate
the textured colour. `assets/textures/assets_manifest.json` lists every
file with its download address. Load colour images as sRGB and normal/roughness maps as linear data. Normal maps use the OpenGL/glTF convention; leaf alpha is straight,
not premultiplied. Keep the metallic factor at zero if using the roughness image
as a combined metallic-roughness texture.

Normal maps add lighting detail without extra geometry. Supply the directions
in `MeshBuffers::tangents` from the corresponding bark or leaf mesh to orient
that detail. For bark, keep the default `BarkUvMode::Continuous`.
`BarkUvMode::Mirrored` matches the JavaScript ez-tree generator's texture layout
but reverses normal-map lighting detail on every other branch section, so
Bevy and glTF export omit bark normal maps in that mode.

For GPU wind, pass `MeshBuffers::wind` and `WindParams` to the `WIND_WESL` shader
module and add its `wind_offset` to vertex positions. For joint-based animation,
create joints with `Tree::rig` and vertex weights with `Rig::skin`.
`BranchGrouping::parts` and `TreeMesh::part` let you extract separate tree parts.
`TrellisOptions::wires` and `TrellisMaterial` describe the optional support wires.

## Plant a grove

A `TreeScatter` mixes several tree species over a stretch of ground, such as a
grove or a woodland. Each species is a `ScatterTree` with its own density in
trees per hectare, a spacing its trunks keep from their neighbours, and how
much it gathers into stands. `TreeScatterPlacement::new` lays a Scatter out
over a `ScatterInput` (the ground, an area, a seed and an optional mask) and
returns one `TreeCopy` per tree:

```rust
use glam::{DVec2, Vec2};
use procgena_tree::{
    FlatGround, ScatterArea, ScatterInput, TreeScatterPlacement, TreeScatterPreset,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scatter = &TreeScatterPreset::oak_woodland().tree_scatter;
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(40.0));
    let input = ScatterInput::new(&FlatGround, area, scatter.default_seed);
    let placement = TreeScatterPlacement::new(scatter, &input)?;
    for copy in placement.instances() {
        // Build each species' variant mesh once, and draw it with this transform.
        let _options = scatter.trees[copy.tree].variant(copy.variant);
        let _transform = copy.transform;
    }
    Ok(())
}
```

Each species is drawn from a few shape variants, so hundreds of trees need only
a handful of meshes. Lowering the density only removes trees; the rest stay
where they were. Trees can stand on uneven ground, and Scatters load and save
as `.tree_scatter.yml` files. See [docs/scatters.md](docs/scatters.md) for the
presets, every setting, and the file format.

## Bevy integration

The workspace also includes `procgena_tree_bevy`. Add its `TreePlugin` after Bevy's
default plugins and spawn a `TreeParams` component to generate and display trees.
It handles background generation, materials, wind, and mesh detail switching.
Spawn a `TreeScatterParams` component to grow a whole grove.
See that crate's documentation for setup and texture paths.
