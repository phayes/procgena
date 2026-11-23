# procgena_mushroom

Generate procedural mushrooms in Rust: single mushrooms, clusters, and mushrooms
scattered over another generator's surface, each at several levels of detail.
`procgena_mushroom` builds the meshes and has no engine dependency;
`procgena_mushroom_bevy` draws them in Bevy.

| Crate | What it does |
|---|---|
| `procgena_mushroom` | Options and presets, meshes at any detail, clusters, scatter, OBJ/glTF export |
| `procgena_mushroom_bevy` | Bevy plugin: background building, distance-based detail switching, material and shaders |

All lengths are in metres, Y is up, and a mushroom's stem base sits at the origin.

## Make a mushroom

```rust
use procgena_mushroom::{Mushroom, MushroomDetail, MushroomPreset};

let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
let close = mushroom.mesh(); // the detail in the options: full by default
let far = mushroom.mesh_with_detail(&MushroomDetail::FLAT).unwrap();
println!("{} vs {} triangles", close.triangle_count(), far.triangle_count());
```

A mesh holds positions, normals, texture coordinates, tangents and per-vertex
colours. Presets include fly agaric, porcini, chanterelle, parasol, liberty cap,
shaggy ink cap, button, enoki, puffball and a storybook toadstool
(`MushroomPreset::ALL`).

To change one, edit its options. The same options and seed always give the same
mushroom.

```rust
use procgena_mushroom::{CapFamily, Mushroom, MushroomPreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = MushroomPreset::Button.options();
    options.seed = 7;
    options.size = 0.06; // cap diameter in metres
    options.cap.family = CapFamily::Umbonate;
    options.warts.count = 40;
    let mushroom = Mushroom::new(options)?;
    println!("{} triangles", mushroom.mesh().triangle_count());
    Ok(())
}
```

Options also load from JSON, which may leave settings out to use their defaults:
`Mushroom::from_json(r#"{"cap": {"lobes": 5, "lobe_depth": 0.1}}"#)`.

### What you can shape

- **Cap:** ten outline families (bell, cone, dome, funnel, egg, globe and more),
  height, rim waves, lopsidedness, tilt and surface bumps.
- **Underside:** rim thickness, sag, and gills that run down the stem.
- **Stem:** height, radius, taper, a collar under the cap, and a lean that can
  bend near the top or sweep out and turn back up (`upturn`).
- **Bulb:** a swollen foot, and a rounded bottom that closes the stem below the
  ground. Turn `bulb.closed` off for mushrooms planted in a landscape.
- **Annulus:** a thin, ragged skirt around the stem.
- **Warts or scales:** raised spots on the cap, drawn by the renderer's shader
  (and painted into vertex colours for exported files).
- **Colours:** per-zone colours, optional gill lines, and a cap pattern
  (speckle, cracks, spots), the last two drawn by the renderer.
- **Cluster:** how many stems grow together, as a tuft, a row or a fairy ring.

## Levels of detail

Distant mushrooms don't need fine detail, so renderers swap in simpler meshes as
the camera moves away. Every level follows the same shape, so the mushroom keeps
its outline when it switches.

| Detail | Surfaces | Use |
|---|---|---|
| `FULL`, `HIGH` | Closed (watertight) cap and stem | Close up |
| `MEDIUM`, `LOW` | Open cap and stem | Middle distance |
| `FLAT` | Open six-sided cap and a flat stem strip that turns toward the camera; about 40 triangles | Far away, or thousands at once |
| `BILLBOARD` | One triangle for the stem and one for the cap, turned toward the camera, vertex colours only | Farthest |

`MushroomLod` says which detail to draw at which distance;
`MushroomLod::for_size(size)` scales the defaults to the mushroom's size.
`Mushroom::lod_meshes(&lod)` builds one mesh per level.

Shapes that turn toward the camera are a `BillboardMesh`: the shapes plus the
data a renderer needs to keep them facing the camera. Renderers that can't turn
them can draw the fixed crossing copies instead.

## Clusters and scatter

A cluster is part of the mushroom's own options:

```rust
use procgena_mushroom::{ClusterLayout, Mushroom, MushroomPreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = MushroomPreset::Button.options();
    options.cluster.stems = 30;
    options.cluster.layout = ClusterLayout::FairyRing;
    let ring = Mushroom::new(options)?;
    for member in ring.members() {
        let body = &ring.bodies()[member.variant as usize];
        let placement = member.transform();
        println!("{} m cap at {:?}", body.cap_radius(), placement.translation);
    }
    Ok(())
}
```

`Mushroom::mesh` and `Mushroom::lod_meshes` combine every member into one mesh;
renderers that draw many copies of one mesh use `bodies()` and `members()`
instead.

`MushroomScatter::on(&host, &mushroom, &options)` spreads mushrooms over the
parts of another generator's `procgena_specimen::Specimen` picked by a tag filter (for
example a rock), on surfaces no steeper than a chosen slope. Each spot grows the
mushroom as configured, so a clustered mushroom scatters whole clusters, with
members dropped onto the surface.

`Mushroom::specimen` and `MushroomScatter::specimen` also return a tagged
`procgena_specimen::Specimen`, so other generators can grow on mushrooms.

## Export

With the `export-obj` and `export-gltf` features, `export::obj::to_obj` and
`export::gltf::to_glb` / `to_gltf` write one object per detail level. To export
every preset:

```bash
cargo run -p procgena_mushroom --example export --features export-obj,export-gltf
```

## Bevy

```rust,ignore
use bevy::prelude::*;
use procgena_mushroom::MushroomPreset;
use procgena_mushroom_bevy::{MushroomParams, MushroomPlugin};

App::new()
    .add_plugins((DefaultPlugins, MushroomPlugin::default()))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(MushroomParams::preset(MushroomPreset::FlyAgaric));
    })
    .run();
```

Spawn `MushroomParams` for one mushroom (or cluster), or
`MushroomScatterParams` as a child of an entity with a `GeneratedSpecimen` (such as
a rock from `procgena_stone_bevy`). Mushrooms are built in the background and
appear as `MushroomPart` children, one or two per level, switched by Bevy's
`VisibilityRange`. A `MushroomFinished` event fires when each build ends.

Try the viewer, which shows every preset, two clusters, and porcini and fairy
rings scattered over a patch of ground:

```bash
cargo run -p procgena_mushroom_bevy --example viewer
```

Add `--closeup` to start beside the enoki cluster, `--flat` or `--billboard` to
see only that level, `--orbit <degrees>` to change the starting angle, and
`--screenshot <file.png>` to save an image and exit.

## Tests

```bash
cargo test -p procgena_mushroom --all-features
cargo test -p procgena_mushroom_bevy
```

One Bevy test renders every detail level on the GPU and checks that every shader
compiles. It is ignored by default:

```bash
cargo test -p procgena_mushroom_bevy --test gpu -- --include-ignored
```

## Status

The procgena studio has a Mushrooms tab for designing mushrooms and exporting
them. See [plan.md](plan.md) for the design, what is done, and what is left.

## Credits

The cap outline families follow [Infinigen](https://github.com/princeton-vl/infinigen)'s
mushroom generator (BSD-3-Clause, Princeton University). Ideas for mushroom
types and stem shapes also came from
[inkwell-webgpu-mushrooms](https://github.com/siliconjungle/inkwell-webgpu-mushrooms)
(MIT).
