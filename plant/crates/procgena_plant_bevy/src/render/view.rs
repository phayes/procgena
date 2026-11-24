//! Per-view state. Every camera that draws grass gets its own instance
//! buffers, so split-screen doubles blade memory but not CPU cost.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use bevy::camera::Camera3d;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::log::{debug, warn};
use bevy::math::{IVec2, IVec4, Mat4, UVec4, Vec3, Vec4};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, Buffer, BufferDescriptor, BufferUsages, MapMode, PipelineCache,
    ShaderType, UniformBuffer,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::view::ExtractedView;

use super::resources::{ExtractedPlants, PlantLayouts, PlantShared};
use crate::PlantSettings;
use crate::api::{NoPlants, PlantCamera};
use crate::gpu_layout::MAX_BRANCHED_CLASSES;
use crate::scatters::{has_unsupported_layer, layer_mask};
use crate::shader_model::DistanceLod;
use crate::stats::GpuReadings;
use crate::{reach, trample};

/// Maximum number of plant shapes a view supports.
///
/// Plants with matching branch counts share a shape class; class 0 holds unbranched
/// blades. The WESL twin is `common::SHAPE_CLASSES`.
pub(crate) const SHAPE_CLASSES: usize = 1 + MAX_BRANCHED_CLASSES;
/// Maximum number of groups of instances, called buckets, per view.
///
/// Each shape class has near, mid and far buckets, starting at `3c` for class `c`.
/// `ViewUniformData::bucket_count` gives the number in use. The WESL twin is
/// `common::MAX_BUCKETS`.
pub(crate) const MAX_BUCKETS: usize = SHAPE_CLASSES * 3;
/// Number of `UVec4`s needed to pack one value per bucket. The WESL twin is
/// `common::BUCKET_WORDS`.
pub(crate) const BUCKET_WORDS: usize = MAX_BUCKETS.div_ceil(4);
/// Number of four-value vectors needed for one value per shape class.
///
/// The WESL twin is `common::CLASS_WORDS`.
pub(crate) const CLASS_WORDS: usize = SHAPE_CLASSES.div_ceil(4);
/// Number of draw-command slots reserved for each view.
///
/// Each bucket has a slot, followed by shadow slots indexed by shape class.
/// Branched class `c` uses `MAX_BUCKETS + c` for its first bucket's shadow draw.
/// The WESL twin is `common::DRAW_ARGS_COUNT`.
pub(crate) const DRAW_ARGS_COUNT: usize = MAX_BUCKETS + SHAPE_CLASSES;
/// Index of the visible-slice counter in the bucket table.
///
/// Slices divide visible chunks into batches for generation. The WESL twin is
/// `BucketTable::slices` in `common.wesl`.
pub(crate) const SLICE_COUNTER: usize = MAX_BUCKETS;
/// Number of counters at the start of the bucket table.
///
/// There is one per instance bucket, plus one for visible chunk slices.
pub(crate) const COUNTERS: usize = MAX_BUCKETS + 1;
/// Starting word of the bucket table's instance offsets.
///
/// Each offset locates a bucket in the instance buffer. The WESL twin is
/// `BucketTable::base`.
pub(crate) const BASE_AT: usize = COUNTERS;
/// Starting word of the bucket table's storage limits.
///
/// Each limit is the number of instances a bucket can hold. The WESL twin is
/// `BucketTable::capacity`.
pub(crate) const CAPACITY_AT: usize = BASE_AT + MAX_BUCKETS;
/// Size of a view's bucket table in `u32` words.
///
/// The table holds counters, instance offsets and storage limits. The WESL twin
/// is `BucketTable` in `common.wesl`.
pub(crate) const BUCKET_TABLE_WORDS: usize = CAPACITY_AT + MAX_BUCKETS;
/// Bytes per instance.
const INSTANCE_SIZE: u64 = 16;
/// Bytes of the largest instance buffer a camera can have:
/// `PlantSettings::MAX_INSTANCE_CAPACITY` blades at LOD 0 and half as many at
/// each of LODs 1 and 2, plus up to `PlantSettings::MAX_PLANT_CAPACITY` branched stems.
/// This is the view's largest storage binding.
pub(crate) const MAX_INSTANCE_BYTES: u64 = (2 * PlantSettings::MAX_INSTANCE_CAPACITY as u64
    + PlantSettings::MAX_PLANT_CAPACITY as u64)
    * INSTANCE_SIZE;
