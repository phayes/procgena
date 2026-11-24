//! The render world of pooled rocks: GPU generation of each pool's shapes,
//! which the shared instanced renderer draws as external meshes.

#[cfg(test)]
mod gpu_test;
mod pools;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::prelude::*;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::prelude::{App, Plugin};
use bevy::render::mesh::MeshVertexBufferLayouts;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use procgena_core_bevy::instanced::InstancedSystems;

use super::PoolDefinitions;

/// Sets up the render world for pooled rocks. Added by [`super::StonePoolPlugin`].
pub(crate) struct PoolRenderPlugin;

impl Plugin for PoolRenderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedPools>()
            .init_resource::<pools::RenderPools>()
            .add_systems(RenderStartup, init_mesh_layout)
            .add_systems(ExtractSchedule, extract_pools)
            .add_systems(
                Render,
                pools::prepare_pools
                    .in_set(RenderSystems::Queue)
                    .before(InstancedSystems::Prepare),
            );
    }
}

/// The vertex layout of every generated mesh: positions then normals, as a
/// Bevy mesh with those two attributes has.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PoolMeshLayout(pub MeshVertexBufferLayoutRef);

fn init_mesh_layout(mut commands: Commands, mut layouts: ResMut<MeshVertexBufferLayouts>) {
    let template = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]]);
    commands.insert_resource(PoolMeshLayout(
        template.get_mesh_vertex_buffer_layout(&mut layouts),
    ));
}

/// The main world's pools, copied when they change.
#[derive(Resource, Debug, Default)]
pub(crate) struct ExtractedPools {
    pub definitions: PoolDefinitions,
}

fn extract_pools(
    mut extracted: ResMut<ExtractedPools>,
    definitions: Extract<Res<PoolDefinitions>>,
) {
    if definitions.version != extracted.definitions.version {
        extracted.definitions = definitions.clone();
    }
}
