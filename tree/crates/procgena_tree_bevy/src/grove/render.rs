//! Draws groves: every tree of a grove from its variants' shared meshes, in
//! Bevy's main, prepass, deferred and shadow passes, without an entity per
//! tree.
//!
//! Each frame the render world:
//!
//! 1. copies every visible grove's trees and variants ([`extract_groves`]);
//! 2. keeps one Bevy mesh record (`MeshUniform`) per tree on the GPU, which
//!    the tree shaders read through the mesh bind group, exactly as they read
//!    a single tree's ([`prepare_grove_records`]);
//! 3. asks each tree material for its grove pipeline in every pass a view
//!    needs ([`specialize_groves`]), and queues one phase item per grove and
//!    phase ([`queue_groves`]);
//! 4. picks, per view, the trees in view and the detail levels each is drawn
//!    at, with the same distance bands and crossfade as single trees
//!    ([`cull_groves`]);
//! 5. draws each variant's part at each detail level once, with one instance
//!    per visible tree ([`DrawGroveParts`]).

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bevy::camera::primitives::Frustum;
use bevy::camera::visibility::VisibilityRange;
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey};
use bevy::core_pipeline::deferred::{AlphaMask3dDeferred, Opaque3dDeferred};
use bevy::core_pipeline::prepass::{
    AlphaMask3dPrepass, DeferredPrepass, Opaque3dPrepass, OpaqueNoLightmap3dBatchSetKey,
    OpaqueNoLightmap3dBinKey,
};
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::material::key::{ErasedMaterialKey, ErasedMaterialPipelineKey, ErasedMeshPipelineKey};
use bevy::material::{AlphaMode, OpaqueRendererMethod};
use bevy::math::{Affine3, Affine3A, UVec2, Vec3A};
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{
    LightEntity, LightKeyCache, MeshFlags, MeshPipeline, MeshPipelineKey, MeshTransforms,
    MeshUniform, PreparedMaterial, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    SetPrepassViewBindGroup, SetPrepassViewEmptyBindGroup, Shadow, ShadowBatchSetKey, ShadowBinKey,
    StandardMaterialKey, ViewKeyCache, ViewKeyPrepassCache, ViewLightEntities,
    alpha_mode_pipeline_key,
};
use bevy::prelude::*;
use bevy::render::combined_bind_group::CombinedBindGroupData;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::material_bind_groups::{MaterialBindGroupAllocators, MaterialBindGroupSlot};
use bevy::render::mesh::allocator::{MeshAllocator, MeshSlabs};
use bevy::render::mesh::{
    MeshMetadataFallbackBuffer, MeshMetadataFallbackMesh, RenderMesh, RenderMeshBufferInfo,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_phase::{
    BinnedPhaseItem, BinnedRenderPhase, BinnedRenderPhaseType, DrawFunctionId, DrawFunctions,
    InputUniformIndex, PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass,
    ViewBinnedRenderPhases,
};
use bevy::render::render_resource::{
    BindGroup, BufferUsages, CachedRenderPipelineId, PipelineCache, RawBufferVec, StorageBuffer,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shape::ViewFrustum;

use super::promote::{PromotedAs, PromotedTree};
use super::{TreeGroveAssets, TreeGroveInstances};
use crate::material::{GROVE_INSTANCE_SIZE, TreeMaterial, WindKey};
use crate::spawn::TreeGenerating;

/// The passes a grove is drawn in.
mod pass {
    /// The main forward pass.
    pub(super) const MAIN: u8 = 0;
    /// The depth, normal and motion-vector prepass of a forward camera.
    pub(super) const PREPASS: u8 = 1;
    /// The G-buffer pass of a deferred camera.
    pub(super) const DEFERRED: u8 = 2;
    /// A shadow map.
    pub(super) const SHADOW: u8 = 3;
    /// How many passes there are.
    pub(super) const COUNT: usize = 4;
}

/// Which parts a draw command draws: those in Bevy's opaque phases, those in
/// its alpha-mask phases, or all of them (in a shadow pass).
mod class {
    pub(super) const OPAQUE: u8 = 0;
    pub(super) const MASK: u8 = 1;
    pub(super) const ALL: u8 = 2;
}

/// Sets up the render world for groves. Added by
/// [`TreePlugin`](crate::TreePlugin).
pub(crate) struct GroveRenderPlugin;

impl Plugin for GroveRenderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedGroves>()
            .init_resource::<Groves>()
            .init_resource::<GroveViews>()
            .init_resource::<GrovePipelineCache>()
            .add_render_command::<Opaque3d, DrawGroveMain<{ class::OPAQUE }>>()
            .add_render_command::<AlphaMask3d, DrawGroveMain<{ class::MASK }>>()
            .add_render_command::<Opaque3dPrepass, DrawGrovePrepass<{ pass::PREPASS }, { class::OPAQUE }>>()
            .add_render_command::<AlphaMask3dPrepass, DrawGrovePrepass<{ pass::PREPASS }, { class::MASK }>>()
            .add_render_command::<Opaque3dDeferred, DrawGrovePrepass<{ pass::DEFERRED }, { class::OPAQUE }>>()
            .add_render_command::<AlphaMask3dDeferred, DrawGrovePrepass<{ pass::DEFERRED }, { class::MASK }>>()
            .add_render_command::<Shadow, DrawGrovePrepass<{ pass::SHADOW }, { class::ALL }>>()
            .add_systems(ExtractSchedule, extract_groves)
            .add_systems(
                Render,
                (
                    (prepare_grove_records, specialize_groves, queue_groves)
                        .chain()
                        .in_set(RenderSystems::Queue),
                    cull_groves.in_set(RenderSystems::PrepareResources),
                ),
            );
    }
}

// --------------------------------------------------------------------------
// Extraction

