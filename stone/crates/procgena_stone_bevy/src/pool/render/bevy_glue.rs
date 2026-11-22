//! Every touch point with Bevy's render internals: pipeline specialization,
//! phase items in `Opaque3d`, `Opaque3dPrepass`, `Opaque3dDeferred` and
//! `Shadow`, and the render commands that draw pooled rocks. Most of a Bevy
//! upgrade happens here and in `pool_bridge.wesl`.
//!
//! All pooled rocks are one `NonMesh` binned phase item per view, whose entity
//! is the [`PoolDrawer`](crate::pool::PoolDrawer). Its render command looks up
//! the view's culling state and draws every pool. Binned phases keep items
//! between frames, so each frame the item is removed and added again.

use bevy::asset::{AssetId, Handle, RenderAssetUsages};
use bevy::core_pipeline::core_3d::{
    CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey,
};
use bevy::core_pipeline::deferred::Opaque3dDeferred;
use bevy::core_pipeline::prepass::{
    DeferredPrepass, Opaque3dPrepass, OpaqueNoLightmap3dBatchSetKey, OpaqueNoLightmap3dBinKey,
    prepass_target_descriptors,
};
use bevy::ecs::prelude::*;
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::log::warn;
use bevy::material::OpaqueRendererMethod;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexBufferLayout};
use bevy::pbr::{
    DefaultOpaqueRendererMethod, LightEntity, MATERIAL_BIND_GROUP_INDEX, MeshPipeline,
    MeshPipelineKey, MeshPipelineViewLayout, PrepassPipeline, SetMeshViewBindGroup,
    SetMeshViewBindingArrayBindGroup, SetPrepassViewBindGroup, SetPrepassViewEmptyBindGroup,
    Shadow, ShadowBatchSetKey, ShadowBinKey, ViewKeyCache, ViewKeyPrepassCache,
};
use bevy::reflect::{PartialReflect, ReflectRef};
use bevy::render::mesh::MeshVertexBufferLayouts;
use bevy::render::mesh::allocator::MeshSlabs;
use bevy::render::render_phase::{
    BinnedPhaseItem, BinnedRenderPhase, BinnedRenderPhaseType, DrawFunctionId, DrawFunctions,
    InputUniformIndex, PhaseItem, RenderCommand, RenderCommandResult, SetItemPipeline,
    TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, CachedRenderPipelineId,
    CompareFunction, DepthBiasState, DepthStencilState, Face, FragmentState, IndexFormat,
    MultisampleState, PipelineCache, PrimitiveState, RenderPipelineDescriptor,
    SpecializedMeshPipeline, SpecializedRenderPipeline, SpecializedRenderPipelines, StencilState,
    VertexAttribute, VertexFormat, VertexState, VertexStepMode,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::settings::WgpuFeatures;
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewUniformOffset, ViewUniforms};
use bevy::shader::{Shader, ShaderDefVal};
use std::collections::HashSet;
use procgena_stone::gpu::GPU_VERTEX_SIZE;

use super::pools::{RenderPools, draw_args_offset};
use super::views::{PoolViews, VISIBLE_ENTRY_SIZE};
use super::{ExtractedPools, PoolLayouts, PoolShaders};

/// The vertex buffers of every pooled-rock draw: the generated vertices
/// (position then normal), and one visible-rock entry per instance.
fn vertex_buffers() -> Vec<VertexBufferLayout> {
    vec![
        VertexBufferLayout {
            array_stride: GPU_VERTEX_SIZE,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        },
        VertexBufferLayout {
            array_stride: VISIBLE_ENTRY_SIZE,
            step_mode: VertexStepMode::Instance,
            attributes: vec![VertexAttribute {
                format: VertexFormat::Uint32x2,
                offset: 0,
                shader_location: 2,
            }],
        },
    ]
}

