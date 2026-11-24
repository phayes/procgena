# Presets

procgena_plant ships 40 field presets, from lawn to prairie to wildflower meadow,
forest-floor moss and ferns, understory bushes, a Baltic bog, a reed marsh, dunes, pampas,
spinifex, frost, kelp, crystal and cel-shaded fields, and 26 plant presets to
build your own from. Each Scatter preset is a `const` on `PlantScatterPreset` and a
`.plant_scatter.yml` file in [`assets/scatters`](../assets/scatters); each
plant preset a `const` on `PlantPreset` (and the same plant on `PlantParams`)
and a `.plant.yml` file in [`assets/plants`](../assets/plants).
[Plants and Scatters](scatters.md) shows how to change one or load one from a
file.

A preset has a `key` (its constant's name in lower case, `"meadow"`), a
display `name` (`"Meadow"`), a `description` and the `group` it is listed in.
Files and `named(key)` use the key; Rust code uses the constant
(`PlantScatterPreset::MEADOW`).
`PlantScatterPreset::all()` and `PlantPreset::all()` list them all, for a menu or an
inspector, `GROUPS` lists them group by group, and `named(key)` finds one by
its key.

The pictures show every preset, two rows per picture, each on a square of
about 6 m of its own ground (sand, snow, peat, a pond, the seabed) under a
top-down ARPG camera. Run `cargo run --release --example showcase` for page 1;
`--page 2` to `--page 6` draw the rest (`--fixed-time --screenshot PATH` saves
one). To fly around the forest, understory, biome, fantasy and toon presets up
close, run `cargo run --release --example gallery`.

![Eight biome presets in square patches of their own ground, each with its name under it: REED_MARSH in standing water, PAMPAS, ELEPHANT_GRASS, RYE_AND_CORNFLOWERS, COTTON_BOG on dark peat, DUNE_MARRAM on sand, SPINIFEX on red earth and FROSTED_MEADOW on snow](images/showcase_biomes.jpg)

![Eight fantasy and toon presets in square patches, each with its name under it: KELP_BED under water, CORKSCREW_ALIEN, CRYSTAL_SPIRES, BLIGHTED, BURNT_REGROWTH in ash, LAVENDER in rows, CEL_FIELD and COZY_PASTEL](images/showcase_fantasy_toon.jpg)

![Six general presets of grass in square patches, each with its name under it: MEADOW, TALL_PRAIRIE, WHEAT, DRY_SAVANNA, LAWN and STYLIZED](images/showcase_general.jpg)

![Eight seasonal, wild and flowering presets in square patches, each with its name under it: SUMMER_TALL, AUTUMN_TALL, SUMMER_WILD, AUTUMN_WILD, WILDFLOWER_MEADOW, SEED_HEADS, WHITE_FLOWERS and YELLOW_FLOWERS](images/showcase_seasons_wild.jpg)

![Six forest-floor and plant presets in square patches, each with its name under it: FOREST_MOSS, FOREST_FERNS and FOREST_FLOOR with green branching ferns, FOREST_BRACKEN with rust-brown fronds, CLOVER and BROAD_LEAF](images/showcase_forest.jpg)

![Seven understory presets in square patches of forest soil, each with its name under it: MATURE_BRACKEN's tall flopping fronds, SAPLING, HUCKLEBERRY, FOREST_UNDERSTORY mixing them all, SALAL, NETTLE and the low OREGON_GRAPE](images/showcase_understory.jpg)

| ![A summer field gone wild: tufts, bare patches, seed heads and a few flowers among tall grass](images/wild.jpg) | ![Wildflower meadow](images/flowers.jpg) | ![The same wild field from a top-down ARPG camera](images/arpg.jpg) |
| --- | --- | --- |

## The groups

Field presets:

| Group | Presets |
| --- | --- |
| `PlantScatterPreset::GENERAL` | `MEADOW` (the default), `TALL_PRAIRIE`, `WHEAT`, `DRY_SAVANNA`, `LAWN`, `STYLIZED` |
| `PlantScatterPreset::SEASONS` | `SUMMER_TALL`, `AUTUMN_TALL`: the same tall field, 0.7–1.3 m, green and brown, to mix or blend between |
| `PlantScatterPreset::WILD` | `SUMMER_WILD`, `AUTUMN_WILD`, `WILDFLOWER_MEADOW`: every kind of variation |
| `PlantScatterPreset::FOREST` | `FOREST_MOSS`, `FOREST_FERNS`, `FOREST_FLOOR`, `FOREST_BRACKEN`: short, wide forest-floor plants in clumps, with fallen leaves and branching ferns mixed in |
| `PlantScatterPreset::UNDERSTORY` | `SALAL`, `SAPLING`, `HUCKLEBERRY`, `OREGON_GRAPE`, `NETTLE`, `MATURE_BRACKEN`, `BRAMBLE`: one kind of knee- to head-high branching plant each, over moss (`BRAMBLE`'s canes arch over and root their tips in the ground); `FOREST_UNDERSTORY` mixes all but the bramble with ferns and fallen leaves |
| `PlantScatterPreset::SHRUBS` | `BROOM` (clumps of tall, upright green rods on short brown trunks, through meadow grass): open-country shrubs growing through grass |
| `PlantScatterPreset::BIOMES` | `COTTON_BOG` (red sphagnum, sedge tussocks, cotton grass, pools), `REED_MARSH` (2 m reeds and cattails), `DUNE_MARRAM` (blue-grey clumps on sand), `RYE_AND_CORNFLOWERS` (rye with cornflowers and poppies), `PAMPAS` (arching fountains under cream plumes), `SPINIFEX` (stiff splayed hummocks), `ELEPHANT_GRASS` (2.4–3.6 m, to hide in), `FROSTED_MEADOW` (stiff, glinting, out of snow), `BURNT_REGROWTH` (charcoal stubble and new shoots), `LAVENDER` (bushes; plant them in rows with a striped density mask) |
| `PlantScatterPreset::FANTASY` | `CRYSTAL_SPIRES` (rigid, faceted, glassy), `CORKSCREW_ALIEN` (ribbons twisted two turns, toon), `KELP_BED` (soft ribbons for under water; pair with a slow, lively wind), `BLIGHTED` (corrupted violet grass with bone-white heads) |
| `PlantScatterPreset::TOON` | `CEL_FIELD` (chunky two-band tufts and flowers), `COZY_PASTEL` (soft pastel grass and flowers, four bands) |

Plant presets, to grow alone with `PlantScatter::new` or to mix into any field
as a `ScatterPlant`:

| Group | Presets |
| --- | --- |
| `PlantPreset::GRASSES` | `MEADOW_GRASS`, `SUMMER_TALL_GRASS`, `LAWN_GRASS`, `WHEAT`, `MARRAM` |
| `PlantPreset::GROUND_COVER` | `SEED_HEADS`, `WHITE_FLOWERS`, `YELLOW_FLOWERS`, `CLOVER`, `BROAD_LEAF`, `MOSS`, `FERN`, `BRACKEN`, `LEAF_LITTER`, `COTTON_GRASS`, `CATTAIL`, `CORNFLOWER`, `POPPY` |
| `PlantPreset::UNDERSTORY` | `SALAL`, `SAPLING`, `HUCKLEBERRY`, `OREGON_GRAPE`, `NETTLE`, `MATURE_BRACKEN`, `BRAMBLE`: the branching plants of the understory presets |
| `PlantPreset::SHRUBS` | `BROOM` |

A field preset named after a plant, such as `SALAL` or `BROOM`, grows that
plant with the moss or grass around it; the other fields mix them in. Most
field presets grow a grass of their own that is not a plant preset; copy it
from the field (`PlantScatterPreset::TALL_PRAIRIE.plant_scatter.plants[0].plant`).

## Sizes

How tall each preset grows, how dense it is, and how many plants grow in it.
The tallest plant is before the mask's green channel, and includes every plant
of the field and their accents: flowers, seed heads, plumes, ferns and bushes.
A grass blade or flower counts its full length. A fern or bush counts the
highest point of its stems and branches at rest, with no wind: broom's long
upright rods reach well above its short trunk, while bramble's canes arch over
far lower than their length. To keep grass below a unit or a character, pick
by the tallest plant, then shorten the field's plants if you need to
([Plants and Scatters](scatters.md#how-tall-the-grass-grows)). For a plant preset,
the plants per m² are its own density, which a field scales; the height is
its own range and the top its highest point.

<!-- preset numbers: checked against the constants by a test, do not edit by hand -->
| Scatter preset | Name | Plants | Tallest plant (m) | Plants per m² | Spacing (cm) |
| --- | --- | --- | --- | --- | --- |
| `meadow` | Meadow | 1 | 0.60 | 60 | 12.9 |
| `tall_prairie` | Tall prairie | 1 | 1.40 | 40 | 15.8 |
| `wheat` | Wheat | 1 | 1.20 | 90 | 10.5 |
| `dry_savanna` | Dry savanna | 1 | 1.00 | 25 | 20.0 |
| `lawn` | Lawn | 1 | 0.12 | 250 | 6.3 |
| `stylized` | Stylised | 1 | 0.80 | 30 | 18.3 |
| `summer_tall` | Summer tall | 1 | 1.30 | 40 | 15.8 |
| `autumn_tall` | Autumn tall | 1 | 1.30 | 40 | 15.8 |
| `summer_wild` | Summer wild | 5 | 1.40 | 40 | 15.8 |
| `autumn_wild` | Autumn wild | 3 | 1.40 | 40 | 15.8 |
| `wildflower_meadow` | Wildflower meadow | 4 | 0.70 | 60 | 12.9 |
| `forest_moss` | Forest moss | 1 | 0.07 | 220 | 6.7 |
| `forest_ferns` | Forest ferns | 3 | 0.83 | 28 | 18.9 |
| `forest_floor` | Forest floor | 5 | 0.83 | 140 | 8.5 |
| `forest_bracken` | Forest bracken | 3 | 1.02 | 24 | 20.4 |
| `salal` | Salal | 2 | 0.83 | 122 | 9.1 |
| `sapling` | Saplings | 2 | 1.17 | 121 | 9.1 |
| `huckleberry` | Huckleberry | 2 | 1.17 | 121 | 9.1 |
| `oregon_grape` | Oregon grape | 2 | 0.45 | 123 | 9.0 |
| `nettle` | Nettles | 2 | 1.01 | 125 | 8.9 |
| `mature_bracken` | Mature bracken | 2 | 2.41 | 121 | 9.1 |
| `bramble` | Bramble | 2 | 0.77 | 120 | 9.1 |
| `forest_understory` | Forest understory | 9 | 2.41 | 123 | 9.0 |
| `broom` | Broom | 2 | 1.53 | 61 | 12.8 |
| `cotton_bog` | Cotton bog | 3 | 0.55 | 200 | 7.1 |
| `reed_marsh` | Reed marsh | 2 | 2.40 | 45 | 14.9 |
| `dune_marram` | Dune marram | 1 | 0.95 | 45 | 14.9 |
| `rye_and_cornflowers` | Rye and cornflowers | 3 | 1.60 | 80 | 11.2 |
| `pampas` | Pampas | 1 | 2.60 | 70 | 12.0 |
| `spinifex` | Spinifex | 1 | 0.70 | 130 | 8.8 |
| `elephant_grass` | Elephant grass | 2 | 3.60 | 40 | 15.8 |
| `frosted_meadow` | Frosted meadow | 1 | 0.32 | 130 | 8.8 |
| `burnt_regrowth` | Burnt regrowth | 3 | 0.90 | 90 | 10.5 |
| `lavender` | Lavender | 1 | 0.60 | 160 | 7.9 |
| `crystal_spires` | Crystal spires | 1 | 1.00 | 30 | 18.3 |
| `corkscrew_alien` | Corkscrew alien | 2 | 1.30 | 14 | 26.7 |
| `kelp_bed` | Kelp bed | 2 | 3.00 | 14 | 26.7 |
| `blighted` | Blighted | 1 | 1.00 | 45 | 14.9 |
| `cel_field` | Cel field | 3 | 0.90 | 22 | 21.3 |
| `cozy_pastel` | Cosy pastel | 4 | 0.45 | 60 | 12.9 |

| Plant preset | Name | Plants per m² | Height (m) | Top (m) |
| --- | --- | --- | --- | --- |
| `meadow_grass` | Meadow grass | 60 | 0.30–0.60 | 0.60 |
| `summer_tall_grass` | Summer tall grass | 40 | 0.70–1.30 | 1.30 |
| `lawn_grass` | Lawn grass | 250 | 0.05–0.12 | 0.12 |
| `wheat` | Wheat | 90 | 0.90–1.20 | 1.20 |
| `marram` | Marram | 45 | 0.50–0.95 | 0.95 |
| `seed_heads` | Seed heads | 40 | 0.80–1.40 | 1.40 |
| `white_flowers` | White flowers | 30 | 0.35–0.70 | 0.70 |
| `yellow_flowers` | Yellow flowers | 40 | 0.30–0.60 | 0.60 |
| `clover` | Clover | 100 | 0.08–0.20 | 0.20 |
| `broad_leaf` | Broad leaf | 20 | 0.20–0.45 | 0.45 |
| `moss` | Moss | 200 | 0.02–0.07 | 0.07 |
| `fern` | Fern | 2 | 0.50–0.95 | 0.83 |
| `bracken` | Bracken | 4 | 0.60–1.10 | 1.02 |
| `leaf_litter` | Leaf litter | 40 | 0.05–0.15 | 0.15 |
| `cotton_grass` | Cotton grass | 30 | 0.30–0.55 | 0.55 |
| `cattail` | Cattail | 30 | 1.60–2.40 | 2.40 |
| `cornflower` | Cornflower | 20 | 0.70–1.10 | 1.10 |
| `poppy` | Poppy | 20 | 0.80–1.25 | 1.25 |
| `salal` | Salal | 1.5 | 0.45–0.85 | 0.83 |
| `sapling` | Sapling | 0.6 | 0.80–1.20 | 1.17 |
| `huckleberry` | Huckleberry | 0.5 | 0.60–1.05 | 1.17 |
| `oregon_grape` | Oregon grape | 2.5 | 0.25–0.50 | 0.45 |
| `nettle` | Nettle | 5 | 0.60–1.00 | 1.01 |
| `bramble` | Bramble | 0.4 | 1.60–3.00 | 0.77 |
| `mature_bracken` | Mature bracken | 1 | 1.60–2.40 | 2.41 |
| `broom` | Broom | 0.7 | 0.35–0.60 | 1.53 |

<!-- end of preset numbers -->

## Adding a preset to the crate

Every preset is declared once, in [`src/presets/`](../src/presets), one file
per group. Adding one is a single entry in the `preset_group!` block (Scatter
presets) or `plant_preset_group!` block (plant presets) for its group: the
constant, its key, name, group and description, the group array, the list
`all()` walks and the embedded asset are all generated from it. Then add its
shipped file, the text `to_yaml(None)` gives, as
`assets/scatters/<key>.plant_scatter.yml` or `assets/plants/<key>.plant.yml`,
and the new rows of the table above: tests fail until every constant has a
file that reads back equal to it, and until the table matches the constants.
