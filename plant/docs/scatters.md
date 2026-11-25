# Plants and Scatters

Two types describe grass:

- A **`PlantParams`** is one plant: a grass blade, a fern, a small bush. It
  holds everything that makes the plant read as that plant: its size, shape,
  colours, branches, how it gathers into tufts, its accent (a share of its
  stems that grow differently, such as the plumes of pampas grass), and the
  density it looks right at on its own.
- A **`PlantScatter`** is a component, the patch of ground that plants grow on.
  It says where (`size`, `ground`, `density_map`), which plants grow there and
  how densely (`plants`, a list of `ScatterPlant`), and what belongs to the
  whole field: a scale on every plant's density, colour variation, bare
  patches, the ground tint, toon shading and how much the wind moves it.

A field holds its plants itself, so two fields never share one; change one and
only that field changes. Start from one of the [presets](presets.md) — a
`PlantScatterPreset` for a whole field, a `PlantPreset` for one plant — and change what
you need.

## Make your own field

A field is a `PlantScatter` component: a size and a ground, and its
`options`, a `PlantScatterOptions` that says what grows and how it looks.
A Scatter preset is a constant; `PlantScatter::from_preset` gives a field of
it at the size you ask for. The plant is one level in, and its shape and
colours one more, in its `segment`. Colours are `Srgb`, sRGB from 0 to 1:

```rust
fn setup(mut commands: Commands) {
    let prairie = PlantScatterPreset::TALL_PRAIRIE.plant_scatter.plants[0].plant;
    let plant = PlantParams {
        height: Vec2::new(0.6, 1.2),
        segment: PlantSegment {
            tip_color: Srgb::new(0.75, 0.70, 0.35),
            stiffness: 0.6,
            ..prairie.segment
        },
        ..prairie
    };
    let options = PlantScatterPreset::TALL_PRAIRIE
        .plant_scatter
        .with_plants(vec![ScatterPlant::new(plant)])
        .with_spacing(0.15); // plants about 15 cm apart
    commands.spawn((
        PlantScatter::with_options(Vec2::new(120.0, 80.0), options),
        Transform::from_xyz(40.0, 2.0, -10.0),
    ));
}
```

`PlantParams`, `PlantSegment`, `PlantBranches`, `PlantAccent` and `ScatterPlant`
are `Copy`. `PlantScatterOptions` is not, so to reuse settings held in a
variable after `..options`, write `..options.clone()`.

## Change a field at runtime

A field is a component: change it through a query, and the grass changes on
the next frame. It is cheap enough to do every frame, for a season or
day/night blend or wet grass:

```rust
fn season(time: Res<Time>, mut fields: Query<&mut PlantScatter>) {
    let t = (time.elapsed_secs() * 0.1).sin() * 0.5 + 0.5;
    for mut field in &mut fields {
        for entry in field.options.plants.to_mut() {
            entry.plant.segment.tip_color = Srgb::new(0.34 + 0.3 * t, 0.56 - 0.1 * t, 0.14);
        }
    }
}
```

What a change costs is in proportion to what it touches. A mask or heightmap
is read from its image once, and again only when that image changes (a 4096 by
4096 mask takes milliseconds to read, once, not per frame). A change to a
field's plants or look rewrites only that field's entries, and the chunks,
masks and heightmaps are neither rebuilt nor uploaded again; a change to
colours alone is cheaper still. Moving a field sideways rewrites only its
entries. Setting a field to what it already was costs nothing.
`PlantStats::tables` counts it: `images_read`, `entries_rewritten`, `rebuilds`
and the expensive `layout_rebuilds`.

## How tall the grass grows

Plant height is set in three places, each for a different reach. Pick the
narrowest one that does what you need:

| To change the height... | Set | What it reaches |
| --- | --- | --- |
| of a plant in a field | `plant.height` of its `ScatterPlant` (and `plant.accent.height` for its accent) | That field |
| in places within a field | the density mask's green channel | Each texel of the mask |
| where the game cuts it, while it runs | `PlantEdit::mow`, or a `PlantBrush` with `PlantEditOp::Mow` | The grass under the edit, until restored or regrown |

