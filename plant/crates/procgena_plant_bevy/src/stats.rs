//! What grass is doing, as numbers: the [`PlantStats`] resource, and the same
//! numbers as Bevy [`Diagnostic`]s for `LogDiagnosticsPlugin` and overlays.
//!
//! "Why is there no grass here?" and "why did my frame spike?" should be
//! answerable without a profiler: how many blades each level of detail drew,
//! whether the buffers overflowed, how many times the tables were rebuilt and
//! how long that took, and whether a limit dropped some fields.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::app::{App, Plugin, PostUpdate};
use bevy::diagnostic::{Diagnostic, DiagnosticPath, Diagnostics, RegisterDiagnostic};
use bevy::ecs::prelude::*;
use bevy::ecs::reflect::ReflectResource;
use bevy::reflect::Reflect;
use bevy::reflect::std_traits::ReflectDefault;

use crate::PlantSystems;
use crate::edit::EditStats;
use crate::render::view::{COUNTERS, MAX_BUCKETS, SLICE_COUNTER};

/// Numbers about what grass is doing. Updated by the plugin; read it for a
/// HUD, a log line or a test.
///
/// The `gpu` numbers come from reading counters back from the GPU, which is
/// done every couple of seconds (120 frames by default, see
/// [`PlantSettings::readback_interval`](crate::PlantSettings::readback_interval))
/// per camera, not every frame:
/// they are the latest reading, summed over cameras.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use procgena_plant_bevy::prelude::*;
/// fn hud(stats: Res<PlantStats>) {
///     if stats.gpu.overflows > 0 {
///         warn!("grass buffers overflowed {} times", stats.gpu.overflows);
///     }
/// }
/// ```
#[derive(Resource, Debug, Clone, PartialEq, Default, Reflect)]
#[reflect(Resource, Default, Debug, PartialEq)]
pub struct PlantStats {
    /// What the CPU built: the tables of fields and chunks.
    pub tables: TableStats,
    /// What the GPU generated, as of its latest reading.
    pub gpu: GpuStats,
    /// What runtime edits did: [`PlantEdit`](crate::PlantEdit)s, brushes
    /// and regrowth.
    pub edits: EditStats,
}

/// The tables of fields and chunks the GPU draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug, PartialEq)]
pub struct TableStats {
    /// Fields in the tables, drawn or not (a hidden field is still in them).
    pub fields: usize,
    /// Chunks of 16 m in the tables.
    pub chunks: usize,
    /// Fields omitted because their entries exceed the table's 256-entry
    /// limit. Each plant and each enabled accent uses one entry.
    pub fields_dropped: usize,
    /// Plants left out because a field grows at most
    /// [`MAX_PLANTS`](crate::MAX_PLANTS). These are not drawn.
    pub plants_dropped: usize,
    /// Number of plant entries excluded by branched plant limits.
    ///
    /// Plants with matching branch counts share a shape class. At most 32 branched
    /// classes are drawn, and each stem may use at most 1024 vertices. Entries
    /// exceeding either limit are not drawn.
    pub branched_dropped: usize,
    /// Times the tables changed because a field, mask or heightmap did. A
    /// field only shown or hidden patches the tables and is not counted, nor
    /// is a change no field shows (a field set to what it already was).
    pub rebuilds: u64,
    /// Of those, the times the chunks, masks or heightmaps were rebuilt and
    /// uploaded again: a field moved, was resized or was added or removed,
    /// or an image changed. The expensive kind. Changing how a field's plants
    /// look, a colour every frame for instance, is not one.
    pub layout_rebuilds: u64,
    /// Masks and heightmaps read texel by texel from their images so far. Each
    /// is read once, and again only when that image changes.
    pub images_read: u64,
    /// Total plant and accent entries rebuilt so far. Changes rewrite only
    /// the entries belonging to affected fields.
    pub entries_rewritten: u64,
    /// How long the latest rebuild took on the CPU.
    pub last_rebuild: Duration,
}