/// The main-pass pipeline: Bevy's mesh pipeline, so the view bindings and
/// shader definitions match what its PBR functions expect, with the pool's
/// shader, vertex buffers and bind group.
#[derive(Resource, Clone)]
pub(crate) struct PoolMainPipeline {
    mesh_pipeline: MeshPipeline,
    shader: Handle<Shader>,
    pool_layout: BindGroupLayoutDescriptor,
    /// The layout of a mesh with positions and normals, which is what
    /// `MeshPipeline::specialize` needs to produce a lit pipeline.
    vertex_layout: MeshVertexBufferLayoutRef,
}

impl SpecializedRenderPipeline for PoolMainPipeline {
    type Key = MeshPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let mut descriptor = self
            .mesh_pipeline
            .specialize(key, &self.vertex_layout)
            .expect("the template layout has every attribute the mesh pipeline requires");
        descriptor.label = Some("ze_stone_pool_main".into());
        descriptor.vertex.shader = self.shader.clone();
        descriptor.vertex.entry_point = Some("vertex".into());
        descriptor.vertex.buffers = vertex_buffers();
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = self.shader.clone();
            fragment.entry_point = Some("fragment".into());
        }
        descriptor.layout = main_pass_layout(
            &descriptor.layout,
            self.mesh_pipeline.get_view_layout(key.into()),
            self.pool_layout.clone(),
        );
        descriptor.primitive.cull_mode = Some(Face::Back);
        descriptor
    }
}

/// The main pass's bind group layouts: Bevy's view (0) and binding-array (1)
/// groups, then the pool group where the mesh group was.
///
/// `MeshPipeline::specialize` (`bevy_pbr` 0.20) builds exactly those three. If
/// a release reorders or adds groups, the pipeline could bind the wrong group
/// without an error, so debug builds check Bevy's layout here.
fn main_pass_layout(
    bevy_layout: &[BindGroupLayoutDescriptor],
    view: MeshPipelineViewLayout,
    pool: BindGroupLayoutDescriptor,
) -> Vec<BindGroupLayoutDescriptor> {
    debug_assert!(
        bevy_layout.len() == 3
            && bevy_layout[0] == view.main_layout
            && bevy_layout[1] == view.binding_array_layout,
        "procgena_stone_bevy: MeshPipeline::specialize no longer builds [view, binding array, mesh] \
         ({} groups); update main_pass_layout and DrawPoolMain",
        bevy_layout.len()
    );
    vec![view.main_layout, view.binding_array_layout, pool]
}

/// The prepass and deferred pipeline: depth, and normals, motion vectors and
/// the G-buffer when the view has them. Specialized by the view's prepass key,
/// as Bevy's own prepass pipeline is.
#[derive(Resource, Clone)]
pub(crate) struct PoolPrepassPipeline {
    view_layout_motion_vectors: BindGroupLayoutDescriptor,
    view_layout_no_motion_vectors: BindGroupLayoutDescriptor,
    empty_layout: BindGroupLayoutDescriptor,
    pool_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
}

