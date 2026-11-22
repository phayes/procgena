# Plant / Field restructure (bevy-grass + zeapp)

## Bottom line

- **Plant is the basic unit.** `GrassPlant` stays and gains its own tuft settings (`tufts`, `tuft_size`, moved from `GrassVariation`). A plant spread out reads as a small patch; pulled in tight it reads as a tuft.
- **Field = plants laid out on land, stored inline.** The `GrassStyle` asset, `Handle<GrassStyle>` and `GrassStyleLoader` are removed. The `GrassField` component holds the definition directly:
  `size, ground, density_map, wind_scale, plants: Cow<'static, [FieldPlant]>, density, variation, ground_tint, toon_bands`.
- **`GrassSpecies` → `FieldPlant { plant, weight, clustering }`.** There is no main plant: weights are normalised to 100% of the density. The limit is 16 plants. An empty list draws nothing.
- **`GrassFieldOverrides` is removed.** `wind_scale` moves onto `GrassField`. `tint`, `height_scale` and `density_scale` go away: edit the field instead.
- **Presets become structs.**
  - `PlantPreset { name, description, group, plant }`
  - `FieldPreset { name, description, group, field: GrassField }` (size 0, flat ground, no map)
  - Each has an `ALL` catalogue and a `named()` lookup. `GrassField::from_preset(&FieldPreset, size)`.
  - The `GrassPlant::FERN`-style constants stay as the source of the plant presets. `GrassStyle::MEADOW` and the other style constants become `FieldPreset::MEADOW` and so on.
- **Shader: pick the plant before the tuft pull.** Each candidate root picks its plant, using clump noise at the pre-tuft position. The tuft pull and outward lean then use that plant's entry, not entry 0. The `FieldGpu` layout is unchanged: `tufts.x/.y` are already per entry. The cull spread uses the largest pulled `tuft_size` across the field's plants, stored on entry 0.
- **Files:**
  - `.grass.ron` describes a field: `base` names a FieldPreset, and `plants: [ { plant: "FERN" | { base: "FERN", ..edits }, weight, clustering } ]`.
  - The asset loader produces `FieldPreset` assets.
  - JSON: `GrassField::to_json`/`from_json` cover the definition, not size, ground or map. `GrassPlant::to_json`/`from_json` are added.
- **zeapp:** the Plants tab becomes a Field editor.
  - A Field preset picker at the top.
  - A **Plants** list. Each row: a PlantPreset picker or Custom, a ✎ customise button, weight %, clustering, delete. Plus `+ Add plant`.
  - Then the Density / Variation / Ground Tint / Toon sections.
  - A Custom row expands an inline `ParamPanel<GrassPlant>`, which includes the plant's tuft settings.
  - `PlantSettings` is replaced by sending the inline `GrassField`. ze_scene overwrites `size` from the environment's Field Size.

**Trade-offs we accepted:**
- No style sharing between fields (copy from a preset instead). No live hot-reload of a field from a `.grass.ron` handle.
- Every edit fires `Changed<GrassField>`. The colour-only fast path must be kept by comparing against the previous build (see Phase 1).
- Picking the plant before thinning gives up the deferred pick on unbranched fields. The pick now runs for every candidate.
- Goldens for tufted or mixed presets will change and need re-blessing.

No backward compatibility (per CLAUDE.md). The CHANGELOG states, for each breaking change, what to write instead.

---

## Context

- In the zeapp Plants tab, only the "Primary Plant" can be edited. Mixed-in species are shown as a read-only note (`species_note`).
- The user's model is two things:
  - **Plant:** one cluster, the basic unit.
  - **Field:** a list of plants, and how they are laid out on the land.
- Today bevy-grass gives one plant special status (`GrassStyle::plant`), treats the rest as `species` sharing whatever is left, and applies tufting to the whole field. It also splits "where" (component) from "what" (asset).
- The user decided:
  - Reshape bevy-grass itself.
  - Tuft pull per plant.
  - Variation, ground tint and toon stay on the Field.
  - Relative weights.
  - Preset structs.
  - Merge the asset into the component.

---

## Phase 1: bevy-grass data model (`src/style.rs`, `src/api.rs`, `src/fields.rs`, `src/lib.rs`)