/// Counters read back from the GPU. Summed over cameras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug, PartialEq)]
pub struct GpuStats {
    /// Blades generated in the latest reading, for the near, mid and far
    /// levels of detail (LOD 0, 1 and 2). Before any buffer overflow they are
    /// what is drawn; after one they are what was wanted.
    pub blades: [u32; 3],
    /// Generated branched-stem counts at near, mid and far detail.
    ///
    /// The latest reading combines all mesh classes. Each fern frond counts
    /// separately. Uses the same counting rules as [`blades`](Self::blades).
    pub stems: [u32; 3],
    /// Slices of visible chunks in the latest reading. A dense chunk is
    /// several slices of 512 candidates.
    pub slices: u32,
    /// Readings with buffer overflow that dropped blades or branched stems. Increase
    /// `PlantSettings::instance_capacity` or `PlantSettings::plant_capacity`,
    /// respectively, or reduce density.
    pub overflows: u64,
    /// Readings taken so far; the numbers above change when this does.
    pub readings: u64,
}

impl GpuStats {
    /// Blades generated in the latest reading, every level of detail together.
    ///
    /// The quickest check that grass is drawing at all, for a smoke test or a
    /// screenshot script: once [`readings`](GpuStats::readings) is above 0,
    /// a camera that sees grass makes this above 0 too. Blades hidden under
    /// terrain count as well, so a flat field buried by hills still shows
    /// here; the `debug` feature's field gizmos show where it lies.
    ///
    /// ```no_run
    /// # use bevy::prelude::*;
    /// # use procgena_plant_bevy::prelude::*;
    /// fn check_plants(stats: Res<PlantStats>) {
    ///     if stats.gpu.readings > 0 && stats.gpu.total_blades() == 0 {
    ///         warn!("no grass drawn: is the field hidden, on other layers, or out of reach?");
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn total_blades(&self) -> u64 {
        self.blades.iter().map(|&blades| u64::from(blades)).sum()
    }

    /// Total branched stems generated in the latest reading.
    ///
    /// Sums all three detail levels and all branched mesh classes.
    #[must_use]
    pub fn total_stems(&self) -> u64 {
        self.stems.iter().map(|&stems| u64::from(stems)).sum()
    }
}

/// The names of grass's [`Diagnostic`]s, for `DiagnosticsStore::get` and
/// `LogDiagnosticsPlugin`.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy::diagnostic::LogDiagnosticsPlugin;
/// # use procgena_plant_bevy::prelude::*;
/// # let mut app = App::new();
/// app.add_plugins((PlantPlugins, LogDiagnosticsPlugin::default()));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PlantDiagnostics;

impl PlantDiagnostics {
    /// Blades generated at the near level of detail (LOD 0).
    pub const BLADES_NEAR: DiagnosticPath = DiagnosticPath::const_new("plant/blades/near");
    /// Blades generated at the mid level of detail (LOD 1).
    pub const BLADES_MID: DiagnosticPath = DiagnosticPath::const_new("plant/blades/mid");
    /// Blades generated at the far level of detail (LOD 2).
    pub const BLADES_FAR: DiagnosticPath = DiagnosticPath::const_new("plant/blades/far");
    /// Generated branched stems across all detail levels.
    ///
    /// Counts each stem separately, including every frond of a fern.
    pub const STEMS: DiagnosticPath = DiagnosticPath::const_new("plant/stems");
    /// Slices of visible chunks.
    pub const VISIBLE_SLICES: DiagnosticPath = DiagnosticPath::const_new("plant/visible_slices");
    /// Readings in which a buffer overflowed, cumulative.
    pub const OVERFLOWS: DiagnosticPath = DiagnosticPath::const_new("plant/overflows");
    /// Full table rebuilds, cumulative.
    pub const TABLE_REBUILDS: DiagnosticPath = DiagnosticPath::const_new("plant/table_rebuilds");
    /// CPU time of the latest full table rebuild, in milliseconds.
    pub const REBUILD_TIME: DiagnosticPath = DiagnosticPath::const_new("plant/rebuild_time");
}

/// Registers the diagnostics and publishes [`PlantStats`] to them. Added by
/// `PlantPlugin`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PlantDiagnosticsPlugin;

impl Plugin for PlantDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        let count = |path: DiagnosticPath| Diagnostic::new(path).with_smoothing_factor(0.0);
        app.init_resource::<PlantStats>()
            .init_resource::<GpuReadings>()
            .register_type::<PlantStats>()
            .register_type::<TableStats>()
            .register_type::<GpuStats>()
            .register_diagnostic(count(PlantDiagnostics::BLADES_NEAR))
            .register_diagnostic(count(PlantDiagnostics::BLADES_MID))
            .register_diagnostic(count(PlantDiagnostics::BLADES_FAR))
            .register_diagnostic(count(PlantDiagnostics::STEMS))
            .register_diagnostic(count(PlantDiagnostics::VISIBLE_SLICES))
            .register_diagnostic(count(PlantDiagnostics::OVERFLOWS))
            .register_diagnostic(count(PlantDiagnostics::TABLE_REBUILDS))
            .register_diagnostic(
                Diagnostic::new(PlantDiagnostics::REBUILD_TIME)
                    .with_suffix(" ms")
                    .with_smoothing_factor(0.0),
            )
            .add_systems(
                PostUpdate,
                (collect_gpu_readings, publish)
                    .chain()
                    .after(PlantSystems::Rebuild),
            );
    }
}

