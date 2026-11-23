# procgena

Procedural content generators for Rust and Bevy, in one Cargo workspace.

| Folder   | Crates                                   | What it makes                                                   |
|----------|------------------------------------------|-----------------------------------------------------------------|
| `stone/` | `procgena_stone`, `procgena_stone_bevy`  | Procedural rocks, with a Bevy plugin for meshes, materials, LOD |
| `tree/`  | `procgena_tree`, `procgena_tree_bevy`    | Procedural trees, with a Bevy plugin for meshes, wind, LOD      |
| `clump/` | `procgena_clump`, `procgena_clump_bevy`  | Clumps of snow, moss, mud and litter on top of a surface mesh, with a Bevy plugin that grows them on rocks, trees and tagged meshes |
| `specimen/` | `procgena_specimen`                   | The tagged specimens generators hand to each other, so one can build on another's result |
| `core/`  | `procgena_core`                          | Engine-free rules every level shares: hashing, random numbers and ground shapes |
| `mushroom/` | `procgena_mushroom`, `procgena_mushroom_bevy` | Procedural mushrooms, clusters and scatter, with a Bevy plugin for LOD |
| `plant/` | `procgena_plant`, `procgena_plant_web`   | Grass and foliage fields for Bevy, plus a browser demo          |
| `terrain/` | `procgena_terrain`                     | Labelled terrain regions that join seamlessly, built in chunks  |

## Units

Every crate uses metres: mesh positions, and every setting that is a length,
size or distance (including level-of-detail switching distances). Assets from
different crates can share one scene without rescaling. Docs write "metres"
(British spelling), or "m" and "cm" next to numbers.

## Assets

Each folder keeps its own `assets/` and docs. The large texture sets are not
in the repository. The `procgena` studio downloads the textures it shows by
itself, the first time it needs them, and keeps them in your cache folder
(such as `~/Library/Caches/procgena/textures/` on macOS). For each crate's own
viewers, examples and texture tests, download a project's textures from the
workspace root with the shared tools in `tools/` (see `tools/README.md`):

```bash
uv run --project tools textures download tree
```

```bash
cargo build --workspace
```