/// Bytes per `DrawIndexedIndirectArgs`.
pub(crate) const DRAW_ARGS_SIZE: u64 = 20;
/// `fixed_lod` value meaning "pick by distance".
const DISTANCE_LOD: u32 = 3;
/// The vertical range instance positions are stored over, in metres either
/// side of the view's origin, at least: a view whose horizontal extent is
/// larger uses that instead.
const MIN_HEIGHT_RANGE: f32 = 256.0;

/// Mirrors `View` in `common.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub(crate) struct ViewUniformData {
    pub planes: [Vec4; 6],
    pub camera_position: Vec4,
    pub camera_forward: Vec4,
    pub shadow_offset: Vec4,
    pub instance_origin: Vec4,
    pub radius: f32,
    pub lod1_start: f32,
    pub lod2_start: f32,
    pub lod_band: f32,
    /// Where the mid-range thinning begins, in metres.
    pub mid_range_start: f32,
    pub density_scale: f32,
    pub fixed_lod: u32,
    pub max_visible_chunks: u32,
    pub interactor_count: u32,
    /// The render layers the view draws, one bit each.
    pub layers: u32,
    /// Narrowest a blade is drawn, in pixels.
    pub min_width_px: f32,
    /// Metres taken off every distance to the camera before the radius,
    /// the thinning and the levels of detail apply
    /// (`PlantCamera::radius_from_ground`).
    pub distance_offset: f32,
    /// Number of buckets in use.
    ///
    /// Includes three for blades and three for each active branched shape class.
    pub bucket_count: u32,
    /// Number of instance slots reserved for the first blade bucket.
    ///
    /// The other two blade buckets each receive half this number.
    pub blade_capacity: u32,
    /// Total stem slots shared by branched plant buckets.
    ///
    /// Zero when no branched shape class is in use.
    pub plant_capacity: u32,
    /// Drawing detail level for each bucket.
    ///
    /// Packed four values per vector; read with [`bucket_value`].
    pub bucket_lod: [UVec4; BUCKET_WORDS],
    /// The trample window's lowest cell this frame (xy) and last frame (zw).
    pub trample_origin: IVec4,
    /// Cell size in metres, and how much flattening recovers this frame.
    pub trample_step: Vec4,
    /// Direction to the first directional light, w 1 when there is one.
    pub sun: Vec4,
    /// The distance table's densities: the share kept where LOD 2 starts and
    /// where the last fade starts, the extra share kept from LOD 1 on, and
    /// where the last fade starts as a share of the radius.
    pub lod_density: Vec4,
    /// The most a thinned blade widens, the largest interactor radius, how
    /// far an edge-on blade turns toward the camera, and the vertical range
    /// of stored positions.
    pub limits: Vec4,
}

/// Packs bucket values four per vector to match shader uniform access.
pub(crate) fn bucket_words(values: &[u32; MAX_BUCKETS]) -> [UVec4; BUCKET_WORDS] {
    pack_words(values)
}

/// Packs shape-class values into groups of four for the shaders.
///
/// Class `c` is stored at `words[c / 4][c % 4]`.
pub(crate) fn class_words(values: &[u32; SHAPE_CLASSES]) -> [UVec4; CLASS_WORDS] {
    pack_words(values)
}

/// Packs values into four-component vectors, leaving unused components zero.
///
/// `W` must provide room for all `N` values: at least `N / 4`, rounded up.
fn pack_words<const N: usize, const W: usize>(values: &[u32; N]) -> [UVec4; W] {
    const { assert!(W * 4 >= N, "too few words for the values") };
    let mut words = [UVec4::ZERO; W];
    for (i, &value) in values.iter().enumerate() {
        words[i / 4][i % 4] = value;
    }
    words
}