impl SpecializedRenderPipeline for PoolPrepassPipeline {
    type Key = MeshPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let normals = key.contains(MeshPipelineKey::NORMAL_PREPASS);
        let motion = key.contains(MeshPipelineKey::MOTION_VECTOR_PREPASS);
        let deferred = key.contains(MeshPipelineKey::DEFERRED_PREPASS);
        let mut defs = prepass_shader_defs(key);
        let mut targets = prepass_target_descriptors(normals, motion, deferred);
        if targets.iter().all(Option::is_none) {
            // Depth only: a fragment shader without outputs, for the fade dither.
            targets.clear();
        } else {
            defs.push("PREPASS_FRAGMENT".into());
        }
        RenderPipelineDescriptor {
            label: Some(
                if deferred {
                    "ze_stone_pool_deferred"
                } else {
                    "ze_stone_pool_prepass"
                }
                .into(),
            ),
            layout: vec![
                if motion {
                    self.view_layout_motion_vectors.clone()
                } else {
                    self.view_layout_no_motion_vectors.clone()
                },
                self.empty_layout.clone(),
                self.pool_layout.clone(),
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: defs.clone(),
                entry_point: Some("vertex".into()),
                buffers: vertex_buffers(),
                ..Default::default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs: defs,
                entry_point: Some("fragment".into()),
                targets,
                ..Default::default()
            }),
            primitive: PrimitiveState {
                cull_mode: Some(Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState {
                count: key.msaa_samples(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// The shader definitions of the prepass and deferred pipeline, before
/// `PREPASS_FRAGMENT`, which depends on the targets.
///
/// Bevy's PBR modules read these, and Bevy sets them in a private function,
/// `PrepassPipeline::specialize` (`bevy_pbr` 0.20, `prepass/mod.rs`), so this
/// copies the ones pooled rocks need. A port compares this list with that
/// function: a definition Bevy renames compiles and is silently ignored.
fn prepass_shader_defs(key: MeshPipelineKey) -> Vec<ShaderDefVal> {
    let normals = key.contains(MeshPipelineKey::NORMAL_PREPASS);
    let motion = key.contains(MeshPipelineKey::MOTION_VECTOR_PREPASS);
    let deferred = key.contains(MeshPipelineKey::DEFERRED_PREPASS);
    let mut defs: Vec<ShaderDefVal> = vec![
        "PREPASS_PIPELINE".into(),
        // Bevy's PBR modules name a material group; pooled rocks have none.
        ShaderDefVal::UInt(
            "MATERIAL_BIND_GROUP".into(),
            MATERIAL_BIND_GROUP_INDEX as u32,
        ),
    ];
    for (on, name) in [
        (
            key.contains(MeshPipelineKey::DEPTH_PREPASS),
            "DEPTH_PREPASS",
        ),
        (normals, "NORMAL_PREPASS"),
        (motion, "MOTION_VECTOR_PREPASS"),
        (deferred, "DEFERRED_PREPASS"),
        (normals || deferred, "NORMAL_PREPASS_OR_DEFERRED_PREPASS"),
        (
            motion || deferred,
            "MOTION_VECTOR_PREPASS_OR_DEFERRED_PREPASS",
        ),
    ] {
        if on {
            defs.push(name.into());
        }
    }
    defs
}

/// The shadow pipeline: depth only. Specialized by whether the light is
/// directional, whose shadow maps clamp geometry behind the near plane rather
/// than clipping it, as Bevy's own shadow pipelines do.
#[derive(Resource, Clone)]
pub(crate) struct PoolShadowPipeline {
    shadow_view_layout: BindGroupLayoutDescriptor,
    empty_layout: BindGroupLayoutDescriptor,
    pool_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    /// Whether the GPU can clamp depth, which directional shadows need. Without
    /// it, pooled rocks cast no directional shadows rather than wrong ones.
    clamps_depth: bool,
}

impl SpecializedRenderPipeline for PoolShadowPipeline {
    /// Whether the light is directional.
    type Key = bool;

    fn specialize(&self, directional: bool) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("ze_stone_pool_shadow".into()),
            layout: vec![
                self.shadow_view_layout.clone(),
                self.empty_layout.clone(),
                self.pool_layout.clone(),
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: vec!["POOL_SHADOW_PASS".into()],
                entry_point: Some("vertex".into()),
                buffers: vertex_buffers(),
                ..Default::default()
            },
            fragment: None,
            primitive: PrimitiveState {
                cull_mode: Some(Face::Back),
                unclipped_depth: directional,
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            ..Default::default()
        }
    }
}

pub(crate) fn init_pipelines(
    mut commands: Commands,
    mesh_pipeline: Res<MeshPipeline>,
    prepass_pipeline: Res<PrepassPipeline>,
    shaders: Res<PoolShaders>,
    layouts: Res<PoolLayouts>,
    mut vertex_layouts: ResMut<MeshVertexBufferLayouts>,
    device: Res<RenderDevice>,
) {
    let template = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]]);
    commands.insert_resource(PoolMainPipeline {
        mesh_pipeline: mesh_pipeline.clone(),
        shader: shaders.main.clone(),
        pool_layout: layouts.draw.clone(),
        vertex_layout: template.get_mesh_vertex_buffer_layout(&mut vertex_layouts),
    });
    // Bevy's own prepass layouts, so its prepass view bind groups fit.
    commands.insert_resource(PoolPrepassPipeline {
        view_layout_motion_vectors: prepass_pipeline.view_layout_motion_vectors.clone(),
        view_layout_no_motion_vectors: prepass_pipeline.view_layout_no_motion_vectors.clone(),
        empty_layout: prepass_pipeline.empty_layout.clone(),
        pool_layout: layouts.draw.clone(),
        shader: shaders.prepass.clone(),
    });
    commands.insert_resource(PoolShadowPipeline {
        shadow_view_layout: layouts.shadow_view.clone(),
        empty_layout: layouts.empty.clone(),
        pool_layout: layouts.draw.clone(),
        shader: shaders.shadow.clone(),
        clamps_depth: device.features().contains(WgpuFeatures::DEPTH_CLIP_CONTROL),
    });
}

/// The light view's bind group, and the empty group after it, for the shadow pass.
#[derive(Resource, Default)]
pub(crate) struct PoolShadowBindGroups {
    view: Option<BindGroup>,
    empty: Option<BindGroup>,
}

/// Rebuilds the shadow pass's view bind group, whose uniform buffer Bevy may
/// reallocate any frame.
pub(crate) fn prepare_shadow_bind_groups(
    mut groups: ResMut<PoolShadowBindGroups>,
    layouts: Res<PoolLayouts>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
) {
    if groups.empty.is_none() {
        groups.empty = Some(device.create_bind_group(
            "ze_stone_pool_empty",
            &pipeline_cache.get_bind_group_layout(&layouts.empty),
            &[],
        ));
    }
    groups.view = view_uniforms.uniforms.binding().map(|view| {
        device.create_bind_group(
            "ze_stone_pool_shadow_view",
            &pipeline_cache.get_bind_group_layout(&layouts.shadow_view),
            &BindGroupEntries::single(view),
        )
    });
}

/// Draws every pool this view culled: one indirect draw per variant and detail
/// level that has rocks.
pub(crate) struct DrawStonePools;

impl<P: PhaseItem> RenderCommand<P> for DrawStonePools {
    type Param = (SRes<RenderPools>, SRes<PoolViews>);
    type ViewQuery = Read<ExtractedView>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (pools, views): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (pools, views) = (pools.into_inner(), views.into_inner());
        for (id, pool) in &pools.pools {
            let Some(state) = views.views.get(&(view.retained_view_entity, *id)) else {
                continue;
            };
            let (true, Some(group), Some(starts)) = (
                state.culled,
                pool.draw_group.as_ref(),
                Some(pool.variant_start.get()),
            ) else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            for batch in &pool.batches {
                pass.set_vertex_buffer(0, batch.vertices.slice(..));
                pass.set_index_buffer(batch.indices.slice(..), IndexFormat::Uint32);
                for mesh in batch.first_mesh..batch.first_mesh + batch.mesh_count {
                    let (variant, lod) = (mesh / pool.lod_count, mesh % pool.lod_count);
                    let count = pool.variant_counts[variant as usize];
                    if count == 0 {
                        continue;
                    }
                    // The rocks of this draw start at this entry of the visible list.
                    let first = u64::from(lod * pool.instance_count + starts[variant as usize]);
                    pass.set_vertex_buffer(
                        1,
                        state.visible.slice(
                            first * VISIBLE_ENTRY_SIZE
                                ..(first + u64::from(count)) * VISIBLE_ENTRY_SIZE,
                        ),
                    );
                    pass.draw_indexed_indirect(&state.draw_args, draw_args_offset(mesh));
                }
            }
        }
        RenderCommandResult::Success
    }
}