1. **`style.rs`**
   - Delete `GrassStyle` and `GrassSpecies`.
   - Add `FieldPlant { plant: GrassPlant, weight: f32, clustering: f32 }` with `FieldPlant::new(plant, weight)` and `.clustered(c)`.
   - Move `tufts` and `tuft_spacing` (renamed `tuft_size`) from `GrassVariation` to `GrassPlant`.
   - `MAX_SPECIES` becomes `MAX_PLANTS = 16`, all plants counted.
   - `Default for GrassPlant/GrassSegment/GroundTint/GrassVariation` currently delegates to `GrassStyle::MEADOW`. Point them at the new MEADOW preset/plant constants.
   - The module stays as "the look of a plant", and maybe gets renamed `plant.rs`.
2. **`api.rs`**
   - `GrassField` gets the inline fields listed above, plus `wind_scale`.
   - `GrassField::new(size, plants…)` and `from_preset(&FieldPreset, size)`.
   - Move `spacing`, `with_spacing`, `with_density` and `with_plants` from `GrassStyle` onto `GrassField`.
   - Delete `GrassFieldOverrides`.
3. **`fields.rs`**
   - Drop `Assets<GrassStyle>`, `AssetEvent`, `warn_missing_style`, `TablesDirty.styles` and `EntryKey.style: AssetId`.
   - Read the definition from the component. `Plan.species` becomes `Plan.plants`. The `MAX_SPECIES - 1` truncation becomes `MAX_PLANTS`.
   - `species_chances` becomes `plant_chances`: weights are normalised, then the existing "chance of what's left" chain is kept. Entry 0 is now a real plant with its own chance, not a dummy 1.0. Update `pick_species` (shader and `mirror.rs`) so the last plant takes whatever is left, instead of falling back to entry 0.
   - **Fast path:** keep, per entity, the last-built definition (or a structure hash vs a look hash) in `EntryKey`. A change that only touches colour or look still becomes a uniform update. This replaces the `changed_styles` path. Port the tests `a_colour_only_style_change_is_a_uniform_update`, `only_the_entries_of_the_changed_style_are_rewritten` and `a_change_no_field_shows_changes_nothing` to the per-field meaning.
   - `field_entries`/`look_entry`:
     - Fill `tufts.x/.y` from each plant.
     - Keep `tufts.z/.w` (bare patches) per field.
     - Take `motion.z` from `field.wind_scale`.
     - Remove the overrides tint and scales.
4. **Other readers of the "main plant":**
   - `edit.rs` `GrassSampler`: height becomes the weighted mean of the plants, not entry 0.
   - `far.rs` `FarGrass::new` takes `&GrassField`, with colours averaged by weight.
   - `stats.rs` and `settings.rs`: docs.
5. **`lib.rs`:** remove the `GrassStyle` asset registration, reflect and re-exports. Register `FieldPlant`, `PlantPreset`, `FieldPreset`. Update the prelude.

## Phase 2: Presets (`src/presets/`)

1. **The `preset_group!` macro** emits `FieldPreset` consts with `name`, `description` (taken from the existing doc comment, written out as a literal string) and `group`. It also emits a per-group array. Add an equivalent `plant_preset_group!` for `PlantPreset`, used in `blades.rs`, `understory.rs` and `shrubs.rs`.
2. **Every `GrassPlant` constant becomes a PlantPreset.**
   - Inline main plants that matter on their own get named constants, e.g. `GrassPlant::MEADOW_GRASS`, `SUMMER_TALL_GRASS`, `LAWN_GRASS`, `WHEAT`, `MARRAM`.
   - Plant constants built from `GrassStyle::SUMMER_TALL.plant` are rebuilt from the new plant constants.
3. **Converting mixes:** the old main plant becomes `plants[0]` with `weight = 1 − Σshares`, and the species keep their shares as weights. The result is the same split under normalisation.
4. **Tufts:**
   - A preset with field tufts gets those values on its inline/main plant only.
   - Plant constants (FERN, CLOVER, …) keep `tufts: 0` unless they are inherently tufted.
   - Constants used in a mix stay exactly equal to their PlantPreset, so zeapp can recognise them.
   - Visible change: species in tufted presets stop being tufted. Re-bless the goldens.
5. **Single-plant style presets** (`plants.rs`: SEED_HEADS, CLOVER…) stay as FieldPresets wrapping one plant. The user can drop them later.
6. **Tests to update** (`presets/tests`, `style/tests`):
   - "shares < 1" becomes "weights > 0, at most 16 plants".
   - `a_plant_is_a_species_and_a_style_of_its_own` becomes "each single-plant FieldPreset equals its PlantPreset".
   - Forest per-m² tests use `weight / Σweights × density`.
   - New: PlantPreset and FieldPreset names are unique, and `named()` finds every one.
   - The `docs/presets.md` table drops the "own plants vs mixed in" column.