/// Reads bucket `i` from a table packed by [`bucket_words`], matching shader access at
/// `words[i / 4u][i % 4u]`.
#[cfg(test)]
pub(crate) fn bucket_value(words: &[UVec4; BUCKET_WORDS], i: usize) -> u32 {
    words[i / 4][i % 4]
}

/// Where the overflow check is in its cycle.
const READBACK_IDLE: u8 = 0;
const READBACK_COPIED: u8 = 1;
const READBACK_MAPPING: u8 = 2;
const READBACK_READY: u8 = 3;

/// A view's GPU buffers.
#[derive(Component)]
pub(crate) struct PlantViewBuffers {
    pub uniform: UniformBuffer<ViewUniformData>,
    visible: Buffer,
    /// GPU table of demand, instance offsets and storage limits for this frame.
    ///
    /// Uses the `BucketTable` layout in `common.wesl`.
    pub buckets: Buffer,
    pub dispatch: Buffer,
    instances: Buffer,
    pub draw_args: Buffer,
    /// The trample window: one packed push per cell (see `trample.rs`).
    trample: Buffer,
    /// This frame's interactor stamps, three `vec4`s each (`trample::Stamp`).
    interactors: Buffer,
    /// Interactors in `interactors` this frame.
    pub interactor_count: u32,
    /// The window's lowest cell last frame; `None` before the first frame.
    trample_origin: Option<IVec2>,
    /// The side of a window cell last frame, in metres. When it changes, the
    /// old cells mean something else and the window starts again.
    trample_cell: f32,
    /// CPU-readable copy of the bucket table for statistics and overflow warnings.
    ///
    /// Updated at the configured readback interval.
    pub readback: Buffer,
    pub readback_state: Arc<AtomicU8>,
    readback_values: Arc<Mutex<[u32; BUCKET_TABLE_WORDS]>>,
    frames_since_readback: u32,
    /// Frames between readbacks; 0 for never. Follows the settings.
    readback_interval: u32,
    /// Blade and plant budgets used to allocate the instance buffer.
    slots: InstanceSlots,
    /// Interactors `interactors` has room for.
    max_interactors: u32,
    /// Entries in `visible`.
    max_slices: u32,
    warned: bool,
}

/// A view's bind groups.
#[derive(Component, Default)]
pub(crate) struct PlantViewBindGroups {
    /// Group 1 of `cull` and `generate`.
    pub generate: Option<BindGroup>,
    /// Group 1 of `size_dispatch` and `write_draw_args`.
    pub args: Option<BindGroup>,
    /// Group 1 of the trample passes.
    pub trample: Option<BindGroup>,
    pub dispatch: Option<BindGroup>,
    pub draw: Option<BindGroup>,
}

/// Counts the buckets needed for blades and the active branched shape classes.
///
/// Each class needs three buckets. Branched classes are capped at
/// `MAX_BRANCHED_CLASSES`.
pub(crate) fn bucket_count(branched_classes: usize) -> u32 {
    (3 * (1 + branched_classes.min(MAX_BRANCHED_CLASSES))) as u32
}

/// Storage budgets for a view's blades and branched stems.
///
/// `plan_buckets` in `generate.wesl` divides the plant budget between buckets
/// each frame according to demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InstanceSlots {
    /// Number of instance slots reserved for the first blade bucket.
    ///
    /// The other two blade buckets each receive half this number.
    pub blade: u32,
    /// Total stem slots available to all branched plant buckets.
    ///
    /// Each bucket's share depends on last frame's demand.
    pub plant: u32,
}

impl InstanceSlots {
    /// Builds storage budgets from the settings and active branched classes.
    ///
    /// Reserves no plant storage when there are no branched classes.
    pub(crate) fn new(settings: &PlantSettings, branched_classes: usize) -> Self {
        Self {
            blade: settings.instance_capacity,
            plant: if branched_classes > 0 {
                settings.plant_capacity
            } else {
                0
            },
        }
    }