/// Binds the light view's uniform for the shadow pass, and the empty group after it.
pub(crate) struct SetPoolShadowView<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetPoolShadowView<I> {
    type Param = SRes<PoolShadowBindGroups>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        groups: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let groups = groups.into_inner();
        let (Some(view), Some(empty)) = (groups.view.as_ref(), groups.empty.as_ref()) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, view, &[offset.offset]);
        pass.set_bind_group(I + 1, empty, &[]);
        RenderCommandResult::Success
    }
}

pub(crate) type DrawPoolMain = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawStonePools,
);

/// Draws pooled rocks into the prepass and the deferred G-buffer, with Bevy's
/// prepass view bindings.
pub(crate) type DrawPoolPrepass = (
    SetItemPipeline,
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    DrawStonePools,
);

pub(crate) type DrawPoolShadow = (SetItemPipeline, SetPoolShadowView<0>, DrawStonePools);

/// Whether opaque materials render deferred by default. Pooled rocks follow
/// the default, like a `StandardMaterial` left at `OpaqueRendererMethod::Auto`.
///
/// The method is a private field of Bevy's resource, so reflection reads it.
/// If Bevy changes the resource's shape this would quietly say "forward", so
/// debug builds assert the shape.
fn deferred_by_default(method: &DefaultOpaqueRendererMethod) -> bool {
    let method = match method.reflect_ref() {
        ReflectRef::TupleStruct(method) => method
            .field(0)
            .and_then(<dyn PartialReflect>::try_downcast_ref::<OpaqueRendererMethod>),
        _ => None,
    };
    debug_assert!(
        method.is_some(),
        "procgena_stone_bevy: DefaultOpaqueRendererMethod is no longer a tuple of an \
         OpaqueRendererMethod; update deferred_by_default"
    );
    method.is_some_and(|method| *method == OpaqueRendererMethod::Deferred)
}

