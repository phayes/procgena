# Codex comment-rewrite backlog

Comments the global instructions want rewritten by a Codex agent, still
pending because the Codex workspace ran out of credits on 2026-11-23. When
credits return, hand each block to one `full_access` Codex agent with the
standard brief (rewrite comments only, end-user audience, no code changes,
no state-changing git commands, report any bugs found). Then build the docs
and run the doctests for each package with `--all-features`. Remove a block
from this file once it is done.

After a block is rewritten, run these for every package the block touches:

```bash
cargo doc -p <package> --all-features
cargo test --doc -p <package> --all-features
```

Each item appears in one block only. When a later list named an item that an
earlier block already holds, the later block lists only what it added. Lines
marked "file level" name files without items, because the worker's own item
list was not kept.

## procgena_impostor (tree/impostor worker, impostor Phase 3b)

- `impostor/crates/procgena_impostor/src/mips.rs`: the module, `mip_chains`,
  `MipChains`, `halve`, `view_coverage`, `keep_coverage`
- `src/ktx2.rs`: the module, `encode_bc7`, `data_format_descriptor`
- `src/cache.rs`: the module, `compress_impostors`, `load`, `read_ktx2`,
  `atlas_images`, `save`, `finish_writes`, `ImpostorWrites`,
  `DiskReads::wait_for`, `supports_bc7`
- `src/bake.rs`: `finish_bakes`, `BakeReadiness::renders`
- `src/lib.rs`: `ImpostorSettings::shipped_dir`

## procgena_tree_bevy

- `tree/crates/procgena_tree_bevy/examples/bake_impostors.rs`: the module
  docs and `finish`

## procgena (studio)

- `procgena/src/scene/{subject,protocol,headless_scene,render_tests,mod}.rs`
  and `procgena/src/studio/status.rs`: the items the studio sub-agent changed
  for bake-camera filtering, impostor settings and grove preparation progress

## procgena_biome (B0)

- `biome/crates/procgena_biome/src/lib.rs`: crate docs, including the
  doctest's prose
- `src/biome.rs`: module doc; `Biome` and its fields `terrain`,
  `ecotope_rules`; `BiomeTerrain` with `Preset`, `Inline`; `EcotopeRule`;
  `BiomeError` with `Options`, `Terrain`; `terrain_seed`; `Biome::empty`,
  `Biome::is_empty`, `Biome::terrain_options`, `Biome::build`
- `src/presets.rs`: module doc; `BiomePreset` and its fields `key`, `name`,
  `description`, `group`, `biome`, `default_seed`; `bare_preset`; `PLAINS`,
  `ROLLING_HILLS`, `MOUNTAINS` (each doc must match its `description`),
  `BARE_LAND`, `GROUPS`; `BiomePreset::all`, `named`, `bare`
- `tests/empty_biomes.rs`: module doc; `SEEDS`, `SIDE`, `GRID`; `region`,
  `square`, `reference`, `assert_same_land`; `Covered` and its fields; the
  inline comments in `assert_same_land`,
  `empty_biome_builds_the_same_land_as_its_terrain`,
  `inline_terrain_builds_the_same_land_and_takes_the_world_seed`,
  `stitched_biomes_build_the_same_world_as_stitched_terrains`

## F1 (procgena_core, procgena_specimen and the renames)

- `core/crates/procgena_core/src/{lib,hash,rng,ground}.rs`: everything
  public, plus test helpers. This includes the whole crate doc in `lib.rs`,
  which F2, R1, C1, F3 and C2 edited since; F2's `derive_seed`, `salt`,
  `seed_for` and `mix64` and C2's `pcg` and `pcg2` in `hash.rs`; and C1's
  `GroundSample::gradient`, `Ground::gradient` and `HeightGrid` in
  `ground.rs`
- `specimen/crates/procgena_specimen/src/`: `lib.rs`, `bevy.rs`,
  `specimen.rs` (`Specimen`, `single`, `SpecimenError`, `Level`),
  `height_grid.rs` (all), `tag.rs` (module doc and the four mushroom tags);
  `specimen/README.md`
- `tree/crates/procgena_tree/src/lib.rs` (the Ground paragraph),
  `src/specimen.rs` (`Tree::specimen`, `Skeleton::specimen`), the frame
  comments in `src/scatter.rs` (was `src/grove.rs`)
- `deadwood/crates/procgena_deadwood/src/scatter.rs` (`place`, the re-export
  doc); the `specimen.rs` modules in deadwood and mushroom; `Stone::specimen`
- `ScaledGround` in `tree_bevy/src/scatter.rs` (was `src/grove.rs`) and
  `deadwood_bevy/src/spawn.rs`

## procgena_terrain (W1)

All under `terrain/crates/procgena_terrain/`:

- `src/elevation.rs`: module doc; `ElevationOptions` and every field;
  `HeightBump` and fields; `ElevationError` and variants; `validate`;
  `steepest`; `Elevation`; `flat`; `new`; `options`; `height`; `gradient`;
  `BUMP_SLOPE`
- `src/noise.rs`: `blend_corners`, `fbm_gradient`, `fbm_slope_bound`,
  `measure_steepest_noise`
- `src/calibration.rs`: `NOISE_SLOPE_MAX`
- `src/relief.rs`: `Relief`; `with_elevation`; `unfaded_height`;
  `own_relief`; `mean_height`; `height`; `height_above_elevation`
- `src/converge.rs`: `Fade::blended`
- `src/terrain.rs`: `TerrainGenerator::with_elevation`
- `src/options.rs`: `BaseOptions::elevation`
- `src/stitch.rs`: `StitchedWorld::build`
- `src/river.rs`: `trace_own`
- `src/plan.rs`: the source-ranking comment in `own_rivers`
- `src/road.rs`: `RIVER_COST`, `BRIDGE_COST`, `MIN_BRIDGE_SINE`,
  `WATER_STEP`, `route`, `water`, `Bridges` and its methods,
  `segments_meet`, the bridge comment in `search`
