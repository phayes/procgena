# Ecotope layers: Scatters inside the Ecotope, keys at every level

> **Record status: decided** by the user 2026-11-26, as the plan for
> milestone [EL](../../ROADMAP.md#el-ecotope-layers); not built yet. It
> supersedes ROADMAP item 9 (tree species keys), C2's fallback from a
> species key to its preset's key, the open `Scatter` trait as a hook,
> and Scatters as presets of their own (`.<asset>_scatter.yml`).

## Bottom line

- **A Scatter stops being its own thing.** It becomes a **layer** of an
  Ecotope preset, written inside the `.ecotope.yml` file. There is no
  Scatter preset, no Scatter file kind and no `Box<dyn Scatter>` slot in
  an Ecotope.
- **A closed set of placement algorithms.** Each layer picks one:
  - `points`: copies on the World lattice: density rank, exact disc
    exclusion, slope and footing, `K_5` spacing, patches, and optional
    cluster members. Today's tree, stone, dead wood and mushroom
    Scatters are all this algorithm with different settings.
  - `cover`: a density field drawn where it is placed (plants). It leaves
    no footprints.
  A new algorithm is a change to procgena, not a hook.
- **One identity rule at every level.** A parent names each child with a
  frozen local key: the key of a YAML map. Seeds and addresses come from
  that key alone. What a child points at (a preset or an asset) is a
  value, never its identity, and neither falls back to the other.

## The rule

The levels are World → region → Biome → Ecotope → layer → entry, then the
asset, which is built from options and a seed.

| Level | Named by | Points at (a value) |
|-------|----------|---------------------|
| Ecotope in a Biome | its key in the Biome | an Ecotope preset |
| Layer in an Ecotope | its map key under `layers:` | an algorithm |
| Entry in a layer | its map key under `entries:` | an asset preset, or inline options |

- **Seeds** chain through keys only:
  `entry seed = child_seed(child_seed(ecotope seed, layer kind, layer key), entry kind, entry key)`.
- **Addresses** are (region, Ecotope, layer, cell, index). The entry is
  decided from the address, not part of it (D3, section 7b).
- **Asset crates take a seed and options, never a key or a preset.** A
  preset key is only for looking a preset up in a catalogue. Renaming a
  preset never reseeds anything.
- **Order is not identity.** `priority` is a value on one World-wide
  scale (D3), so reordering layers changes nothing.
- **Every key is explicit.** Layers and entries are maps even when a
  layer has one entry, so a file built on a `base` preset can override one
  entry by its key. This replaces today's mix: trees as a list with an
  optional `key`, stone classes as a list with a required `key:`, and dead
  wood and mushrooms with no entry key at all.

What this buys:

- Swap `pine_large` for another tree preset and every tree stands where
  it stood.
- Rename an entry and only that entry's own streams (rank, clustering,
  variant choice) change, so only the cells it wins or loses move.
- Rename a layer and the layer is placed afresh: its key is its identity.

## Layers and entries

- **A layer owns** its algorithm, `priority`, `clearance`, and the
  algorithm's layer-wide settings.
- **An entry owns** its asset and how that asset is placed: `density`,
  `spacing`, slope limit, size range, cluster members, patches. Placement
  belongs to the Ecotope, so `TreeStand` (density, spacing, slope limit)
  leaves the tree preset and becomes entry settings.
- **One `points` layer may mix asset kinds** (the user, 2026-11-26): trees
  and boulders can share one lattice and space against each other. The
  lattice cell is the smallest entry spacing, narrowed so the summed
  chance per cell stays at most 1. Each entry ranks the cell with its own
  stream (salted by its key), and the spot goes to the entry whose rank
  its density keeps by the widest margin. Nothing depends on entry order,
  so renaming an entry changes only the cells that entry wins or loses. Today's stone rule
  ("first class in list order") is replaced.
