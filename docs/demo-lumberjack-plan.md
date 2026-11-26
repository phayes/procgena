# Demo: the lumberjack and the wind (plan, draft)

**State: draft, waiting on the user's answers to "Questions for the user"
at the end.** Nothing here is built. Where a choice below says
"recommended", it is the plan's default until the user answers.

The brief (the user, 2026-11-25): *"It's a game where you are a lumberjack
and you can walk around the world. You also have a hand glider you can use
to fly when the wind is high. You can also whistle to call the wind. I want
fantastic atmospherics. Good lighting, light rays through trees, little
wisps of leaves floating, animal calls and whistles, gentle music."*

## Bottom line

- **One new binary crate, `procgena_demo_lumberjack`**, in
  `demo/crates/procgena_demo_lumberjack/`. It is a consumer of procgena,
  like any game: it uses only the public APIs of the terrain, World,
  Ecotope and asset Bevy crates, and adds no code to them. Gaps it finds
  become engine tasks on [ROADMAP.md](../ROADMAP.md), not demo-side
  patches. It is not part of the studio (question Q1).
- **Systems, as Bevy plugins inside the crate:**

  | Plugin | Owns | Reads | Writes |
  |--------|------|-------|--------|
  | `LandPlugin` | The World, terrain streaming around the player, Ecotope tiles in the play area | `WorldOptions` preset, player position | terrain chunks, Ecotope copies, ground colliders |
  | `PlayerPlugin` | Walking, the camera rig, input | colliders, input | player transform, `PlayerMode` |
  | `WindPlugin` | **`WindState`**: the one wind of the game, and the whistle | whistle input, time | tree `Wind`, `PlantWind`, the leaves, audio parameters |
  | `GliderPlugin` | Taking off, flying, landing | `WindState`, terrain slope, `PlayerMode` | player velocity, glider pose |
  | `SkyPlugin` | Sun, atmosphere, fog, light shafts, exposure, bloom, grading, time of day | camera, `WindState` (haze) | camera components, sun |
  | `LeavesPlugin` | Drifting leaves and seeds (GPU particles) | `WindState`, camera, nearby tree copies | particle effects |
  | `SoundPlugin` | Ambience beds, spatial animal calls, the whistle, wind sound, music | `WindState`, `PlayerMode`, time of day, nearby Ecotope | audio graph |
  | `ChopPlugin` (if in scope, Q3) | Felling, stumps, logs | the copy under the axe | engine edits |

- **Data flow.** `WindState` is the hub: the whistle raises its target,
  the wind eases towards it, and every frame it is copied into the tree
  `Wind` and `PlantWind` resources, the leaf emitters and the audio mixer.
  The glider reads it for lift. Nothing else owns wind.

  ```text
  whistle ──► WindState ──┬─► tree Wind + PlantWind (sway)
               ▲          ├─► LeavesPlugin (spawn rate, drift)
     time, gusts          ├─► SoundPlugin (wind bed, leaf rustle, music layer)
                          ├─► GliderPlugin (lift)  ──► player
                          └─► SkyPlugin (haze, cloud speed)
  WorldOptions ─► LandPlugin ─► terrain chunks + Ecotope tiles + colliders ─► PlayerPlugin
  ```