/// The pipelines `queue_pools` specializes.
#[derive(SystemParam)]
pub(crate) struct PoolPipelines<'w> {
    cache: Res<'w, PipelineCache>,
    main: Res<'w, PoolMainPipeline>,
    main_specialized: ResMut<'w, SpecializedRenderPipelines<PoolMainPipeline>>,
    prepass: Res<'w, PoolPrepassPipeline>,
    prepass_specialized: ResMut<'w, SpecializedRenderPipelines<PoolPrepassPipeline>>,
    shadow: Res<'w, PoolShadowPipeline>,
    shadow_specialized: ResMut<'w, SpecializedRenderPipelines<PoolShadowPipeline>>,
    view_keys: Res<'w, ViewKeyCache>,
    prepass_view_keys: Res<'w, ViewKeyPrepassCache>,
    renderer_method: Res<'w, DefaultOpaqueRendererMethod>,
}

/// The phases `queue_pools` fills, and their draw functions.
#[derive(SystemParam)]
pub(crate) struct PoolPhases<'w> {
    opaque: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    prepass: ResMut<'w, ViewBinnedRenderPhases<Opaque3dPrepass>>,
    deferred: ResMut<'w, ViewBinnedRenderPhases<Opaque3dDeferred>>,
    shadow: ResMut<'w, ViewBinnedRenderPhases<Shadow>>,
    opaque_draws: Res<'w, DrawFunctions<Opaque3d>>,
    prepass_draws: Res<'w, DrawFunctions<Opaque3dPrepass>>,
    deferred_draws: Res<'w, DrawFunctions<Opaque3dDeferred>>,
    shadow_draws: Res<'w, DrawFunctions<Shadow>>,
}