    /// Counts all instance slots needed for blades and branched stems.
    ///
    /// Includes all three blade buckets and the shared plant budget.
    pub(crate) fn total(self) -> u32 {
        self.blade + 2 * (self.blade / 2) + self.plant
    }
}

impl PlantViewBuffers {
    fn new(device: &RenderDevice, settings: &PlantSettings, branched_classes: usize) -> Self {
        let slots = InstanceSlots::new(settings, branched_classes);
        let total = slots.total();
        let buffer = |label: &'static str, size: u64, usage: BufferUsages| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            uniform: UniformBuffer::default(),
            visible: buffer(
                "plant_visible_chunks",
                // A `VisibleChunk`: one slice of a visible chunk, 16 bytes.
                u64::from(settings.max_chunk_slices) * 16,
                BufferUsages::STORAGE,
            ),
            // A new buffer has zero demand, so plant buckets start with equal shares.
            buckets: buffer(
                "plant_buckets",
                BUCKET_TABLE_WORDS as u64 * 4,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            ),
            dispatch: buffer(
                "plant_dispatch",
                12,
                BufferUsages::STORAGE | BufferUsages::INDIRECT,
            ),
            instances: buffer(
                "plant_instances",
                u64::from(total) * INSTANCE_SIZE,
                // The GPU readback test copies instances out.
                if cfg!(test) {
                    BufferUsages::STORAGE | BufferUsages::COPY_SRC
                } else {
                    BufferUsages::STORAGE
                },
            ),
            draw_args: buffer(
                "plant_draw_args",
                DRAW_ARGS_COUNT as u64 * DRAW_ARGS_SIZE,
                // The GPU readback test copies the arguments out.
                if cfg!(test) {
                    BufferUsages::STORAGE | BufferUsages::INDIRECT | BufferUsages::COPY_SRC
                } else {
                    BufferUsages::STORAGE | BufferUsages::INDIRECT
                },
            ),
            // Zeroed on creation: nothing is flattened yet.
            trample: buffer(
                "plant_trample",
                (trample::TEXELS * trample::TEXELS) as u64 * 4,
                BufferUsages::STORAGE,
            ),
            interactors: buffer(
                "plant_interactors",
                u64::from(settings.max_interactors) * 48,
                BufferUsages::STORAGE | BufferUsages::COPY_DST,
            ),
            interactor_count: 0,
            trample_origin: None,
            trample_cell: settings.trample_cell_size,
            readback: buffer(
                "plant_readback",
                BUCKET_TABLE_WORDS as u64 * 4,
                BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            ),
            readback_state: Arc::new(AtomicU8::new(READBACK_IDLE)),
            readback_values: Arc::new(Mutex::new([0; BUCKET_TABLE_WORDS])),
            frames_since_readback: 0,
            readback_interval: settings.readback_interval,
            slots,
            max_interactors: settings.max_interactors,
            max_slices: settings.max_chunk_slices,
            warned: false,
        }
    }

    /// Checks whether current allocations match the requested capacities and active
    /// class count. Changes require new buffers and bind groups.
    fn fits(&self, settings: &PlantSettings, branched_classes: usize) -> bool {
        self.slots == InstanceSlots::new(settings, branched_classes)
            && self.max_interactors == settings.max_interactors
            && self.max_slices == settings.max_chunk_slices
    }

    /// The instance buffer, for the GPU readback test.
    #[cfg(test)]
    pub(crate) fn instances_buffer(&self) -> &Buffer {
        &self.instances
    }

    /// Whether this frame's compute should copy the counters for the check.
    pub(crate) fn wants_readback(&self) -> bool {
        self.readback_interval > 0
            && self.frames_since_readback >= self.readback_interval
            && self.readback_state.load(Ordering::Acquire) == READBACK_IDLE
    }

    /// Called after the copy is recorded.
    pub(crate) fn readback_copied(&mut self) {
        self.frames_since_readback = 0;
        self.readback_state
            .store(READBACK_COPIED, Ordering::Release);
    }
}