[Presets](presets.md#sizes) lists each preset's tallest plant. Units 2 m tall
stand well above `MEADOW` (0.6 m at most); to keep grass under 0.4 m there as
well, scale the heights of the field's plants:

```rust
let mut field = PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(800.0));
for entry in field.options.plants.to_mut() {
    entry.plant.height *= 0.4 / 0.6;
    entry.plant.accent.height *= 0.4 / 0.6;
}
commands.spawn(field);
```

A preset such as `PlantScatterPreset::MEADOW` is a constant, and `from_preset` gives
a copy of its field; changing the copy never changes the constant.

## Mix plants

A field grows one or more plants, each a `ScatterPlant`: a `PlantParams`, a
`density_scale` on that plant's own density, and how much it grows in patches
(`clustering`). Every plant has a `density`, the plants per m² that suit it
on its own: 60 for `MEADOW_GRASS`, 40 for `SEED_HEADS`, 2 for `FERN`. In a
field it grows at its density times its `density_scale` (1 by default):

```rust
let options = PlantScatterPreset::SUMMER_TALL
    .plant_scatter
    .with_plants(vec![
        // 40 × 0.1 = 4 seed heads per m².
        ScatterPlant::new(PlantParams::SEED_HEADS).scaled(0.1),
        // 30 × 0.04 = 1.2 white flowers per m², in patches.
        ScatterPlant::new(PlantParams::WHITE_FLOWERS).scaled(0.04).clustered(0.8),
        // A plant of your own sets its own density: 0.8 purple flowers per m².
        ScatterPlant::new(PlantParams {
            density: 0.8,
            head_color: Srgb::new(0.70, 0.45, 0.95),
            ..PlantParams::WHITE_FLOWERS
        })
        .clustered(0.8),
        // 40 × 0.85 = 34 grass plants per m².
        ScatterPlant::new(PlantParams {
            tufts: 0.3, // gather the grass into tufts
            segment: PlantSegment {
                droop: 0.25, // tall blades arch over
                ..PlantParams::SUMMER_TALL_GRASS.segment
            },
            ..PlantParams::SUMMER_TALL_GRASS
        })
        .scaled(0.85),
    ]);
let wild = PlantScatterOptions {
    variation: PlantVariation {
        bare_share: 0.05,  // 5% of the ground bare
        patch_amount: 0.4, // colour drifts in patches several metres across
        dry_tips: 0.2,
        ..options.variation
    },
    ..options
};
let field = PlantScatter::with_options(Vec2::splat(100.0), wild);
```

How the plants share the field:

- **Densities add up.** The field grows the sum of its plants' densities per
  m² (`options.density()`), and each plant's share is its density over that
  sum. A plant scaled to 0 does not grow, and a field with no plants, or none
  with a density above 0, grows nothing.
- **The field's own `density_scale`** multiplies every plant's density at
  once (1 by default), so a whole mix gets sparser or denser without its
  shares changing. `with_density(per_m2)` sets it to reach a total, and
  `with_spacing(metres)` to reach a spacing.
- **Order matters for clustering.** Plants are chosen in list order, and the
  last plant with a density above 0 fills whatever the others leave, so its
  `clustering` has no effect. Put the plant that fills the gaps (usually the
  grass) last, and the plants that grow in patches before it.
- **At most 16 plants** a field. More are left out with a warning.

`PlantScatterPreset::SUMMER_WILD`, `AUTUMN_WILD` and `WILDFLOWER_MEADOW` are
ready-made examples.

### Build a mixed preset

A mix is not made by adding two whole fields together. `FOREST_FLOOR`, for
example, starts from `FOREST_MOSS` and grows ferns, leaf litter, clover and
broad leaves among its moss (see
[`src/presets/forest.rs`](../crates/procgena_plant/src/presets/forest.rs) in `procgena_plant`). Each plant has its own
`PlantParams`, so its shape, height, colours and tufts can differ from every
other plant's:

```rust
let mixed = PlantScatterOptions {
    density_scale: 1.0, // FOREST_MOSS grows its moss at 1.1
    plants: vec![
        // 2 × 0.7 = 1.4 ferns per m².
        ScatterPlant::new(PlantParams::FERN).scaled(0.7).clustered(0.5),
        // 20 × 0.7 = 14 rust-coloured broad leaves per m².
        ScatterPlant::new(PlantParams {
            height: Vec2::new(0.12, 0.30),
            width: 0.07,
            segment: PlantSegment {
                tip_color: Srgb::new(0.45, 0.22, 0.08),
                ..PlantParams::BROAD_LEAF.segment
            },
            ..PlantParams::BROAD_LEAF
        })
        .scaled(0.7)
        .clustered(0.4),
        // 30 × 0.2 = 6 pink flowers per m².
        ScatterPlant::new(PlantParams {
            head_color: Srgb::new(0.80, 0.30, 0.55),
            ..PlantParams::WHITE_FLOWERS
        })
        .scaled(0.2)
        .clustered(0.8),
        // 200 × 0.6 = 120 moss plants per m².
        ScatterPlant::new(PlantParams::MOSS).scaled(0.6),
    ]
    .into(),
    ..PlantScatterPreset::FOREST_MOSS.plant_scatter
};
```

The same mix as a `.plant_scatter.yml` file, with no Rust code at all. A
plant is either a plant preset's key or written out, starting from a plant
preset's `base`:

```yaml
base: forest_moss
plant_scatter:
  density_scale: 1.0
  plants:
    - plant: fern
      density_scale: 0.7
      clustering: 0.5
    - plant:
        base: broad_leaf
        height: [0.12, 0.30]
        width: 0.07
        segment: { tip_color: [0.45, 0.22, 0.08] }
      density_scale: 0.7
      clustering: 0.4
    - plant: { base: white_flowers, head_color: [0.80, 0.30, 0.55] }
      density_scale: 0.2
      clustering: 0.8
    - plant: moss
      density_scale: 0.6
```

### What belongs to a plant, and what to the field

What makes a plant read as that plant is the plant's: its size, shape,
colours, branches, head, tufts, accent, and the density it looks right at on
its own. What spreads over the whole patch of ground is the field's: patch
colour, dry tips, dead share, bare patches, the ground tint, toon bands,
`wind_scale`, and a `density_scale` on all its plants. A `ScatterPlant` adds
how much denser or sparser that plant grows here, and its clustering.

So a plant's density travels with it: drop `PlantParams::FERN` into any field
and it grows at its own 2 per m² unless the field scales it. `clustering`
changes where they grow, not how many. That is why `FOREST_FLOOR` scales
its moss to 109 per m² under the ferns instead of the carpet's 220.

## Tufts

`tufts` gathers a plant's stems into tufts, from 0 (spread evenly) to 1
(tight tufts with bare ground between), and `tuft_size` is the distance
between tufts. Tufted stems lean out from the tuft's centre. The number of
plants stays the same.

Tufts belong to the plant, so in a mix each plant gathers into its own tufts,
or none: tussock grass in tufts with clover spread evenly between them.

## Accents

An accent is a share of a plant's stems that grow differently from the rest:
the tall plumes rising out of a pampas tussock, a float on some kelp fronds,
a few great shards among a crystal cluster, seed stalks above blighted grass.
Accent stems grow in place of some of the plant's own stems, in its tufts, so
they stand out of the same clumps:

```rust
let pampas = PlantParams {
    tufts: 1.0,
    tuft_size: 1.8,
    accent: PlantAccent {
        share: 0.06, // six stems in a hundred are plumes
        height: Vec2::new(2.2, 3.0),
        width: 0.01,
        head_length: 0.3,
        head_width: 0.045,
        head_color: Srgb::new(0.94, 0.88, 0.74),
        ..PlantAccent::NONE
    },
    ..PlantScatterPreset::PAMPAS.plant_scatter.plants[0].plant
};
```

An accent has its own height, width, lean, translucency, segment and head. It
is never branched, and keeps the plant's root occlusion, sheen, stems per
plant and tufts. `PlantAccent::NONE` (share 0) is an accent that is off.

Use an accent when the stems belong to the plant; use another `ScatterPlant`
when they are a plant of their own (cattails among reeds, flowers among
grass), which can then cluster and tuft on its own.

## Plants that branch

Every plant is built from one part, a `PlantSegment`: a tapered, curved
strip with a shape (`profile`, `droop`, `twist`, `fold`), a stiffness and two
colours. A grass blade is one segment. A fern is a segment, its stalk, with
a row of smaller segments, its leaflets, along it. A bush is a stem with
twigs along it and leaves along the twigs.

A `PlantParams` has its own segment and two `levels` of `PlantBranches`:
`levels[0]` grows along the plant's own segment, `levels[1]` along each of
those. A level with `count: 0` is off, and `PlantBranches::NONE` is a level
that is off. `stems_per_plant` grows several stems from one root, spread
around it like the ring of fronds of a fern:

```rust
let fern = PlantParams {
    height: Vec2::new(0.5, 0.95), // the stalk's length
    width: 0.007,                 // the stalk; the leaflets make the frond wide
    stems_per_plant: 6,
    levels: [
        PlantBranches {
            count: 28,              // 14 pairs of leaflets
            arrangement: PlantArrangement::Opposite,
            attachment_start: 0.25, // a bare quarter of stalk at the base
            length: 0.3,            // the longest leaflet, a share of the stalk
            length_envelope: 0.3,   // longest a third of the way up
            width: 0.22,            // a share of the leaflet's own length
            angle: 0.22,
            segment: PlantSegment {
                profile: 0.3,
                droop: 0.25,
                ..PlantBranches::NONE.segment
            },
            ..PlantBranches::NONE
        },
        PlantBranches::NONE,
    ],
    ..PlantParams::FERN
};
```

The same plant as a `.plant.yml` file. Levels are listed in order, each
optional setting over the base plant's level:

```yaml
name: My fern
base: fern
plant:
  height: [0.5, 0.95]
  width: 0.007
  stems_per_plant: 6
  segment: { droop: 0.55, tip_color: [0.20, 0.34, 0.10] }
  levels:
    - count: 28
      arrangement: opposite
      attachment_start: 0.25
      length: 0.3
      length_envelope: 0.3
      width: 0.22
      angle: 0.22
      segment: { profile: 0.3, droop: 0.25, tip_color: [0.22, 0.44, 0.14] }
```

Every branched plant preset (`FERN`, `SALAL`, `MATURE_BRACKEN` and the others
in [Presets](presets.md)) has its settings written out this way in its file
in [`assets/plants`](../crates/procgena_plant/assets/plants), to copy from.

What to know about branched plants:

- **Density counts plants.** A fern with six fronds is one plant. A fern or
  bush covers far more ground than a blade, so its own `density` is small:
  `FERN` grows 2 per m², `SALAL` 1.5 and `SAPLING` 0.6.
- **They cost more than grass.** A fern frond is a few hundred vertices
  close up, a blade 15. Branched plants thin with distance only at the far
  end, fading out, not at mid range as grass does. Measure a dense mix with
  the `bench` example's `--field` option.
- **They have their own buffer.** `PlantSettings::plant_capacity` holds the
  visible stems of every kind of branched plant in use, shared by how many
  each kind asked for the frame before. A warning names the setting when it runs
  out.
- **At most 32 kinds at once.** Plants that branch alike (the same `count` on
  each level) are drawn together as one kind, whatever their sizes and
  colours. A 33rd kind in use is left out with a warning and counted in
  `PlantStats::tables.branched_dropped`. A plant with more segments than one
  draw holds (about 1000 vertices close up) is left out the same way.
- **No head.** A plant with `levels[0]` on has no seed head or flower; a
  file that sets both is an error.
- **`levels[1]` needs `levels[0]`.** In a file, `levels[1]` under a
  `levels[0]` with `count: 0` is an error; in code, that `levels[1]` is not
  drawn.

## Load plants and fields from files, and edit them live

Every preset ships as a file: plants in
[`assets/plants`](../crates/procgena_plant/assets/plants) as `.plant.yml`, Scatters in
[`assets/scatters`](../crates/procgena_plant/assets/scatters) as `.plant_scatter.yml`. Copy one
into your game's assets and load it like any asset: a `.plant.yml` loads as
a `PlantPreset`, a `.plant_scatter.yml` as a `PlantScatterPreset`. The
loaded preset's `key` is the file's name without its extensions, and its
`group` is the file's `group`, or `"File"`.

Every preset is also an asset that ships inside the crate, so a game can load
one without copying anything (the names are the preset keys):

```rust
let summer: Handle<PlantScatterPreset> =
    assets.load("embedded://procgena_plant/scatters/summer_tall.plant_scatter.yml");
let fern: Handle<PlantPreset> = assets.load("embedded://procgena_plant/plants/fern.plant.yml");
```

A Scatter file holds the plants and look, not where the Scatter is, so its
A file holds a Scatter's `options`; `size`, `ground` and `density_map` are
the component's own. Copy a loaded Scatter's options onto yours when it
loads, and again whenever the file changes:

```rust
#[derive(Component)]
struct ScatterFile(Handle<PlantScatterPresetAsset>);

fn follow_scatter_files(
    mut events: MessageReader<AssetEvent<PlantScatterPresetAsset>>,
    presets: Res<Assets<PlantScatterPresetAsset>>,
    mut fields: Query<(&ScatterFile, &mut PlantScatter)>,
) {
    for event in events.read() {
        let (AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id }) = *event
        else {
            continue;
        };
        let Some(PlantScatterPresetAsset(preset)) = presets.get(id) else { continue };
        for (file, mut field) in &mut fields {
            if file.0.id() == id {
                field.options = preset.plant_scatter.clone();
            }
        }
    }
}
```

A file has up to five top-level keys: `base`, `name`, `description`,
`group`, and its settings under its kind, `plant:` or `plant_scatter:`.
Every setting is optional, and a missing one comes from `base`, a preset's
key: a `PlantScatterPreset`'s in a Scatter file (`meadow` when not given), a
`PlantPreset`'s in a plant file or a plant written inside a Scatter file
(`meadow_grass` when not given). A variant lists only what differs: groups
of settings such as `segment` or `variation` merge setting by setting. A
Scatter file's `plants` list, when given, replaces the base's plants whole; a
plant's `levels` list applies entry by entry, the first over the base's first
level. Colours are sRGB, written as a list, `[0.70, 0.72, 0.74]`, or as the
hex string a colour picker copies, in quotes: `"#b3b8bd"` (without quotes,
YAML reads `#` as the start of a comment). The `name` and `description`
default to the key and to nothing, and the `group` to `"File"`.

```yaml
name: Wild edge
base: summer_tall
plant_scatter:
  variation:
    bare_share: 0.05
  plants:
    - plant: seed_heads
      density_scale: 0.1
    - plant: { base: white_flowers, head_color: "#c08cf2", density: 1.2 }
      clustering: 0.8
    - plant: { base: summer_tall_grass, tufts: 0.3 }
      density_scale: 0.85
```

With Bevy's `file_watcher` feature, saving the file changes the grass in the
running game. A broken file logs what is wrong (an unknown preset key lists
the valid ones, and an unknown setting is named by its path, such as
`plant_scatter.variation.dry_tip`) and leaves the grass as it was.
`PlantParams` and `PlantScatter` also read and write a file's text in code
with `from_yaml` and `to_yaml`, and implement serde's `Serialize` and
`Deserialize` with the same settings as the part under the file's kind. To
make a file a named preset in this crate instead, see
[Adding a preset](presets.md#adding-a-preset-to-the-crate).

## Reference

Units are metres unless stated.

**`PlantParams`** (a `ScatterPlant`'s `plant`):

| Setting | Meaning |
| --- | --- |
| `default_seed` | The seed used when the plant is built on its own. Not used yet: changing it changes nothing drawn |
| `density` | Plants per m² that suit it on its own: 60 for meadow grass, 2 for ferns; a field scales it |
| `height: Vec2` | Shortest and tallest plant: the length of its own segment |
| `width` | Width of its own segment at the root; each plant varies ±25% |
| `lean` | How far the tip leans at rest, as a share of the height |
| `stems_per_plant` | Stems from one root, spread around it, 1 to 8; 1 for grass |
| `segment` | The plant's own segment (`PlantSegment`, below): a blade, a fern's stalk |
| `levels` | Two levels of `PlantBranches` (below); `levels[0]` on the plant's segment, `levels[1]` on each of those |
| `root_occlusion` | How much darker the plant is near the ground, 0–1 |
| `translucency` | How much sunlight shows through a backlit segment, 0–1 |
| `specular` | Rough sheen on the upper half of the plant, 0–1 |
| `head_length`, `head_width`, `head_color` | A seed head or flower at the top: its share of the length, width and colour. It turns toward the camera. Plants without branches only |
| `tufts`, `tuft_size` | How strongly its stems gather into tufts (0–1), and the distance between tufts. The plant count stays the same |
| `accent` | Stems that grow differently (`PlantAccent`, below) |

**`PlantAccent`** (a plant's accent):

| Setting | Meaning |
| --- | --- |
| `share` | Share of the plant's stems that are accents, 0–1; 0 is off |
| `height`, `width`, `lean`, `translucency` | As the plant's, for accent stems |
| `segment` | What each accent stem looks like |
| `head_length`, `head_width`, `head_color` | A head on each accent stem; `head_length` 0 has none |
| `PlantAccent::NONE` | An accent that is off |

**`PlantSegment`** (a plant's own segment, each level's, and an accent's):

| Setting | Meaning |
| --- | --- |
| `profile` | Outline, 0 (narrowing to a point, like grass) to 1 (oval, like a leaf) |
| `stiffness` | Resistance to wind, 0 (soft) to 1 (stiff). On a branch, how much it sways on its own |
| `droop` | How far it arches over and hangs, 0–1; a plant's tall segments the most. A main stem also takes 1–2, which lowers every stem's tip until, at 2, it rests on the ground in an arch, like a bramble cane (it needs some `lean`) |
| `twist` | Twist along it, in turns at the tip; each its own way |
| `fold` | How far it folds along the midrib, 0–1; branches rise along the V |
| `root_color`, `tip_color` | Colours along it |

**`PlantBranches`** (one level):

| Setting | Meaning |
| --- | --- |
| `count` | Children on each parent segment, 0 (off) to 32 |
| `arrangement` | `PlantArrangement::Opposite` (in pairs; an odd count rounds up) or `Alternate` |
| `attachment_start` | Share of the parent left bare at its base, 0 to below 1 |
| `length` | The longest child, as a share of its parent's length |
| `length_envelope` | Where along the parent children are longest, 0 (base) to 1 (tip) |
| `width` | Each child's widest width, as a share of its own length |
| `angle` | 0 at right angles to the parent, 1 along it toward its tip |
| `spread` | How far each child turns round the parent from the last, 0–1 in half turns: 0 flat like a fern, 0.5 crossed pairs like a nettle. Above 0, leaves face the sky |
| `segment` | What each child looks like |
| `PlantBranches::NONE` | A level that is off |

**`ScatterPlant`:**

| Item | What it does |
| --- | --- |
| `plant` | What it looks like: a `PlantParams`. In a file, a plant preset's key or the plant's settings |
| `density_scale` | Multiplies the plant's own `density` in this field: 1 as it is, 0.5 half as dense, 0 removes it |
| `clustering` | 0: scattered evenly. 1: grows in patches about a metre across. Unused on the last plant |
| `ScatterPlant::new(plant)` | A scattered plant at its own density (`const`) |
| `.scaled(density_scale)` | The same plant, denser or sparser |
| `.clustered(amount)` | The same plant, growing in patches |
| `density()` | Its plants per m², before the field's own scale |

**`PlantScatter`:**

| Setting | Meaning |
| --- | --- |
| `size`, `ground`, `density_map` | Where it grows; see [Terrain](terrain.md) and [Editing](editing.md). Not in files |
| `plants` | The plants that grow there, a list of `ScatterPlant` (up to 16) |
| `density_scale` | Multiplies every plant's density, 1 by default, keeping the mix |
| `wind_scale` | How much the wind moves this field: 1 as the wind says, 0 still (in the lee of a wall) |
| `default_seed` | Where the blades fall, used when the field's own `PlantScatter::seed` is `None`. Two fields with the same seed that overlap grow the same blades there |
| `ground_tint` (`GroundTint`: `color`, `amount`) | Blend the lowest third of each blade toward the ground's colour, 0–1 |
| `toon_bands` | Flat toon shading with this many bands; 0 is physically based |
| `variation.patch_color`, `patch_amount`, `patch_size` | Large patches where blades drift toward another colour |
| `variation.dry_tips` | How far the tops of blades dry toward straw, 0–1 |
| `variation.dead_share` | Share of blades that are dead and straw-coloured |
| `variation.bare_share`, `bare_patch_size` | Share of the ground left bare, in patches whose edges thin out, and their size |

Every variation setting (a segment's `twist` through `bare_share`, tufts and
accents) is off at 0, and costs nothing while no field uses it.

| Method | What it does |
| --- | --- |
| `PlantScatter::new(size, plants)` | A field of these plants, with the `MEADOW` field's look |
| `PlantScatter::from_preset(&preset, size)` | A preset's field at this size |
| `density()` | Plants per m² of all its plants together at full density; a plant with several stems counts once |
| `spacing()` | Average distance between neighbouring plants at full density (`1 / √density()`) |
| `with_spacing(metres)`, `with_density(per_m2)` | The same field and mix, denser or sparser (sets `density_scale`) |
| `with_plants(list)` | The same field with these plants |
| `from_yaml(text)`, `to_yaml()` | Read or write the plants and look as `.plant_scatter.yml` text (`PlantParams` has the same, as `.plant.yml`) |

**Presets** (`PlantScatterPreset` and `PlantPreset`):

| Item | What it does |
| --- | --- |
| `key`, `name`, `description`, `group` | The constant's name in lower case (`"meadow"`), a display name, one paragraph, and the group it is listed in |
| `plant_scatter` / `plant` | The preset's Scatter or plant |
| `PlantScatterPreset::MEADOW`, `PlantPreset::FERN`, … | Each preset, as a constant; `PlantParams::FERN` is the same plant |
| `GROUPS`, `all()`, `named(key)` | Every group of presets, every preset, and a preset by its key |
| `from_yaml(key, text)`, `to_yaml(base)` | Read or write a preset as its file; with a `base` preset, the file holds only what differs from it |