- `src/grid.rs`: `Grid::cells_near`
- `src/graph.rs`: the whole module, once it is committed
- `tests/common/mod.rs`: `Land`, `TILT`, `BUMP`, `Config::elevation`,
  `TILTED`, `CURVED`, `WITH_LAND`, `two_regions`
- `tests/elevation.rs`: every item
- `tests/features.rs`: `bridges`,
  `roads_cross_a_river_again_only_to_save_a_long_way_round`
- `tests/probes.rs`: `river_crossings_on_a_slope`

Added by the exact-E border fix (a41016a):

- `src/seam.rs`: the `negotiate` doc, `elevation_samples`, `relative_to`
- `src/terrain.rs`: `Seam`, the `base_height` comment, `Targets::raised`
- `src/relief.rs`: `elevation`, `gradient`
- `src/elevation.rs`: `is_flat`
- `tests/common/mod.rs`: `three_regions`
- `tests/elevation.rs`: `bare`, `bare_ground_is_e_everywhere`,
  `sample_points`

## procgena_core (F2)

All under `core/crates/procgena_core/`. The public items of `lib.rs` and
`hash.rs` are in the F1 block.

- `src/hash.rs`: the inline comment in `mix64`; the tests
  `seeds_match_reference_values`, `mix64_matches_reference_values`
- `src/content_hash.rs`: module doc; `CANONICAL_NAN_F32`,
  `CANONICAL_NAN_F64`; `ContentHasher` and each method (`new`, `finish`,
  `write`, `write_u64`, `write_u32`, `write_usize`, `write_i64`,
  `write_i32`, `write_bool`, `write_f32`, `write_f64`, `write_bytes`,
  `write_str`); `ContentHash` (with its doctest) and its `content_hash`
  method; the function `content_hash`; `OptionsHash`, `OptionsHash::of`;
  the impl docs for `[T]`, `[T; N]`, `Vec<T>`, `Option<T>`, `Range<T>`,
  `Vec2`, `Vec3`, `Vec4`, `Quat`, `IVec2`, `IVec3`, `UVec2`, `UVec3`; the
  section comments; the test `Sample` and `hashes_match_reference_values`
- `src/seed.rs`: module doc; `CHILD_KEY_MAX_LEN`; `ChildKey`,
  `ChildKey::from_static`, `new`, `as_str`, `salt`; `KeyError` and its
  variants; `KeyError::panic`; the function `check`; `child_seed`; the
  key-registry section comment; `SPECIMEN_SEED`; `KEYS`; the test
  `child_seed_matches_reference_values`
- `src/specimen_id.rs`: module doc; `GeneratorKind`, `from_key`, `salt`;
  `PoolId` and its fields; `PoolId::new`; `SpecimenId` and its fields;
  `SpecimenId::new`, `SpecimenId::seed`; the `ContentHash` impl docs of the
  three id types; the test `seeds_match_reference_values`

## procgena_core and procgena_core_bevy (R1)

The new `lib.rs` module-doc bullet and serde sentence in `procgena_core` are
part of the crate doc in the F1 block.

- `core/crates/procgena_core/src/lod.rs`: module docs; `FarForm` and its
  variants; `LevelForm` and its variants; `LevelForm::order`; `LodLevel`
  and its fields; `LodLevel::new`, `with_form`, `without_shadows`; `Lod`
  and its fields, and its doc example; `Lod::new`, `with_cull`,
  `map_detail`, `factor`, `ranges` (with its inline comments),
  `ranges_ending_at`, `level_at`, `shadow_level_at`, `levels_touched`,
  `validate`; `LodRange` and its fields; `LodRange::ALWAYS`,
  `dither_level`, `is_drawn`, `is_always`, `to_vec4`; `band_level`;
  `LodError` and its variants
- `core/crates/procgena_core/src/quality.rs`: module docs; `Quality` and
  its variants; `Quality::ALL`, `distance_scale`, `name`
- `core/crates/procgena_core/tests/lod.rs`: module docs, helpers and test
  comments
- `core/crates/procgena_core/Cargo.toml`: the `serde` feature comment
- `core/crates/procgena_core_bevy/src/lib.rs`: crate docs and their
  example; `DITHER_WESL`; `LodPlugin`; `LodQuality`; `visibility_range`;
  test comments
- `core/crates/procgena_core_bevy/src/shaders/dither.wesl`: header comment;
  `band_level`, `dither_level`, `is_drawn`, `switch_distance`;
  `DITHER_THRESHOLD_MAP`; `dither_discard`
- `core/crates/procgena_core_bevy/tests/dither.rs`: module docs; `compile`;
  `RENDERER_MAIN`; `shader_levels`

## procgena_core, procgena_stone and procgena_stone_bevy (C1)

- `core/crates/procgena_core/src/scatter/mod.rs` (was `scatter.rs`): module
  doc; `ScatterArea` and its fields, `new`, `world`, `contains`; `Mask` and
  `Mask::density`; `ScatterInput` and its fields, `new`, `with_mask`,
  `density`, `sample`; `steeper_than`; the test comments
- `core/crates/procgena_core/src/ground.rs` (its public items are in the F1
  block): `central_differences`; the new test comments
- `stone/crates/procgena_stone/src/options.rs`: `StoneOptions::variant`,
  `unit_hash`
- `stone/crates/procgena_stone/src/scatter.rs`: module doc; the constants;
  `StoneClass` and all its fields, `new`, `variant`, `validate`;
  `StoneScatterOptions` and its fields, `boulder_field`, `validate`,
  `Default`; `StoneScatterError` and its variants; `StoneInstance` and its
  fields; `StoneScatter`, `new`, `variant`, `instances`; the private
  `measure`, `patches`, `value_noise`, `lerp_log`
- `stone/crates/procgena_stone/tests/scatter.rs`: test comments
- `stone/crates/procgena_stone_bevy/src/pool/mod.rs`:
  `StonePool::variant_options`
- `stone/crates/procgena_stone_bevy/examples/viewer.rs`: module doc (the C
  key, `--scatter`); `SCATTER_SIZE`, `SCATTER_CELL`; `Viewer::scatter`;
  `show_scatter`