/// Inward-facing, normalized frustum planes of a clip-from-world matrix.
/// Bevy uses reversed depth, so clip `z` runs from `w` (near) down to 0 (far);
/// an infinite far plane comes out degenerate and is replaced by one that
/// passes everything.
pub(crate) fn frustum_planes(clip_from_world: &Mat4) -> [Vec4; 6] {
    let row = |i| clip_from_world.row(i);
    let raw = [
        row(3) + row(0),
        row(3) - row(0),
        row(3) + row(1),
        row(3) - row(1),
        row(3) - row(2),
        row(2),
    ];
    raw.map(|plane| {
        let length = plane.truncate().length();
        if length < 1e-6 {
            Vec4::W
        } else {
            plane / length
        }
    })
}

/// The resources and queries `prepare_views` needs, grouped to keep the
/// system signature readable.
#[expect(clippy::type_complexity, reason = "Bevy query types are verbose")]
#[derive(SystemParam)]
pub(crate) struct PrepareViews<'w, 's> {
    commands: Commands<'w, 's>,
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static ExtractedView,
            Option<&'static RenderLayers>,
            Option<&'static PlantCamera>,
            Option<&'static mut PlantViewBuffers>,
        ),
        (With<Camera3d>, Without<NoPlants>),
    >,
    warned_layers: Local<'s, bool>,
    gone: RemovedComponents<'w, 's, PlantViewBuffers>,
    readings: Res<'w, GpuReadings>,
    extracted: Res<'w, ExtractedPlants>,
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
}

/// Creates buffers for new grass views and writes every view's uniform.
pub(crate) fn prepare_views(
    PrepareViews {
        mut commands,
        mut views,
        mut warned_layers,
        mut gone,
        readings,
        extracted,
        device,
        queue,
    }: PrepareViews,
) {
    // A camera that is gone stops counting in the stats.
    for entity in gone.read() {
        readings.forget(entity);
    }
    let branched_classes = extracted.tables.shape_classes.len();
    for (entity, view, layers, camera, buffers) in &mut views {
        // Create buffers for new views or changed bucket capacities. Bind groups must
        // be recreated because they reference the old buffers.
        let Some(mut buffers) =
            buffers.filter(|buffers| buffers.fits(&extracted.settings, branched_classes))
        else {
            commands.entity(entity).insert((
                PlantViewBuffers::new(&device, &extracted.settings, branched_classes),
                PlantViewBindGroups::default(),
            ));
            continue;
        };
        // The resource's settings, with this camera's own over them.
        let settings = &extracted.settings.for_camera(camera);
        let offset = reach::distance_offset(
            camera,
            view.world_from_view.translation(),
            view.clip_from_view.w_axis.w == 1.0,
            &extracted.tables,
        );
        let mut data = view_uniform(
            view,
            settings,
            offset,
            buffers.slots,
            bucket_count(branched_classes),
            extracted
                .shadow_light
                .filter(|_| settings.shadows.cascades() > 0),
        );
        data.sun = extracted.sun.map_or(Vec4::ZERO, |sun| sun.extend(1.0));
        data.layers = layer_mask(layers);
        if has_unsupported_layer(layers) && !*warned_layers {
            *warned_layers = true;
            warn!(
                "procgena_plant: a camera is on a render layer above 31; grass supports layers 0 \
                 to 31, so the others are ignored."
            );
        }
        update_trample(
            &mut buffers,
            &mut data,
            view,
            settings,
            offset,
            &extracted,
            &queue,
        );
        buffers.uniform.set(data);
        buffers.uniform.write_buffer(&device, &queue);
        buffers.readback_interval = settings.readback_interval;
        buffers.frames_since_readback += 1;
        check_readback(&mut buffers, entity, &readings);
    }
}