/// Adds the pool item to a prepass or deferred phase.
fn add_prepass_item<P>(
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

/// Puts the pool item into the opaque phase of every camera that culled pooled
/// rocks (or, for a deferred camera, its deferred phase), into its prepass when
/// it has one, and into the shadow phase of every light view that culled them.
pub(crate) fn queue_pools(
    mut pipelines: PoolPipelines,
    mut phases: PoolPhases,
    cameras: Query<(&ExtractedView, Has<DeferredPrepass>), Without<LightEntity>>,
    light_views: Query<(&ExtractedView, &LightEntity)>,
    extracted: Res<ExtractedPools>,
    views: Res<PoolViews>,
    mut warned: Local<bool>,
) {
    let Some(item) = extracted.drawer else {
        return;
    };
    let culled: HashSet<RetainedViewEntity> = views.views.keys().map(|(view, _)| *view).collect();
    let draw_main = phases.opaque_draws.read().id::<DrawPoolMain>();
    let draw_prepass = phases.prepass_draws.read().id::<DrawPoolPrepass>();
    let draw_deferred = phases.deferred_draws.read().id::<DrawPoolPrepass>();
    let draw_shadow = phases.shadow_draws.read().id::<DrawPoolShadow>();
    let deferred_default = deferred_by_default(&pipelines.renderer_method);
    let triangles = MeshPipelineKey::from_primitive_topology_and_strip_index(
        PrimitiveTopology::TriangleList,
        None,
    );

    for (view, deferred_prepass) in &cameras {
        let retained = view.retained_view_entity;
        let draws = culled.contains(&retained);
        let prepass_key = pipelines
            .prepass_view_keys
            .get(&retained)
            .map(|key| *key | triangles);

        // A deferred camera draws pooled rocks into its G-buffer and nowhere else.
        let mut deferred = false;
        if let Some(phase) = phases.deferred.get_mut(&retained) {
            phase.remove(item.1);
            if let (true, true, true, Some(key)) =
                (draws, deferred_prepass, deferred_default, prepass_key)
            {
                let pipeline = pipelines.prepass_specialized.specialize(
                    &pipelines.cache,
                    &pipelines.prepass,
                    key | MeshPipelineKey::DEFERRED_PREPASS,
                );
                add_prepass_item(phase, pipeline, draw_deferred, item);
                deferred = true;
            }
        }
        // A forward camera with a prepass draws pooled rocks there too, so
        // effects that read the prepass see them.
        if let Some(phase) = phases.prepass.get_mut(&retained) {
            phase.remove(item.1);
            if let (true, false, Some(key)) = (draws, deferred, prepass_key) {
                let pipeline = pipelines.prepass_specialized.specialize(
                    &pipelines.cache,
                    &pipelines.prepass,
                    key,
                );
                add_prepass_item(phase, pipeline, draw_prepass, item);
            }
        }
        if let Some(phase) = phases.opaque.get_mut(&retained) {
            phase.remove(item.1);
            if let (true, false, Some(key)) =
                (draws, deferred, pipelines.view_keys.get(&retained).copied())
            {
                let pipeline = pipelines.main_specialized.specialize(
                    &pipelines.cache,
                    &pipelines.main,
                    key | triangles,
                );
                phase.add(
                    Opaque3dBatchSetKey {
                        draw_function: draw_main,
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
        }
    }

    // Every shadow view (directional cascades, spot lights and point-light cube
    // faces) that culled pooled rocks.
    for (light_view, kind) in &light_views {
        let retained = light_view.retained_view_entity;
        let Some(phase) = phases.shadow.get_mut(&retained) else {
            continue;
        };
        phase.remove(item.1);
        let directional = matches!(kind, LightEntity::Directional { .. });
        if !culled.contains(&retained) {
            continue;
        }
        if directional && !pipelines.shadow.clamps_depth {
            if !*warned {
                *warned = true;
                warn!(
                    "procgena_stone_bevy: this GPU cannot clamp shadow depth, so pooled rocks cast no directional shadows"
                );
            }
            continue;
        }
        let pipeline = pipelines.shadow_specialized.specialize(
            &pipelines.cache,
            &pipelines.shadow,
            directional,
        );
        phase.add(
            ShadowBatchSetKey {
                pipeline,
                draw_function: draw_shadow,
                material_bind_group_index: None,
                slabs: MeshSlabs::default(),
            },
            ShadowBinKey {
                asset_id: AssetId::<Mesh>::default().untyped(),
            },
            item,
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
        );
    }
}