## procgena_deadwood, procgena_deadwood_bevy and procgena (wave 3 lane: F2, R1)

All under `deadwood/crates/`:

- `procgena_deadwood/src/seed_keys.rs`: module doc; every key constant
  (`GENERATOR_KIND`, `STEM_SHAPE`, `STEM_SURFACE`, `END_BASE`, `END_TIP`,
  `STUB_SHAPE`, `STUB_SURFACE`, `BRANCH_SHAPE`, `BRANCH_SURFACE`,
  `ROOT_SHAPE`, `ROOT_SURFACE`, `BARK_PATCHES`, `BARK_CRACKS`,
  `HOLLOW_WALL`, `LEAF_SPRAYS`, `LEAF_KEEP`, `SCATTER_VARIANT`,
  `SCATTER_CELL`); `KEYS`
- `procgena_deadwood/src/rng.rs`: module doc; `stream`; `RngExt`, `jitter`,
  `chance`
- `procgena_deadwood/src/build.rs`: the stream comment block above `build`;
  the comment above `sprouted` in `add_children`
- `procgena_deadwood/src/leaves.rs`: the comment above the leaf stream in
  `place`; `keeps`
- `procgena_deadwood/src/content_hash.rs`: module doc; `DEADWOOD_KIND`;
  `DeadwoodOptions::pool_id`; `ScatterOptions::variant_pool`; the impl docs
  for `DeadwoodOptions`, `PieceKind`, `EndKind` and `DeadwoodDetail`
- `procgena_deadwood/src/settle.rs`: `ANALYSIS_SIDES`
- `procgena_deadwood/src/detail.rs`: module doc; `DEFAULT_LOD_SIZE`;
  `DeadwoodLod`; `LodLevel`; `default_lod`; `lod_size`; `size_factor`;
  `validate_lod`; `sized_levels`
- `procgena_deadwood/src/options.rs`: `OptionsError::Lod`
- `procgena_deadwood/src/piece.rs`: `Piece::lod_meshes`
- `procgena_deadwood/src/specimen.rs`: the errors section of
  `Piece::specimen`; `specimen_from_meshes`
- `procgena_deadwood/src/lib.rs`: the crate-doc LOD paragraph and the
  example
- `procgena_deadwood/src/scatter.rs`: module doc; the `ScatterOptions`
  fields `default_seed` (was `seed`), `max_slope` and `piece`;
  `fallen_logs`; `Instance::position`; `StickScatter::new`, with its inline
  comments; the errors section of `StickScatter::specimen`; `Fit` and its
  fields; `PieceKind::placement`; `fit`, with its inline comments;
  `put_down`
- `procgena_deadwood/tests/content_hash.rs`: module doc, `GOLDEN` and the
  test comments
- `procgena_deadwood/tests/scatter.rs`: `small`, `small_area`, `scatter`,
  `stumps` and the test comments
- `procgena_deadwood_bevy/src/lib.rs`: `DeadwoodPlugin` and its `lod` field
- `procgena_deadwood_bevy/src/spawn.rs`: `DeadwoodScatterParams` and its
  fields `scatter` and `size`; `Input::FlatScatter`; `SpawnSettings::lod`;
  `VariantKey` and its fields; `BuiltVariant::key`, `BuiltVariant::lod`;
  `VariantAssets::lod`, `VariantAssets::size`; `build_variant`,
  `scatter_sticks`, `lod_hash`, `form_number`, `material_hash`,
  `variant_ranges`; the comment above `ranges` in `finish_generation`; the
  comments in the test
  `levels_hand_over_and_fade_out_at_the_cull_distance`
- `procgena_deadwood_bevy/examples/viewer.rs`: the `--scatter` doc line;
  `Scene::scatter`

Also in the studio:

- `procgena/src/scene/subject.rs`: `scatter_options`

## procgena_core, procgena_stone, procgena_stone_bevy, procgena_clump and procgena_clump_bevy (stone and clump lanes: F2, R1, Placement)

- `core/crates/procgena_core/src/scatter/mod.rs` (was `scatter.rs`; its
  module doc is in the C1 block): `Placement` and its fields and
  `Default`; `Rest` and its variants; `Footprint` and its fields; `Placed`
  and its fields; `place`, with its inline comments; `fit_plane`; the new
  tests' comments
- `stone/crates/procgena_stone/src/content.rs`: module doc; `STONE_KIND`;
  `StoneOptions::pool_id`; every `ContentHash` impl doc
- `stone/crates/procgena_stone/src/options.rs` (`StoneOptions::variant` is
  in the C1 block): `StoneOptions::ground_placement`; the `check_detail`
  inline comment; the new validation test comments
- `stone/crates/procgena_stone/src/detail.rs`: `StoneDetail::lod`,
  `LOD_REFERENCE_SIZE`, `StoneLevel` and its fields
- `stone/crates/procgena_stone/src/scatter.rs` (`StoneClass`, its fields,
  `new` and `variant`, and `StoneScatter::new` are in the C1 block): the
  inline comments in `place_member`
- `stone/crates/procgena_stone/tests/content_hash.rs`: test comments,
  `Change` (the test comments in `tests/scatter.rs` are in the C1 block)
- `stone/crates/procgena_stone_bevy/src/spawn.rs`: `StoneKey` and
  `StoneKey::of`; `level_ranges`; `bevy_range`
- `stone/crates/procgena_stone_bevy/src/pool/mod.rs`: `StonePool::options`,
  `StonePool::world_seed`, `StonePool::new`
- `stone/crates/procgena_stone_bevy/examples/viewer.rs`: the `world_seed`
  comment in `show_scatter`
- `clump/crates/procgena_clump/src/content.rs`: module doc; `CLUMP_KIND`;
  `ClumpOptions::pool_id`; every impl doc; `write_tag_list`, `write_tags`,
  `write_filter`
- `clump/crates/procgena_clump/tests/content_hash.rs`: test comments
- `clump/crates/procgena_clump_bevy/src/spawn.rs`: `follow_host_range`,
  `host_range`; the new test's comments
