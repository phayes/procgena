# procgena_mushroom

Generate procedural mushrooms in Rust: single mushrooms, clusters, and mushrooms
scattered over another generator's surface, each at several levels of detail.
`procgena_mushroom` builds the meshes and has no engine dependency;
`procgena_mushroom_bevy` draws them in Bevy.

| Crate | What it does |
|---|---|
| `procgena_mushroom` | Options and presets, meshes at any detail, clusters, scatter, OBJ/glTF export |
| `procgena_mushroom_bevy` | Bevy plugin: background building, detail and impostors switched by size on screen, material and shaders |

All lengths are in metres, Y is up, and a mushroom's stem base sits at the origin.

## Make a mushroom

```rust
use procgena_mushroom::{Mushroom, MushroomDetail, MushroomPreset};

let mushroom = Mushroom::from_preset(MushroomPreset::fly_agaric()).unwrap();
let close = mushroom.mesh(); // the game level in the options: HIGH by default
let far = mushroom.mesh_with_detail(&MushroomDetail::LOW).unwrap();
println!("{} vs {} triangles", close.triangle_count(), far.triangle_count());
```

A mesh holds positions, normals, texture coordinates, tangents and per-vertex
colours. Presets include fly agaric, porcini, chanterelle, parasol, liberty cap,
shaggy ink cap, button, enoki, puffball, a storybook toadstool and a fairy ring
(`MushroomPreset::all()`). Each has a function named by its key, such as
`MushroomPreset::shaggy_ink_cap()`, and `MushroomPreset::named("shaggy_ink_cap")`
looks one up by key.

To change one, edit a copy of its options. The same options and
`default_seed` always give the same mushroom.

```rust
use procgena_mushroom::{CapFamily, Mushroom, MushroomPreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = MushroomPreset::button().mushroom.clone();
    options.default_seed = 7;
    options.size = 0.06; // cap diameter in metres
    options.cap.family = CapFamily::Umbonate;
    options.warts.count = 40;
    let mushroom = Mushroom::new(options)?;
    println!("{} triangles", mushroom.mesh().triangle_count());
    Ok(())
}
```

Presets also load from `.mushroom.yml` files, which name a built-in preset as
their `base` and give only the settings that differ (without a `base`, missing
settings use their defaults). Colours start with `#`, so quote them:

```yaml
base: button
mushroom:
  cap:
    lobes: 5
    lobe_depth: 0.1
  material:
    cap_centre: "#c09060"
```

Read one with `MushroomPreset::from_yaml(key, text)`, and write one with
`preset.to_yaml(Some(base))`.

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

A mushroom's preset sets its game level (`MushroomOptions::game_level`,
`HIGH` by default): its real shape, which picking, growing things on it and
its impostor use. Renderers also draw one coarser mesh when the mushroom is
small on screen, then an impostor. Every detail follows the same shape, so the
mushroom keeps its outline when it switches.

| Detail | Surfaces |
|---|---|
| `FULL`, `HIGH` | Closed (watertight) cap and stem |
| `MEDIUM`, `LOW` | Open cap and stem |

`Mushroom::mesh` builds the game level, `Mushroom::lod` gives the game level
and its lower detail, and `Mushroom::mesh_with_detail` builds any other.

## Clusters and scatter

A cluster is part of the mushroom's own options:

```rust
use procgena_mushroom::{ClusterLayout, Mushroom, MushroomPreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = MushroomPreset::button().mushroom.clone();
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
        commands.spawn(MushroomParams::preset(MushroomPreset::fly_agaric()));
    })
    .run();
```

Spawn `MushroomParams` for one mushroom (or cluster), loaded from a
`.mushroom.yml` file with `MushroomParams::Asset`, or
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

The procgena studio edits mushrooms as a kind, and exports them. What is
left is in [ROADMAP.md](../ROADMAP.md).

## Credits

The cap outline families follow [Infinigen](https://github.com/princeton-vl/infinigen)'s
mushroom generator (BSD-3-Clause, Princeton University). Ideas for mushroom
types and stem shapes also came from
[inkwell-webgpu-mushrooms](https://github.com/siliconjungle/inkwell-webgpu-mushrooms)
(MIT).