/// A visible grove, copied from the main world this frame.
struct ExtractedGrove {
    world_from_grove: Affine3A,
    instances: TreeGroveInstances,
    assets: TreeGroveAssets,
    /// Trees promoted to entities whose parts are drawn, which the grove
    /// leaves out, as positions in the grove's trees.
    promoted: HashSet<u32>,
}

/// Every visible grove with placed trees, copied from the main world this
/// frame.
#[derive(Resource, Default)]
pub(crate) struct ExtractedGroves(HashMap<MainEntity, ExtractedGrove>);

/// Copies every visible grove that has trees into the render world, with
/// the trees promoted to entities that draw themselves.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn extract_groves(
    mut extracted: ResMut<ExtractedGroves>,
    groves: Extract<
        Query<(
            Entity,
            &TreeGroveInstances,
            &TreeGroveAssets,
            &GlobalTransform,
            &InheritedVisibility,
        )>,
    >,
    promoted: Extract<Query<&PromotedTree, (With<PromotedAs>, Without<TreeGenerating>)>>,
) {
    extracted.0.clear();
    for (entity, instances, assets, transform, visibility) in &groves {
        if !visibility.get() || instances.trees().is_empty() {
            continue;
        }
        extracted.0.insert(
            MainEntity::from(entity),
            ExtractedGrove {
                world_from_grove: transform.affine(),
                instances: instances.clone(),
                assets: assets.clone(),
                promoted: HashSet::new(),
            },
        );
    }
    for promotion in &promoted {
        if let Some(grove) = extracted.0.get_mut(&MainEntity::from(promotion.grove))
            && let Some(index) = grove.instances.index_of(promotion.id)
        {
            grove.promoted.insert(index);
        }
    }
}

// --------------------------------------------------------------------------
// Per-grove GPU data

/// One mesh a grove draws: a variant's part at one detail level.
struct GroveDraw {
    /// The variant: its position in [`TreeGroveAssets::variants`].
    slot: u32,
    lod: u32,
    mesh: AssetId<Mesh>,
    material: AssetId<TreeMaterial>,
}

/// A tree's bounding sphere and origin in the world, for culling and
/// picking its detail level.
struct TreeBounds {
    /// The tree's variant, or `u32::MAX` if the variant draws nothing.
    slot: u32,
    origin: Vec3A,
    center: Vec3A,
    radius: f32,
}

/// What the render world keeps for one grove between frames.
struct GroveGpu {
    /// The trees and transform the records were built for.
    instances: TreeGroveInstances,
    assets: TreeGroveAssets,
    world_from_grove: Affine3A,
    /// One mesh record per tree.
    records: StorageBuffer<Vec<MeshUniform>>,
    /// The mesh bind group over `records`, rebuilt every frame because
    /// Bevy's mesh metadata buffer may move.
    bind_group: Option<BindGroup>,
    draws: Vec<GroveDraw>,
    trees: Vec<TreeBounds>,
    /// Each variant's detail-level distance bands, nearest level first.
    ranges: Vec<Vec<Option<VisibilityRange>>>,
    /// The most detail levels of any variant.
    levels: u32,
    /// Trees promoted to entities this frame, which the grove leaves out.
    promoted: HashSet<u32>,
}

impl GroveGpu {
    /// The index in a view's visible lists of `slot`'s detail level `lod`.
    fn list(&self, slot: u32, lod: u32) -> usize {
        (slot * self.levels + lod) as usize
    }
}

/// The render world's groves.
#[derive(Resource, Default)]
pub(crate) struct Groves(HashMap<MainEntity, GroveGpu>);

/// Brings each grove's mesh records, draws and bounds up to date, and
/// rebuilds its mesh bind group.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn prepare_grove_records(
    extracted: Res<ExtractedGroves>,
    mut groves: ResMut<Groves>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mesh_pipeline: Res<MeshPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mesh_allocator: Res<MeshAllocator>,
    fallback_mesh: Res<MeshMetadataFallbackMesh>,
    fallback_buffer: Option<Res<MeshMetadataFallbackBuffer>>,
) {
    groves
        .0
        .retain(|entity, _| extracted.0.contains_key(entity));
    let Some(fallback_buffer) = fallback_buffer else {
        return;
    };
    let metadata_index = mesh_allocator
        .mesh_metadata_slice(&fallback_mesh.0.id())
        .map(|slice| slice.range.start);

    for (entity, grove) in &extracted.0 {
        let gpu = groves
            .0
            .entry(*entity)
            .or_insert_with(|| new_grove_gpu(grove));
        let trees_changed =
            !gpu.instances.same(&grove.instances) || gpu.world_from_grove != grove.world_from_grove;
        let assets_changed = !gpu.assets.same(&grove.assets);

        // Mesh records: one per tree, in the order of the grove's trees.
        if trees_changed || gpu.records.get().is_empty() {
            let records = grove
                .instances
                .trees()
                .iter()
                .map(|tree| {
                    let world = grove.world_from_grove * tree.transform.compute_affine();
                    tree_record(world, metadata_index)
                })
                .collect();
            gpu.records.set(records);
            gpu.records.write_buffer(&device, &queue);
        }
        if trees_changed || assets_changed || gpu.draws.is_empty() {
            gpu.instances = grove.instances.clone();
            gpu.assets = grove.assets.clone();
            gpu.world_from_grove = grove.world_from_grove;
            describe_grove(gpu);
        }
        gpu.promoted.clone_from(&grove.promoted);

        gpu.bind_group = gpu.records.binding().map(|records| {
            mesh_pipeline.mesh_layouts.model_only(
                &device,
                &pipeline_cache,
                &records,
                &fallback_buffer.buffer,
            )
        });
    }
}