/// The raw view values `view_uniform` reads from `ExtractedView`, grouped so
/// the math can be tested without constructing a Bevy view.
struct ViewParts {
    world_from_view: Mat4,
    clip_from_view: Mat4,
    clip_from_world: Option<Mat4>,
    viewport: UVec4,
    position: Vec3,
    forward: Vec3,
}

fn view_uniform(
    view: &ExtractedView,
    settings: &PlantSettings,
    distance_offset: f32,
    slots: InstanceSlots,
    bucket_count: u32,
    shadow_light: Option<Vec3>,
) -> ViewUniformData {
    let parts = ViewParts {
        world_from_view: view.world_from_view.to_matrix(),
        clip_from_view: view.clip_from_view,
        clip_from_world: view.clip_from_world,
        viewport: view.viewport,
        position: view.world_from_view.translation(),
        forward: view.world_from_view.forward().as_vec3(),
    };
    view_uniform_from_parts(
        &parts,
        settings,
        distance_offset,
        slots,
        bucket_count,
        shadow_light,
    )
}

/// The pure math behind [`view_uniform`], split out so tests can feed it raw
/// matrices without a Bevy view.
fn view_uniform_from_parts(
    parts: &ViewParts,
    settings: &PlantSettings,
    distance_offset: f32,
    slots: InstanceSlots,
    bucket_count: u32,
    shadow_light: Option<Vec3>,
) -> ViewUniformData {
    let clip_from_world = parts
        .clip_from_world
        .unwrap_or_else(|| parts.clip_from_view * parts.world_from_view.inverse());
    let orthographic = parts.clip_from_view.w_axis.w == 1.0;
    let position = parts.position;
    let forward = parts.forward;
    let pixel_scale = 2.0 / (parts.clip_from_view.x_axis.x * parts.viewport.z.max(1) as f32);

    let lod = DistanceLod::from_settings(settings);
    // Positions are quantized to 16 bits over ±extent around an origin that
    // moves in whole quantization steps, so blades never shimmer as the
    // camera moves. Heights use the same steps, over at least 256 m.
    let extent = if orthographic {
        settings.orthographic_extent
    } else {
        settings.radius + distance_offset + 16.0
    };
    let height_range = extent.max(MIN_HEIGHT_RANGE);
    let step = 2.0 * extent / 65_535.0;
    let snap = step * 128.0;
    let origin = (position / snap).round() * snap;

    let shadow_offset = shadow_light.map_or(Vec4::ZERO, |light| {
        let down = (-light.y).max(0.2);
        Vec4::new(light.x / down, 0.0, light.z / down, 1.0)
    });
    // Perspective buckets draw near, mid and far detail respectively. A forced level,
    // and orthographic views, put each class's instances in its first bucket and draw
    // that one level.
    let (fixed_lod, first_lod) = match settings.forced_lod {
        Some(lod) => (lod, lod),
        None if orthographic => (settings.orthographic_lod, settings.orthographic_lod),
        None => (DISTANCE_LOD, 0),
    };
    let bucket_lod: [u32; MAX_BUCKETS] = std::array::from_fn(|b| {
        if b % 3 == 0 {
            first_lod
        } else {
            (b % 3) as u32
        }
    });
    ViewUniformData {
        planes: frustum_planes(&clip_from_world),
        camera_position: position.extend(if orthographic { 1.0 } else { 0.0 }),
        camera_forward: forward.extend(pixel_scale),
        shadow_offset,
        instance_origin: origin.extend(extent),
        radius: lod.radius,
        lod1_start: lod.lod_start[0],
        lod2_start: lod.lod_start[1],
        lod_band: lod.band,
        mid_range_start: lod.mid_range_start,
        density_scale: settings.density_scale,
        fixed_lod,
        max_visible_chunks: settings.max_chunk_slices,
        interactor_count: 0,
        layers: 1,
        min_width_px: settings.min_width_px,
        distance_offset,
        bucket_count,
        blade_capacity: slots.blade,
        plant_capacity: slots.plant,
        bucket_lod: bucket_words(&bucket_lod),
        trample_origin: IVec4::ZERO,
        trample_step: Vec4::ZERO,
        sun: Vec4::ZERO,
        lod_density: Vec4::new(
            lod.density_at[0],
            lod.density_at[1],
            lod.mid_range_density,
            lod.fade_start,
        ),
        limits: Vec4::new(
            settings.max_widening,
            settings.max_interactor_radius,
            settings.turn_to_camera,
            height_range,
        ),
    }
}