/// What the render world read back, shared with the main world. A readback
/// completes on the render side, so this is how it gets across.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct GpuReadings(Arc<Mutex<Readings>>);

#[derive(Debug, Default)]
struct Readings {
    /// Latest counters by render-world camera entity: instance counts per bucket,
    /// followed by visible chunk slices.
    views: HashMap<Entity, [u32; COUNTERS]>,
    overflows: u64,
    samples: u64,
}

impl GpuReadings {
    /// Records a reading of `view`'s counters, and whether it overflowed.
    pub(crate) fn record(&self, view: Entity, counters: &[u32; COUNTERS], overflowed: bool) {
        if let Ok(mut readings) = self.0.lock() {
            readings.views.insert(view, *counters);
            readings.overflows += u64::from(overflowed);
            readings.samples += 1;
        }
    }

    /// Forgets a camera that is gone, so its last reading stops counting.
    pub(crate) fn forget(&self, view: Entity) {
        if let Ok(mut readings) = self.0.lock() {
            readings.views.remove(&view);
        }
    }

    /// The readings so far, summed over cameras.
    fn total(&self) -> GpuStats {
        let Ok(readings) = self.0.lock() else {
            return GpuStats::default();
        };
        let mut stats = GpuStats {
            overflows: readings.overflows,
            readings: readings.samples,
            ..GpuStats::default()
        };
        for counters in readings.views.values() {
            // Class 0 contains the unbranched near, mid and far counts.
            for (sum, count) in stats.blades.iter_mut().zip(&counters[..3]) {
                *sum = sum.saturating_add(*count);
            }
            // Combine all branched classes while keeping detail levels separate.
            for (bucket, count) in counters[3..MAX_BUCKETS].iter().enumerate() {
                let sum = &mut stats.stems[bucket % 3];
                *sum = sum.saturating_add(*count);
            }
            stats.slices = stats.slices.saturating_add(counters[SLICE_COUNTER]);
        }
        stats
    }
}

/// Copies the render world's readings into [`PlantStats`] when there is a new
/// one. Reading `stats` first keeps its change flag quiet otherwise.
fn collect_gpu_readings(readings: Res<GpuReadings>, mut stats: ResMut<PlantStats>) {
    let total = readings.total();
    if total != stats.gpu {
        stats.gpu = total;
    }
}

/// Measures the diagnostics from [`PlantStats`] when it changed.
fn publish(stats: Res<PlantStats>, mut diagnostics: Diagnostics, mut published: Local<(u64, u64)>) {
    let (rebuilds, readings) = *published;
    if stats.tables.rebuilds != rebuilds {
        diagnostics.add_measurement(&PlantDiagnostics::TABLE_REBUILDS, || {
            stats.tables.rebuilds as f64
        });
        diagnostics.add_measurement(&PlantDiagnostics::REBUILD_TIME, || {
            stats.tables.last_rebuild.as_secs_f64() * 1000.0
        });
    }
    if stats.gpu.readings != readings {
        let gpu = stats.gpu;
        diagnostics.add_measurement(&PlantDiagnostics::BLADES_NEAR, || f64::from(gpu.blades[0]));
        diagnostics.add_measurement(&PlantDiagnostics::BLADES_MID, || f64::from(gpu.blades[1]));
        diagnostics.add_measurement(&PlantDiagnostics::BLADES_FAR, || f64::from(gpu.blades[2]));
        diagnostics.add_measurement(&PlantDiagnostics::STEMS, || gpu.total_stems() as f64);
        diagnostics.add_measurement(&PlantDiagnostics::VISIBLE_SLICES, || f64::from(gpu.slices));
        diagnostics.add_measurement(&PlantDiagnostics::OVERFLOWS, || gpu.overflows as f64);
    }
    *published = (stats.tables.rebuilds, stats.gpu.readings);
}

#[cfg(test)]
mod tests;