- `clump/crates/procgena_clump_bevy/src/lib.rs`: the host-rules bullet; the
  `spawn_systems` doc
- Prose: `docs/decisions/C1.md` and `ARCHITECTURE.md` open question 3

## procgena_mushroom and procgena_mushroom_bevy (wave 3 lane: F2, R1, C1, placement)

All under `mushroom/crates/`. This block joins the lane's three lists: the
first list, the C1 additions and the placement additions.

- `procgena_mushroom/src/rng.rs`: module doc; the seed-key section;
  `SHAPE_CAP`, `SHAPE_WARTS`, `SHAPE_NOISE`, `CLUSTER_BODY`,
  `CLUSTER_VARIATION`, `CLUSTER_LAYOUT`, `SCATTER_SITES`,
  `SCATTER_CLUSTER`; `KEYS`; `RngExt`, `jitter`, `int`
- `procgena_mushroom/src/specimen_id.rs`: module doc;
  `GENERATOR_KIND_KEY`; `GENERATOR_KIND`; `MushroomOptions::pool`; the
  content-hash section; the `ContentHash` impl for `MushroomOptions`
- `procgena_mushroom/src/shape.rs`: `NOISE_CAP`, `NOISE_STEM`,
  `NOISE_ANNULUS`
- `procgena_mushroom/src/cluster.rs`: `body_seed`, `make_bodies`, `arrange`
- `procgena_mushroom/src/mushroom.rs`: `MushroomBody`; `BodyId` and its
  fields; `Mushroom::new`, `Mushroom::lod_meshes`; `MushroomBody::new`,
  `single`, `id`, `lod_meshes`
- `procgena_mushroom/src/scatter.rs`: the `ScatterOptions` doc and the docs
  of its fields `default_seed` (was `seed`), `grows_on`, `up`,
  `follow_surface`, `specimens` and `placement`;
  `ScatterOptions::placement_for`; `MAX_SPECIMENS`;
  `ScatterError::TooLarge`; `MushroomCopy` and its fields;
  `MushroomScatter::on`, `specimen`, `instances`, `copies`
- `procgena_mushroom/src/options.rs`: `MushroomOptions::default_seed` (was
  `seed`); `MushroomOptions::ground_placement`; `OptionsError::Lod`
- `procgena_mushroom/src/detail.rs`: module doc; `DEFAULT_LOD_SIZE`;
  `MushroomLod`; `MushroomLodLevel`; `default_lod`; `validate_lod`
- `procgena_mushroom/src/specimen.rs`: `Mushroom::specimen`;
  `specimen_from_meshes`
- `procgena_mushroom/src/ground.rs`: module doc; `MAX_CELLS`; `MIN_CELL`;
  `BASE_RING`; `Candidate` and its fields; `MushroomScatter::new`, with its
  placement paragraph; `base_footprint`; the inline comments
- `procgena_mushroom/src/lib.rs`: the crate doc's first paragraph, its
  scatter paragraph, its LOD paragraph and its example
- `procgena_mushroom/tests/hash.rs`: module doc; `hash`; the comments in the
  tests
- `procgena_mushroom/tests/ground.rs`: all comments
- `procgena_mushroom_bevy/src/lib.rs`: the crate-doc LOD paragraph;
  `MushroomPlugin`, `MushroomPlugin::lod`
- `procgena_mushroom_bevy/src/spawn.rs`: `BodyKey`; `BuiltVariant::key`,
  `BuiltVariant::levels`; `VariantAssets::size`, `VariantAssets::levels`;
  `MushroomCache::materials`; `material_hash`; `build`; `build_variant`;
  `SpawnSettings::lod`; the spawn comment in `finish_generation`; the test
  helpers `margins` and `ranges` and the two LOD tests;
  `MushroomGroundScatterParams` and its fields; `Input::Ground`;
  `MushroomBuildError::Scatter`; the ground test
- `procgena_mushroom_bevy/examples/viewer.rs`: the `--flat`/`--billboard`
  comment; the ground patch comments; `GROUND_SIZE`, `GROUND_CELL`,
  `GROUND_Z`; `rolling`; `ground_mesh`

## procgena_core, procgena_stone, procgena_stone_bevy, procgena_tree, procgena_tree_bevy, procgena_deadwood, procgena_deadwood_bevy, procgena_mushroom, procgena_mushroom_bevy, procgena_clump, procgena_clump_bevy, procgena_terrain, procgena_biome, procgena_plant, procgena_plant_web and procgena (F3 presets and YAML files)

The lead's own list:

- `core/crates/procgena_core/src/preset/`: every item in every file (the
  crate doc in `lib.rs` is in the F1 block)
- `stone/crates/procgena_stone/src/presets.rs`: all items
- `stone/crates/procgena_stone/src/options.rs`: module doc; `StoneOptions`;
  `default_seed`; `MeshOptions::detail`; the `Mesher` doc (the `variant` doc
  is in the C1 block)
- `stone/crates/procgena_stone/src/stone.rs`: `from_preset`
- `stone/crates/procgena_stone/src/lib.rs`: the crate example
- `stone/crates/procgena_stone_bevy/src/spawn.rs`: `StoneParams::Asset`,
  `StonePresetAsset`, `StonePresetLoader`, `StoneGenerating`
- `stone/crates/procgena_stone_bevy/src/lib.rs`: crate docs; `StonePlugin`
- `tree/crates/procgena_tree/src/presets.rs`: all items
- `tree/crates/procgena_tree/src/scatter.rs`: module doc; `ScatterTree`,
  `ScatterTree::new`, `variant`; `TreeScatterVariation`; `TreeScatter` and
  its `default_seed`, `Default`, `new`; `TreeScatterPlacement::new`
- `tree/crates/procgena_tree/src/scatter/file.rs` and
  `src/scatter/presets.rs`: all items
