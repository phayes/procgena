//! The render world of pooled rocks: GPU generation of each pool's shapes,
//! per-view culling, and the draws in Bevy's main, prepass, deferred and
//! shadow passes.

mod bevy_glue;
#[cfg(test)]
mod gpu_test;
mod pools;
mod views;

use bevy::asset::Handle;
use bevy::core_pipeline::core_3d::Opaque3d;
use bevy::core_pipeline::deferred::Opaque3dDeferred;
use bevy::core_pipeline::prepass::Opaque3dPrepass;
use bevy::core_pipeline::schedule::camera_driver;
use bevy::ecs::prelude::*;
use bevy::pbr::Shadow;
use bevy::prelude::{App, Plugin};
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only_sized, storage_buffer_sized, texture_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntries, SamplerBindingType, ShaderStages,
    SpecializedRenderPipelines, TextureSampleType,
};
use bevy::render::renderer::RenderGraph;
use bevy::render::sync_world::{MainEntity, RenderEntity};
use bevy::render::view::ViewUniform;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

use super::{PoolDefinitions, PoolDrawer, PoolInstances};
use pools::PoolMaterialUniform;
use views::CullUniform;

/// The pooled-rock shaders, loaded in the main app.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PoolShaders {
    /// Main-pass vertex and fragment shader.
    pub main: Handle<Shader>,
    /// Prepass and deferred G-buffer shader.
    pub prepass: Handle<Shader>,
    /// Shadow-map vertex shader.
    pub shadow: Handle<Shader>,
    /// Culling compute shader.
    pub cull: Handle<Shader>,
}

/// Sets up the render world for pooled rocks. Added by [`super::StonePoolPlugin`].
pub(crate) struct PoolRenderPlugin {
    pub shaders: PoolShaders,
}

impl Plugin for PoolRenderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(self.shaders.clone())
            .insert_resource(PoolLayouts::new())
            .init_resource::<ExtractedPools>()
            .init_resource::<pools::RenderPools>()
            .init_resource::<views::PoolViews>()
            .init_resource::<bevy_glue::PoolShadowBindGroups>()
            .init_resource::<SpecializedRenderPipelines<bevy_glue::PoolMainPipeline>>()
            .init_resource::<SpecializedRenderPipelines<bevy_glue::PoolPrepassPipeline>>()
            .init_resource::<SpecializedRenderPipelines<bevy_glue::PoolShadowPipeline>>()
            .add_render_command::<Opaque3d, bevy_glue::DrawPoolMain>()
            .add_render_command::<Opaque3dPrepass, bevy_glue::DrawPoolPrepass>()
            .add_render_command::<Opaque3dDeferred, bevy_glue::DrawPoolPrepass>()
            .add_render_command::<Shadow, bevy_glue::DrawPoolShadow>()
            .add_systems(
                RenderStartup,
                (
                    bevy_glue::init_pipelines
                        .after(bevy::pbr::MeshPipelineSystems)
                        .after(bevy::pbr::init_prepass_pipeline),
                    views::init_cull_pipelines,
                ),
            )
            .add_systems(ExtractSchedule, extract_pools)
            .add_systems(
                Render,
                (
                    (pools::prepare_pools, views::prepare_views)
                        .chain()
                        .in_set(RenderSystems::PrepareResources),
                    (
                        pools::prepare_pool_bind_groups,
                        views::prepare_view_bind_groups,
                        bevy_glue::prepare_shadow_bind_groups,
                    )
                        .in_set(RenderSystems::PrepareBindGroups),
                    bevy_glue::queue_pools.in_set(RenderSystems::Queue),
                ),
            )
            .add_systems(RenderGraph, views::run_cull.before(camera_driver));
    }
}

/// Bind group layouts shared by the pipelines and bind groups.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PoolLayouts {
    /// Group 2 of every pooled-rock draw: the pool's rocks, material and textures.
    pub draw: BindGroupLayoutDescriptor,
    /// Group 0 of the culling passes: one view's culling of one pool.
    pub cull: BindGroupLayoutDescriptor,
    /// Group 0 of the shadow pass: the light view's uniform.
    pub shadow_view: BindGroupLayoutDescriptor,
    /// Group 1 of the shadow pass, empty so the pool group stays at 2.
    pub empty: BindGroupLayoutDescriptor,
}

impl PoolLayouts {
    fn new() -> Self {
        // Binding numbers match `pool_draw.wesl` and `pool_cull.wesl`.
        let draw = BindGroupLayoutDescriptor::new(
            "procgena_stone_pool_draw",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    storage_buffer_read_only_sized(false, None).visibility(ShaderStages::VERTEX),
                    uniform_buffer::<PoolMaterialUniform>(false)
                        .visibility(ShaderStages::VERTEX_FRAGMENT),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let cull = BindGroupLayoutDescriptor::new(
            "procgena_stone_pool_cull",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<CullUniform>(false),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_sized(false, None),
                    storage_buffer_sized(false, None),
                ),
            ),
        );
        let shadow_view = BindGroupLayoutDescriptor::new(
            "procgena_stone_pool_shadow_view",
            &BindGroupLayoutEntries::single(
                ShaderStages::VERTEX,
                uniform_buffer::<ViewUniform>(true),
            ),
        );
        let empty = BindGroupLayoutDescriptor::new("procgena_stone_pool_empty", &[]);
        Self {
            draw,
            cull,
            shadow_view,
            empty,
        }
    }
}

/// The main world's pools and pooled rocks, copied when they change, and the
/// entity that stands for pooled rocks in Bevy's draw lists.
#[derive(Resource, Debug, Default)]
pub(crate) struct ExtractedPools {
    pub definitions: PoolDefinitions,
    pub instances: PoolInstances,
    /// The render and main entity of the [`PoolDrawer`], once it exists.
    pub drawer: Option<(Entity, MainEntity)>,
}

fn extract_pools(
    mut extracted: ResMut<ExtractedPools>,
    definitions: Extract<Res<PoolDefinitions>>,
    instances: Extract<Res<PoolInstances>>,
    drawer: Extract<Query<(Entity, &RenderEntity), With<PoolDrawer>>>,
) {
    if definitions.version != extracted.definitions.version {
        extracted.definitions = definitions.clone();
    }
    if instances.version != extracted.instances.version {
        extracted.instances = instances.clone();
    }
    extracted.drawer = drawer
        .single()
        .ok()
        .map(|(main, render)| (render.id(), MainEntity::from(main)));
}