/// Empty GPU data for a grove, filled in by [`prepare_grove_records`].
fn new_grove_gpu(grove: &ExtractedGrove) -> GroveGpu {
    let mut records = StorageBuffer::default();
    records.set_label(Some("procgena_tree_grove_records"));
    GroveGpu {
        instances: grove.instances.clone(),
        assets: grove.assets.clone(),
        world_from_grove: grove.world_from_grove,
        records,
        bind_group: None,
        draws: Vec::new(),
        trees: Vec::new(),
        ranges: Vec::new(),
        levels: 1,
        promoted: HashSet::new(),
    }
}

/// A grove tree's mesh record: its world transform, which never changes
/// between frames, and Bevy's flags for a shadow-receiving mesh.
fn tree_record(world_from_local: Affine3A, metadata_index: Option<u32>) -> MeshUniform {
    let mut flags = MeshFlags::SHADOW_RECEIVER.bits() | u32::from(u16::MAX);
    if world_from_local.matrix3.determinant().is_sign_positive() {
        flags |= MeshFlags::SIGN_DETERMINANT_MODEL_3X3.bits();
    }
    let transform = Affine3::from(world_from_local);
    MeshUniform::new(
        &MeshTransforms {
            world_from_local: transform,
            previous_world_from_local: transform,
            flags,
        },
        0,
        MaterialBindGroupSlot(0),
        None,
        None,
        None,
        None,
        metadata_index,
    )
}

/// Lists the grove's draws, each variant's detail-level bands, and every
/// tree's bounds in the world.
fn describe_grove(gpu: &mut GroveGpu) {
    let variants = gpu.assets.variants();
    gpu.draws.clear();
    gpu.ranges.clear();
    gpu.levels = 1;

    // Each variant's draws and bands, and a sphere around all its parts.
    let mut spheres: Vec<Option<(Vec3A, f32)>> = Vec::with_capacity(variants.len());
    for (slot, variant) in variants.iter().enumerate() {
        let Some(generated) = variant.generated.as_ref() else {
            gpu.ranges.push(Vec::new());
            spheres.push(None);
            continue;
        };
        let levels = generated.level_count();
        gpu.levels = gpu.levels.max(levels as u32);
        gpu.ranges.push(
            (0..levels)
                .map(|lod| generated.range(lod).cloned())
                .collect(),
        );
        let mut min = Vec3A::splat(f32::INFINITY);
        let mut max = Vec3A::splat(f32::NEG_INFINITY);
        for part in variant.parts() {
            gpu.draws.push(GroveDraw {
                slot: slot as u32,
                lod: part.lod as u32,
                mesh: part.mesh.id(),
                material: part.material.id(),
            });
            min = min.min(part.bounds.min());
            max = max.max(part.bounds.max());
        }
        spheres.push((min.x <= max.x).then(|| {
            let center = (min + max) / 2.0;
            // Leave room for wind sway, which the bounds don't include.
            (center, (max - center).length() * SWAY_ALLOWANCE)
        }));
    }

    // Each tree's sphere in the world.
    gpu.trees.clear();
    gpu.trees.extend(gpu.instances.trees().iter().map(|tree| {
        let world = gpu.world_from_grove * tree.transform.compute_affine();
        let origin = world.translation;
        match spheres[tree.slot as usize] {
            Some((center, radius)) => TreeBounds {
                slot: tree.slot,
                origin,
                center: world.transform_point3a(center),
                radius: radius * largest_scale(&world),
            },
            None => TreeBounds {
                slot: u32::MAX,
                origin,
                center: origin,
                radius: 0.0,
            },
        }
    }));
}

/// How much a variant's bounding sphere is enlarged to cover wind sway.
const SWAY_ALLOWANCE: f32 = 1.15;

/// The largest factor `transform` stretches any direction by.
fn largest_scale(transform: &Affine3A) -> f32 {
    let m = transform.matrix3;
    m.x_axis
        .length()
        .max(m.y_axis.length())
        .max(m.z_axis.length())
}

impl TreeGroveInstances {
    /// Whether both are the same placement, compared by identity.
    fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.trees, &other.trees)
    }
}

impl TreeGroveAssets {
    /// Whether both are the same list of variants, compared by identity.
    fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.variants, &other.variants)
    }
}

// --------------------------------------------------------------------------
// Per-view state

/// What one view draws of one grove this frame.
#[derive(Default)]
struct ViewGrove {
    /// The pipeline of each of the grove's draws in each pass, or `None`
    /// when the draw isn't drawn in that pass or its pipeline isn't ready.
    pipelines: [Vec<Option<CachedRenderPipelineId>>; pass::COUNT],
    /// Whether each draw is in Bevy's alpha-mask phases rather than its
    /// opaque ones.
    masked: Vec<bool>,
    /// The visible trees of each variant at each detail level, as ranges of
    /// [`GroveViews::visible`].
    visible: Vec<Range<u32>>,
}

/// Every view's grove draws this frame.
#[derive(Resource)]
pub(crate) struct GroveViews {
    views: HashMap<(RetainedViewEntity, MainEntity), ViewGrove>,
    /// Every view's visible trees: per tree, its index in the grove's mesh
    /// records and its detail-level dither.
    visible: RawBufferVec<UVec2>,
}

impl Default for GroveViews {
    fn default() -> Self {
        let mut visible = RawBufferVec::new(BufferUsages::VERTEX);
        visible.set_label(Some("procgena_tree_grove_visible"));
        Self {
            views: HashMap::new(),
            visible,
        }
    }
}

// --------------------------------------------------------------------------
// Specialization

/// A pipeline to look up: one draw of one grove in one pass of one view.
struct Work {
    view: RetainedViewEntity,
    grove: MainEntity,
    pass: u8,
    draw: usize,
    key: MeshPipelineKey,
}

