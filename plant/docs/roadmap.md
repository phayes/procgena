# Roadmap and scope

procgena_plant does one thing: **it grows the ground cover under a game (grass
and the ferns and low bushes among it, built as branching curves), draws it,
moves it (wind and trampling), and lets the game read and change it.** Ground
cover is background: beyond trampling, nothing in it is interactable.
Anything outside that is out of scope, however useful, so the crate stays
small, fast and easy to keep up to date.

## Keeping up with Bevy

We try to release a procgena_plant for each Bevy minor version **within two weeks
of its release**. The port comes first, whatever else is in progress.

- procgena_plant versions follow Bevy's: procgena_plant 0.20 is for Bevy 0.20, 0.21
  will be for Bevy 0.21. The patch number is ours.
- Only the latest Bevy minor gets new features. Fixes go to the previous one
  while it is reasonable to.
- Each port is checked by the full test suite, the golden images and the
  240 Hz gate before it is tagged ([Testing](testing.md)).
  [Porting to a new Bevy](porting.md) lists every place grass touches Bevy's
  renderer and the order to port them in.
- New fields in the design types (`GrassField`, `GrassPlant`,
  `FieldPlant`, `GrassAccent`, `GrassSegment`, `GrassBranches`,
  `GrassSettings`, `GrassShadows`, `GrassPlugin`, `GrassCamera`) arrive in a
  minor release
  only, which is breaking anyway, and the CHANGELOG says what to add. Write
  their literals with `..default()` (or `..GrassSettings::HIGH`, a preset)
  and a new field never breaks your code. They are not `#[non_exhaustive]`,
  because that would forbid `..GrassSettings::HIGH` outside the crate.
  `.field.ron` and `.plant.ron` files usually keep loading across releases:
  a missing setting takes its base preset's value. A change to the shape of
  the format, such as the move from `.grass.ron` styles to plants and
  fields, can break them; the CHANGELOG then says what to write instead.

## Next

In order:

1. **Distribution.** procgena_plant is on crates.io. Next: listings on Bevy
   Assets and awesome-bevy, and contributor files: CONTRIBUTING, issue
   templates and good-first-issue labels.
2. **More ground cover** on the branching model: more bushes, clover and
   trefoils, and stones and small debris (a different path from plants).
3. **1.0.** Freeze the API, with the performance gate held.

## Done

- **Branching plants** (unreleased): every plant is a stalk with up to two
  levels of branches, so ferns are fronds of leaflets and bushes are stems,
  twigs and leaves, drawn alongside the grass in the same passes and wind.
  The understory presets (`SALAL`, `SAPLING`, `HUCKLEBERRY`, `OREGON_GRAPE`,
  `NETTLE`, `MATURE_BRACKEN`, `FOREST_UNDERSTORY`) use them. See
  [Plants that branch](fields.md#plants-that-branch).
- **More branched plant shapes per scene** (unreleased): a scene draws up
  to 32 kinds of branched plant, and they share
  `GrassSettings::plant_capacity` by what each asked for the frame before
  instead of splitting it evenly. See [Rendering](rendering.md).

- **Strategy cameras and terrain from code** (0.20.6): grass across the
  ground a high camera sees (`GrassCamera::radius_from_ground`), fields on
  terrain built from a height or density function, and a sun or wind the
  game drives itself. See [Settings](settings.md#strategy-cameras) and
  [Fields, terrain and visibility](terrain.md#grow-grass-on-terrain-built-from-code).
- **Browser builds** (0.20.5): grass runs in browsers through WebGPU, in 10
  storage buffers per shader stage, with a hosted demo and a wasm build in
  CI. WebGL2 stays unsupported: grass needs compute shaders. Safari waits on
  Bevy: it rejects a shader in Bevy 0.20's standard material. See
  [Running in the browser](web.md).
- **Runtime editing and queries** (0.20.2): clear, paint, mow and
  restore grass through the density mask, brushes that edit as they move,
  regrowth, and density and height at any point on the CPU. See
  [Editing and reading the grass](editing.md).

- **Rotated fields** (0.20.2): a field follows its transform's rotation about
  Y.
- **Mesh terrain** (0.20.2): `GrassGround::Mesh` bakes a mesh into a
  heightmap on the CPU, once, so grass grows on terrain that is not a
  heightmap. See [Fields, terrain and visibility](terrain.md#grow-grass-on-a-mesh).
- **Rapier and Avian** (0.20.2): physics bodies trample the grass in the
  shape of their colliders.

## Non-goals

Requests for these are declined, so the crate stays one thing done well:

- **Other vegetation:** trees, woody shrubs taller than a person, plants a
  game interacts with (harvests, collides with, hides loot in), or scattering
  your own meshes. Use a scattering crate alongside.
- **Terrain:** rendering or generating it. procgena_plant grows on yours.
- **Splatmaps,** or blending several fields' looks across one field. Overlap fields
  with density masks instead.
- **Impostor cards** for distant grass. `FarGrassMaterial` already carries
  the colour and the gusts past the last blade.
- **WebGL2.**
- **Mesh shaders or meshlets** as a requirement. The frame is dominated by
  shading the blades' pixels, which they do not reduce; see the
  [declined proposal](proposals/meshlets-and-clumps.md).
- **Occlusion culling** of chunks behind hills. Culling acts on the 3–5% of
  grass time spent in compute, not on the pixel shading that dominates.
- **Simulating blades with physics.** Physics bodies trampling grass is in
  scope; blades as rigid or soft bodies are not.
- **2D.**