- `tree/crates/procgena_tree/src/options.rs`: module doc; `TreeOptions`;
  `default_seed`; `PerLevel`; the `OptionsError::InvalidSetting::field`
  doc; the `PerLevel` deserialize doc; `ClusterShape::kind`
- `tree/crates/procgena_tree/src/mesh.rs`: the `LodDetail` doc
- `tree/crates/procgena_tree/src/content_hash.rs`: the `options_hash`,
  `specimen_id` and `cache_key` docs
- `tree/crates/procgena_tree/src/tree.rs`: module doc; `from_preset`; the
  `Tree` example
- `tree/crates/procgena_tree/src/lib.rs`: the crate doc's presets, preset
  files and Scatter sections
- `tree/crates/procgena_tree/presets/README.md`
- `deadwood/crates/procgena_deadwood/src/options.rs`:
  `DeadwoodOptions::kind`
- `procgena/src/export/mod.rs`: `preset_key`, `save_preset`, `read_preset`
- The studio's stone, deadwood and mushroom export panels in
  `procgena/src/export/`: module docs, the save and load functions,
  `read_options`
- `procgena/src/{stones,deadwood,mushrooms,clump}/mod.rs`: `START_PRESET`,
  `start_preset`, `Subject::start`, `settings`; and the editors'
  `file_stem` and `preset`
- Prose: `ARCHITECTURE.md` (rule 10, "Decisions to make", the TODOs, the
  survey table and two names); `CLAUDE.md` (the `procgena_core` row)

The sub-agents' lists, file level:

- clump: `clump/crates/procgena_clump/src/{lib,options,presets}.rs`;
  `clump/crates/procgena_clump_bevy/src/lib.rs`
- mushroom: `mushroom/crates/procgena_mushroom/src/{options,presets,mushroom,scatter,ground,specimen_id,lib}.rs`;
  its tests; `procgena_mushroom_bevy/src/{lib,spawn}.rs`; its viewer;
  `mushroom/README.md`
- deadwood: `deadwood/crates/procgena_deadwood/src/{presets,options,detail,piece,lib,settle,content_hash}.rs`;
  its tests; `procgena_deadwood_bevy/src/{spawn,lib}.rs`
- terrain and biome:
  `terrain/crates/procgena_terrain/src/{presets,options,lib,relief,elevation}.rs`;
  `terrain/crates/procgena_terrain/tests/common/`;
  `biome/crates/procgena_biome/src/{biome,presets}.rs`
- tree_bevy: `tree/crates/procgena_tree_bevy/src/{lib,spawn,scatter}.rs`,
  `src/scatter/*.rs`; its examples
- plant: `plant/crates/procgena_plant/src/{files,look,lib}.rs` and
  `src/presets/mod.rs`; `plant/crates/procgena_plant_bevy/src/{api,lib}.rs`
  and its examples; `plant/web`; both crates' `Cargo.toml`
- studio: the tree, grove, plant and field tabs
  (`procgena/src/{tree,grove,plant,field}/`); `procgena/src/studio/`;
  `procgena/src/scene/`; `procgena/src/environment.rs`

## procgena_terrain (W4)

All under `terrain/crates/procgena_terrain/`:

- `src/world.rs`: module doc; `DEFAULT_CACHE`; `WorldTerrainOptions` and
  its fields `seed`, `elevation`, `regions`, `convergence`, `cache`;
  `TerrainSource` and `TerrainSource::terrain_options`; `WorldError` and
  every variant; `WorldTerrain` (with its doctest); `WorldTerrain::new`,
  `options`, `elevation`, `graph`, `site`, `region_at`, `region`,
  `terrain`, `terrain_at`, `height`, `water_level`, and the private
  `owner`, `region_of`, `settings`, `plan`, `build_plan`, `build_terrain`;
  `impl HeightField for WorldTerrain::height`; the private `Settings`,
  `Plan` and their fields; `Slot`, `Cache`, `CacheState`, `Cache::get`
- `src/lib.rs`: the crate doc's `WorldTerrain` sentence under "How regions
  join"
- `tests/world.rs`: module doc and every helper and test doc comment

## procgena_core, procgena_tree, procgena_tree_bevy, procgena_stone, procgena_deadwood and procgena_mushroom (C2)

- `core/crates/procgena_core/src/address.rs`: all items
- `core/crates/procgena_core/src/seed.rs`: the `ChildKey` serde impls
- `core/crates/procgena_core/src/scatter/mod.rs` (the module doc,
  `ScatterInput` and its fields and `new` are in the C1 block):
  `ScatterArea::bounds`; `ScatterInput::with_prefix`, `with_exclusion`,
  `over`, `local_density`, `excluded`, `sample_at`
- `core/crates/procgena_core/src/scatter/{lattice,footprint,mask,placing,cache,conformance}.rs`:
  all items
- `tree/crates/procgena_tree/src/scatter.rs` (`ScatterTree::new` and
  `TreeScatterPlacement::new` are in the F3 block): `ScatterTree::key`,
  `custom`; `TreeScatterError::KeyClash`; `TreeCopy::address`, `position`,
  `footprint`; `TreeScatterPlacement` and its `instances`;
  `TreeScatter::slot`; the
  `impl Scatter for TreeScatter` doc; the private `Seeds`, `pick_tree`,
  `stand`, `in_glade`, `patch_value`, `Site`
- `tree/crates/procgena_tree_bevy/src/scatter.rs`: `PlacedTree::address`;
  `TreeScatterInstances::get`, `index_of`; the test's `Drawn`
- `tree/crates/procgena_tree_bevy/src/scatter/promote.rs`: `PromotedTree`,
  `PromotedTree::address`, `new`; `TreeScatterHit::address`
- `tree/docs/scatters.md`
- `stone/crates/procgena_stone/src/scatter.rs` (`StoneClass` and its
  fields, `StoneScatterOptions` and its fields and `validate`, the
  `StoneScatterError` variants, `StoneInstance` and its fields, and
  `StoneScatter::new`, `instances` are in the C1 block; the inline comments
  in `place_member` are in the stone and clump block):
  `StoneScatter::prepare`, `options`, `slot`, `stones`; the
  `impl Scatter` doc; the private `Candidate`, `Seeds`, `candidates_in`,
  `place_member`