/// A view's pipeline keys and the grove passes it draws.
struct SpecializeView {
    retained: RetainedViewEntity,
    /// The forward pass key, or the light's key for a shadow view.
    key: MeshPipelineKey,
    /// The prepass key, when the camera has a prepass or a G-buffer pass.
    prepass_key: Option<MeshPipelineKey>,
    /// Whether the camera has a forward prepass.
    has_prepass: bool,
    /// Whether the camera draws deferred materials into a G-buffer.
    deferred: bool,
    /// Whether this is a shadow view.
    shadow: bool,
}

/// Grove pipelines already looked up, by pass, mesh key, mesh and material.
#[derive(Resource, Default)]
pub(crate) struct GrovePipelineCache(
    HashMap<(u8, MeshPipelineKey, AssetId<Mesh>, AssetId<TreeMaterial>), CachedRenderPipelineId>,
);

/// Works out, for every view and grove, which passes each of the grove's
/// draws is drawn in, and the pipeline for each.
///
/// The pipelines come from the tree material's own pipeline builders, with
/// the material key's grove flag set, so they are Bevy's tree pipelines plus
/// the per-tree vertex input.
pub(crate) fn specialize_groves(world: &mut World) {
    let views = specialize_views(world);
    let work = collect_work(world, &views);
    world.resource_mut::<GroveViews>().views.clear();

    for item in work {
        let Some(id) = specialize(world, &item) else {
            continue;
        };
        let groves = world.resource::<Groves>();
        let count = groves.0.get(&item.grove).map_or(0, |g| g.draws.len());
        let mut views = world.resource_mut::<GroveViews>();
        let view = views.views.entry((item.view, item.grove)).or_default();
        let pipelines = &mut view.pipelines[item.pass as usize];
        if pipelines.len() < count {
            pipelines.resize(count, None);
        }
        pipelines[item.draw] = Some(id);
    }

    // Record which phases each draw belongs in.
    world.resource_scope(|world, mut views: Mut<GroveViews>| {
        let groves = world.resource::<Groves>();
        let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
        for ((_, grove), view) in &mut views.views {
            let Some(grove) = groves.0.get(grove) else {
                continue;
            };
            view.masked = grove
                .draws
                .iter()
                .map(|draw| {
                    materials
                        .get(draw.material.untyped())
                        .is_some_and(|m| matches!(m.properties.alpha_mode, AlphaMode::Mask(_)))
                })
                .collect();
        }
    });
}

/// Every camera and shadow view that has pipeline keys this frame.
fn specialize_views(world: &mut World) -> Vec<SpecializeView> {
    let mut cameras =
        world.query_filtered::<(&ExtractedView, Has<DeferredPrepass>), Without<LightEntity>>();
    let mut lights = world.query_filtered::<&ExtractedView, With<LightEntity>>();
    let view_keys = world.resource::<ViewKeyCache>();
    let prepass_keys = world.resource::<ViewKeyPrepassCache>();
    let light_keys = world.resource::<LightKeyCache>();
    let prepass_phases = world.resource::<ViewBinnedRenderPhases<Opaque3dPrepass>>();
    let deferred_phases = world.resource::<ViewBinnedRenderPhases<Opaque3dDeferred>>();

    let mut views = Vec::new();
    for (view, deferred_camera) in cameras.iter(world) {
        let retained = view.retained_view_entity;
        let Some(key) = view_keys.get(&retained).copied() else {
            continue;
        };
        let has_prepass = prepass_phases.contains_key(&retained);
        let deferred = deferred_camera && deferred_phases.contains_key(&retained);
        views.push(SpecializeView {
            retained,
            key,
            prepass_key: prepass_keys
                .get(&retained)
                .copied()
                .filter(|_| has_prepass || deferred),
            has_prepass,
            deferred,
            shadow: false,
        });
    }
    for view in lights.iter(world) {
        let retained = view.retained_view_entity;
        let Some(key) = light_keys.get(&retained).copied() else {
            continue;
        };
        views.push(SpecializeView {
            retained,
            key,
            prepass_key: None,
            has_prepass: false,
            deferred: false,
            shadow: true,
        });
    }
    views
}

/// Lists the pipelines every view needs for every grove: one per draw and
/// pass, skipping draws whose mesh or material isn't ready.
fn collect_work(world: &World, views: &[SpecializeView]) -> Vec<Work> {
    let groves = world.resource::<Groves>();
    let meshes = world.resource::<RenderAssets<RenderMesh>>();
    let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
    let mut work = Vec::new();
    for view in views {
        let msaa = Msaa::from_samples(view.key.msaa_samples());
        for (grove_entity, grove) in &groves.0 {
            for (index, draw) in grove.draws.iter().enumerate() {
                let (Some(mesh), Some(material)) = (
                    meshes.get(draw.mesh),
                    materials.get(draw.material.untyped()),
                ) else {
                    continue;
                };
                let properties = &material.properties;
                let mesh_bits = MeshPipelineKey::from_bits_retain(mesh.key_bits.bits());
                let alpha = alpha_mode_pipeline_key(properties.alpha_mode, &msaa);
                let dither = if grove.ranges[draw.slot as usize].len() > 1 {
                    MeshPipelineKey::VISIBILITY_RANGE_DITHER
                } else {
                    MeshPipelineKey::NONE
                };
                let mut add = |pass: u8, key: MeshPipelineKey| {
                    work.push(Work {
                        view: view.retained,
                        grove: *grove_entity,
                        pass,
                        draw: index,
                        key: key | mesh_bits,
                    });
                };

                // A shadow view casts every part's shadow.
                if view.shadow {
                    if properties.shadows_enabled {
                        let discard = match properties.alpha_mode {
                            AlphaMode::Opaque => MeshPipelineKey::NONE,
                            _ => MeshPipelineKey::MAY_DISCARD,
                        };
                        add(
                            pass::SHADOW,
                            view.key | discard | MeshPipelineKey::PREPASS_READS_MATERIAL,
                        );
                    }
                    continue;
                }

                // A deferred camera draws deferred materials into its
                // G-buffer only; everything else is drawn forward, and into
                // the camera's prepass if it has one.
                let read_material = MeshPipelineKey::PREPASS_READS_MATERIAL;
                if view.deferred && properties.render_method == OpaqueRendererMethod::Deferred {
                    if let Some(prepass) = view.prepass_key {
                        let deferred = MeshPipelineKey::DEFERRED_PREPASS;
                        add(
                            pass::DEFERRED,
                            prepass | alpha | deferred | read_material | dither,
                        );
                    }
                    continue;
                }
                let bits: MeshPipelineKey = properties.mesh_pipeline_key_bits.downcast();
                add(pass::MAIN, view.key | bits | alpha | dither);
                if let (true, true, Some(prepass)) = (
                    view.has_prepass,
                    properties.prepass_enabled,
                    view.prepass_key,
                ) {
                    add(pass::PREPASS, prepass | alpha | read_material | dither);
                }
            }
        }
    }
    work
}