/// Moves the view's trample window, uploads the interactors it stamps this
/// frame and fills the uniform's trample fields.
fn update_trample(
    buffers: &mut PlantViewBuffers,
    data: &mut ViewUniformData,
    view: &ExtractedView,
    settings: &PlantSettings,
    distance_offset: f32,
    extracted: &ExtractedPlants,
    queue: &RenderQueue,
) {
    let cell = settings.trample_cell_size;
    let center = trample::focus(
        &trample::TrampleView::new(
            &view.world_from_view,
            &view.clip_from_view,
            settings.radius + distance_offset,
        ),
        cell,
        &extracted.tables,
    );
    let origin = trample::window_origin(center, cell);
    // Before the first frame, and when the cells change size, every slot
    // counts as newly in the window, which clears it.
    if buffers.trample_cell != cell {
        buffers.trample_cell = cell;
        buffers.trample_origin = None;
    }
    let previous = buffers
        .trample_origin
        .unwrap_or(origin + IVec2::splat(trample::TEXELS * 4));
    buffers.trample_origin = Some(origin);

    let stamped = trample::select_interactors(
        &extracted.interactors,
        center,
        settings.max_interactors,
        cell,
        settings.max_interactor_radius,
    );
    let words: Vec<[Vec4; 3]> = stamped.into_iter().map(trample::Stamp::gpu).collect();
    if !words.is_empty() {
        queue.write_buffer(&buffers.interactors, 0, bytemuck::cast_slice(&words));
    }
    buffers.interactor_count = words.len() as u32;

    let elapsed = (extracted.frame.time - extracted.frame.previous_time).max(0.0);
    data.interactor_count = buffers.interactor_count;
    data.trample_origin = IVec4::new(origin.x, origin.y, previous.x, previous.y);
    data.trample_step = Vec4::new(cell, elapsed / settings.trample_recovery, 0.0, 0.0);
}

/// Advances the overflow check: maps a copied buffer, or reports a mapped one.
fn check_readback(buffers: &mut PlantViewBuffers, view: Entity, readings: &GpuReadings) {
    match buffers.readback_state.load(Ordering::Acquire) {
        READBACK_COPIED => {
            buffers
                .readback_state
                .store(READBACK_MAPPING, Ordering::Release);
            let buffer = buffers.readback.clone();
            let state = buffers.readback_state.clone();
            let values = buffers.readback_values.clone();
            buffers
                .readback
                .slice(..)
                .map_async(MapMode::Read, move |result| {
                    if result.is_ok() {
                        if let Ok(bytes) = buffer.slice(..).get_mapped_range() {
                            let words: &[u32] = bytemuck::cast_slice(&bytes);
                            if let Ok(mut out) = values.lock() {
                                out.copy_from_slice(&words[..BUCKET_TABLE_WORDS]);
                            }
                        }
                        buffer.unmap();
                    }
                    state.store(READBACK_READY, Ordering::Release);
                });
        }
        READBACK_READY => {
            let table = buffers
                .readback_values
                .lock()
                .map_or([0; BUCKET_TABLE_WORDS], |v| *v);
            buffers
                .readback_state
                .store(READBACK_IDLE, Ordering::Release);
            let counts = &table[..MAX_BUCKETS];
            let capacities = &table[CAPACITY_AT..];
            let slices = table[SLICE_COUNTER];
            // Trim trailing buckets with neither slots nor demand from the debug output.
            let in_use = (3..MAX_BUCKETS)
                .rev()
                .find(|&b| capacities[b] > 0 || counts[b] > 0)
                .map_or(3, |b| b + 1);
            debug!(
                "procgena_plant: {slices} visible chunk slices; blades per bucket {} / {} / {}; \
                 branched stems per bucket {:?} in {:?} slots",
                counts[0],
                counts[1],
                counts[2],
                &counts[3..in_use],
                &capacities[3..in_use]
            );
            let over: Vec<String> = (0..MAX_BUCKETS)
                .filter(|&b| counts[b] > capacities[b])
                .map(|b| {
                    let kind = if b < 3 { "blades" } else { "branched stems" };
                    format!("bucket {b}: {} {kind} for {}", counts[b], capacities[b])
                })
                .collect();
            let overflowed = !over.is_empty() || slices > buffers.max_slices;
            let counters = table
                .first_chunk()
                .expect("the bucket table starts with the counters");
            readings.record(view, counters, overflowed);
            if !over.is_empty() && !buffers.warned {
                buffers.warned = true;
                warn!(
                    "procgena_plant: instance buffers overflowed ({}); the extra plants are dropped. \
                     Raise PlantSettings::instance_capacity for blades or plant_capacity for \
                     branched plants, or lower the style density.",
                    over.join(", ")
                );
            }
            if slices > buffers.max_slices && !buffers.warned {
                buffers.warned = true;
                warn!(
                    "procgena_plant: {slices} visible chunk slices for {}; the grass of the rest is \
                     dropped. Lower the style density or the grass radius, or raise \
                     PlantSettings::max_chunk_slices.",
                    buffers.max_slices
                );
            }
        }
        _ => {}
    }
}

