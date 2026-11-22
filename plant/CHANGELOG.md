# Changelog

All notable changes to procgena_plant, newest first. Until 1.0 any release may
break the API; each breaking change below says what to write instead.

## Unreleased

Plants and fields. A `GrassPlant` is now everything that makes one plant read
as that plant, its tufts and its accents included, and a `GrassField` holds
its own plants and look instead of pointing at a shared `GrassStyle` asset.
Each plant has the density it looks right at on its own, and a field grows
any mix of up to 16 plants, each at its own density scaled up or down, with
its own tufts. A plant can have an accent: a share of its stems that grow differently,
such as the plumes of pampas grass or the floats on kelp. The presets are
split into 40 `FieldPreset`s and 26 `PlantPreset`s, each with a key, a
display name, a description and a group, and the files into `.field.ron`
and `.plant.ron`.

**Breaking, and what to write instead**

| Before | Now |
| --- | --- |
| `GrassStyle`, `Assets<GrassStyle>`, `GrassField::style` | Gone: `GrassField` holds its plants and look itself |
| `GrassField::new(size, GrassStyle::X, &mut styles)`, `GrassField { style: styles.add(GrassStyle::X), .. }` | `GrassField::from_preset(&FieldPreset::X, size)` |
| `GrassStyle::X` (any preset) | `FieldPreset::X`; its `.field` is the `GrassField`, with size 0 |
| `style.variation`, `ground_tint`, `toon_bands`, `spacing()`, `with_spacing`, `with_density` | The same on `GrassField`; `with_density` and `with_spacing` set its new `density_scale` |
| `style.density` (plants per m²) | Each plant's own `GrassPlant::density`, times its `FieldPlant::density_scale`, times the field's `density_scale`; read the total with `GrassField::density()`. To keep a style's density, set the plants' scales so their densities add up to it |
| `style.plant` | A plant in `field.plants`; a one-plant preset's is `field.plants[0].plant`, and a mixed preset's main plant is the last |
| `GrassSpecies { plant, share, clustering }`, `GrassSpecies::new(plant, share)` | `FieldPlant { plant, density_scale, clustering }`, `FieldPlant::new(plant).scaled(density_scale)`: the plant grows at its own density times the scale, so a species at share `s` in a style of density `d` becomes a scale of `s × d / plant.density` |
| `style.species` beside `style.plant`, `with_species(list)` | `field.plants`, every plant the field grows, `with_plants(list)`: list the species first and the old main plant last. The densities add up, and the last plant fills what the others leave |
| `variation.tufts`, `variation.tuft_spacing` (and `tufts`, `tuft_spacing` in a file) | `GrassPlant::tufts` and `GrassPlant::tuft_size`, on each plant that gathers into tufts |
| `GrassFieldOverrides { wind_scale, .. }` | `GrassField::wind_scale` |
| `GrassFieldOverrides::tint`, `height_scale` | Removed: change the field's plants' colours or `height` |
| `GrassFieldOverrides::density_scale` | `GrassField::density_scale` |
| `GrassStyle::preset(name)`, `GrassStyle::presets()`, `GrassStyle::GENERAL` and the other groups | `FieldPreset::named(key)`, `FieldPreset::all()`, `FieldPreset::GENERAL`, and `FieldPreset::GROUPS` for every group |
| The `PLANTS` presets: `SEED_HEADS`, `WHITE_FLOWERS`, `YELLOW_FLOWERS`, `CLOVER`, `BROAD_LEAF` | Plant presets (`PlantPreset::CLOVER`, the same plant as `GrassPlant::CLOVER`). For a field of one: `GrassField::new(size, vec![FieldPlant::new(GrassPlant::CLOVER)])` |
| `GrassField::default()` waits for a style | `GrassField::default()` is the `MEADOW` field, 0 m across |
| `FarGrass::new(&style)` | `FarGrass::new(&field)` |
| `.grass.ron` files, `GrassStyleLoader`, `assets/grass_styles` | `.field.ron` files (`GrassFieldLoader`, which loads a `FieldPreset`) and `.plant.ron` files (`GrassPlantLoader`, a `PlantPreset`); `assets/grass_fields` and `assets/grass_plants` |
| A field with a `Handle<GrassStyle>` follows its file as it reloads | Copy the loaded `FieldPreset`'s `field` onto the field on its `AssetEvent`; see [Plants and fields](docs/fields.md#load-plants-and-fields-from-files-and-edit-them-live) |
| `GrassStyleFilePlugin` | `GrassFilePlugin` |
| `GrassStyleFileError`, `UnknownBase` | `GrassFileError`, `UnknownField` (a field preset key) and `UnknownPlant` (a plant preset key) |
| `embedded://procgena_plant/styles/<name>.grass.ron` | `embedded://procgena_plant/fields/<key>.field.ron` and `embedded://procgena_plant/plants/<key>.plant.ron` |
| `GrassStyle::from_ron`, `to_ron` | `GrassField::from_ron`, `to_ron` (the plants and look; not `size`, `ground` or `density_map`) and `GrassPlant::from_ron`, `to_ron` |
| `TableStats::species_dropped` | `TableStats::plants_dropped` |
| `BLESS_STYLES=1 cargo test` | `BLESS_PRESETS=1 cargo test` |
| The `styles` example; `--styles` (`walk`), `--style` (`bench`, `variation`) | The `fields` example; `--fields`, `--field` |
| `docs/styles.md` | [Plants and fields](docs/fields.md) |

So a field built from a style, such as

```rust
let style = GrassStyle {
    density: 80.0,
    species: vec![GrassSpecies::new(GrassPlant::CLOVER, 0.1)].into(),
    ..GrassStyle::MEADOW
};
commands.spawn(GrassField { size: Vec2::splat(64.0), style: styles.add(style), ..default() });
```

becomes

```rust
// 8 clover (100 per m² on its own × 0.08) and 72 grass (60 × 1.2) per m².
commands.spawn(GrassField {
    plants: vec![
        FieldPlant::new(GrassPlant::CLOVER).scaled(0.08),
        FieldPlant::new(GrassPlant::MEADOW_GRASS).scaled(1.2),
    ]
    .into(),
    ..GrassField::from_preset(&FieldPreset::MEADOW, Vec2::splat(64.0))
});
```

In a file, the field's settings stay at the top level, and every plant, the
main one included, is an entry of `plants`, named by a plant preset's key or
written out:

```ron
(
    base: "MEADOW",
    plants: [
        (plant: "CLOVER", density_scale: 0.08),
        (plant: "MEADOW_GRASS", density_scale: 1.2),
    ],
)
```

A file's field-level `density` is now `density_scale`, and a plant written
out may set its own `density`.

A file's `tufts` and `tuft_size` go on the plant that gathers into tufts.
The `min_width_px` setting and the old setting names that `.grass.ron`
accepted (`head`, `dead_blades`, `bare_patches` and the like) are gone; a
file that uses one gets an error naming it.

Fields look different where tufts were shared: before, every species grew in
the style's tufts; now each plant gathers into its own tufts or none. The
pampas plumes, kelp floats, great crystal shards and the seed stalks of
`BLIGHTED` and `FROSTED_MEADOW` are now accents of their plant, so they still
stand out of its tufts, but they no longer grow in patches. Untufted mixes
look as before.

