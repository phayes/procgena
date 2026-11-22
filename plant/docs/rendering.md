# Shadows and rendering paths

## Shadows

Grass receives shadows from every light. It casts them for one directional
light, set by `PlantSettings::shadows` (`Off`, `Near`, `Full`), and a blade is
only a centimetre or two wide, so how that light's cascades are set up decides
whether blade shadows look right. Use a shallow first cascade, and mark the
light grass should follow with `PlantSun`:

```rust
commands.spawn((
    DirectionalLight { shadow_maps_enabled: true, ..default() },
    CascadeShadowConfigBuilder {
        num_cascades: 3,
        first_cascade_far_bound: 12.0,
        maximum_distance: 60.0,
        ..default()
    }
    .build(),
    PlantSun,
));
```

- **Why 12 m.** Grass draws its shadows into the first cascade (`Near`) or the
  first two (`Full`). A shadow texel is about `1.5 × first cascade depth ÷
  shadow map size`: with one cascade over 60 m in a 2048 map that is 4 to
  5 cm, wider than a blade, so each blade's shadow switches on and off as it
  sways across a texel centre and blade shadows blink. At 12 m a texel is
  under 1 cm.
- **Branched plants cast from simpler leaves.** In the shadow map, every leaf
  with nothing growing from it is a single triangle, widened at its base to
  cover the same area as the curved leaf; stalks and twigs keep their rows.
  That leaves a fern's near frond 101 vertices to draw in the shadow pass
  instead of 269, and the outline of a shadow barely moves at the resolution a
  shadow map has.
- **Which light.** With a sun and a moon there are two directional lights.
  Without `PlantSun`, grass follows the first one that casts shadows (the one
  spawned first); with it, the marked one, and a marked light that casts no
  shadows means grass casts none. A deferred camera also treats the marked
  light as the sun.
- **The log helps.** The plugin warns, once, when the first cascade is deeper
  than the narrowest blades allow (it says how deep it can be), when several
  lights cast shadows and none is marked, when a marked light does not cast
  shadows, and when more than one light is marked.

### A sun of your own

The renderer reads the light it follows from one resource a frame,
`PlantLightFrame`, as it reads the wind from `PlantWindFrame`. The built-in
`PlantLightPlugin` fills it from Bevy's directional lights as above. A game
whose sun is something else (a sky, an atmosphere, a day/night cycle that
drives a component of its own) turns it off and writes the frame in
`PlantLightSystems::Sample`:

```rust
app.add_plugins(PlantPlugins.set(PlantPlugin { lights: false, ..default() }))
    .add_systems(PostUpdate, follow_the_sky.in_set(PlantLightSystems::Sample));

fn follow_the_sky(sky: Res<Sky>, mut light: ResMut<PlantLightFrame>) {
    light.sun_direction = Some(sky.sun_shines);
}
```

`sun_direction` is what a deferred camera lights the far side of blades by;
the forward pass lights them by Bevy's own lights. `shadow_direction` only
decides which blades outside the view are kept because their shadows fall
into it: grass's shadows are drawn into Bevy's shadow maps, so they still
come from a shadow-casting `DirectionalLight`.

## Rendering paths

| Camera has | Grass is drawn into |
| --- | --- |
| Nothing extra | The main opaque pass |
| `DepthPrepass`, `NormalPrepass`, `MotionVectorPrepass` | The prepass too: SSAO, TAA, SSR and anything else reading it see grass |
| `DeferredPrepass`, with `DefaultOpaqueRendererMethod::deferred()` | The G-buffer, lit by Bevy's deferred lighting pass |

Deferred grass matches forward grass closely. It approximates translucency
less well, and toon bands are forward-only. Thin blades shimmer without
anti-aliasing; TAA is recommended, and grass supplies exact motion vectors
for it. The [`passes`](../examples/passes.rs) example shows each path.

## Branching plants

