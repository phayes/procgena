# procgena

Procedural content generators for Rust and Bevy, in one Cargo workspace.

How the pieces fit together, from one asset up to a whole world, is in
[ARCHITECTURE.md](ARCHITECTURE.md); what is built and what comes next is in
[ROADMAP.md](ROADMAP.md).

| Folder   | Crates                                   | What it makes                                                   |
|----------|------------------------------------------|-----------------------------------------------------------------|
| `stone/` | `procgena_stone`, `procgena_stone_bevy`  | Procedural rocks, built on the CPU or the GPU, with a Bevy plugin |
| `tree/`  | `procgena_tree`, `procgena_tree_bevy`    | Procedural trees and tree Scatters, with a Bevy plugin for wind and impostors |
| `fruit/` | `procgena_fruit`, `procgena_fruit_bevy`  | Fruit and flower clusters that trees carry on their branches |
| `deadwood/` | `procgena_deadwood`, `procgena_deadwood_bevy` | Fallen logs, branches, sticks and stumps, and their Scatter |
| `mushroom/` | `procgena_mushroom`, `procgena_mushroom_bevy` | Procedural mushrooms, clusters and fairy rings, and their Scatter |
| `plant/` | `procgena_plant`, `procgena_plant_bevy`, `procgena_plant_web` | Grass and foliage, grown on the GPU, plus a browser demo |
| `clump/` | `procgena_clump`, `procgena_clump_bevy`  | Clumps of snow, moss, mud and litter on top of a surface mesh, with a Bevy plugin that grows them on rocks, trees and tagged meshes |
| `terrain/` | `procgena_terrain`, `procgena_terrain_bevy` | Labelled terrain regions that join seamlessly, meshed and streamed in chunks |
| `biome/` | `procgena_biome`, `procgena_biome_bevy`  | Biomes, and Ecotopes: stacks of Scatters laid over the same ground |
| `world/` | `procgena_world`                         | A World of regions, each with its own Biome |
| `impostor/` | `procgena_impostor`                   | Octahedral impostors baked from any specimen |
| `specimen/` | `procgena_specimen`                   | The tagged specimens generators hand to each other, so one can build on another's result |
| `core/`  | `procgena_core`, `procgena_core_bevy`    | Rules every level shares: hashing, seeds, ids, ground, placement, the LOD chain, presets; and the shared Bevy renderer, dither and textures |
| `procgena/` | `procgena`                            | The desktop studio for editing and viewing every kind of preset |

## Units

Every crate uses metres: mesh positions, and every setting that is a length,
size or distance. Assets from different crates can share one scene without
rescaling. Detail levels switch by size on screen, not by distance. Docs write "metres"
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