/// The pipeline for one work item, from the cache or the material's
/// pipeline builder, or `None` if it can't be built yet.
fn specialize(world: &mut World, item: &Work) -> Option<CachedRenderPipelineId> {
    let groves = world.resource::<Groves>();
    let draw = groves.0.get(&item.grove)?.draws.get(item.draw)?;
    let (mesh, material) = (draw.mesh, draw.material);
    let cache_key = (item.pass, item.key, mesh, material);
    if let Some(id) = world.resource::<GrovePipelineCache>().0.get(&cache_key) {
        return Some(*id);
    }
    let layout: MeshVertexBufferLayoutRef = world
        .resource::<RenderAssets<RenderMesh>>()
        .get(mesh)?
        .layout
        .clone();
    let properties = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(material.untyped())?
        .properties
        .clone();

    // The tree material's key, with the grove flag set.
    let mut material_key: CombinedBindGroupData<StandardMaterialKey, WindKey> =
        properties.material_key.to_key();
    material_key.extension = WindKey { grove: true };
    let key = ErasedMaterialPipelineKey {
        mesh_key: ErasedMeshPipelineKey::new(item.key),
        material_key: ErasedMaterialKey::new(material_key),
        type_id: TypeId::of::<TreeMaterial>(),
    };
    let result = if item.pass == pass::MAIN {
        (properties.base_specialize?)(world, key, &layout, &properties)
    } else {
        (properties.prepass_specialize?)(world, &key, &layout, &properties)
    };
    match result {
        Ok(id) => {
            world
                .resource_mut::<GrovePipelineCache>()
                .0
                .insert(cache_key, id);
            Some(id)
        }
        Err(error) => {
            error!("procgena_tree_bevy: cannot build a grove pipeline: {error}");
            None
        }
    }
}

// --------------------------------------------------------------------------
// Queueing

/// The phases [`queue_groves`] fills, and their draw functions.
#[derive(SystemParam)]
pub(crate) struct GrovePhases<'w> {
    opaque: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    mask: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3d>>,
    opaque_prepass: ResMut<'w, ViewBinnedRenderPhases<Opaque3dPrepass>>,
    mask_prepass: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3dPrepass>>,
    opaque_deferred: ResMut<'w, ViewBinnedRenderPhases<Opaque3dDeferred>>,
    mask_deferred: ResMut<'w, ViewBinnedRenderPhases<AlphaMask3dDeferred>>,
    shadow: ResMut<'w, ViewBinnedRenderPhases<Shadow>>,
    opaque_draws: Res<'w, DrawFunctions<Opaque3d>>,
    mask_draws: Res<'w, DrawFunctions<AlphaMask3d>>,
    opaque_prepass_draws: Res<'w, DrawFunctions<Opaque3dPrepass>>,
    mask_prepass_draws: Res<'w, DrawFunctions<AlphaMask3dPrepass>>,
    opaque_deferred_draws: Res<'w, DrawFunctions<Opaque3dDeferred>>,
    mask_deferred_draws: Res<'w, DrawFunctions<AlphaMask3dDeferred>>,
    shadow_draws: Res<'w, DrawFunctions<Shadow>>,
}

/// The first pipeline among `view`'s draws in `pass` whose class is
/// `masked`, if it draws any.
fn first_pipeline(
    view: &ViewGrove,
    pass: u8,
    masked: Option<bool>,
) -> Option<CachedRenderPipelineId> {
    view.pipelines[pass as usize]
        .iter()
        .zip(&view.masked)
        .find(|(id, m)| id.is_some() && masked.is_none_or(|want| **m == want))
        .and_then(|(id, _)| *id)
}