- **Your own assets** come in as entries: a mesh (`MeshScatter` becomes a
  `mesh:` entry) or your own generator behind an asset trait that takes
  options and a seed and returns a specimen. The Replace hook moves from
  "your own Scatter" to "your own asset".

## The pine forest floor, rewritten

```yaml
name: Pine forest floor
group: Forest
ecotope:
  default_seed: 11
  layers:
    pines:
      algorithm: points
      priority: 900
      clearance: 0.6
      entries:
        small:  { tree: pine_small, density: 0.0135, spacing: 3.0, clustering: 0.6 }
        large:  { tree: pine_large, density: 0.018, spacing: 6.0 }
        medium: { tree: pine_medium, density: 0.027, spacing: 4.5 }
    stones:
      algorithm: points
      priority: 850
      clearance: 0.3
      entries:
        boulders: { stone: lichen_boulder, density: 0.006, spacing: 5.0, size: [0.5, 1.1] }
        rocks:    { stone: river_stone, density: 0.02, spacing: 1.5, members: [1, 3] }
    logs:
      algorithm: points
      priority: 800
      clearance: 0.3
      entries:
        mossy: { deadwood: mossy_log, density: 0.012, spacing: 4.0 }
    mushrooms:
      algorithm: points
      priority: 200
      clearance: 0.1
      entries:
        porcini:     { mushroom: porcini, density: 0.15, spacing: 0.8 }
        fly_agarics: { mushroom: fly_agaric, density: 0.03, spacing: 1.5 }
    undergrowth:
      algorithm: cover
      priority: 150
      entries:
        floor: { plant: forest_floor, max_slope: 40.0 }
```

The tree densities and spacings are placeholders for what the tree
presets' stands give today. The two mushroom Scatters become one layer,
which the old shape could not express.

## Code shape

```rust
struct Layer { algorithm: Algorithm, priority: u32, clearance: f32, entries: BTreeMap<ChildKey, Entry> }
enum Algorithm { Points(PointsSettings), Cover(CoverSettings) }
struct Entry { asset: AssetRef, placement: EntryPlacement }
enum AssetRef { Tree(TreeOptions), Stone(StoneOptions), Deadwood(PieceOptions), Mushroom(MushroomOptions), Plant(PlantOptions), Mesh(MeshRef), Custom(Box<dyn Asset>) }
// asset crates: build(&options, seed); no key, no preset
```

`ScatterKind` (an algorithm and an asset fused) and the per-asset
adapters in `procgena_biome/src/ecotope/scatters.rs` go. C2's `Scatter`
trait, `ChunkCache` and the lattice stay as the internals of `points`.
Whether `cover` runs on the CPU or the GPU is the algorithm's business
(plant's P4).

## What it supersedes

- **ROADMAP item 9 and its follow-up** ("tree species keys are
  required"): covered by the general rule; species are entries.
- **C2's species key fallback** (a missing key takes the preset's key):
  gone; `ScatterTree::new(preset)` reading `preset.key` goes with it.
- **Scatter file kinds** (ARCHITECTURE "Decisions to make", F3's
  `.<asset>_scatter.yml`): no Scatter presets. What happens to the eight
  built-in `.tree_scatter.yml` files and the asset crates' own Scatter
  APIs (`TreeScatter`, `StoneScatter`, `StickScatter`, `MushroomScatter`,
  `PlantScatter`) is decided when EL is built.
- **"Your own Scatter in an Ecotope"** (ARCHITECTURE hooks, "Bringing your
  own assets"): replaced by your own asset in an entry.
- **saved-file-format's "Keep seeds" step**: a rename never touches seeds,
  so Import never writes keys out.
- **D3's example** keyed mix entries by map under `scatters:`; this record
  keeps that shape and adds layers and algorithms.

## Not decided here

- The names and exact settings of `PointsSettings`, `CoverSettings` and
  `EntryPlacement`.
- The fate of the `.tree_scatter.yml` files and the asset-level Scatter
  APIs (the user, 2026-11-26: "not yet").
