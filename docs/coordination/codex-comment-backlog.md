# Codex comment-rewrite backlog

Comments the global instructions want rewritten by a Codex agent, still
pending because the Codex workspace ran out of credits on 2026-11-23. When
credits return, hand each block to one `full_access` Codex agent with the
standard brief (rewrite comments only, end-user audience, no code changes,
no state-changing git commands, report any bugs found). Then build the docs
and run the doctests for each package with `--all-features`. Remove a block
from this file once it is done.

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

## procgena_terrain (W1)

- Pending: the terrain worker sends its list with the W1 commit.