/// Puts each grove into every phase of every view that draws some of it,
/// and takes it out of the rest.
pub(crate) fn queue_groves(
    mut phases: GrovePhases,
    extracted: Res<ExtractedGroves>,
    views: Res<GroveViews>,
    cameras: Query<&ExtractedView, Without<LightEntity>>,
    lights: Query<&ExtractedView, With<LightEntity>>,
) {
    let main_opaque = phases
        .opaque_draws
        .read()
        .id::<DrawGroveMain<{ class::OPAQUE }>>();
    let main_mask = phases
        .mask_draws
        .read()
        .id::<DrawGroveMain<{ class::MASK }>>();
    let prepass_opaque = phases
        .opaque_prepass_draws
        .read()
        .id::<DrawGrovePrepass<{ pass::PREPASS }, { class::OPAQUE }>>();
    let prepass_mask = phases
        .mask_prepass_draws
        .read()
        .id::<DrawGrovePrepass<{ pass::PREPASS }, { class::MASK }>>();
    let deferred_opaque = phases
        .opaque_deferred_draws
        .read()
        .id::<DrawGrovePrepass<{ pass::DEFERRED }, { class::OPAQUE }>>();
    let deferred_mask = phases
        .mask_deferred_draws
        .read()
        .id::<DrawGrovePrepass<{ pass::DEFERRED }, { class::MASK }>>();
    let shadow = phases
        .shadow_draws
        .read()
        .id::<DrawGrovePrepass<{ pass::SHADOW }, { class::ALL }>>();
    let no_view = ViewGrove::default();

    for view in &cameras {
        let retained = view.retained_view_entity;
        for grove in extracted.0.keys() {
            let item = (Entity::PLACEHOLDER, *grove);
            let state = views.views.get(&(retained, *grove)).unwrap_or(&no_view);
            if let Some(phase) = phases.opaque.get_mut(&retained) {
                phase.remove(*grove);
                if let Some(pipeline) = first_pipeline(state, pass::MAIN, Some(false)) {
                    add_opaque(phase, pipeline, main_opaque, item);
                }
            }
            let target = (state, retained, item);
            requeue(&mut phases.mask, target, pass::MAIN, true, main_mask);
            requeue(
                &mut phases.opaque_prepass,
                target,
                pass::PREPASS,
                false,
                prepass_opaque,
            );
            requeue(
                &mut phases.mask_prepass,
                target,
                pass::PREPASS,
                true,
                prepass_mask,
            );
            requeue(
                &mut phases.opaque_deferred,
                target,
                pass::DEFERRED,
                false,
                deferred_opaque,
            );
            requeue(
                &mut phases.mask_deferred,
                target,
                pass::DEFERRED,
                true,
                deferred_mask,
            );
        }
    }

    for view in &lights {
        let retained = view.retained_view_entity;
        let Some(phase) = phases.shadow.get_mut(&retained) else {
            continue;
        };
        for grove in extracted.0.keys() {
            phase.remove(*grove);
            let Some(state) = views.views.get(&(retained, *grove)) else {
                continue;
            };
            if let Some(pipeline) = first_pipeline(state, pass::SHADOW, None) {
                phase.add(
                    ShadowBatchSetKey {
                        pipeline,
                        draw_function: shadow,
                        material_bind_group_index: None,
                        slabs: MeshSlabs::default(),
                    },
                    ShadowBinKey {
                        asset_id: AssetId::<Mesh>::default().untyped(),
                    },
                    (Entity::PLACEHOLDER, *grove),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                );
            }
        }
    }
}

/// Adds a grove item to an opaque main phase.
fn add_opaque(
    phase: &mut BinnedRenderPhase<Opaque3d>,
    pipeline: CachedRenderPipelineId,
    draw_function: DrawFunctionId,
    item: (Entity, MainEntity),
) {
    phase.add(
        Opaque3dBatchSetKey {
            draw_function,
            pipeline,
            material_bind_group_index: None,
            lightmap_slab: None,
            slabs: MeshSlabs::default(),
        },
        Opaque3dBinKey {
            asset_id: AssetId::<Mesh>::default().untyped(),
        },
        item,
        InputUniformIndex::default(),
        BinnedRenderPhaseType::NonMesh,
    );
}

/// Takes a grove out of one of a camera's alpha-mask, prepass or deferred
/// phases, and puts it back if the camera draws parts of the grove there.
fn requeue<P>(
    phases: &mut ViewBinnedRenderPhases<P>,
    (state, view, item): (&ViewGrove, RetainedViewEntity, (Entity, MainEntity)),
    pass: u8,
    masked: bool,
    draw_function: DrawFunctionId,
) where
    P: BinnedPhaseItem<
            BatchSetKey = OpaqueNoLightmap3dBatchSetKey,
            BinKey = OpaqueNoLightmap3dBinKey,
        >,
{
    let Some(phase) = phases.get_mut(&view) else {
        return;
    };
    phase.remove(item.1);
    if let Some(pipeline) = first_pipeline(state, pass, Some(masked)) {
        add_no_lightmap(phase, pipeline, draw_function, item);
    }
}

/// Adds a grove item to an alpha-mask, prepass or deferred phase.
fn add_no_lightmap<P>(
    phase: &mut BinnedRenderPhase<P>,
    pipeline: CachedRenderPipelineId,
    draw_function: DrawFunctionId,
    item: (Entity, MainEntity),
) where
    P: BinnedPhaseItem<
            BatchSetKey = OpaqueNoLightmap3dBatchSetKey,
            BinKey = OpaqueNoLightmap3dBinKey,
        >,
{
    phase.add(
        OpaqueNoLightmap3dBatchSetKey {
            pipeline,
            draw_function,
            material_bind_group_index: None,
            slabs: MeshSlabs::default(),
        },
        OpaqueNoLightmap3dBinKey {
            asset_id: AssetId::<Mesh>::default().untyped(),
        },
        item,
        InputUniformIndex::default(),
        BinnedRenderPhaseType::NonMesh,
    );
}

// --------------------------------------------------------------------------
// Culling

/// A view to cull groves for.
struct CullView {
    retained: RetainedViewEntity,
    frustum: Frustum,
    /// Where detail levels are measured from: the camera, also for its
    /// shadow views.
    lod_origin: Vec3A,
    /// Whether to skip the near plane, as for a directional light's shadow,
    /// which also takes casters between it and the view.
    skip_near: bool,
}