**Added**

- `GrassAccent` and `GrassPlant::accent`: a share of a plant's stems that
  grow differently, with their own height, width, lean, translucency, segment
  and head, in place of the plant's own stems and in its tufts. `PAMPAS`,
  `KELP_BED`, `CRYSTAL_SPIRES`, `BLIGHTED` and `FROSTED_MEADOW` use them. See
  [Accents](docs/fields.md#accents).
- `FieldPreset` and `PlantPreset`: every preset with its `key`, `name`,
  `description` and `group`, as assets, with `GROUPS`, `all()` and
  `named(key)`. New plant presets: `MEADOW_GRASS`, `SUMMER_TALL_GRASS`,
  `LAWN_GRASS`, `WHEAT` and `MARRAM`.
- `.plant.ron` files, and `.field.ron` files whose plants name a plant preset
  or are written out; both may set a `name` and a `description`.
- `GrassPlant::from_ron`, `to_ron`, `from_json` and `to_json`.
- `GrassPlant::density`, the plants per m² a plant looks right at on its
  own, set on every plant preset (60 for `MEADOW_GRASS`, 2 for `FERN`; see
  [Presets](docs/presets.md#sizes)); `FieldPlant::density_scale` and
  `GrassField::density_scale`, which scale it; `FieldPlant::density()` and
  `GrassField::density()`, the plants per m² that result.
- `MAX_PLANTS`, the 16 plants a field grows at most.

**Changed**

- `GrassSample::height` is the typical height of every plant in the field,
  each by its share, accents included; before, the main plant's alone.
- `FarGrass::new` takes its colour from every plant the field grows (the
  first `MAX_PLANTS`), each by its share.

Ferns and bushes. A plant is no longer one blade but a stalk with up to two
levels of branches, so `GrassPlant::FERN` is a ring of fronds with fourteen
pairs of leaflets each, and new understory presets grow salal, huckleberry,
Oregon grape, nettles, saplings and head-high bracken over moss. Branching
plants are built on the GPU like grass, sway branch by branch, go through
every pass and shadow, and cost nothing while no field has them. Grass alone
draws as before.

**Breaking, and what to write instead**

`GrassBlade` is now `GrassPlant`, and its shape and colours have moved into
`GrassPlant::segment`, a `GrassSegment`:

| Before | Now |
| --- | --- |
| `GrassBlade { .. }` | `GrassPlant { .. }` |
| `GrassBlade::FERN` (and every other plant constant) | `GrassPlant::FERN` |
| `style.blade`, `species.blade` | A `FieldPlant`'s `plant` (see the table above) |
| `blade.height`, `width`, `lean`, `root_occlusion`, `translucency`, `specular`, `head_length`, `head_width`, `head_color` | the same fields of `GrassPlant` |
| `blade.stiffness`, `droop`, `twist`, `fold`, `root_color`, `tip_color` | `plant.segment.stiffness` and so on |
| `GrassSpecies::new(blade, share)` | `FieldPlant::new(plant).scaled(density_scale)` (see the table above) |

So a literal such as

```rust
GrassBlade { tip_color: TIP, droop: 0.3, ..GrassBlade::CLOVER }
```

becomes

```rust
GrassPlant {
    segment: GrassSegment { tip_color: TIP, droop: 0.3, ..GrassPlant::CLOVER.segment },
    ..GrassPlant::CLOVER
}
```

`GrassPlant` has new fields: `stems_per_plant` (1 for grass), `segment`,
`levels` (`[GrassBranches::NONE; 2]` for grass), `tufts` and `tuft_size`
(above) and `accent` (`GrassAccent::NONE`). `GrassSegment` has one setting a
blade did not, `profile` (0 for grass). A `GrassPlant` literal that names
every field needs them added; one written with `..GrassPlant::X` does not.

In a `.plant.ron` file, or a plant written out in a `.field.ron` file, the
plant's shape and colours go under `segment: (...)`:
`tip_color: (0.7, 0.7, 0.4), height: (0.3, 0.6)` becomes
`height: (0.3, 0.6), segment: (tip_color: (0.7, 0.7, 0.4))`.

`FERN` and `BRACKEN` branch now, and a field's density counts plants, not
stems: one fern of six fronds is one plant. A field that mixed in
`GrassBlade::FERN` at a share chosen for blades, such as 0.2, now grows a
fern every few centimetres; `GrassPlant::FERN` now grows at its own 2 per
square metre, so scale it rather than giving it a share. `FOREST_FERNS`, `FOREST_FLOOR`
and `FOREST_BRACKEN` are retuned for this and look different: real fronds
where there were wide blades.

`GrassSettings` has a new field, `plant_capacity`. A literal that names
every field without `..GrassSettings::HIGH` (or another tier) needs
`plant_capacity: 150_000` added.

`GpuStats` has a new field, `stems`, and `TableStats` one,
`branched_dropped`. Code that builds them in full (tests, usually) adds
`stems: [0; 3]` and `branched_dropped: 0`.

**Added**

- `GrassPlant`, `GrassSegment`, `GrassBranches` and `GrassArrangement`: a
  plant is its own segment and up to two levels of smaller segments along
  it, each level with its count, `Opposite` or `Alternate` arrangement, bare
  base, length, where along the parent the children are longest, width,
  angle and `spread` (how far each child turns round its parent: 0 flat
  like a fern, 0.5 crossed pairs like a nettle). `stems_per_plant` grows
  several stems from one root. See
  [Plants that branch](docs/fields.md#plants-that-branch).
- The `UNDERSTORY` presets: `SALAL`, `SAPLING`, `HUCKLEBERRY`,
  `OREGON_GRAPE`, `NETTLE` and `MATURE_BRACKEN`, one plant each over moss,
  and `FOREST_UNDERSTORY` with all of them, ferns and fallen leaves. The
  plants are also `GrassPlant` and `PlantPreset` constants of the same names.
- `BRAMBLE`, in the `UNDERSTORY` presets: bramble canes that arch over and
  root their tips in the ground, with clusters of leaves on short side
  shoots, over moss. The plant is also `GrassPlant::BRAMBLE`.
- A main stem's `GrassSegment::droop` now goes from 1 to 2, lowering every
  stem's tip until, at 2, it rests on the ground in an arch. Values up to 1
  look as before.
- The `SHRUBS` presets: `BROOM`, clumps of broom (a short brown trunk under
  tall, upright green rods) through meadow grass. The plant is also
  `GrassPlant::BROOM`.
- The size table in [Presets](docs/presets.md#sizes) gives a fern or bush's
  real height at rest, branches included, instead of its main stem's length.
- `GrassSettings::plant_capacity` and `GrassQuality::plant_capacity()`: the
  visible stems of branched plants each camera holds, shared between the
  kinds of branched plant in use by how many each asked for the frame before;
  nothing is allocated while there are none. `GpuStats::stems`,
  `total_stems()` and the `grass/stems` diagnostic count them, and
  `TableStats::branched_dropped` counts plants left out because more than
  32 kinds were in use or one was too big to draw.
- The `bench` example's `--field <preset>` measures a preset's cost, such
  as `--field forest_understory`.
- The `showcase` example has a sixth page, the understory presets.
- `GrassField::to_json` and `GrassField::from_json` (and the same on
  `GrassPlant`): the `.field.ron` and `.plant.ron` formats as JSON, with the
  same optional fields, `base` presets, hex colours and checks, for apps that
  save and load fields and plants.

**Changed**

- A plant or field file now rejects only settings that cannot be drawn. It accepts
  `GrassPlant::height` in either order (stems still fall between the two),
  a head length on a branched plant (the head stays hidden), and a second
  level of branches while the first is off (it stays hidden), so an editor
  can save whatever its user left behind. Heights in either order also draw
  correctly in code: before, a shortest above the tallest broke the
  field's height limits.
- Branched plants cast their shadows from simpler leaves: in the shadow map
  each leaf with nothing growing from it is one triangle, as wide at its base
  as it needs to cover the same ground as the curved leaf, while stalks and
  twigs keep their shape. A fern's near frond casts from 101 vertices instead
  of 269, a huckleberry stem from 249 instead of 505. The shadows' outlines
  change slightly; the plants themselves do not.
- Branched plants at mid detail keep fewer, wider leaves. Each parent keeps
  about half its children and at most 8, at both levels (before, the first
  level was halved and every leaf on the survivors kept), spread evenly and
  widened by how many they stand in for, up to 2.4 times. Their leaves are
  single triangles. A fern frond at mid detail is 33 vertices instead of 51,
  a huckleberry stem 69 instead of 181. Orthographic cameras, which draw at
  `GrassSettings::orthographic_lod` (mid detail by default), see the change
  close up.

- At far detail a branched stem keeps only its 6 largest parts, each a
  single triangle, from the level with the most total area (a fern's leaves,
  a huckleberry's leaflets, a broom's twigs). Stalks and twigs that do not
  carry the plant's look are culled, and the survivors widen to cover for
  them. Before, each stem was its stalk alone, widened to the plant's width.
  A far stem is 18 vertices for every kind, instead of 5.
- `GrassSettings::forced_lod` draws every blade and branched plant at one
  level of detail, at any distance and in every camera, for previews and
  tools. `None`, the default, keeps choosing by distance. A `GrassSettings`
  written out in full needs `forced_lod: None`.

## 0.20.6 (October 24, 2026)

Strategy cameras, terrain built from code, and no more blinking blades.
`GrassCamera::radius_from_ground` draws grass across the ground a high
camera sees; `GrassField::with_ground_from_fn` and `with_density_from_fn`
grow a field on terrain a game builds itself; `GrassLightFrame` and
`GrassWindFrame::advance` let a game drive grass from a sun or wind of its
own. Blades no longer flash the clear colour behind a depth prepass or
white in a low sun's glint, gusts no longer show a grid from above, and an
MSAA sample count the GPU cannot do no longer turns the window black. The
[web demo](https://gemi.github.io/bevy-grass/) has every setting of the
crate in its panel and copies what you tune as Rust.

**Breaking, and what to write instead**

`GrassSettings` has a new field, `mid_range_start`. A struct literal that
names every field without `..GrassSettings::HIGH` (or another tier) needs
`mid_range_start: 0.1` added.

`GrassWindFrame` no longer implements Bevy's `ShaderType`, and `GrassCamera`
and `NoGrass` no longer implement `ExtractComponent`. The public types are
plain data now, so a change to the shaders' wind layout or to Bevy's
extraction API no longer breaks them. Nothing changes for a game that writes
them; the renderer copies them into the render world itself. A game that
uploaded `GrassWindFrame` to a shader of its own copies its fields into a
`ShaderType` struct of its own.

`GrassPlugin` has a new field, `lights` (`true` by default). A literal that
names every field without `..default()` needs `lights: true` added.

`GrassCamera` has a new field, `radius_from_ground` (`false` by default). A
literal that names every field without `..default()` needs
`radius_from_ground: false` added.

`GrassFieldOverrides` has a new field, `wind_scale` (1 by default). A literal
that names every field without `..default()` needs `wind_scale: 1.0` added.

`GrassWindFrame::gust_offset` and `previous_gust_offset` now wrap at 256
repeats of the gust texture instead of 1, for the second layer of gusts (see
Fixed). A custom wind that calls `GrassWindFrame::advance`, or leaves the
offsets at zero, needs no change. One that wraps the offsets itself wraps
them at 256 (`rem_euclid(Vec2::splat(256.0))`); wrapped at 1, the gusts
jump each time they cross a repeat.

**Added**

- The web demo's panel has every public setting of the crate, built from
  the types' `Reflect` data and their doc comments: `GrassSettings`,
  `GrassStyle` with its species, `GrassWind` and its presets, `FarGrass`,
  `GrassFieldOverrides`, `GrassRegrowth`, `GrassInteractor`, `GrassCamera`,
  a `GrassBrush` on the left mouse button, `GrassDebug` and `GrassStats`.
  Each section copies as Rust (only what differs from its tier or preset),
  the style also as a `.grass.ron`, and a `.grass.ron` can be pasted in. The
  address keeps the changes, so a link shares them. It replaces the
  wind-strength slider.
- `FarGrass::with_sheen(sheen)`, like `with_fade` and `with_area`: a struct
  literal with `..FarGrass::new(&style)` does not compile outside the crate,
  so setting the sheen took a variable.
- `FarGrass::uniform(&wind, &settings)` and `FarGrass::GUSTS`: what a
  terrain material of the game's own needs to call `grass_far_color` from
  its shader, the far grass `FarGrassMaterial` draws. Before, its uniform
  could not be built outside the crate. The new `custom_terrain` example is
  a material with its own shader doing it.
- `GrassFieldOverrides::wind_scale`: one field moves less in the shared
  wind, or more: a courtyard, a greenhouse, a windswept ridge. The gusts
  stay in step with the fields around it. The tip flutter scales with it.
- `GrassCamera::radius_from_ground`, for strategy and city-builder cameras:
  the radius is measured from the ground below the camera, so grass reaches
  the same distance across the ground at any zoom, thinned for the camera's
  height so the number of blades, and the cost, stay about the same. The
  new `rts_camera` example and [Strategy cameras](docs/settings.md#strategy-cameras)
  show the setup and its costs. Off by default; nothing changes without it.
- `GrassField::with_ground_from_fn(&mut images, cell_size, height)` and
  `GrassField::with_density_from_fn(&mut images, cell_size, density)`: a
  field on terrain built from code, from the function that built it. Each is
  sampled once into a heightmap or a mask, so terrain in chunk meshes, or
  meshes kept only on the GPU, needs no heightmap file and no field per
  chunk. A height that is not finite leaves no ground, for coastlines. The
  new `code_terrain` example is an island in 16 chunks with one field.
- `GpuStats::total_blades()`, the blades of every level of detail together:
  `stats.gpu.total_blades() == 0` after the first reading means nothing was
  drawn. The README's new "Check it works" paragraph points to it and to the
  debug gizmos, which show a field buried under hills.
- A warning, once per camera, when a perspective camera has had grass fields
  in view for two seconds and all of them were beyond its grass radius, so
  it drew nothing. The radius is a distance from the camera itself, so a
  strategy camera high above the ground drew no grass at all, and nothing in
  the log said why. The check is one box test per camera and field a frame.
- `GrassWindFrame::advance(delta, gust_speed)` scrolls the gusts along the
  frame's direction, as the built-in wind does, so a custom wind source no
  longer has to work out the gust texture's scrolling itself. The
  `custom_wind` example uses it.
- `GrassLightFrame`, the light grass follows this frame (the shadow-casting
  light's direction and the sun's), written by the new built-in
  `GrassLightPlugin` from Bevy's directional lights in
  `GrassLightSystems::Sample`. A game whose sun is not a `DirectionalLight`
  builds `GrassPlugin { lights: false, .. }` and writes the frame itself, as
  it can the wind. The renderer reads the frame instead of querying Bevy's
  lights, so Bevy's light types are read in one place.

**Fixed**

- A camera asking for an MSAA sample count the GPU does not support no
  longer turns the window black. Bevy stops rendering at the first
  unsupported count, and WebGPU supports only 1 and 4 samples, so
  `Msaa::Sample2` and `Msaa::Sample8` stopped every browser. Grass now logs
  one warning naming the camera and uses the most samples below it that the
  device has (8x becomes 4x, 2x becomes off). The web demo offers only Off
  and 4x.
- With a depth prepass (which SSAO, depth of field, outlines and contact
  shadows need), a few blades a frame flashed the clear colour, often a
  pale sky blue, all over the screen. The main pass draws a blade only where
  its depth equals the prepass's, and the two shaders were free to round a
  blade's position differently; where the main pass came out a hair
  farther, neither the blade nor the ground behind it was drawn. Both now
  mark the position `@invariant`, so they place blades identically. In a
  game scene, 21 such pixels a frame became none. The goldens differ by a
  mean of at most 0.7 of 255 as blade edges move by a rounding step.
- Blades with sheen (`GrassBlade::specular`, 0.3 in `MEADOW`) flashed white
  for a frame, a few at a time all over the screen, when the sun was low and
  in front of the camera: a blade a pixel or two wide that swayed into the
  sun's mirror angle caught a glint hundreds of times its usual brightness,
  which bloom spread. The sheen now widens where a blade's normal turns
  within a pixel (specular antialiasing), and a blade's reflectance stops at
  0.3, below where Bevy's grazing reflectance reaches full. Looking into a
  low sun, `MEADOW` flashes about 0.2 pixels a frame instead of 36. Elsewhere
  the look is unchanged to the eye; the goldens differ by a mean of at most
  0.5 of 255, and the forward and deferred paths change alike. Styles
  without sheen pay nothing for it.
- Seen from a strategy camera, the gusts rolled across the field in squares.
  The gust texture repeats every `gust_scale` metres (25 m in `BREEZE`), and
  from 100 m up dozens of repeats show at once, in a grid along the world's
  axes. The gusts are now two layers of it, the second turned 36.8° and 1.17
  times as large, moving together; the sum does not repeat. Gusts keep their
  size, speed and depth, but their shapes change, and so do the goldens.
- `FarGrassMaterial`'s gust sheen, the paler undersides of bent grass, now
  shows where the ground is seen at a glancing angle and fades as the camera
  looks down on it, as on real grass. From a strategy camera the far ground
  showed gust patterns with no grass in sight, and through the gaps between
  blades. Ground-level views are unchanged (the `meadow` example differs by
  at most 2 of 255). Where the sheen would change the colour by less than
  0.4%, the gust texture is not read.
- A perspective camera high above the ground, a strategy game's, kept its
  trample window (64 m across) centred under itself, while the ground it
  showed began 65 m or more ahead, so units in view flattened no grass. The
  window now moves ahead onto the nearest ground in view, never past where
  grass is drawn. It moves only once that ground is about 11 m away (a sixth
  of the window), so first- and third-person cameras keep the window around
  themselves as before, and the trail behind a walker with it.
- `GrassSampler` reported grass where the ground has a hole, past the edge
  of a `GrassGround::Mesh` or where a height function gave no height, while
  the GPU draws none there. Its `density` is now 0 there, so footsteps and
  stealth checks agree with what is drawn.

**Changed**

- The mid-range thinning (`mid_range_density`, half the blades, the rest
  wider) now begins at its own distance, `mid_range_start`: 10% of the radius
  by default (10 m at High), instead of at `lod1_start` (20 m). Blades keep
  full detail to `lod1_start` as before. On a T1200 laptop GPU this cut the
  main pass's grass cost by 9–13% in perspective, with more ground showing
  between blades from 10 m. Set `mid_range_start: 0.2` for the old look.

**Documentation**

From the notes of two games that integrated procgena_plant, an RTS and an
action game on a procedural island:

- The README has a "Fit it to your game" table after the quick start: the
  setting each kind of camera, ground and game needs, and its page.
- [Presets](docs/presets.md#sizes) lists every preset's blade heights, its
  tallest plant, density, spacing and the plants mixed in. A test keeps the
  table equal to the constants (`BLESS_STYLES=1 cargo test` rewrites it).
- [Styles](docs/styles.md#how-tall-the-grass-grows) puts the four ways to
  change blade height side by side; [Wind](docs/wind.md) says why the wind
  is global and how a sheltered field moves less; the quick start says the
  scene's light already casts grass shadows; [Terrain](docs/terrain.md)
  shows spawning a field behind a loading screen.

**Internal**

- The golden check also draws `MEADOW` through eight render paths
  (`path_*.png`): forward with and without MSAA, the depth prepass, every
  prepass with TAA, deferred, HDR with bloom, orthographic, and no grass
  shadows. It fails on any `ERROR` in the log as well, which is how a
  shader or pipeline that Bevy broke shows up: it draws nothing rather than
  failing.
- The three places where grass relies on Bevy internals that do not fail
  to compile when they change are now checked. The main pass's bind groups
  are taken from Bevy's view layout and compared with what
  `MeshPipeline::specialize` built, rather than cut to two. The deferred
  default, read through reflection, is asserted and unit-tested. The
  prepass's shader definitions are in one function, `prepass_shader_defs`,
  with the material group taken from `bevy::pbr::MATERIAL_BIND_GROUP_INDEX`
  rather than a hard-coded 3.
- The field and chunk tables' GPU layout (`FieldGpu`, `ChunkGpu`, `NO_MAP`)
  moved from `fields.rs` to a module of its own, `gpu_layout.rs`: the one
  place where the field logic depends on the shaders' layout and on Bevy's
  `ShaderType`.
- The grass shaders import Bevy's shader modules in one file,
  `bevy_bridge.wesl`: the view, the sun and ambient light, `PbrInput`,
  lighting and post-processing, the G-buffer and the prepass's normal and
  motion-vector conventions, behind small functions in grass's terms.
  `look.wesl` is grass logic only, and builds a `GrassSurface` that the
  bridge turns into Bevy's `PbrInput`. Images are unchanged.
- [Porting to a new Bevy](docs/porting.md) lists every place grass touches
  Bevy's renderer, what breaks there and how it is caught, and the order of
  a port. The roadmap states the policy for new fields in the design types.

## 0.20.5 (October 22, 2026)

Grass in the browser: it draws through WebGPU, in 10 storage buffers per
shader stage, and a device that cannot draw it gets one warning instead of a
stopped renderer. A [web demo](https://gemi.github.io/bevy-grass/) and
[docs/web.md](docs/web.md) come with it. Tested in Chrome, Edge and Brave on
Windows and in Chrome on macOS, and natively on macOS (Metal). Safari 27
cannot run Bevy 0.20's standard material yet, with or without grass
([docs/web.md](docs/web.md#safari)). 0.20.4 was not published; its changes
are in this release.

**Breaking, and what to write instead**

`GrassStyleFileError::Parse` no longer holds `ron`'s `SpannedError`, so a
`ron` upgrade is no longer a breaking change of this crate. It is now
`Parse { line, column, message }`: where the parser stopped, counted from 1,
and why. Match `Parse { message, .. }` instead of `Parse(error)`, and read
`message` or the error's `Display` (unchanged apart from showing only the
start of the span) instead of the `ron` error.

**Added**

- The examples build for the browser (`wasm32-unknown-unknown`, WebGPU):
  `python tools/web_example.py` builds and serves them, and CI checks the
  wasm build. [docs/web.md](docs/web.md) lists the browsers tested.
- A web demo for GitHub Pages (`web/`, a package of its own that is not
  published): a meadow with a quality switch, starting at `ULTRA`, a preset
  picker, a wind-strength slider, and toggles for shadows and antialiasing.
  `python tools/web_demo.py` builds it for size, serves it, and smoke-tests it
  in a browser.
- `examples/webgpu_minimal.rs`: the quick start, ready for the browser
  (a canvas that fills the page, `LOW` on the web, and a message on screen
  when `GrassSupport` says the device cannot draw grass). The README has a
  Platforms section and links it.
- [docs/web.md](docs/web.md), running in the browser: how to build for
  WebGPU, the browsers tested, what a device needs, the shadow and timing
  caveats, and frame times per tier on the devices measured.
- `GrassSupport`, a resource: whether this GPU can draw grass, and if not,
  why (no compute shaders, as in WebGL2; no indirect draws; no storage
  buffers in the vertex shader; too few storage buffers per shader stage; too
  small a storage binding). Decided once when the renderer starts.
- The examples' `--no-grass` flag: the same scene without grass.
- The web demo's and examples' pages log each shader a browser rejects, with
  the compiler's reason and line.
- The examples' `--max-storage-buffers N` flag: ask the GPU for at most N
  storage buffers per shader stage, as a weaker device or browser offers.

**Fixed**

- On a device that cannot draw grass, the grass pipelines were still created,
  and the wgpu validation error stopped Bevy's renderer. Now grass logs one
  warning saying why and draws nothing, and the rest of the scene renders.
  Grass binds 10 storage buffers in its busiest shader stage, so a device
  that allows fewer gets the warning.

**Changed**

- Grass fits in 10 storage buffers per shader stage, down from 14, so it
  draws on devices that offer 10 to 13: most browsers on macOS, Samsung
  Internet and some of Firefox. No picture changes. Each family of compute
  passes has its own layout, the masks and heightmaps share one buffer, and
  the draws bind storage buffers in the vertex stage only (see
  [docs/rendering.md](docs/rendering.md)). Fields whose masks and
  heightmaps together are larger than one storage binding (128 MiB in
  WebGPU) log a warning and draw no grass, where before each had a binding
  of its own.
- The web demo starts at `ULTRA` and the `WILDFLOWER_MEADOW` preset, and its
  page has sliders and toggles for wind strength, shadows and antialiasing.
  The bare circle in the middle of the field is gone, so the grass covers the
  whole meadow.

## 0.20.3 (October 21, 2026)

Docs only: no API or behaviour changed. The tagline on crates.io and the
README's opening now describe the crate as grass and foliage for 3D games,
instead of "GPU-driven, wind-animated tall grass", which undersold what it
grows and led with how it is built.

## 0.20.2 (October 21, 2026)

Runtime editing, grass on mesh terrain, and fields that turn.

**Added**

- Runtime editing ([docs/editing.md](docs/editing.md)). `GrassEdit` messages
  clear, paint, mow, change the height of, and restore grass in a
  `GrassBrushShape` (circle, stroke or turned rectangle), with a soft edge
  and a strength: at 1 an edit sets the grass and repeating it changes
  nothing, below 1 edits add up. Edits change the field's density mask,
  which a field without one gets on its first edit (0.25 m texels), and
  only the rows they touch are uploaded; the shaders are unchanged, so a
  game that edits nothing pays nothing.
- `GrassBrush`: a component that edits wherever its entity goes, at once and
  swept along its path (a mower) or at a rate while it stays (worn paths, a
  fire). `GrassBrushFootprint` makes it a rectangle under the entity, turned
  and scaled with it and swept between frames: a mower's deck, a vehicle's
  tracks, a bulldozer's blade.
- `GrassRegrowth`: a field's edits grow back toward its own mask, at a rate
  for density and one for height.
- `GrassSampler`: reads density, blades per square metre and blade height at
  any point on the CPU, with every edit and the style's bare patches, for
  stealth, footsteps and AI; `mask_image` returns a field's mask with its
  edits, to save.
- `GrassSystems::Edit`, between `DetectChanges` and `Rebuild`, and
  `GrassStats::edits` (`EditStats`).
- The `editing` example.

- `GrassGround::Mesh { mesh, cell_size }` and `GrassGround::mesh(handle)`:
  grass on the top of a mesh, where it is drawn with the field's transform.
  The mesh is baked once on the CPU into a heightmap (0.5 m cells by
  default, at most 4096 per side) and drawn like one, with no cost per
  frame. Blades grow on the highest surface and nowhere the mesh does not
  reach. It is baked again when the mesh, the field's size or the cell size
  changes, not when the field moves. A mesh that cannot be baked (not
  triangles, or no CPU copy) draws nothing and logs one warning.
- The `mesh_terrain` example: a meadow on rolling hills built as a mesh.

**Changed**

- A field turns with its transform's rotation about Y, mask and heightmap
  included. It used to be drawn unturned with a warning; now only a tilt,
  which grass ignores because it grows upright, logs one.
- The debug gizmos draw fields and chunks turned, and a heightmap field's
  upper rectangle at its highest ground rather than at `height_scale`.

## 0.20.1 (October 20, 2026)

The first tagged release. Built as 0.2.0, it is numbered after the Bevy
version it targets: procgena_plant 0.20 is for Bevy 0.20. It holds everything
since 0.1.0, below: Avian and Rapier support, the API and settings pass, and
the polish pass.

### Avian support, footprints, and a physics bridge not tied to one engine (October 20, 2026)

`GrassInteractorFromCollider` now works with Avian as well as Rapier, and a
large body flattens grass as one shape: the grass under a moving vehicle lies
the way it drives, and only the grass along its sides is pushed out.

**Added**

- `GrassInteractorFootprint`: a box (in the entity's local space, with a
  rounding for balls and capsules) that an interactor flattens instead of a
  disc. Under a moving box the grass lies down the way it moves and only its
  sides push sideways, so the trail behind keeps lying the way it went; a box
  at rest pushes out from its nearest edge. The box is laid on the ground by
  its shadow every frame, so turning and tumbling bodies follow, and a log
  that falls over flattens a strip along its length. It is one stamp on the
  GPU, however large (clipped to 16 m per side).
- The `avian` cargo feature: `GrassInteractorFromCollider` for `avian3d` 0.8
  colliders. It brings `avian3d` in with `default-features = false` (`3d`
  and `parry-f32` only), so it does not change how Bevy is compiled. `avian`
  and `rapier` can both be on.
- `GrassColliderBounds`, the engine-neutral input: a collider's box along
  the entity's axes, in metres with the scale applied, how rounded it is, and
  whether it is active. The Avian and Rapier backends write it; a game writes
  it for a collider type of its own, or with no physics engine at all.
- `GrassColliderSystems::{Bounds, Fit}`, in `Update`, to order such a system
  before the fit.
- A warning, once per entity, when a `GrassInteractorFromCollider` gets no
  collider size within three frames. It names the likely causes: no physics
  feature, the adapter on an entity without the `Collider`, or a
  `bevy_rapier3d`/`avian3d` version other than the one procgena_plant is built
  against (Cargo then builds two copies and the bridge cannot see the game's
  colliders; before, the interactor silently did nothing).
- The debug gizmos draw each footprint's outline as it lies on the ground.
- `examples/avian.rs` and `examples/avian_minimal.rs`, the Rapier scenes with
  Avian.

**Changed**

- `GrassInteractorFromCollider` fills in a `GrassInteractorFootprint` instead
  of a `GrassInteractorSpread`, and requires it. A spread drew one radial fan
  per disc, which made a large body look like several small ones side by
  side, with rows of grass bent at odd angles. `radius_scale` is now `scale`:
  it scales the whole footprint. A character capsule is a disc of its own
  radius, fully flat to its edge, where it used to be a disc of its bounding
  square's half diagonal fading from 60 %.
- `GrassInteractorFromCollider` is always compiled, and is in the prelude
  without a feature. The engine features only add the backend that reads
  their `Collider`; the fit is in the core, shared by both.
- The interactor buffer holds three `vec4`s per stamp (48 bytes) instead of
  one; a disc is stamped exactly as before.
- The `rapier` and `avian` examples drive the vehicle lengthwise, add a crate
  scaled to 2.5 m driving a circle, and throw in a log every six seconds that
  tumbles and lies down across the grass.

**Fixed**

- A scaled collider was covered at its scale squared: the collider's box
  already had the scale applied, and extraction applied it again. A 6 m box
  at scale 2 put discs 3.6 m past its ends. The footprint now matches the
  simulated collider, for uniform and non-uniform scale.
- A body lying on its side got one small disc: the cover only looked at the
  collider's local X and Z. It is now covered where its shadow lies.
- A child collider whose parent rigid body has `RigidBodyDisabled` kept
  flattening grass. Rapier's backend checks the nearest body above the
  collider, Avian's the body in `ColliderOf`.

### A new walk film for the README (October 20, 2026)

The film at the top of the README now walks through three presets, three
seconds each: `WILDFLOWER_MEADOW`, `RYE_AND_CORNFLOWERS` and `REED_MARSH`,
at twice their density, with Ultra shadows in a 4096 map. The meadow and
the rye wave in a stronger wind than the breeze; the reeds stand in almost
still air, for the contrast.

**Added**

- The `walk` example, which films it: a field per preset, each with its own
  walker and following camera, and cuts between them. `--styles` and
  `--shot` pick the presets and how long each is shown.
- `tools/record_walk.py`, which records `walk` and encodes
  `docs/images/walk.webp`.

**Fixed**

- Blade shadows under the examples' water (`REED_MARSH`, `KELP_BED`) blinked
  as the camera moved. The water box's floor lay on the ground, and the two
  z-fought: in some frames the floor hid the shadows on the ground under a
  second layer of water. The box now reaches 5 cm below the ground.

### Sixteen distinct presets, and fewer near-duplicates (October 19, 2026)

Fourteen of the 26 presets were the same blade in another colour or height.
Six of them are gone, and sixteen new ones each push a setting the others
leave alone: tufts and droop to their limits, stiffness at 0 or 1, twist of
two turns, fold into facets, toon shading on mixed fields, heads as the main
feature, and colours outside green and brown.

**Added**

- `GrassStyle::BIOMES`: `COTTON_BOG`, `REED_MARSH`, `DUNE_MARRAM`,
  `RYE_AND_CORNFLOWERS`, `PAMPAS`, `SPINIFEX`, `ELEPHANT_GRASS`,
  `FROSTED_MEADOW`, `BURNT_REGROWTH` and `LAVENDER`.
- `GrassStyle::FANTASY`: `CRYSTAL_SPIRES`, `CORKSCREW_ALIEN`, `KELP_BED` and
  `BLIGHTED`.
- `GrassStyle::TOON`: `CEL_FIELD` and `COZY_PASTEL`.
- `GrassBlade::{COTTON_GRASS, CATTAIL, CORNFLOWER, POPPY}`, to mix into any
  style as species.
- All of them as `.grass.ron` files and embedded assets, like every preset.
- `examples/gallery.rs`: the sixteen new presets on 18 m plots of their own
  ground, to fly around, with a walker trampling each plot and a wind per
  plot (a slow current over the kelp).
- `showcase` and `styles` show every preset on its own ground (sand, snow,
  peat, a pond, the seabed, rows for lavender), from
  `examples/common/setting.rs`; `showcase` has a fifth page.

**Breaking, and what to write instead**

`GrassStyle::SUMMER` and `GrassStyle::AUTUMN` are now one group,
`GrassStyle::SEASONS` (`SUMMER_TALL`, `AUTUMN_TALL`). The variants that only
changed a colour or a height are removed; a file that names one as its
`base` now fails with `UnknownBase`. Each is its main look with a few
settings, in a file:

| Removed | Now |
| --- | --- |
| `SUMMER_LUSH` | `(base: "SUMMER_TALL", height: (0.9, 1.5), lean: 0.15, root_color: (0.03, 0.08, 0.02), tip_color: (0.22, 0.44, 0.10), root_occlusion: 0.7, density: 50.0)` |
| `SUMMER_MEADOW` | `(base: "SUMMER_TALL", height: (0.5, 1.0), lean: 0.18, stiffness: 0.3, root_color: (0.07, 0.13, 0.03), tip_color: (0.48, 0.62, 0.18))` |
| `SUMMER_SEEDING` | `(base: "SUMMER_TALL", height: (0.8, 1.4), lean: 0.28, stiffness: 0.18, root_color: (0.06, 0.12, 0.03), tip_color: (0.66, 0.64, 0.38))` |
| `AUTUMN_RUST` | `(base: "AUTUMN_TALL", root_color: (0.16, 0.07, 0.03), tip_color: (0.54, 0.29, 0.13))` |
| `AUTUMN_STRAW` | `(base: "AUTUMN_TALL", height: (0.6, 1.1), stiffness: 0.45, root_color: (0.24, 0.19, 0.10), tip_color: (0.72, 0.62, 0.38), translucency: 0.5)` |
| `AUTUMN_FADED` | `(base: "AUTUMN_TALL", height: (0.5, 1.1), lean: 0.4, stiffness: 0.15, root_color: (0.11, 0.10, 0.07), tip_color: (0.44, 0.39, 0.30), translucency: 0.3)` |

For a colour change on one field only, `GrassFieldOverrides::tint` needs no
style at all.

**Internal**

- The golden scene gives each camera room for 24,000 blades instead of
  60,000 (the densest patch has 16,000). With 60,000, WARP crashed once there
  were 34 cameras. Every golden was re-blessed: removing six presets moved
  the later patches along the row, onto other wind and noise. Rendered at
  their old places, all twenty kept presets matched their old goldens
  exactly, so none of them changed.

### Optional Rapier interaction bridge (October 19, 2026)

**Added**

- The `rapier` cargo feature and `GrassInteractorFromCollider`: put it next to
  a `bevy_rapier3d` `Collider` and the radius, strength and height of the
  grass interactor follow the collider. A capsule gets one disc; a vehicle or
  wall gets a spread of discs. Disabled colliders/bodies and sensors flatten
  nothing.
- `GrassInteractorSpread`, a core component listing local disc centres for a
  large object. `GrassInteractorSpread::covering_aabb` covers a local AABB
  with at most 32 discs, and works without a physics engine.
- `examples/rapier.rs` (a kinematic capsule and a large vehicle driving
  through a meadow) and `examples/rapier_minimal.rs` (the shortest
  integration example).

The adapter reads the collider's local AABB, not a physics transform: Rapier
already writes `Transform`/`GlobalTransform`, and grass already reads
`GlobalTransform`, so the bridge only decides the footprint. It runs in
`Update` and is independent of the physics schedule. `bevy_rapier3d` is
brought in with `default-features = false` and only `dim3`, so enabling the
feature does not change how Bevy is compiled.

Avian was not included at first: `avian3d` 0.7 depended on Bevy 0.19. It
arrived with `avian3d` 0.8, above.

### Missing-style feedback and a constructor (October 18, 2026)

**Added**

- `GrassField::new(size, style, &mut styles)` adds a style (usually a preset)
  and returns the field in one call. `GrassField::from_preset` is the same
  constructor under a preset-flavoured name.
- A field with the empty default style handle now logs one warning after a few
  frames, naming the entity and how to give it a style, instead of waiting
  forever with no visible feedback.

**Changed**

- The README quick start and `examples/minimal.rs` use `GrassField::new`.

### Wind as a plugin (October 18, 2026)

The wind is now a self-contained model behind one per-frame resource, so it
can be replaced without touching the grass renderer.

**Breaking, and what to write instead**

| Before | Now |
| --- | --- |
| `GrassPlugin { settings }` | `GrassPlugin { settings, ..default() }`; the new `wind: bool` is `true` |

**Added**

- `GrassWindFrame`, the exact per-frame values the shaders read. The built-in
  wind writes it; another wind model writes it instead.
- `GrassWindPlugin`, the built-in wind model (direction, strength, scrolling
  gusts, sway and flutter), now separate from `GrassPlugin`. Leave it out with
  `GrassPlugin { wind: false, ..default() }`.
- `GrassWindSystems::Sample`, where a custom wind source writes its frame, so
  the blades and `FarGrassMaterial` read the same wind.
- `examples/custom_wind.rs`, a complete scene driven by a `Weather` resource
  instead of the built-in model.
- `src/wind/`, containing the settings, the gust clock and the tests; the
  shader side lives in `shaders/wind.wesl`.

**Internal**

- The wind has its own GPU uniform and bindings instead of sharing the frame
  uniform, and `FarGrassUniform` embeds the same `Wind` struct. The built-in
  model can therefore be extracted or replaced without changing the blades'
  layout.

### Forest-floor presets (October 18, 2026)

**Added**

- Four forest-floor `GrassStyle` presets, also shipped as `.grass.ron` files:
  `FOREST_MOSS` (a short, wide moss carpet), `FOREST_FERNS` (arching fern
  clumps), `FOREST_FLOOR` (moss with ferns, fallen leaves, clover and broad
  leaves mixed in) and `FOREST_BRACKEN` (rusty autumn ferns over leaf litter).
- `GrassStyle::FOREST`, the group holding the four.
- `GrassBlade::{MOSS, FERN, BRACKEN, LEAF_LITTER}`, for building a forest floor
  of your own or mixing one of its plants into another style.
- `examples/forest_floor.rs`, the four presets side by side.
- `showcase` and `styles` now show the forest-floor presets too.

**Internal**

- Every preset definition and the one preset list now live in `src/presets/`, one file per group.
  One entry generates the constant, the group array, `PRESETS` and the embedded
  asset, so adding a preset no longer means editing a second list in
  `style_file.rs`. Presets ship as `.grass.ron` files generated from the same
  constants by `BLESS_STYLES=1 cargo test`.

### The 240 Hz gate (October 17, 2026)

The measurements that make the 240 FPS target testable instead of a comment.

**Added**

- `GrassQuality::Competitive` and `GrassSettings::COMPETITIVE`: a 240 Hz /
  competitive-game tier with 40 m of half-density grass and no grass shadow
  casting. It is the cheapest tier and the candidate the gate measures.
- `bench --target-hz N` and `--budget-ms MS`: fail with a non-zero exit code
  when frame p95 exceeds the budget.
- `bench --baseline-csv PATH`: paired grass-cost p50/p95 against a previous
  `--no-grass` run.
- `bench --machine NAME`, and shadow-pass time in the summary and CSV.
- `tools/bench_240.py`: runs High, Competitive and no-grass on a named
  reference machine, in both scenes, and writes JSONL, CSV, logs and a
  Markdown table under `target/bench-240/`.
- `.github/workflows/perf-240.yml`: a manual gate on the self-hosted reference
  runner labels `t1200`, `rtx3060` and `rtx4070`.
- `docs/performance.md` now documents the 240 Hz budget, the three
  configurations, the reference machines and where to record runs.

### The API and settings pass (October 16, 2026)

Built from a review of the crate's API and developer experience, items P1-1 to P1-9.

**Breaking, and what to write instead**

| Before | Now |
| --- | --- |
| `GrassPlugin { quality, shadows, instance_capacity, max_interactors }` | `GrassPlugin { settings: GrassSettings }`; `GrassSettings::{LOW, MEDIUM, HIGH, ULTRA}`, or `GrassSettings::from(GrassQuality::High)` |
| `Res<GrassShadows>` | `GrassSettings::shadows` |
| `GrassStyle { height, tip_color, density, tufts, .. }` | `GrassStyle { blade: GrassBlade { height, tip_color, .. }, density, variation: GrassVariation { tufts, .. }, ground_tint: GroundTint { .. }, .. }` |
| `GrassSpecies { style: GrassStyle, .. }`, `GrassSpecies::new(GrassStyle::SEED_HEADS, 0.1)` | `GrassSpecies { blade: GrassBlade, .. }`, `GrassSpecies::new(GrassBlade::SEED_HEADS, 0.1)` |
| `GrassStyle::min_width_px` | `GrassSettings::min_width_px`, or `GrassCamera::min_width_px` on one camera. Old `.grass.ron` files that set it still load, with a warning |
| a species in a `.grass.ron` file with `density`, `tufts` and other field settings | an error that names them: move them to the style that lists the species |

**Added**

- `GrassSettings`, a resource with a preset per hardware tier; every setting
  changes at runtime.
- `GrassCamera`: per-camera overrides of `radius`, `density_scale`,
  `min_width_px` and `shadows`.
- Fields honour `Visibility` (their own and their parent's), `RenderLayers`
  0 to 31 and their transform's scale. A rotated field logs a warning.
- `GrassSystems::{DetectChanges, Rebuild}`, to order your systems against.
- Full reflection: components, resources and the style asset, so fields work
  in scene files and inspectors.
- `GrassSun`, which marks the light grass follows, and warnings about
  cascades too deep for blade shadows.
- `GrassStats`, seven Bevy diagnostics, and `GrassDebugPlugin` (cargo feature
  `debug`).
- Incremental tables: a style edit, even every frame, rewrites only the
  entries that use it.

### The polish pass (October 17, 2026)

Built from the review's P2 items.

**Breaking, and what to write instead**

| Before | Now |
| --- | --- |
| `app.add_plugins(GrassPlugin::default())` | `app.add_plugins(GrassPlugins)`. `GrassPlugin` alone no longer registers the far-grass material or the `.grass.ron` loader (`FarGrassPlugin`, `GrassStyleFilePlugin`) |
| `GrassPlugin { settings }` | `GrassPlugins.set(GrassPlugin { settings })` |
| `Wind`, `Wind::STORM` | `GrassWind`, `GrassWind::STORM` |
| `procgena_plant::api::GrassField` | `procgena_plant::GrassField`: there is no `api` module; every public type is at the crate root and in the prelude |
| `GrassField::density` | `GrassField::density_map` (the style's `density` is blades per m²; the field's was a mask) |
| `GrassBlade::head` | `GrassBlade::head_length` |
| `GrassVariation::tuft_size` | `GrassVariation::tuft_spacing` |
| `GrassVariation::dead_blades` | `GrassVariation::dead_share` |
| `GrassVariation::bare_patches` | `GrassVariation::bare_share` |
| `FarGrass::new(&style, quality)`, `FarGrass::fade: Vec2` | `FarGrass::new(&style)`; the fade follows `GrassSettings::radius`. `FarGrass::fade` is an `Option<Vec2>`, and `.with_fade(Vec2)` sets one |
| `grass_far_color(.., far, time, gusts, sampler)` in WESL | `grass_far_color(.., far, gusts, sampler)`: the gusts' scroll is `far.strength.yz`, which `FarGrassPlugin` keeps equal to the blades' |
| `GrassInteractor { radius, strength }` | the same, plus `max_height: Option<f32>`; add `..default()` to a literal |

The `.grass.ron` files keep loading under the old names: `head`,
`tuft_size`, `dead_blades` and `bare_patches` are accepted as aliases, and
files are written with the new ones.

**Added**

- `GrassPlugins`, a plugin group, with `FarGrassPlugin` and
  `GrassStyleFilePlugin` to leave out what a game does not use.
- The presets as assets: `assets.load("embedded://procgena_plant/styles/meadow.grass.ron")`.
- Hex colours in `.grass.ron` files: `tip_color: "#5c8f24"`. Tuples still work.
- `GrassFieldOverrides` (`tint`, `density_scale`, `height_scale`) on a field,
  to change one field without a style asset of its own.
- `GrassInteractor::max_height`, so something flying over the grass does not
  flatten it, a warning for a radius over the 2 m limit, and the interactor
  limits (2 m radius, the count, the 64 m window) in its documentation.
- `GrassWind::toward(Dir2)`, and `PartialEq` for `GrassField` and
  `GrassGround`.
- `examples/minimal.rs`, which is the README's quick start (a test keeps the
  two equal).
- This changelog.

**Fixed**

- The wind jumped once an hour, when Bevy's clock wraps: sway and flutter
  were sines of a wrapped time, and the gusts scrolled by time times speed.
  Sway and flutter frequencies are now whole cycles per period, and the gust
  scroll is accumulated in `f64` on the CPU, so neither jumps, and a change
  of the wind's speed or direction no longer jerks the gusts either.

**Changed**

- Bevy is `0.20`, not `=0.20.0`: any 0.20.x works, and a game that pins `=0.20.0` itself still gets exactly that. Only 0.20.0 has been tested, since no later patch exists yet.

**Internal**

- The `.grass.ron` format is declared once, in one macro, from the settings it
  holds; a new setting is one line there, and a test fails if one is missing.

### The tunables pass (October 17, 2026)

Numbers that were fixed in the code and shaders, and that another game might
want different, are settings now. **Every default is the number it replaces**,
so nothing changes unless you set one: the rendered image is unchanged to
within one 8-bit level (llvmpipe rounds the new uniform reads differently, so
its goldens were re-blessed), and a GPU test checks the shaders against the CPU
mirror at the defaults and at other values.

**Added to `GrassSettings`** (the first group are its distances and densities;
see the README's table for each range and default):

- The blade mesh: `blade_rows` (rows per level of detail, `[7, 3, 1]`).
- Distance and thinning: `lod1_start`, `lod2_start` and `fade_start` (shares
  of `radius`), `lod_blend` (width of the dithered band between levels, in
  metres), `lod2_density`, `fade_density` and `mid_range_density` (how many
  blades are kept at each distance), `max_widening` (how far thinned blades
  widen), `turn_to_camera` (how far a blade seen edge-on turns toward the
  camera).
- Limits of a view: `orthographic_extent` (the area around an orthographic
  camera in which blades are stored: it was a fixed 256 m, and a blade
  further away was misplaced), `max_chunk_slices` (the work list of blade
  generation, a fixed 8,192) and `readback_interval` (frames between the
  GPU readbacks behind `GrassStats` and the overflow warnings; 0 turns them
  off).
- Trampling: `trample_recovery` (seconds, was 1.5), `trample_cell_size`
  (metres, was 0.25: the window is 256 cells across, so it was 64 m) and
  `max_interactor_radius` (metres, was 2).

**Added to `GrassWind`**: `sway`, `sway_speed`, `flutter` and `gust_contrast`,
multipliers of the wind's movement that are 1 in every preset.

**Breaking**

| Before | Now |
| --- | --- |
| `GrassSettings { radius, density_scale, min_width_px, shadows, orthographic_lod, instance_capacity, max_interactors }` written out in full | add `..GrassSettings::HIGH` (or any tier): the struct has more settings, all with defaults. Code that starts from a tier is unaffected |
| `GrassWind { direction, strength, gust_scale, gust_speed }` written out in full | add `..GrassWind::BREEZE` (or any preset) |

**Internal**

- The CPU mirror of the distance table, `DistanceLod`, is built from the
  settings; a test runs the GPU against it with other distances.
- `GrassDebugPlugin` draws the trample window and the interactor discs the
  settings give, and needs `GrassSettings` (which `GrassPlugin` inserts).
- Lavapipe golden folders are named after the Mesa and LLVM builds, and CI
  pins the Linux golden runner to Ubuntu 24.04. A `golden-bless` job writes
  and uploads them on that same runner.

## 0.1.0

Never released; the version the first milestones were built as. The first
version: GPU-driven grass with levels of detail, wind, trampling, shadows, the
prepass and deferred paths, flowers and other species, 22 presets, and
`FarGrassMaterial`.