- `stone/crates/procgena_stone/src/options.rs`: `variant_id`
- `deadwood/crates/procgena_deadwood/src/scatter.rs` (`Fit` and its fields
  and `put_down` are in the deadwood block): `Instance::address`,
  `footprint`; `StickScatter`, `prepare`, `sticks`, `instances`; the
  `impl Scatter` doc; the private `Candidate`
- `deadwood/crates/procgena_deadwood/src/options.rs`: `EndKind::Taper`
- `mushroom/crates/procgena_mushroom/src/scatter.rs` (`MushroomCopy` and
  its fields are in the mushroom block): the `MushroomScatter` doc and its
  `ground` field; `ScatterError::HostOnly`; `MAX_SITES`;
  `ScatterOptions::density`
- `mushroom/crates/procgena_mushroom/src/ground.rs` (`MAX_CELLS` and
  `MushroomScatter::new` are in the mushroom block):
  `MushroomScatter::prepare`; the `impl Scatter` doc; the private
  `GroundSpecimens`, `grow`, `widest`, `copies_over`

## procgena_core_bevy, procgena_stone_bevy, procgena_deadwood_bevy, procgena_mushroom_bevy and procgena_tree_bevy (R2 shared instanced renderer)

- `core/crates/procgena_core_bevy/src/instanced/mod.rs`: module docs;
  `InstancedPlugin`; `InstancedScatter` and its fields; `InstancedSlot` and
  its fields; `InstancedPart` and its fields; `PartMesh` and its variants;
  `ExternalMeshId`; `InstancedCopy` and its fields; `SpecimenCache` with
  `get`, `insert`, `get_levels`, `len`, `is_empty`; `CacheEntry`,
  `PRUNE_FRAMES`, `prune_cache`
- `core/crates/procgena_core_bevy/src/instanced/material.rs`: module docs;
  `INSTANCE_LOCATION`, `INSTANCE_SIZE`, `instance_buffer_layout`;
  `instanced_vertex`; `INSTANCED_MESH_SHADER`, `INSTANCED_PREPASS_SHADER`;
  `InstancedStandardMaterial`, `InstancedStandard`, `InstancedStandardKey`;
  `add_standard_material`, `InstancedKeyFn`, `InstancedMaterials`;
  `InstancedMaterialPlugin` and `new`; `register`
- `core/crates/procgena_core_bevy/src/instanced/render.rs`: module docs;
  every item (`InstancedSystems`, `ExternalMesh` and its fields,
  `ExternalMeshes`, and the private types and systems)
- `core/crates/procgena_core_bevy/src/instanced/tests.rs`,
  `src/instanced/render/gpu_test.rs`: test docs
- `core/crates/procgena_core_bevy/src/instanced/render/cull_test.rs`: all
- `core/crates/procgena_core_bevy/src/shaders/`: `instanced.wesl`,
  `instanced_mesh.wesl`, `instanced_prepass.wesl`, `instanced_cull.wesl`
- `core/crates/procgena_core_bevy/src/lib.rs`: `load_instanced_shaders`
- `stone/crates/procgena_stone_bevy/src/material.rs`: `TriplanarKey`,
  `StoneMaterialKey`
- `stone/crates/procgena_stone_bevy/src/pool/mod.rs`: `UNPLACED_RADIUS`;
  `PoolDefinitions`, `PoolDefinition`, `external_mesh_id`; `PoolDrawers`,
  `PoolDrawer`, `StonePoolPlugin`; `update_definitions`, `pool_material`,
  `pool_slots`, `collect_instances`
- `stone/crates/procgena_stone_bevy/src/pool/render/mod.rs`: module docs;
  `PoolMeshLayout`; `ExtractedPools`
- `stone/crates/procgena_stone_bevy/src/pool/render/pools.rs`: module docs;
  `RenderPools`; `prepare_pools`
- `stone/crates/procgena_stone_bevy/src/pool/render/gpu_test.rs`: module
  docs and test docs
- `deadwood/crates/procgena_deadwood_bevy/src/lib.rs`: the crate doc's
  drawing paragraph
- `deadwood/crates/procgena_deadwood_bevy/src/material.rs`: `DeadwoodKey`,
  `DeadwoodMaterialKey`, `leaf_material`
- `deadwood/crates/procgena_deadwood_bevy/src/spawn.rs`
  (`DeadwoodScatterParams` is in the deadwood block): module docs;
  `DeadwoodPart`; `Built::specimens`, `BuiltVariant::bounds`; `mesh_box`,
  `sphere_around`; `MaterialAssets`, `TextureSource`; `scatter_draw`,
  `level_parts`; `prune_cache`; the test helper `copies`
- `deadwood/crates/procgena_deadwood_bevy/src/shaders/deadwood_vertex.wesl`
  and `deadwood_prepass.wesl`
- `deadwood/crates/procgena_deadwood_bevy/examples/viewer.rs`: `Drawn`
- `mushroom/crates/procgena_mushroom_bevy/src/lib.rs`: the crate doc's
  drawing paragraph
- `mushroom/crates/procgena_mushroom_bevy/src/material.rs`: `MushroomKey`,
  `MushroomMaterialKey`
- `mushroom/crates/procgena_mushroom_bevy/src/spawn.rs`: module docs;
  `MushroomPart`; `Built::pieces`, `BuiltVariant::bounds`; `body_piece`;
  `mesh_box`, `sphere_around`; `TextureSource`; `scatter_draw`,
  `level_parts`; `prune_cache`; the test helper `copies`
- `mushroom/crates/procgena_mushroom_bevy/src/shaders/mushroom_vertex.wesl`
  and `mushroom_prepass.wesl`
- `mushroom/crates/procgena_mushroom_bevy/examples/viewer.rs`: `Drawn`
- `tree/crates/procgena_tree_bevy/src/scatter/draw.rs`: module docs;
  `SWAY_ALLOWANCE`; `GroveDraw` and its `hidden`; `update_grove_draws`,
  `grove_slots`, `variant_parts`, `variant_bounds`, `grove_copies`,
  `wind_phase`