## Phase 3: Shader and mirror (`src/shaders/generate.wesl`, `common.wesl`, `src/mirror.rs`, `gpu_layout.rs`)

1. In `evaluate`:
   - Compute `pre = field_to_world(local)`.
   - `entry = pick_species(first, field, seed, clump_of(pre.xz))` for every field with more than one plant.
   - Then `look = fields[entry]`, and apply the tuft pull with `look.tufts.x/.y`.
   - Remove the two later pick sites (:391, :430).
2. Facing lean (:463-472) uses `look.tufts.x`. Bare patches stay on `field.tufts.z/.w`, sampled at the post-tuft root. `blade.clump` stays post-tuft: it only affects colour.
3. Cull (:170-172): read the aggregated "largest pulled tuft size" written onto entry 0. Pick a spare lane: either `tufts.y` on entry 0 means "largest" there, or use another unused component. Document it in `common.wesl` and `gpu_layout.rs`.
4. `mirror.rs`: update `pick_species` to the new fallback and add a twin of the pre-tuft clump. Update `mirror/tests`.
5. Test change: `fields/tests::every_species_is_an_entry_after_its_field` (`heads.tufts == first.tufts`) now expects per-plant tufts.

## Phase 4: Files and JSON (`src/style_file.rs` → `src/field_file.rs`)