/// Fills every view's visible trees for every grove it draws, and uploads
/// them.
pub(crate) fn cull_groves(
    mut views: ResMut<GroveViews>,
    groves: Res<Groves>,
    cameras: Query<(&ExtractedView, Option<&ViewLightEntities>), Without<LightEntity>>,
    lights: Query<(&ExtractedView, &LightEntity)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    // Each camera, then each of its shadow views measured from the camera.
    let mut cull_views = Vec::new();
    for (view, light_views) in &cameras {
        let origin = view.world_from_view.translation_vec3a();
        cull_views.push(CullView {
            retained: view.retained_view_entity,
            frustum: view_frustum(view),
            lod_origin: origin,
            skip_near: false,
        });
        for light in light_views.into_iter().flat_map(|l| &l.lights) {
            let Ok((light_view, kind)) = lights.get(*light) else {
                continue;
            };
            cull_views.push(CullView {
                retained: light_view.retained_view_entity,
                frustum: view_frustum(light_view),
                lod_origin: origin,
                skip_near: matches!(kind, LightEntity::Directional { .. }),
            });
        }
    }

    // Fill each view's lists, grouped by variant and level.
    let views = &mut *views;
    views.visible.clear();
    let mut lists: Vec<Vec<UVec2>> = Vec::new();
    for cull in &cull_views {
        for (entity, grove) in &groves.0 {
            let Some(state) = views.views.get_mut(&(cull.retained, *entity)) else {
                continue;
            };
            let count = (grove.ranges.len() as u32 * grove.levels) as usize;
            lists.iter_mut().for_each(Vec::clear);
            lists.resize_with(count.max(lists.len()), Vec::new);
            for (index, tree) in grove.trees.iter().enumerate() {
                if tree.slot == u32::MAX
                    || grove.promoted.contains(&(index as u32))
                    || !in_frustum(cull, tree)
                {
                    continue;
                }
                let distance = (tree.origin - cull.lod_origin).length();
                for (lod, range) in grove.ranges[tree.slot as usize].iter().enumerate() {
                    let Some(dither) = dither_level(range.as_ref(), distance) else {
                        continue;
                    };
                    lists[grove.list(tree.slot, lod as u32)]
                        .push(UVec2::new(index as u32, dither as u32));
                }
            }
            state.visible.clear();
            for list in &lists[..count] {
                let start = views.visible.len() as u32;
                for entry in list {
                    views.visible.push(*entry);
                }
                state.visible.push(start..views.visible.len() as u32);
            }
        }
    }
    views.visible.write_buffer(&device, &queue);
}

/// The frustum of `view`.
fn view_frustum(view: &ExtractedView) -> Frustum {
    let clip_from_world = view
        .clip_from_world
        .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
    Frustum(ViewFrustum::from_clip_from_world(&clip_from_world))
}

/// Whether `tree` can be seen from (or cast a shadow into) `view`.
///
/// A plane that isn't finite, such as the far plane of a perspective camera
/// that sees to infinity, removes nothing.
fn in_frustum(view: &CullView, tree: &TreeBounds) -> bool {
    let center = tree.center.extend(1.0);
    view.frustum
        .half_spaces
        .iter()
        .enumerate()
        .all(|(index, half_space)| {
            let plane = half_space.normal_d();
            (view.skip_near && index == ViewFrustum::NEAR_PLANE_IDX)
                || !plane.is_finite()
                || plane.dot(center) + tree.radius > 0.0
        })
}

/// The dither level a tree `distance` metres from the camera is drawn with
/// at a detail level with `range`, as Bevy's shaders work it out for single
/// trees, or `None` if the level isn't drawn at that distance.
///
/// Levels -16 to 0 fade in over the near band, 0 to 16 fade out over the
/// far band, and 0 is fully drawn. A level without a range is always drawn.
fn dither_level(range: Option<&VisibilityRange>, distance: f32) -> Option<i32> {
    let Some(range) = range else {
        return Some(0);
    };
    if distance < range.start_margin.start || distance >= range.end_margin.end {
        return None;
    }
    let (offset, band) = if distance >= range.end_margin.start {
        (0, &range.end_margin)
    } else {
        (-16, &range.start_margin)
    };
    let width = band.end - band.start;
    let level = if width > 0.0 {
        (((distance - band.start) / width * 16.0).round() as i32).clamp(0, 16)
    } else {
        16
    };
    Some(offset + level)
}

// --------------------------------------------------------------------------
// Drawing

/// Draws a grove's parts of one class in one pass: for each part with
/// visible trees, binds its pipeline, mesh and material, and draws one
/// instance per visible tree.
pub(crate) struct DrawGroveParts<const PASS: u8, const CLASS: u8>;