- `tree/crates/procgena_tree_bevy/src/lib.rs`: the crate doc's
  grove-drawing sentence; `TreeMaterialKey`, `ImpostorMaterialKey`; the
  comment on the instanced material plugins
- `tree/crates/procgena_tree_bevy/src/scatter.rs`: the drawing sentence of
  `TreeScatterInstances`; the new comments in the promotion test
- `tree/crates/procgena_tree_bevy/tests/gpu.rs`: all
- Prose: `ARCHITECTURE.md` (the TODOs "Shared instanced renderer" and "One
  wind"; "Bringing your own assets"); `ROADMAP.md` (R2's status line);
  `CLAUDE.md` (the `procgena_core_bevy` row)

## procgena_tree, procgena_impostor and procgena_tree_bevy (I1b cluster cards)

- `tree/crates/procgena_tree/src/cluster_card.rs`: the whole module
- `tree/crates/procgena_tree/src/mesh.rs`: `LodDetail::cluster_cards`;
  `TreeMesh::fruit_cards`, `TreeMesh::flower_cards`
- `tree/crates/procgena_tree/src/cluster.rs`: `grow_lone_cluster`,
  `cluster_extent`
- `impostor/crates/procgena_impostor/src/key.rs`: `BakeLayout`;
  `BakeSettings::layout`, `BakeSettings::elevations`
- `impostor/crates/procgena_impostor/src/layout.rs`: the `elevation_*`
  functions (`elevation_direction`, `elevation_index`, `elevation_basis`,
  `elevation_of`)
- `impostor/crates/procgena_impostor/src/shaders/layout.wesl`:
  `elevation_index`
- `tree/crates/procgena_tree_bevy/src/material.rs`: `CARD_BAKE`,
  `card_material`, `WindExtension::card`, `WindKey::card`
- `tree/crates/procgena_tree_bevy/src/spawn.rs`: `CardBake`,
  `Entry::Baking`, `card_key`, `bake_cards`
- `tree/crates/procgena_tree_bevy/src/shaders/`: `turn_card` (in
  `tree.wesl`, `tree_prepass.wesl` and `tree_billboard.wesl`); `CardVertex`
  and `CARD_FRAMES` in `tree_billboard.wesl`
- `tree/crates/procgena_tree_bevy/examples/viewer.rs`: the
  `--cluster-cards` docs

## Lanes whose item lists are in the shared notes folder

Each lane's report in the coordinator's notes folder
(`~/Library/Application Support/Claude/side-session-notes/local_a7c3560a-043b-437f-a6b6-85f52de6e974/`)
lists its comments by file and item:

- **G4b terrain labels** (`a1dbdd3`, `7971222`, `b0f794d`): `g4b-report.md`.
- **GM2 ground textures** (`fd3425d`, `017315d`, `70eafeb`, `86421fd`):
  `gm2-report.md`, plus the two shader commits' changes to
  `terrain/crates/procgena_terrain_bevy/src/shaders/terrain_fragment.wesl`.
- **ED Ecotope draw** (`b9b2f40`, `78ae0a9`, `70f01ff`, `c9eb45e`):
  `ecotope-gpu-profile/comment-rewrite-queue.md`.
- **Valheim-sized terrain presets** (`6574da4`, `e3976ec`):
  `valheim-presets-report.md`.

## procgena and procgena_core (ST2 saving presets)

- `procgena/src/studio/documents/{history,panels,pickers,save}.rs`: every item
- `procgena/src/studio/project/mod.rs`: `Presets`, `ProjectPlugin`
- `core/crates/procgena_core/src/preset/registry.rs`: `builtin_root`,
  `base_for`, `find_from`, `resolve_from`
- `core/crates/procgena_core/src/preset/merge.rs`: `diff`
- `procgena/src/grove/mod.rs`: `choice_of`, `custom_choice`
- `procgena/src/ecotope/editor.rs`: `forward_tree_picks`
- `procgena/src/studio/kinds/mod.rs`: `Kind::settings`

## Lanes whose item lists were not saved (file level)

These lanes reported their comment lists only in session messages that are
no longer at hand. Rewrite the doc comments their commits added or changed
in these files (`git show <commits> -- <file>` shows which).

- **C3a, early Ecotope** (commits `8175cba`, `224de7b`, `9bd317e`, and the
  merge fix `b683fc3`): `biome/crates/procgena_biome/src/{lib,ecotope}.rs`,
  `src/ecotope/{file,footprints,presets,scatters}.rs`,
  `tests/ecotopes.rs`; `biome/crates/procgena_biome_bevy/src/lib.rs` and
  `examples/viewer.rs`.
- **Terrain G2, G1 and G3 steps 1 and 2** (commits `f86a9a4`, `a081d93`,
  `f7709bc`, `3c73967`, `6f85f2b`):
  `terrain/crates/procgena_terrain/src/{border,carve,converge,drainage,feature,gully,lib,noise,plan,region,relief,seam,stitch,terrain,world}.rs`,
  `tests/{bench,ground,world}.rs`.
- **Road builder RB0, RB1, RB3 and canonical TIN CT0–CT2** (commits
  `08cd6d1`, `a788b74`, `8b23ded`):
  `terrain/crates/procgena_terrain/src/road/{mod,search,level,quality}.rs`
  (`RouteStats`, `Scratch`, `search`, `least_gap`, `bands`, `rises`,
  `levels_within`, `road_quality`, `RoadQuality`), `src/grid.rs`
  (`Grid::with_cell_where`), `src/terrain.rs` (`Terrain::road_stats`,
  `moved`), `src/tin/*.rs` (every item), `tests/tin.rs`,
  `examples/{road_cost,tin_cost}.rs`.

## procgena_world (W0 weighted Biome selection, 2026-11-25)

- **W0a–W0f** (commits `59d9fa7`, `9e1634b`):
  `world/crates/procgena_world/src/options.rs` (`WorldOptions::biome_selection`,
  `BIOME_SELECTION_VERSION`, `MAX_ITERATIONS`, `BiomeSelectionOptions`,
  `BiomeSuitability`, `FieldPreference`, `BiomeCompatibility`,
  `CompatibilityPair`, `SelectionError`, `WorldOptionsError::Selection`),
  `src/world.rs` (`RegionBiome::pinned`, `RegionBiome::fields`, `World`,
  `World::biomes`, `Choice`, `Mode`), `src/selection/{mod,sampling,suitability,compatibility,relaxation}.rs`
  (every item), `tests/selection.rs` (module docs), `examples/large_world.rs`
  (module docs, `Args::selection`).
- **W0d.1** (commit `03786e3`): `world/crates/procgena_world/src/world.rs`
  (`Choice::graph`, `World::region_at`, `World::pin_region_at`,
  `World::pin_region`), `src/selection/relaxation.rs`
  (`lattice_neighbours`); `terrain/crates/procgena_terrain/src/graph.rs`
  (`RegionKey::lattice_neighbours`, `RegionGraphOptions::jitter`,
  `RegionGraph`, `Layout`, `MAX_CORNER_DRIFT`, `EdgePolicy`, `Edges`,
  `RegionGraph::lattice`, `with_edge_policy`, `site_jitter`, `warps`,
  `outline_at`, `outline_holding`, `Local::drift`), `src/hash.rs`
  (`GRAPH_CORNER_DRIFT`), `src/world.rs`
  (`WorldTerrain::with_lattice_regions`).

## procgena_terrain (RB4 canonical road network, 2026-11-25)

- **RB4** (commit `7b119a9` and the commit after it):
  `terrain/crates/procgena_terrain/src/road/network.rs` (every item),
  `src/road/align.rs` (every item), `src/road/assemble.rs` (every item),
  `src/road/mod.rs` (`RoadStart`, `Waters`, `Hub`, `Routes`,
  `RouteStats::sharp_bends`, `RouteStats::river_crossings`, `route`,
  `coarse_routes`, `refined_routes`, `level_middle`), `src/carve.rs`
  (`Features::new`, `OPEN_END`, `past_open_end`, `PoiKind`,
  `PointOfInterest::partner`, `Features::nearby`), `src/plan.rs`
  (`HUB_TAG`, `BORDER_TAG`, `PIECE_TAG`, `hub_node`, `piece_nodes`,
  `add_piece`, `waters`, `Roads`), `src/terrain.rs` (`Terrain::roads`,
  `Terrain::road_network`), `src/options.rs` (`RoadOptions::min_radius`,
  `MAX_ROAD_RADIUS`), `tests/features.rs` (`segment_distance`,
  `network_hashes`, `roads_leave_the_rivers_they_cross_open`,
  `road_networks_are_the_same_on_any_number_of_threads`, `along_river`).

## procgena_terrain and procgena_terrain_bevy (CT5a TIN products and streaming, 2026-11-25)

- `terrain/crates/procgena_terrain/src/tin/surface.rs` (every item),
  `src/tin/product.rs` (every item), `tests/tin_products.rs` (every item),
  `examples/tin_query.rs` (module docs).
- `terrain/crates/procgena_terrain_bevy/src/canonical.rs` (every item);
  `src/lib.rs` (`TerrainStream::tin`, `CanonicalChunk`, `Geometry`,
  `ChunkRead`, `ChunkRead::read_with_gpu`, `ChunkRead::stand_in`,
  `ChunkRead::tin`, `ChunkRead::mesh`, `StreamState::seams`, `seams_for`,
  `spawn_chunk`, the seam and canonical-ground blocks in `stream_chunks`,
  and the tests `canonical_hashes`, `Tops`, `drawn_edges`,
  `assert_canonical_seams_close`,
  `canonical_chunks_close_their_seams_and_keep_their_hashes`,
  `without_the_switch_every_chunk_is_regular`); `src/stream.rs`
  (`Stream::ground_height`); `examples/viewer.rs` (module docs for `T`,
  `--wireframe` and `--tin`, `toggle_debug`, the wireframe setup in
  `main`, `Scene::tin`, `LevelFigures::canonical`).

## procgena_terrain (CT3 rivers and ponds in the TIN, 2026-11-25)

- `terrain/crates/procgena_terrain/src/tin/water.rs` (every item),
  `src/tin/resolve.rs` (every item), `src/tin/tolerance.rs` (every item).
- `src/tin/refine.rs`: `build_chunk`, `follow_ground`, `Lattice`,
  `Lattice::sample`, `Lattice::domain`, `Lattice::lattice_of`,
  `Candidate`, `refine`, `Scorer`, `Scorer::score`, `Scorer::slope`,
  `CREASE_CLEARANCE`.
- `src/tin/contract.rs`: `EdgeCrossing`, `EdgeContract`,
  `EdgeContract::build`, `ChunkEdgeKey::from_corner`,
  `ChunkEdgeKey::end`, `CHUNK_MM`.
- `src/tin/cdt.rs`: `Label`, `ChunkCdt` (fields `labels`, `dropped`),
  `ChunkCdt::border_positions`, `ChunkCdt::add_lines`,
  `ChunkCdt::record_dropped`, `ChunkCdt::to_chunk`,
  `ChunkCdt::split_from`.
- `src/tin/mesh.rs`: `ConstraintKind` (new variants),
  `ConstraintKind::is_breakline`, `DroppedConstraint`,
  `TinChunk::dropped`. `src/tin/hash.rs`: the dropped block in
  `content_hash`.
- `src/chunk.rs`: `ChunkFeature`, `ChunkFeatureShape`,
  `ChunkFeature::of`, `ChunkFeature::order`,
  `ChunkSource::lines_within` and both implementations,
  `sorted_features`, `segment_meets_box`.
- `tests/tin.rs` (module docs and every item after
  `a_world_chunk_across_a_region_border_tiles_and_ignores_batching`),
  `examples/tin_cost.rs` (every item).