Ferns and bushes ([Plants that branch](fields.md#plants-that-branch)) go
through every path above, shadows included, as grass does. They are drawn
apart from grass, because one draw gives every plant the same number of
vertices: grass keeps its 15-vertex blades, and branching plants are grouped
by how they branch.

- **A kind of plant** is the branch counts of its two levels. Plants that
  branch alike are one kind, whatever their size, shape and colours: every
  `PlantParams::FERN` in a scene, and any plant with 28 leaflets and no second
  level, are one kind.
- **At most 32 kinds** are drawn at once, across every field in use. A 33rd is
  left out with a warning (it says to give more plants the same counts), and
  counted in `PlantStats::tables.branched_dropped`. So is a plant with more
  than 1024 vertices a stem close up. With `c0` and `c1` the two counts
  (after `Opposite` rounds them up to even), a stem is `17 + 9 × c0`
  vertices with one level on, and `17 + 5 × c0 + 7 × c0 × c1` with two: a
  fern of 28 leaflets is 269, and `c0` 10 with `c1` 12 is 907.
- **Each kind costs a draw** per level of detail and pass. Grass alone draws
  exactly as before: the branched draws, their pipelines and their buffers
  exist only while a field has branched plants.
- **Levels of detail.** Close up, a plant has every branch. At mid detail
  each parent keeps about half its children and at most 8, at both levels,
  spread evenly along it (opposite pairs stay together) and widened to cover
  for the ones left out, up to 2.4 times. Leaves there are single triangles,
  widened at the base to cover the curved leaf's area. A fern frond goes from
  269 vertices close up to 33 at mid detail, a huckleberry stem from 505
  to 69. At the far level a stem keeps only its 6 largest parts, each a single
  triangle: 18 vertices for any kind. They come from the level with the most
  total area, the part a viewer sees from afar: the leaves of a fern, the
  leaflets of a huckleberry, the long twigs of a broom, or the stalk of a
  plant that is only a stalk. Within that level they are the parts mid detail
  keeps that sit nearest where the parts are longest, 2 twigs with 3 leaves
  each when the leaves grow on twigs. Everything else, thin stalks and twigs
  included, is culled, and the survivors are widened to cover for it, up to
  6 times, so the band beyond `lod2_start` covers as much ground as mid
  detail would. Branched plants are not thinned at mid range; they fade out
  at the end of the radius only. Orthographic cameras draw them at
  `PlantSettings::orthographic_lod`, as they draw grass.
- **Buffers.** `PlantSettings::plant_capacity` holds the visible stems of
  every kind in use. Each frame, before anything is generated, the GPU gives
  each kind and level of detail the slots it asked for the frame before, a
  quarter more and 256 to spare, then shares out what is left evenly. When
  they want more than the setting holds, every share shrinks alike. So one
  kind can no longer run short while another has room, but a kind that
  appears suddenly, or a fast turn of the camera, can run short for one
  frame and drop whole plants for that frame.

## Bind groups and the storage-buffer budget

Every grass pipeline binds at most 10 storage buffers in a shader stage: what
98% of WebGPU devices offer (the spec promises 8). A device that offers fewer
draws no grass and logs one warning; `PlantSupport` says why. A unit test keeps
every pipeline within 10, and the GPU tests run on a device lowered to 10.
[Running in the browser](web.md) has what else a device needs.

| Pipeline | Group 0 | Group 1 | Group 2 | Storage buffers |
| --- | --- | --- | --- | --- |
| `cull`, `generate` | fields, chunks, scene, tile, clumps, tufts | view, visible slices, bucket table, instances, trample cells | none | 10 |
| `plan_buckets`, `size_dispatch`, `write_draw_args` | the same 6 | view, bucket table, draw arguments | dispatch arguments (`size_dispatch`) | 9 |
| `trample_decay`, `trample_stamp` | the same 6 | view, trample cells, interactors | none | 8 |
| Grass draws | Bevy's view groups | Bevy's | frame, view, instances, fields, noise, wind, bucket table | 3, in the vertex stage only |

The scene buffer holds every density mask and then every heightmap, so the
two share one binding, and together they must fit the device's largest
storage binding (at least 128 MiB on a WebGPU device). If they do not,
grass logs one warning and draws nothing until the fields shrink. A 4096 ×
4096 heightmap is 64 MiB, and a mask of that size 32 MiB.

The draws' storage buffers are visible to the vertex stage only: the
blade's look is worked out per vertex, and the fragment
shaders read only the grass view uniform, so grass adds nothing to Bevy's own
fragment-stage storage buffers.

`generate` is exactly at the budget. A feature that needs another buffer
there has two ways to make room: one buffer for the tile, clump and tuft
tables (frees 2), or the visible slices at the start of the instance buffer
(frees 1).