impl<P: PhaseItem, const PASS: u8, const CLASS: u8> RenderCommand<P>
    for DrawGroveParts<PASS, CLASS>
{
    type Param = (
        SRes<Groves>,
        SRes<GroveViews>,
        SRes<PipelineCache>,
        SRes<MeshAllocator>,
        SRes<RenderAssets<RenderMesh>>,
        SRes<ErasedRenderAssets<PreparedMaterial>>,
        SRes<MaterialBindGroupAllocators>,
    );
    type ViewQuery = Read<ExtractedView>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (groves, views, pipelines, allocator, meshes, materials, allocators): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (groves, views, pipelines, allocator, meshes, materials, allocators) = (
            groves.into_inner(),
            views.into_inner(),
            pipelines.into_inner(),
            allocator.into_inner(),
            meshes.into_inner(),
            materials.into_inner(),
            allocators.into_inner(),
        );
        let grove_entity = item.main_entity();
        let (Some(grove), Some(state)) = (
            groves.0.get(&grove_entity),
            views.views.get(&(view.retained_view_entity, grove_entity)),
        ) else {
            return RenderCommandResult::Skip;
        };
        let (Some(records), Some(visible)) = (grove.bind_group.as_ref(), views.visible.buffer())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(tree_allocator) = allocators.get(&TypeId::of::<TreeMaterial>()) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(2, records, &[]);

        for (index, draw) in grove.draws.iter().enumerate() {
            let masked = state.masked.get(index).copied().unwrap_or(false);
            if (CLASS == class::OPAQUE && masked) || (CLASS == class::MASK && !masked) {
                continue;
            }
            let Some(Some(pipeline)) = state.pipelines[PASS as usize].get(index) else {
                continue;
            };
            let Some(trees) = state.visible.get(grove.list(draw.slot, draw.lod)) else {
                continue;
            };
            if trees.is_empty() {
                continue;
            }

            // The part's pipeline, material, mesh and index data.
            let Some(pipeline) = pipelines.get_render_pipeline(*pipeline) else {
                continue;
            };
            let Some(bind_group) = materials
                .get(draw.material.untyped())
                .and_then(|m| tree_allocator.get(m.binding.group))
                .and_then(|group| group.bind_group())
            else {
                continue;
            };
            let (Some(mesh), Some(vertices)) = (
                meshes.get(draw.mesh),
                allocator.mesh_vertex_slice(&draw.mesh),
            ) else {
                continue;
            };
            let RenderMeshBufferInfo::Indexed {
                count,
                index_format,
            } = mesh.buffer_info
            else {
                continue;
            };
            let Some(indices) = allocator.mesh_index_slice(&draw.mesh) else {
                continue;
            };

            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(3, bind_group, &[]);
            pass.set_vertex_buffer(0, vertices.buffer.slice(..));
            pass.set_vertex_buffer(
                1,
                visible.slice(
                    u64::from(trees.start) * GROVE_INSTANCE_SIZE
                        ..u64::from(trees.end) * GROVE_INSTANCE_SIZE,
                ),
            );
            pass.set_index_buffer(indices.buffer.slice(..), index_format);
            pass.draw_indexed(
                indices.range.start..indices.range.start + count,
                vertices.range.start as i32,
                0..trees.end - trees.start,
            );
        }
        RenderCommandResult::Success
    }
}

/// Draws grove parts in Bevy's forward opaque or alpha-mask phase.
pub(crate) type DrawGroveMain<const CLASS: u8> = (
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawGroveParts<{ pass::MAIN }, CLASS>,
);

/// Draws grove parts in a prepass, deferred or shadow phase.
pub(crate) type DrawGrovePrepass<const PASS: u8, const CLASS: u8> = (
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    DrawGroveParts<PASS, CLASS>,
);

#[cfg(test)]
mod tests {
    use bevy::camera::{
        CameraProjection, OrthographicProjection, PerspectiveProjection, ScalingMode,
    };

    use super::*;

    /// A middle detail level, faded in over 8–12 m and out over 18–22 m.
    fn middle() -> VisibilityRange {
        VisibilityRange {
            start_margin: 8.0..12.0,
            end_margin: 18.0..22.0,
            use_aabb: false,
        }
    }

    #[test]
    fn detail_levels_fade_over_their_bands_as_single_trees_do() {
        let range = middle();
        let at = |distance| dither_level(Some(&range), distance);

        // Outside the level, fading in, fully drawn, fading out.
        assert_eq!(at(7.9), None);
        assert_eq!(at(8.0), Some(-16));
        assert_eq!(at(10.0), Some(-8));
        assert_eq!(at(12.0), Some(0));
        assert_eq!(at(15.0), Some(0));
        assert_eq!(at(18.0), Some(0));
        assert_eq!(at(20.0), Some(8));
        assert_eq!(at(21.9), Some(16));
        assert_eq!(at(22.0), None);

        // A level that starts at the camera is fully drawn there, and a lone
        // level everywhere.
        let first = VisibilityRange {
            start_margin: 0.0..0.0,
            ..middle()
        };
        assert_eq!(dither_level(Some(&first), 0.0), Some(0));
        assert_eq!(dither_level(None, 1e6), Some(0));
    }

    #[test]
    fn trees_are_culled_by_their_bounding_sphere() {
        // A camera at the origin looking down -Z, with an infinite far plane,
        // and a directional light's shadow view looking the same way.
        let perspective = PerspectiveProjection {
            fov: 1.0,
            aspect_ratio: 1.0,
            near: 0.1,
            ..default()
        };
        let camera = perspective.get_clip_from_view();
        let mut orthographic = OrthographicProjection {
            near: 0.1,
            far: 50.0,
            scaling_mode: ScalingMode::Fixed {
                width: 40.0,
                height: 40.0,
            },
            ..OrthographicProjection::default_3d()
        };
        orthographic.update(40.0, 40.0);
        let shadow = orthographic.get_clip_from_view();
        let view = |skip_near| CullView {
            retained: RetainedViewEntity::new(MainEntity::from(Entity::PLACEHOLDER), None, 0),
            frustum: Frustum(ViewFrustum::from_clip_from_world(if skip_near {
                &shadow
            } else {
                &camera
            })),
            lod_origin: Vec3A::ZERO,
            skip_near,
        };
        let tree = |center: Vec3, radius| TreeBounds {
            slot: 0,
            origin: center.into(),
            center: center.into(),
            radius,
        };

        // Far ahead (beyond any far plane), beside the view but overlapping
        // it, and wholly beside or behind it.
        assert!(in_frustum(
            &view(false),
            &tree(Vec3::new(0.0, 0.0, -1e5), 1.0)
        ));
        assert!(in_frustum(
            &view(false),
            &tree(Vec3::new(6.0, 0.0, -10.0), 2.0)
        ));
        assert!(!in_frustum(
            &view(false),
            &tree(Vec3::new(20.0, 0.0, -10.0), 2.0)
        ));
        assert!(!in_frustum(
            &view(false),
            &tree(Vec3::new(0.0, 0.0, 10.0), 2.0)
        ));

        // A directional shadow takes casters behind its near plane.
        assert!(in_frustum(
            &view(true),
            &tree(Vec3::new(0.0, 0.0, 10.0), 2.0)
        ));
    }
}