/// Builds each view's compute and draw bind groups.
pub(crate) fn prepare_view_bind_groups(
    mut views: Query<(&PlantViewBuffers, &mut PlantViewBindGroups)>,
    shared: Res<PlantShared>,
    layouts: Res<PlantLayouts>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    let (Some(frame), Some(fields), Some(wind)) = (
        shared.frame.binding(),
        shared.fields.binding(),
        shared.wind.binding(),
    ) else {
        return;
    };
    for (buffers, mut groups) in &mut views {
        let Some(uniform) = buffers.uniform.binding() else {
            continue;
        };
        groups.generate = Some(device.create_bind_group(
            "plant_generate_view",
            &pipeline_cache.get_bind_group_layout(&layouts.generate_view),
            &BindGroupEntries::with_indices((
                (0, uniform.clone()),
                (1, buffers.visible.as_entire_binding()),
                (2, buffers.buckets.as_entire_binding()),
                (3, buffers.instances.as_entire_binding()),
                (5, buffers.trample.as_entire_binding()),
            )),
        ));
        groups.args = Some(device.create_bind_group(
            "plant_args_view",
            &pipeline_cache.get_bind_group_layout(&layouts.args_view),
            &BindGroupEntries::with_indices((
                (0, uniform.clone()),
                (2, buffers.buckets.as_entire_binding()),
                (4, buffers.draw_args.as_entire_binding()),
            )),
        ));
        groups.trample = Some(device.create_bind_group(
            "plant_trample_view",
            &pipeline_cache.get_bind_group_layout(&layouts.trample_view),
            &BindGroupEntries::with_indices((
                (0, uniform.clone()),
                (5, buffers.trample.as_entire_binding()),
                (6, buffers.interactors.as_entire_binding()),
            )),
        ));
        groups.dispatch = Some(device.create_bind_group(
            "plant_dispatch",
            &pipeline_cache.get_bind_group_layout(&layouts.dispatch),
            &BindGroupEntries::single(buffers.dispatch.as_entire_binding()),
        ));
        groups.draw = Some(device.create_bind_group(
            "plant_draw",
            &pipeline_cache.get_bind_group_layout(&layouts.draw),
            &BindGroupEntries::sequential((
                frame.clone(),
                uniform,
                buffers.instances.as_entire_binding(),
                fields.clone(),
                &shared.gust_view,
                &shared.gust_sampler,
                wind.clone(),
                buffers.buckets.as_entire_binding(),
            )),
        ));
    }
}

#[cfg(test)]
mod tests;
