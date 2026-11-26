# Demo: the lumberjack and the wind (plan, draft)

**State: draft, waiting on the user's answers to "Questions for the user"
at the end.** Nothing here is built. Where a choice below says
"recommended", it is the plan's default until the user answers.

**It is an experience more than a game** (the user, 2026-11-25: "Its
more of an experience than a game I guess"; "The works for a small
engaging game"). It is light on goals and mechanics and heavy on mood,
wandering, gliding and sound, with the full set of atmospheric effects,
the sun's lens flare included.

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
  | `PlayerPlugin` | Walking, the rigged lumberjack and its animation graph, the camera rig, input | colliders, input | player transform, `PlayerMode`, animation state |
  | `WindPlugin` | **`WindState`**: the one wind of the game, and the whistle | whistle input, time | tree `Wind`, `PlantWind`, the leaves, audio parameters |
  | `GliderPlugin` | Taking off, flying, landing | `WindState`, terrain slope, `PlayerMode` | player velocity, glider pose |
  | `SkyPlugin` | Sun, atmosphere, fog, light shafts, exposure, bloom, the sun's lens flare, grading, time of day | camera, `WindState` (haze) | camera components, sun, flare pass |
  | `LeavesPlugin` | Drifting leaves and seeds (GPU particles) | `WindState`, camera, nearby tree copies | particle effects |
  | `SoundPlugin` | Ambience beds, spatial animal calls, the whistle, wind sound, music | `WindState`, `PlayerMode`, time of day, nearby Ecotope | audio graph |
  | `ChopPlugin` (if in scope, Q3) | Felling a tree as a small ritual: the swing, the fall, a stump | the copy under the axe | engine edits |

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
  7. **A sun lens flare drawn by the demo.** Bevy 0.20 has no lens flare
     and no maintained crate offers one, so the demo adds a small
     screen-space flare pass of its own (see "Atmospherics").
  8. **A ready-made rigged character and animation clips (CC0),** not a
     procedural or self-made one. Quaternius's Universal Base Characters
     with the Universal Animation Libraries share one humanoid rig, so
     their clips play on the character with no retargeting. The few poses
     no library has (hanging under the glider, the whistle) are authored
     on that rig. Nothing is downloaded until the user picks from the
     shortlist (Q9).
- **What the demo needs from engine work in flight:** CT5 (collision and
  height products), S1 (streaming cover), W0's selection work only for a
  prettier World, ED (fast first placement), IX (how near impostors can
  come: it decides the frame budget from a glider), the "One wind" rule
  (ARCHITECTURE.md, rule 9) and, if chopping is in, R4 and X1. None of them
  blocks the first walkable slice. See "What the demo needs from engine
  work", and "Roadmap requests" for the changes in order the demo asks
  the user for.

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

### The lumberjack's body and movements

- **The model** is a rigged glTF (GLB) humanoid loaded with Bevy's glTF
  loader, dressed as a lumberjack (plaid shirt, beard, cap) by a texture
  and hair choice, or by swapping in outfit parts on the same rig. The
  axe and the folded glider hang from hand and back bones.
- **Clips** play through Bevy's `AnimationGraph`: a locomotion blend
  (idle, walk, run by speed), one-shots (jump, fall, land, whistle, chop)
  and glider states (launch, hang, flare, land). Root motion is not used:
  the controller moves the body and the clips follow its speed.
- **Sharing a skeleton.** Bevy finds each clip's bones by their names
  along the hierarchy, and it has no retargeting at run time. A clip from
  another file plays on the character only if the bone names and
  hierarchy match exactly. So every clip must come from the character's
  own rig family, or be retargeted once, offline in Blender, onto it.
  The first step after download is a headless check that every chosen
  clip moves the chosen model.
- **Clips no library has:**
  - **Hanging under the glider**: the body hangs from the bar, arms up,
    legs trailing; a short authored loop, with the lean added at run time
    from bank and pitch (a small additive or IK offset on the spine).
  - **The whistle**: fingers to the mouth, a breath; a short authored
    clip, upper body only, so it plays while walking (a masked blend).
  - **Glider launch and landing** may borrow a jump and a landing clip,
    re-timed.
  Authoring is a few hours in Blender on the rig; the demo needs a
  person to do it, or the hang pose can start as a pose set in code.
- **Feet on slopes**: a two-bone foot IK pass so feet rest on uneven
  ground and roots (later in L5; a polish item).

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
  grading per time of day, a touch of `MotionBlur` at glider speed, faint
  `ChromaticAberration` at the screen edge, and `DepthOfField` in a photo
  mode and for a soft background when the camera is close.
- **Sun lens flare** (the user, 2026-11-25). Bevy 0.20 has no lens flare
  (checked in its 0.20.1 sources) and crates.io has no maintained Bevy
  crate for one, so the demo writes a small post-process pass:
  - project the sun to the screen;
  - measure how much of it is visible from a few depth samples around
    that point, smoothed over a few frames, so the flare fades as the
    sun slips behind trunks and leaves (this is what makes it lovely in
    a forest);
  - draw a soft halo, a faint starburst, and a few ghosts along the line
    through the screen centre, tinted by the sun's colour from the
    atmosphere and scaled by exposure;
  - add a subtle lens dirt texture lit by the bloom.
  It is one render-graph node drawing into the HDR image before bloom and
  tone mapping, so exposure and bloom treat it like real light, with its own
  settings in `demo.yml`. If it turns out generally useful, it could move
  to `procgena_core_bevy` later.
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
- The chosen character (Q9) with idle, walk, run, jump, fall and land
  blended by speed.
- **Done when:** a player can walk across the play area over hills, along
  a road and among trees without falling through, getting stuck or
  passing through trunks; the lumberjack's feet don't slide at any speed;
  tile borders are invisible; the loading screen takes under 30 s in
  release on the reference machine.

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
  camera, and the glider clips (launch, hang loop leaning with bank and
  pitch, flare, landing).
- **Done when:** with whistled wind, a player can take off from a ridge,
  stay up for more than 60 s using ridge lift and thermals, cross the play
  area, and land; in calm air the glider won't open; the flight model's
  maths has headless tests; the frame rate holds the target while flying
  (needs IX's findings).

### L5: Full atmospherics

- Tuned atmosphere, light shafts, exposure, grading, time of day, the
  sun's lens flare, motion blur and the rest of the post, leaves and
  motes with hanabi, birds and butterflies, foot IK.
- **Done when:** the user signs off a set of reference shots (deep forest
  with shafts and the sun flaring through a gap, meadow at golden hour,
  ridge in strong wind with leaves, the view from the glider) and the
  frame budget holds.

### L6: Sound and music

- Buses, beds, spatial calls, whistle, wind, footsteps, music stems and
  their mixing.
- **Done when:** a five-minute walk and flight has no audible loop seams
  or clicks, calls come from believable places, the wind audio follows
  the visuals, and every file's licence is in the asset manifest.

### L7: Gentle purpose (scope set by Q3)

An experience, not a game: no score, no inventory, no failure.

- Recommended: a few quiet invitations rather than goals. Cairns on far
  ridges that the glider reaches (each one a view and a music cue);
  birds that answer the whistle and lead somewhere; a cabin to return to
  at dusk. And, if chopping is in, felling a tree as a small ritual: a
  few swings, the creak, the fall through the canopy, a stump left
  behind. Felling needs engine edits (see "Roadmap requests").
- **Done when:** a first-time player, given no instructions, finds the
  whistle, flies, and reaches a cairn within ten minutes; and, if
  chopping is in, a felled tree stays felled from near and far within the
  session.

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

## Roadmap requests

The user allows the demo to ask for changes in the order of engine work
(2026-11-25). These are requests for the user to decide; this plan does
not edit ROADMAP. They are ordered by how much they matter to the
experience.

| # | Milestone | What the demo needs | Proposed change | Why | What it would delay |
|---|-----------|---------------------|-----------------|-----|---------------------|
| R1 | [IX](../ROADMAP.md#ix-experiment-lean-heavily-on-impostors) | How close impostors can come, at what frame cost; **impostors that sway in the wind** | Keep IX high; **add** impostor sway (a cheap vertex wobble driven by the shared wind) as an IX item | From the glider almost every tree is an impostor; a forest frozen under a whistled gale breaks the moment the demo is built around | Little: a small shader item inside IX |
| R2 | New item, `procgena_biome_bevy` | Read access to a placed Ecotope's copies (position, scale, specimen bounds, kind) from Bevy, for example on `EcotopeFinished` | **Add** a small item: hand out the engine-free `EcotopePlacement` (it already holds the copies) once placing finishes | The demo builds trunk colliders, leaf emitters in crowns and bird-call spots from it; without it the demo must place the Ecotope twice | Nothing measurable; a few hours |
| R3 | "One wind" (ARCHITECTURE.md, asset alignment rule 9) | One wind resource and shader module for trees, grass and clumps | **Move earlier**, as a demo-sized slice: one shared wind resource in `procgena_core_bevy` that tree and plant read, before the full shader merge | The whistle is the heart of the experience; one source of wind keeps trees and grass in step and removes the demo's mapping layer | Touches every asset's Bevy crate (one rebuild wave); otherwise small |
| R4 | [ED](../ROADMAP.md#ed-ecotope-draw-speed) | Fast first placement of the play area's tiles, and no shader-compile stalls after the loading screen | **Un-defer** the pipeline warm-up as an API a game can call behind a loading screen | 0.3–1.6 s stalls in the first minute are the first thing a player notices; a studio can live with them, an experience can't | Small; ED's other items unchanged |
| R5 | [GM5](../ROADMAP.md#gm5-far-grass-and-forest-tints) | Far forest tint beyond the play area | **Move earlier**, ahead of L5 | Until S1, everything outside the play area is bare land; from the glider the tint is what makes it read as forest to the horizon | Whatever GM5 currently follows in the ground materials track |
| R6 | [S1](../ROADMAP.md#s1-streaming) | Cover streaming around the player | **Cut down to a demo-sized slice**: single-Biome streaming over one Terrain on CPU placement, which S1 already lists as able to start earlier (needs B1, G6, R2, I2), ahead of GP | Ends the play area's edge, so the player can glide anywhere | GP and R3's GPU building for deadwood, mushroom and tree move later |
| R7 | [R4](../ROADMAP.md#r4-promotion-and-bevy-queries-everywhere) and [X1](../ROADMAP.md#x1-edits-interaction-core-queries-hooks-and-meshscatter) | Promote the tree under the axe; Remove and Replace edits for tree copies (felled tree, stump) | **Only if chopping is in (Q3):** **cut down** to a demo slice: promotion and Remove/Replace edits for tree copies in one Ecotope, in session memory (no saving) | Chopping as a ritual needs only these two edits | C2's remaining work and X1's queries and hooks, if the slice jumps ahead of them |
| R8 | [CT5](../ROADMAP.md#ct5-products-and-streaming) | Collision that matches the drawn ground | **No change.** Ask only that CT5's collision product be easy for a game to turn into a physics collider per chunk | L1 works on chunk-mesh colliders until then | Nothing |

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
| Character and movements | See "The lumberjack: character and animation shortlist" | CC0 preferred; Mixamo's licence allows use in a game but not redistribution of the raw files |
| Glider, axe | Simple, original models made for the demo (glTF), or procedural meshes | Original only, no look borrowed from an existing game. |
| UI | Minimal: a whistle cooldown ring, a wind gauge, a log count | — |

### The lumberjack: character and animation shortlist

Read from each source's page on 2026-11-25. **Nothing is downloaded
until the user picks (Q9).** Approved files go into `demo/assets/`
(ignored by git through its own `.gitignore`), and each file's source and
licence is then recorded in this section.

| # | Candidate | Source | Licence | Files and sizes | Rig and clips | Account |
|---|-----------|--------|---------|-----------------|---------------|---------|
| A | **Quaternius Universal Base Characters**, Standard (recommended) | [quaternius.com](https://quaternius.com/packs/universalbasecharacters.html), [itch.io](https://quaternius.itch.io/universal-base-characters) | CC0 | `Universal Base Characters[Standard].zip`, 122 MB, free (2 base models, 5 hairstyles). Source zip 600 MB is paid ($19.99) | Humanoid rig; glTF and FBX; made to work with the animation libraries below. Dressed as a lumberjack by texture and hair, or with Quaternius's modular outfits on the same rig (not yet checked) | No; itch.io "name your price" (0 is fine) |
| B | **Quaternius Universal Animation Library**, Standard (recommended) | [quaternius.com](https://quaternius.com/packs/universalanimationlibrary.html), [itch.io](https://quaternius.itch.io/universal-animation-library) | CC0 | `Universal Animation Library[Standard].zip`, 15 MB, free (45 clips). Pro 41 MB ($9.99, 120+ clips); Source 46 MB ($14.99) | Same rig naming as the base characters since its v2.0 (its changelog); GLB and FBX. Locomotion in 8 directions, jog, sprint, jumps, swimming, sitting, emotes; root motion on locomotion since v3.0. Exact clip names are not on the page: checked after download or in [its viewer](https://quaternius.com/animviewer.html) | No |
| C | **Quaternius Universal Animation Library 2**, Standard | [quaternius.com](https://quaternius.com/packs/universalanimationlibrary2.html), [itch.io](https://quaternius.itch.io/universal-animation-library-2) | CC0 | `Universal Animation Library 2[Standard].zip`, 17 MB, free (42 clips). Source 50 MB ($14.99) | Universal humanoid rig (that it matches A's bone names is likely but not stated). Free clips include `WALK_CARRY`, `IDLE_CARRY`, `CLIMB_UP_1M`, `HARVEST`, `WAVE`, `IDLE_LANTERN`, `PLANT_SEED`, `OVERHAND_THROW`, `SLIDE`; full set adds farming, fishing and parkour | No |
| D | **Mixamo** characters and animations (alternative) | mixamo.com (Adobe) | Adobe's Mixamo terms: free use in games, raw files not redistributable (verify at source) | One FBX per character or clip, chosen on the site (sizes not checked) | Large clip library on one Mixamo skeleton (axe swings, hanging and falling poses to check after sign-in); not glTF, so each file is converted once in Blender. Quaternius's libraries say they are compatible with Mixamo rigs | **Yes, the user's own Adobe sign-in.** The user signs in and downloads; I never enter credentials |
| E | **Kenney Animated Characters** (fallback) | [kenney.nl](https://kenney.nl/assets/animated-characters-1) | CC0 | One free zip (size not listed) | Simple, blocky style with a few basic clips; would clash with procgena's realistic-leaning look | No |

No CC0 rigged *lumberjack* model was found ready-made; the paid
lumberjack models on CGTrader and similar stores are "royalty free" with
terms to read one by one. The recommendation is **A + B (+ C)**: all
free, CC0, one rig, about 154 MB together. Missing from all of them:
the glider hang, the whistle and, in the free sets, an axe chop. These
are authored on the rig (see "The lumberjack's body and movements"), or
the chop comes from B's Pro set ($9.99) or from Mixamo (retargeted once
in Blender). Buying a paid set is the user's call.

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
- **Q3. How much purpose in the experience, and is chopping in?** It is an
  experience, so there is no score or failure in any option. Chopping
  needs promotion and edits from the engine (roadmap request R7), so it
  lands late unless that slice moves up. Options: (a) pure wandering and
  flying, with a cabin and a day passing; (b) (a) plus gentle invitations:
  cairns on far ridges reached by glider, birds that answer the whistle
  and lead somewhere (recommended); (c) (b) plus felling a tree as a
  small ritual, no logs or inventory (recommended too if R7's slice is
  acceptable); (d) a fuller lumberjack loop with logs and building.
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
- **Q9. Which character and animation files may I download?** Recommended:
  A, B and C from the shortlist (all CC0 and free, about 154 MB, into a
  git-ignored `demo/assets/`). Paid sets (B Pro at $9.99 for an axe chop
  and 120+ clips) and Mixamo (your own Adobe sign-in) only if you want
  them.
- **Q10. Roadmap requests R1–R8:** which may go ahead? Recommended: R1,
  R2, R4 now; R3 and R5 before L5; R6 and R7 when the user sets the
  experience's scope.

## Follow-ups for the coordinator

- Add a demo milestone block to ROADMAP once the user approves the plan
  (this plan does not edit ROADMAP).
- Put "Roadmap requests" R1–R8 to the user, and have the agreed ones
  applied by ROADMAP's owner.
- Whatever L2 finds about procgena materials under Bevy's atmospherics
  becomes further engine requests.