- **Major decisions (recommended, each open to the user):**
  1. **A bounded play area first, streaming later.** The first demo plays
     on one square of land, about 1.5 km across, inside a real World. The
     terrain already streams around a camera (`procgena_terrain_bevy`), but
     cover over a World waits on [B1](../ROADMAP.md#b1-biome-with-ecotopes),
     [W1](../ROADMAP.md#w1-world-with-cover) and
     [S1](../ROADMAP.md#s1-streaming). Rather than write its own cover
     streaming (which would duplicate S1), the demo places a fixed grid of
     Ecotope tiles once, with a loading screen, and fades the edge of the
     area into fog and impostors. When S1 lands the demo drops the tiles
     and the edge (milestone L8). Trade-off: a soft wall at the edge of the
     map until then (Q2).
  2. **Walking on physics colliders built from near terrain chunks,
     replaced by CT5's collision products later.** Avian (`avian3d` 0.8,
     already an optional dependency of `procgena_plant_bevy`) with the
     `bevy-tnua` character controller. Trunks and big stones get simple
     colliders from their placed copies. D6b already decided the canonical
     TIN is the collision surface; until
     [CT5](../ROADMAP.md#ct5-products-and-streaming) gives it, the demo uses
     the finest streamed chunk mesh near the player, which is what is drawn.
  3. **All atmospherics from Bevy 0.20 itself.** The workspace is on Bevy
     0.20.1, which has the procedural `Atmosphere` (with an environment
     map from it), `VolumetricFog` with `VolumetricLight` on the sun (light
     shafts from the shadow map), `FogVolume`, `DistanceFog`,
     `AutoExposure`, `Bloom`, `DepthOfField`, TAA, SSAO, contact shadows,
     `Vignette` and colour grading. No sky or fog crate is needed. The one
     real risk is whether procgena's own materials (instanced renderer,
     impostors, grass, terrain) work with all of them: milestone L2 is a
     spike that settles it before the look is tuned.
  4. **GPU leaves with `bevy_hanabi` 0.20** (Q6). It is the standard
     Bevy GPU particle crate and already targets Bevy 0.20.
  5. **Audio through `bevy_seedling` 0.9** (Firewheel), for real spatial
     audio, mixer buses and parameter automation the wind and music need
     (Q6). `bevy_kira_audio` is the more downloaded alternative but its
     spatial audio is only volume and panning.
  6. **Music as layered stems, mixed by state** (Q5): a few calm loops
     recorded in layers, faded in and out by time of day, wind and whether
     the player is flying. Generative music is a later option.
- **What the demo needs from engine work in flight:** CT5 (collision and
  height products), S1 (streaming cover), W0's selection work only for a
  prettier World, ED (fast first placement), IX (how near impostors can
  come: it decides the frame budget from a glider), the "One wind" rule
  (ARCHITECTURE.md, rule 9) and, if chopping is in, R4 and X1. None of them
  blocks the first walkable slice. See "What the demo needs from engine
  work".

## What procgena already gives a demo

Read on 2026-11-25 at `836b715`. For whether each milestone is done, see
its block in ROADMAP; this list says only what the demo can use.

| Piece | Where | What the demo gets | Gap for the demo |
|-------|-------|--------------------|------------------|
| World of Biomes | `procgena_world` (W0) | `World`, `.world.yml` presets, a Biome per region, region seeds, land through `WorldTerrain` | Biomes are empty (no Ecotopes yet, B0); cover is chosen by the demo |
| Terrain and streaming | `procgena_terrain`, `procgena_terrain_bevy` | `TerrainPlugin` streams chunks around a `TerrainViewer`, chunk levels by size on screen, rivers, roads, ponds as `TerrainWater`, ground materials and textures; `WorldTerrain` implements `Ground` | No colliders; the first build of a region is slow in dev builds (minutes, per CLAUDE.md), seconds-scale in release |
| Ecotopes | `procgena_biome`, `procgena_biome_bevy` | `EcotopeParams { ecotope, seed, ground, area, coverage, patch }` places a whole Ecotope (trees, stones, dead wood, mushrooms, plants) on any `Ground`, in the background, with painted ground layers | One area per entity, no streaming; tree Scatters are capped at 2048 m (`MAX_SCATTER_SIZE`) |
| Trees | `procgena_tree`, `procgena_tree_bevy` | Wind (`Wind` resource over `WindParams`), impostors, fruit as attached copies | Impostors do not sway |
| Grass and plants | `procgena_plant_bevy` | GPU grass with `PlantWind` (direction, strength, gust scale and speed, sway, flutter), `PlantSun`, shadows policy, trampling by interactors from avian or rapier colliders (`avian`, `rapier` features) | Grass wind and tree wind are separate resources (the "One wind" rule plans to merge them) |
| Stones, dead wood, mushrooms, fruit, clumps | each asset's crates | Drawn through the shared instanced renderer with the distance chain; dead wood is natural "log" scenery | No colliders built from `game_level` (deferred by IX) |
| Shared renderer | `procgena_core_bevy` | GPU-culled instanced copies with shadow and prepass support (`INSTANCED_PREPASS_SHADER`), dither LOD fades, quality levels | Whether volumetric fog, TAA motion vectors and SSAO all behave with it is unchecked (L2) |
| Studio | `procgena` binary | A place to author the demo's World, Ecotope and tree presets | Not a game shell; the demo stays out of it (Q1) |
| Environment code to learn from | `procgena/src/scene/environment.rs`, `plant_bevy/examples/walk.rs`, `plant_bevy/examples/avian.rs` | Sun, cascades, sky colour fog, one switch driving both winds; a walker trampling grass | Studio-only, not reusable as is |

**Bevy version:** 0.20.1 across the workspace.

## Architecture in detail

### Crate and files

`demo/crates/procgena_demo_lumberjack/`:

- `src/main.rs`: app setup, command line (`--headless --screenshot`,
  `--seed`, `--preset`, `--quality`), states (`Loading`, `Playing`,
  `Paused`).
- `src/land.rs`, `src/player.rs`, `src/glider.rs`, `src/wind.rs`,
  `src/sky.rs`, `src/leaves.rs`, `src/sound/` (beds, calls, whistle,
  music), `src/chop.rs` (if in scope).
- `presets/`: the demo's own `.world.yml`, Ecotope presets and a
  `demo.yml` with tunables (wind curve, glider polar, mix levels), read
  through `procgena_core::preset` so the studio can open the World and
  Ecotope presets.
- `assets/`: only small files the demo's licence lets it commit; sound and
  music come through a manifest and a download step, like the textures
  (see "Art and audio assets").
- `tests/`: headless tests (wind easing, glider maths, the whistle's state
  machine, tile layout) plus screenshot checks through
  `--headless --screenshot` (the bevy-screenshots skill).

Adding the crate touches the root `Cargo.toml` and `Cargo.lock`, which
only one agent may edit at a time. The new dependencies go in in **one
batched change, approved by the coordinator**, and the crate becomes a
workspace member only after its files exist.

### Land and cover (`LandPlugin`)

- Builds a `World` from the demo's `.world.yml`, picks a start region with
  forest-friendly fields, and gives `TerrainPlugin` a
  `TerrainStream::world(world_terrain)` with the player's camera as the
  `TerrainViewer`.
- **The play area** (recommended 1.5 km square, Q2): a grid of Ecotope
  tiles (for example 6 × 6 tiles of 256 m), each an `EcotopeParams` over
  the shared `Arc<WorldTerrain>` as `Ground`, all of one forest Ecotope
  with a coverage mask from terrain labels (no trees on water, roads or
  cliffs). Tiles are placed during the loading screen. Tile borders must
  not show; if per-tile placement makes visible seams, that is an engine
  question for C2's world-anchored ids, not something the demo papers over.
- **The edge**: beyond the area, terrain keeps streaming to the horizon,
  bare, under distance fog and the far-forest tint once
  [GM5](../ROADMAP.md#gm5-far-grass-and-forest-tints) exists. The glider's
  ceiling and a turning wind keep the player inside.
- **Colliders**: for each terrain chunk at the finest level within about
  100 m, an avian trimesh collider from the chunk mesh; trunks as capsules
  and big stones as convex hulls (or spheres) from each placed copy's
  transform and its specimen's bounds, within the same radius. Built and
  dropped as the player moves.

### Walking (`PlayerPlugin`)

- `bevy-tnua` with its avian backend: walk, run, jump, slope limit, step
  up roots and stones, crouch to pick up (later).
- Camera: third-person by default with a first-person toggle (Q4);
  third-person shows the glider and the axe and suits a screenshot demo.
- Input through `bevy_enhanced_input` (keyboard, mouse, gamepad), or
  Bevy's own input if the user prefers no extra crate (Q6).
- The player is a grass interactor (plant's `avian` feature), so grass
  parts around the feet as in `examples/walk.rs`.

### Wind and the whistle (`WindPlugin`)

One resource, `WindState`:

- `direction` (a slowly wandering angle), `strength` 0 to 1, `gustiness`,
  and `target_strength`.
- **Calm by default**: strength wanders between 0.1 and 0.3 on a slow
  noise.
- **The whistle**: a key (and a short whistle animation and sound) sets
  `target_strength` to about 0.85 for 30 to 60 s, then lets it fall back.
  The wind answers after a beat: first a distant rush in the audio and
  leaves lifting far off, then grass waves rolling towards the player,
  then full strength over about 4 s. A cooldown (about 20 s) stops
  spamming. The wind turns to blow roughly along where the player faces
  when whistling, so the player can aim a ridge for lift.
- **Every frame**, `WindState` is written into the tree `Wind` (its
  `WindParams`), `PlantWind` (direction, strength, gust scale and speed,
  flutter), the leaf effects' properties and the audio parameters. The
  demo maps one strength onto each crate's own scale; when the "One wind"
  rule (ARCHITECTURE.md, rule 9) merges plant and tree wind, the mapping
  shrinks to one write.
- Impostors do not sway today. From a glider at strong wind, far trees
  standing still is visible. Recorded as an ask to IX (impostor sway, even
  a cheap vertex wobble), not patched in the demo.

### Gliding (`GliderPlugin`)

A simple, arcade model tuned for fun, not a flight simulator:

- **Taking off** needs `WindState.strength` above a threshold (about 0.6)
  and either a run off a slope or a jump from a height. Below it, the
  glider will not open (the player hears the canvas flap and sees it sag).
- **State**: position, heading, airspeed, bank, pitch. Controls: bank
  (left/right), pitch (forward/back, trading speed for height), and a flare
  to land.
- **Sink polar**: sink rate rises with the square of the distance from the
  best-glide speed: `sink = s_min + k (v − v_best)²`, plus extra sink in
  steep banks. Tuned to about 10:1 glide ratio in still air.
- **Lift** (added to vertical speed):
  - **Ridge lift**: wind blowing up a slope rises:
    `lift_ridge = c_r · strength · max(0, wind · upslope) · f(height above ground)`,
    using the terrain's slope from `Ground` and fading out above about
    80 m.
  - **Thermals**: a few columns per square kilometre over sunny, open
    ground (terrain labels such as meadows and bare slopes), placed by
    seed, stronger at midday. Shown by circling birds and lifted leaves.
  - **Wind drift**: the glider drifts with the wind's horizontal vector.
- **Landing**: a flare near the ground bleeds speed; a hard landing
  stumbles the player (no damage). Trees are colliders, so flying into the
  canopy drops the player through branches to the ground, gently.
- **Look and sound**: the canvas ripples with airspeed, the camera pulls
  back and widens its field of view, wind roar rises with airspeed, and
  the music's flight layer fades in.

### Atmospherics (`SkyPlugin`, `LeavesPlugin`)

From Bevy 0.20 (component names checked in the 0.20.1 sources):

- **Sky and sun**: `Atmosphere` plus `AtmosphereSettings` on the camera,
  `AtmosphereEnvironmentMapLight` for ambient light that follows the sky,
  `SunDisk` on the `DirectionalLight`, real light units with
  `AutoExposure` so walking out of deep forest into a meadow blooms and
  settles.
- **Light shafts**: `VolumetricFog` on the camera, `VolumetricLight` on
  the sun (needs its shadow map), and a `FogVolume` around the player
  (denser low in valleys and in the morning) so shafts show through
  canopy gaps. Shadow cascades are tuned for forest: a short, sharp first
  cascade for leaves and trunks near the player.
- **Distance**: `DistanceFog` (atmospheric, coloured from the sun) sells
  depth from the glider and hides the play area's edge.
- **Post**: `Bloom` (soft), TAA (needed for foliage and the fog's noise),
  SSAO under the canopy, contact shadows, a light `Vignette`, colour
  grading per time of day, `DepthOfField` only in a photo mode.
- **Time of day**: a slow cycle (recommended: a fixed golden morning by
  default, cycle optional) driving sun angle, fog density, grading,
  ambience and music.
- **Leaves**: `bevy_hanabi` effects emitted from tree crowns near the
  player (the placed tree copies give positions), carried by `WindState`
  with curl-noise turbulence, landing and fading on the ground. More and
  faster in strong wind; seed fluff and pollen motes in light shafts in
  calm air. One effect per kind, emitter positions updated as the player
  moves, so particle count stays bounded.
- **Small life**: butterflies and birds as cheap instanced sprites or
  hanabi particles, not simulated animals (Q3 if more is wanted).

**Risk, settled first (L2):** whether procgena's materials and Bevy's
effects work together. Today the shared renderer draws into shadow maps
and the depth, normal and motion-vector prepass, and terrain has a prepass
pipeline; but impostors are excluded from shadows while baking, grass has
its own shadow policy and `PlantSun`, and none of it has been seen under
volumetric fog, TAA, SSAO and the atmosphere together. L2 checks each
material and lists what to ask of the engine crates.

### Sound and music (`SoundPlugin`)

- **Buses**: ambience, wind, creatures, player (footsteps, whistle, axe),
  music, each with its own level in the menu.
- **Ambience beds**: looping stereo beds per place (deep forest, meadow,
  river, ridge), cross-faded by what is around the player (the Ecotope's
  tree density and the terrain's labels, water distance).
- **Spatial calls**: birds, a woodpecker, an owl at dusk, frogs near
  ponds, a distant deer bark. Emitters are spawned at random spots in
  suitable places around the player (trees for birds, water for frogs),
  with spatial panning, distance fade and a low-pass with distance, and
  call-and-answer timing between two birds.
- **The whistle**: the player's two-note whistle; birds sometimes answer
  it; the wind answers it (a far rush first).
- **Wind**: a layered wind bed whose level and filter follow
  `WindState.strength`, leaf rustle near trees, and airspeed roar on the
  glider.
- **Footsteps** by ground material (the painted ground layers say grass,
  needles, soil, rock).
- **Music**: three or four calm pieces, each recorded as stems (for
  example a base pad, a melodic line, a flight layer), mixed by time of
  day, wind and flight. Silence between pieces is part of the design.

## Milestones

Each milestone ends in something to look at, a headless screenshot or a
short recording, and a "Done when". Numbers are on the reference machine
(Q7).

### L0: Crate and dependencies

- The crate, its command line, states and an empty scene; the batched
  dependency change through the coordinator.
- **Done when:** `cargo run -p procgena_demo_lumberjack -- --headless
  --screenshot <png>` writes an image of a sky and a flat ground, and the
  crate's clippy and tests pass.

### L1: Walkable vertical slice

- World land streaming around the player, one forest Ecotope over the
  play area in tiles, colliders, `bevy-tnua` walking, third-person camera,
  a plain sun and `DistanceFog`, grass parting at the feet.
- **Done when:** a player can walk across the play area over hills, along
  a road and among trees without falling through, getting stuck or
  passing through trunks; tile borders are invisible; the loading screen
  takes under 30 s in release on the reference machine.

### L2: Atmospherics spike

- Turn on every effect from "Atmospherics" in one scene and check each
  procgena material against it: shadows into the volumetric fog, prepass
  for TAA and SSAO, fog on impostors and grass, exposure.
- **Done when:** a written list says, per material, what works and what
  needs an engine change, each such change is filed against its crate's
  milestone, and side-by-side screenshots show forest with and without
  each effect.

### L3: Wind and the whistle

- `WindState`, the whistle, every consumer wired (trees, grass, audio
  placeholder, haze).
- **Done when:** a whistle visibly rolls a wind front across grass and
  trees towards the player within 2 s and builds to full strength in about
  4 s, decays after its hold time, and headless tests pin the easing and
  the cooldown.

### L4: Gliding

- Take-off rules, the flight model, ridge lift, thermals, landing, the
  camera.
- **Done when:** with whistled wind, a player can take off from a ridge,
  stay up for more than 60 s using ridge lift and thermals, cross the play
  area, and land; in calm air the glider won't open; the flight model's
  maths has headless tests; the frame rate holds the target while flying
  (needs IX's findings).

### L5: Full atmospherics

- Tuned atmosphere, light shafts, exposure, grading, time of day, leaves
  and motes with hanabi, birds and butterflies.
- **Done when:** the user signs off a set of reference shots (deep forest
  with shafts, meadow at golden hour, ridge in strong wind with leaves,
  the view from the glider) and the frame budget holds.

### L6: Sound and music

- Buses, beds, spatial calls, whistle, wind, footsteps, music stems and
  their mixing.
- **Done when:** a five-minute walk and flight has no audible loop seams
  or clicks, calls come from believable places, the wind audio follows
  the visuals, and every file's licence is in the asset manifest.

### L7: Gameplay (scope set by Q3)

- Recommended minimum: an axe that fells marked trees with a satisfying
  fall, a stump left behind and a log to pick up; a small goal (deliver
  logs to a cabin, or reach a far ridge by glider). Felling needs engine
  edits (see below).
- **Done when:** the chosen loop can be played start to end, and a felled
  tree stays felled from near and far within the session.

### L8: Streaming world (after S1)

- Replace the tiles with S1's streaming, drop the edge, CT5 collision
  replaces chunk-mesh colliders.
- **Done when:** a player can walk or fly in a straight line for 10 km
  without a hitch over the frame budget and without seeing land or cover
  pop in.

## What the demo needs from engine work

Status lives in each ROADMAP block; this says what the demo needs and when.

| Engine milestone | The demo needs | From milestone |
|------------------|----------------|----------------|
| [CT5](../ROADMAP.md#ct5-products-and-streaming): TIN products and streaming | Collision surfaces and height queries matching the drawn ground; replaces L1's chunk-mesh colliders | L8 (L1 works without it) |
| [S1](../ROADMAP.md#s1-streaming): streaming | Cover streaming around the player; ends the play area's edge | L8 |
| [W0](../ROADMAP.md#w0-world-of-bare-land): World selection | Varied, natural Biome layout around the play area; the demo only needs today's World to pick a start region | L1 (already enough) |
| [B1](../ROADMAP.md#b1-biome-with-ecotopes), [W1](../ROADMAP.md#w1-world-with-cover) | Cover chosen by the World instead of by the demo | L8 |
| [ED](../ROADMAP.md#ed-ecotope-draw-speed): Ecotope draw speed | Fast first placement of many tiles (loading time) | L1 |
| [IX](../ROADMAP.md#ix-experiment-lean-heavily-on-impostors): impostors | How close impostors can come, and the frame budget from the glider; impostor sway in wind is an ask | L4, L5 |
| "One wind" (ARCHITECTURE.md, asset alignment rule 9) | One wind resource for trees, grass and clumps | Nice for L3; the demo maps both until then |
| [GM5](../ROADMAP.md#gm5-far-grass-and-forest-tints) | Far forest tint beyond the play area | L5 |
| [R4](../ROADMAP.md#r4-promotion-and-bevy-queries-everywhere) and [X1](../ROADMAP.md#x1-edits-interaction-core-queries-hooks-and-meshscatter) | Promoting the tree under the axe, removing and replacing copies (felled tree, stump), colliders from `game_level` (deferred by IX) | L7, only if chopping is in |
| [FF](../ROADMAP.md#ff-the-far-field-drawn-to-the-horizon-later) | Forest to the horizon from the glider | After L8 |

## What can run in parallel with the engine work

- **Now, with no engine change:** L0, L1 (bounded area), L2, L3, L4's
  flight model and tests, all of L6's audio code and asset sourcing, the
  art direction and reference shots. The demo is a consumer, so it rebuilds
  only when the crates it uses change.
- **Waits on engine:** L4's frame budget (IX), L5's far view (GM5, IX),
  L7's felling (R4, X1), L8 (S1, CT5, W1).
- **Contention to plan for:** the demo build pulls in every asset's Bevy
  crate, so it rebuilds after most engine changes. Demo workers run scoped
  checks only (`-p procgena_demo_lumberjack`) and in the background, per
  CLAUDE.md.

## Art and audio assets

**The workspace licence is MIT**, so anything committed must allow
redistribution in source form. Raw sound libraries usually allow use in a
game but forbid redistributing the raw files, so **sounds and music are
listed in a manifest (`demo/.../audio.yml`: source, licence, attribution,
checksum) and downloaded by a tool step**, like the textures
(`uv run --project tools textures download ...`). Only CC0 files may be
committed. Licence findings below come from third-party summaries in a
web search on 2026-11-25: **verify each at its source before use.**

| Need | Source options | Licence notes |
|------|----------------|---------------|
| Ambience beds, wind, foley, footsteps | Sonniss GDC bundles; Freesound (CC0 and CC-BY only); Kenney audio packs | Sonniss: royalty-free, embedded use only, no standalone redistribution, no AI training (verify the current EULA). Freesound: per file, avoid CC-BY-NC. Kenney: CC0. |
| Animal calls | Freesound CC0/CC-BY; Sonniss nature sets; commissioned or own field recordings | xeno-canto licences are per recording and mostly non-commercial (CC BY-NC-SA): ideas and reference only unless the recordist agrees. Use calls of real species with care, or stylised, made-up calls (fits a fantasy forest). |
| Whistle | Record it (any phone, a quiet room) or synthesise | Own recording: no licence issue. |
| Music | Commission three or four stem-based pieces; CC0 music; CC-BY music with credit | Commissioned music: get the stems and a licence to redistribute in the download. |
| Textures | Already handled by the asset crates' `textures.yml` | — |
| Character, glider, axe | Simple, original low-poly models made for the demo (glTF), or procedural meshes | Original only, no look borrowed from an existing game. |
| UI | Minimal: a whistle cooldown ring, a wind gauge, a log count | — |

**Art style** (Q8): the procgena assets are realistic-leaning PBR. The
recommendation is a painterly-realistic look by grading, light and
atmosphere alone, not a new stylised shading model.

## Performance targets

Proposed, to be confirmed against "Test machines and performance budgets"
(ROADMAP, "For the user", item 18), which is still open:

- **Reference machine**: an M1 Max at 1440p (the machine the D2 spike
  used). Target 60 fps walking, 60 fps gliding at 300 m above ground, no
  frame over 33 ms after loading.
- **Low machine** (if one is named): 30 fps at 1080p on the lowest quality
  level, with volumetric fog at half resolution or off.
- **Loading**: under 30 s to play in release; whistle-to-wind under 2 s.
- **Budgets to measure in L2 and L5**: volumetric fog (expected the most
  expensive effect: a few ms), shadow cascades with foliage, particle
  count (leaves under about 20 000 live), audio voices (under 32).
- Each quality level (the shared `Quality`) maps to a set of atmospheric
  settings.

## Dependencies the demo would add

All support Bevy 0.20 (checked on crates.io, 2026-11-25). Downloads are
all-time / last 90 days.

| Purpose | Crate | Version | Downloads | Licence | Notes |
|---------|-------|---------|-----------|---------|-------|
| Physics | `avian3d` | 0.8.0 | 546k / 217k | MIT or Apache-2.0 | Already optional in plant_bevy |
| Physics (alt.) | `bevy_rapier3d` | 0.37.0 | 647k / 78k | Apache-2.0 | Already optional in plant_bevy |
| Character controller | `bevy-tnua` + `bevy-tnua-avian3d` | 0.33.2 / 0.13.2 | 127k / 17k; 57k / 8k | MIT or Apache-2.0 | Floating-capsule controller, mature |
| Controller (alt.) | `bevy_ahoy` | 0.3.0 | 4k / 2k | MIT or Apache-2.0 | Young; kinematic; avian |
| Controller (alt.) | none: own kinematic controller | — | — | — | More code, no dependency |
| Particles | `bevy_hanabi` | 0.20.0 | 229k / 64k | MIT or Apache-2.0 | GPU particles; WebGPU only on wasm |
| Audio | `bevy_seedling` (Firewheel) | 0.9.0 | 39k / 12k | MIT or Apache-2.0 | Spatial audio, buses, automation; young, very active |
| Audio (alt.) | `bevy_kira_audio` (Kira) | 0.27.1 | 576k / 57k | MIT or Apache-2.0 | Mature; spatial is volume and panning only |
| Audio (alt.) | Bevy's own `bevy_audio` | 0.20.1 | — | — | No dependency; spatial panning only, no buses or filters |
| Synthesis (optional) | `fundsp` | 0.23.0 | 288k / 95k | MIT or Apache-2.0 | For a synthesised wind bed or generative music |
| Input | `bevy_enhanced_input` | 0.27.2 | 247k / 97k | MIT or Apache-2.0 | Contexts and gamepad mapping |
| Input (alt.) | `leafwing-input-manager` | 0.22.0 | 670k / 146k | MIT or Apache-2.0 | Older, widely used |

No sky or fog crate: `bevy_atmosphere` (last release May 2025) is
superseded by Bevy's own `Atmosphere`.

## Questions for the user

- **Q1. Standalone binary or part of the studio?** Recommended: a
  standalone binary crate, so the demo is a true consumer and the studio
  stays an editor. The studio can still open the demo's presets.
- **Q2. Bounded play area now, or wait for streaming (S1)?** Recommended:
  a 1.5 km area now, with a soft edge, replaced by S1 later.
- **Q3. How much gameplay? Is chopping in scope?** Chopping needs R4
  (promotion) and X1 (edits) from the engine, so it would land late (L7)
  unless those move up. Options: (a) no chopping, a pure wander-and-fly
  mood piece; (b) chopping marked trees with a stump and a log, and a
  small delivery goal (recommended, after R4 and X1); (c) a fuller loop
  (carry logs, build something, a day's goals).
- **Q4. First-person or third-person?** Recommended: third-person with a
  first-person toggle.
- **Q5. Music: stems or generative?** Recommended: commissioned or sourced
  stems mixed by state; generative (with `fundsp`) as a later experiment.
- **Q6. New dependencies.** Recommended set: `avian3d` with `bevy-tnua`,
  `bevy_hanabi`, `bevy_seedling`, `bevy_enhanced_input`. Alternatives are
  in the table above. Which are acceptable?
- **Q7. Target platforms and hardware?** Recommended: macOS (Apple
  silicon) first, then Windows and Linux desktop; no web build (volumetric
  fog and hanabi need WebGPU). Which machine is the reference, and is a
  low-end machine available?
- **Q8. Art style?** Recommended: painterly-realistic by light and grading
  on today's assets.

## Follow-ups for the coordinator

- Add a demo milestone block to ROADMAP once the user approves the plan
  (this plan does not edit ROADMAP).
- Engine asks raised by this plan: impostor sway in wind (IX), whatever L2
  finds about procgena materials under Bevy's atmospherics, and colliders
  from `game_level` (deferred by IX) if chopping is in.