1. **`GrassPlantFile`:** `base` now names a **PlantPreset** (not a style's main plant). Add `tufts` and `tuft_size`. Add `GrassPlant::from_ron/to_ron/from_json/to_json`.
2. **`GrassFieldFile`:**
   - `base` names a FieldPreset.
   - Field settings. Remove `tufts`/`tuft_size` from the field level.
   - `plants: Vec<FieldPlantFile { plant: PlantRef, weight, clustering }>`, where `PlantRef` is untagged: a preset name string, or a `GrassPlantFile`.
   - `GrassField::from_ron/to_ron/from_json/to_json` cover the definition only.
   - Validation: weights ≥ 0 with at least one > 0, at most 16 plants, plus the existing plant/segment/branch checks.
3. **Loader:** `GrassFieldLoader` (`.grass.ron`) produces `FieldPreset` assets. `GrassFieldFilePlugin` embeds every FieldPreset as before.
4. **Shipped files:** re-bless `assets/grass_styles/*.grass.ron` (rename the folder to `grass_fields/`) with `BLESS_STYLES=1`. Fix the Cargo.toml comment ("the crate embeds them" is really built from the constants).
5. **Tests to port:** the `style_file/tests` round-trip, shipped-parity and species-rule tests. New tests: a plant given by preset name resolves, and an unknown plant name lists the PlantPresets.

## Phase 5: Callers in bevy-grass

- GPU tests (`render/gpu_test*.rs`): `app_with_style` becomes `app_with_field`. `look.rs`/`buckets.rs` mixes use `FieldPlant`. `wind.rs` uses `field.wind_scale`.
- All 22 examples. The ones with the most changes:
  - `bench` (species splice, tufts)
  - `styles` (group arrays → `FieldPreset::ALL` grouped)
  - `variation`
  - `rts_camera` (overrides)
  - `forest_floor`
  - `golden` (iterates `FieldPreset::ALL`)
- The `web/` workspace member (`main.rs`, `tune.rs`, `page.rs`, `tune/tests`): overrides become field edits.
- `docs/` (`styles.md`, `presets.md`, `api.md`, `editing.md`, `wind.md`, `performance.md`, `roadmap.md`), the README quick start and `.claude/skills/bevy-grass-style/SKILL.md`.
- CHANGELOG `## Unreleased`: a **Breaking, and what to write instead** table covering:
  - GrassStyle → GrassField inline
  - GrassSpecies → FieldPlant
  - share → weight
  - overrides removed
  - tufts per plant
  - `.grass.ron` shape
- Goldens: re-bless the tufted and mixed presets on the backends CI uses. Add the missing goldens for the 9 understory/shrub presets: `golden` already fails on them today.

## Phase 6: zeapp (`/opt/zeapp`)

1. **ze_scene**
   - `PlantSettings` → `FieldSettings { field: GrassField }`, keeping `Reflect` because zeapp's tests use it. Default: `FieldPreset::MEADOW`.
   - `ViewerCommand::SetPlants` → `SetField`. In `scene.rs`, drop `Assets<GrassStyle>`/`state.grass`. `SetField` and the resize path insert `GrassField { size: env size, ..field }` on `state.subject`.
   - Update `tests/render.rs` (`changed_plant_colours_are_drawn` edits `field.plants[0].plant…`).
2. **params:** `ParamPanel.id` becomes `SharedString`, so a panel per row works. `Accordion` and `Switch` already take `Into<ElementId>`.
3. **`plants/params.rs`**
   - Split into `plant_sections()` (for `ParamPanel<GrassPlant>`: Size, Light, Stem, Branches, Leaves, Head, plus a new **Tufts** group) and `field_sections()` (for `ParamPanel<GrassField>`: Density/Toon, Variation, Ground Tint).
   - Keys become reflect paths relative to each type.
   - Port all the existing tests to both tables: unique keys, writes the named field, valid range ends, show/hide order, no self-hide, presets fit, hex round trip. Iterate PlantPresets for the plant table and FieldPresets for the field table. "Range ends valid" round-trips through `GrassPlant::from_ron`/`GrassField::from_ron`.
4. **`plants/mod.rs` → a Field editor**
   - Field preset picker. Choosing a preset rebuilds every row.
   - **Plants list.** Each `PlantRow` entity holds:
     - a stable id
     - a searchable PlantPreset `Select` with a "Custom" entry, plus a ✎ button
     - a `customising` flag
     - weight and clustering sliders, in a small `ParamPanel<FieldPlant>` with those two params
     - a `ParamPanel<GrassPlant>`, shown when customising or when no preset matches
   - The matching preset is worked out by comparing `plant == preset.plant`. Picking a preset clears `customising`. ✎ sets it.
   - **Header:** "Plant N — Name · 34%", with the share shown normalised. Delete button. `+ Add plant` adds Clover at weight 0.1, and is disabled at 16. Delete is disabled on the last remaining plant.
   - Then the `ParamPanel<GrassField>` sections.
   - Save/Load JSON uses `GrassField::to_json/from_json`, with `field.json` as the default name.
   - Delete `species_note`. Move `display_name` into a shared helper, or use `PlantPreset.name` directly if the presets carry display names.
5. **`studio.rs`:** wire up the `FieldEdited` event and `SetField`. The tab label can stay "Plants" or become "Field" (user's call).

---

## Open questions for the implementing session (ask before acting)

1. **`wind_scale`:** keep it on `GrassField` (as planned), or drop it with the other overrides?
2. **PlantPreset names:** a display string ("Oregon Grape") plus a constant key, or the constant key only?
3. **Standalone plant files** (`.plant.ron`) and a Plant-only preview in zeapp: now, or later?
4. **Single-plant FieldPresets** (`plants.rs`, the understory "plant + moss" presets): keep them, or let users build them from PlantPresets?
5. **Tab name:** "Plants" or "Field"?

## Verification

- bevy-grass:
  - `cargo test --all-features` (presets, fields, mirror, style_file, edit and the rest)
  - `cargo test --doc --all-features`
  - `cargo doc --all-features`
  - `BLESS_STYLES=1 cargo test` once, then a clean run
  - GPU tests where available (`render/gpu_test`)
  - `cargo run --example golden`, comparing and re-blessing only the expected presets
  - Build `web/` and all examples (`cargo build --examples --all-features`)
- zeapp:
  - `cargo test -p zeapp -p ze_scene`, including the ported params tests
  - Run the app: add, delete, switch preset, Custom, ✎, Save/Load JSON, and check the viewport updates
- Never run `cargo fmt --all` in zeapp: it reformats sibling repos. Format per crate.
- Codex comment-rewrite pass at the end (global CLAUDE.md).

## Notes for the implementer

- `/opt/bevy-grass` has uncommitted changes (CHANGELOG, Cargo.toml, several `.grass.ron`). Don't discard them. Build on top.
- Work phase by phase. Phases 1–5 land in bevy-grass. Phase 6 follows in zeapp.

---

## Refinements (2026-11-21)

The open questions above were answered by the user. Where this section disagrees with the plan above, this section wins.

| Question | Answer |
| --- | --- |
| `wind_scale` | Keep, on `GrassField` (default 1.0) |
| PlantPreset names | Display name + constant key, on both PlantPreset and FieldPreset |
| `.plant.ron` + Plant-only preview | **Now**: a `.plant.ron` loader and shipped files; zeapp gets a separate, independent **Plant** tab |
| Single-plant FieldPresets | Remove the PLANTS group only (SEED_HEADS, WHITE_FLOWERS, YELLOW_FLOWERS, CLOVER, BROAD_LEAF). Keep the understory "X over moss" presets and BROOM |
| Loader asset type | `PlantPreset` / `FieldPreset` are Assets with `Cow<'static, str>` key, name, description and group. Files may set `name` and `description`. A loaded file's key is its file stem, and its group is "File" |
| Field file extension | `.field.ron` in `assets/grass_fields/`; plants are `.plant.ron` in `assets/grass_plants/` |
| zeapp tab names | "Plant" and "Plant Field" |
| zeapp Plant tab | Independent of the Plant Field rows |
| Which plants get PlantPresets | The existing `GrassPlant` constants plus a handful of notable new ones (`MEADOW_GRASS`, `LAWN_GRASS`, `SUMMER_TALL_GRASS`, `WHEAT`, `MARRAM`). Other preset fields keep their main plant inline, and zeapp shows them as "Custom" |

**Further design decisions:**

- **Plant order.**
  - The pick runs down the list. Each plant takes its chance of what the plants before it left.
  - The last plant with a non-zero weight takes the remainder (chance 1.0), and its clustering is unused.
  - Old mixes convert to `[species… (share as weight), old main plant last (weight 1 − Σshares)]`. With the same salt per position, this reproduces the old pick exactly, so untufted mixes render the same.
- **Tufts** move onto the plant that carried the tufted look. That is the old main plant in general. In the understory presets and BROOM it is the moss or grass filler.
- **Cull spread.** The field's largest `tuft_size × 1.5` over its tufted plants goes in the unused `variation.w` lane of entry 0.
- **Change detection.** Each field slot keeps a snapshot of the definition it was built from. A colour-only edit stays a uniform update.
- **Files.**
  - `GrassFilePlugin` registers `GrassFieldLoader` and `GrassPlantLoader`.
  - The error type `GrassFileError` has `UnknownField` and `UnknownPlant`.
  - `BLESS_STYLES` is renamed `BLESS_PRESETS`.
  - `style.rs` is renamed `look.rs`, and `style_file.rs` is renamed `files.rs`.
- **Goldens.**
  - Before/after captures on this Mac (Metal, which has no reference folder) are compared numerically. The user judges any changed look.
  - Re-blessing runs on the named renderers only: the CI `golden-bless` job for llvmpipe, and the user's machines for nvidia-t1200 and WARP.

The full implementation plan is at `/Users/phayes/.claude/plans/background-from-the-session-lexical-dove.md`.

## Phase 2b: accents (2026-11-21, agreed with the user)

**Rule:** anything that reads as one plant belongs to `GrassPlant`; the field only decides which plants grow where and how they cluster (an ecosystem). Tufts stay on the plant: a tuft is how a plant made of many blades reads as one plant.

- **`GrassAccent`** on `GrassPlant.accent`: a `share` of the plant's stems that grow differently, in place of main stems. Its fields: `share` (0 = none), `height`, `width`, `lean`, `translucency`, `segment`, `head_length`, `head_width`, `head_color`. It is always unbranched, and it inherits the plant's tufts, so accents land in the plant's own tufts by construction.
- **Presets** that become one accented plant (instead of a hand-matched second `FieldPlant`): PAMPAS (plumes), KELP_BED (kelp with floats), CRYSTAL_SPIRES (great shards), BLIGHTED and FROSTED_MEADOW (their grass's seed stalks). Every other mix is an ecosystem and stays a list of plants.
- **Tables:** a field's plant entries come first, as now; then one accent entry for each plant whose accent share is above 0. A new `accent: Vec4` lane on `FieldGpu` / `Field` (x: share, y: the accent entry's index, or `NO_ACCENT`) lets the shader find it.
  - Accents count toward the 256-entry cap (8-bit entry index in a packed blade).
  - `mix.z` still counts plants only. `mix.w` (the tallest stem) includes accents.
- **Shader (Phase 3):** after the plant pick, one unclustered hash against `accent.x` picks the accent entry. There is no extra read, and the blade count is unchanged. CPU twin in `mirror.rs`.
- **Files:** `accent: (share: …, segment: (…), …)` inside a plant. **Sampler:** the typical height includes accents by share.
- **Cost (measured by reasoning, not benchmarked):** 448 B per accent entry; one hash per candidate.
