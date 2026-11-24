# Stats, debugging and tooling

## See what grass is doing

"Why is there no grass here?" and "why did that frame spike?" can be answered
without a profiler.

**`PlantStats`** is a resource with the numbers: blades and branched stems
(`gpu.stems`, each frond of a fern) per level of detail,
visible chunk slices, buffer overflows, table rebuilds and how long the last
one took, and fields or plants a limit left out. The GPU numbers are read
back every 120 frames (`PlantSettings::readback_interval`; 0 turns the
readback off), summed over cameras.

```rust
fn hud(stats: Res<PlantStats>) {
    if stats.gpu.overflows > 0 {
        warn!("grass buffers overflowed: raise PlantSettings::instance_capacity");
    }
    if stats.tables.fields_dropped > 0 {
        warn!("{} fields are not drawn: the tables hold 256 entries", stats.tables.fields_dropped);
    }
}
```

`stats.gpu.total_blades()` adds the three levels of detail up: once
`stats.gpu.readings` is above 0, it is 0 only when nothing was drawn, which
makes it the check for a smoke test or a screenshot script. Blades hidden
under terrain still count, so for grass in the wrong place, look at the
gizmos below.

The same numbers are Bevy diagnostics, for `LogDiagnosticsPlugin` or an
overlay: `grass/blades/near`, `/mid`, `/far`, `grass/stems` (branched
stems, every level of detail together), `grass/visible_slices`,
`grass/overflows`, `grass/table_rebuilds` and `grass/rebuild_time` (ms), as
the constants of `PlantDiagnostics`.

**`PlantDebugPlugin`** draws gizmos for what grass has: every field's
rectangle (grey while hidden, so scale, rotation and visibility problems
show), the 16 m chunks, each interactor's disc (red where a radius over the
limit is cut off), each footprint's rounded rectangle, and each camera's
trample window. All follow the settings. Toggle parts with the `PlantDebug`
resource; `enabled` is the master switch (the `meadow` example binds it to
F3). It needs the `debug` cargo feature, which turns on Bevy's `bevy_gizmos`,
so adding this crate never changes how Bevy is compiled:

```toml
procgena_plant = { version = "0.20.6", features = ["debug"] }
```

## Scenes, inspectors and save files

Every public type is reflected, with the type data tools look for: every
component has `ReflectComponent`, `PlantWind` and `PlantSettings` have
`ReflectResource`, and `PlantScatterPreset` and `PlantPreset` have `ReflectAsset`
(the plugin calls `register_asset_reflect`). So fields, with their plants,
can be spawned from `.scn.ron` scenes, BSN or Blender exporters such as Skein
and Blenvy; `PlantWind`, `PlantSettings` and fields show up in the resource
and entity panels of inspectors such as `bevy-inspector-egui`; and a designer
can tune a meadow live there, down to one plant's colour
(`plants[0].plant.segment.tip_color`), instead of editing a file.

## Order your systems against the grass

Fields and images are turned into GPU tables only when they change.
Three public system sets in `PostUpdate`, `PlantSystems::DetectChanges`,
`PlantSystems::Edit` and `PlantSystems::Rebuild`, run after Bevy has
propagated transforms and visibility and sent asset events. Order your own
systems against them when a change must show this frame, for procedural
terrain or runtime painting:

```rust
// Moving or showing fields: before DetectChanges.
app.add_systems(PostUpdate, stream_in_the_level.before(PlantSystems::DetectChanges));
// Writing `PlantEdit`s: before Edit. Reading `PlantSampler`: after it, to
// see this frame's edits.
app.add_systems(PostUpdate, (clear_under_buildings.before(PlantSystems::Edit),
    footstep_sounds.after(PlantSystems::Edit)));
// Editing a density mask or a heightmap: before Bevy sends the asset
// events.
app.add_systems(PostUpdate, paint_density_mask.before(AssetEventSystems));
```

Systems in `Update` need no ordering. Without any, a change shows the next
frame. The collider bridge runs in `Update` itself: a system that writes
`PlantColliderBounds` goes in `PlantColliderSystems::Bounds` (see
[Another engine, or none](trampling.md#another-engine-or-none)).
