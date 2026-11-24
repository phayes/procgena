//! What a material needs to be drawn by the instanced renderer.

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

use bevy::app::{App, Plugin};
use bevy::asset::{Asset, AssetApp};
use bevy::ecs::resource::Resource;
use bevy::material::key::ErasedMaterialKey;
use bevy::mesh::{MeshVertexBufferLayoutRef, VertexBufferLayout};
use bevy::pbr::{
    ExtendedMaterial, Material, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MaterialPlugin, StandardMaterial, StandardMaterialKey,
};
use bevy::reflect::Reflect;
use bevy::render::RenderApp;
use bevy::render::combined_bind_group::CombinedBindGroupData;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexAttribute,
    VertexFormat, VertexStepMode,
};
use bevy::shader::ShaderRef;

use super::InstancedPlugin;

/// Shader location of the per-copy input of instanced pipelines.
pub const INSTANCE_LOCATION: u32 = 12;

/// Bytes per copy in the per-copy vertex buffer: two `u32`s.
pub const INSTANCE_SIZE: u64 = 8;

/// The second vertex buffer of an instanced pipeline: per copy, two `u32`s
/// at shader location [`INSTANCE_LOCATION`]. The first is the copy's index in
/// Bevy's mesh records (use it wherever a shader would use
/// `instance_index`), the second the bits of its dither level as an `i32`.
pub fn instance_buffer_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: INSTANCE_SIZE,
        step_mode: VertexStepMode::Instance,
        attributes: vec![VertexAttribute {
            format: VertexFormat::Uint32x2,
            offset: 0,
            shader_location: INSTANCE_LOCATION,
        }],
    }
}

/// Turns a material's pipeline into its instanced variant: adds the
/// `INSTANCED` shader def to the vertex stage and the per-copy vertex buffer
/// ([`instance_buffer_layout`]).
///
/// Call it from the material's `specialize` when its pipeline key asks for
/// the instanced variant (the key change you give
/// [`InstancedMaterialPlugin::new`]). With `INSTANCED` defined, the vertex
/// shader must read `@location(12) instance: vec2<u32>` and use
/// `copy_record(instance)` in place of `instance_index` and
/// `copy_dither(instance)` as the visibility-range dither, both from
/// `procgena_core_bevy::shaders::instanced`. Materials without a vertex
/// shader of their own can use [`INSTANCED_MESH_SHADER`] and
/// [`INSTANCED_PREPASS_SHADER`], which do this.
pub fn instanced_vertex(descriptor: &mut RenderPipelineDescriptor) {
    descriptor.vertex.shader_defs.push("INSTANCED".into());
    descriptor.vertex.buffers.push(instance_buffer_layout());
}

/// Bytes per attached copy in the per-copy vertex buffer of an attached
/// pipeline: four `u32`s and three `vec4<f32>`s.
pub const ATTACHED_SIZE: u64 = 64;

/// The second vertex buffer of an attached pipeline (see
/// [`InstancedAttached`](super::InstancedAttached)): per attached copy,
/// - at [`INSTANCE_LOCATION`], two `u32`s as for [`instance_buffer_layout`],
///   the first naming the *host* copy's mesh record;
/// - at the next location, the copy's position relative to its host in
///   `xyz` and its scale in `w`;
/// - at the one after, its rotation relative to its host, as a quaternion;
/// - at the one after that, its [`wind`](super::AttachedRow::wind) in `xyz`.
pub fn attached_buffer_layout() -> VertexBufferLayout {
    let vec4 = |offset: u64, location: u32| VertexAttribute {
        format: VertexFormat::Float32x4,
        offset,
        shader_location: location,
    };
    VertexBufferLayout {
        array_stride: ATTACHED_SIZE,
        step_mode: VertexStepMode::Instance,
        attributes: vec![
            VertexAttribute {
                format: VertexFormat::Uint32x2,
                offset: 0,
                shader_location: INSTANCE_LOCATION,
            },
            vec4(16, INSTANCE_LOCATION + 1),
            vec4(32, INSTANCE_LOCATION + 2),
            vec4(48, INSTANCE_LOCATION + 3),
        ],
    }
}

/// Turns a material's pipeline into its attached variant: adds the
/// `INSTANCED` and `ATTACHED` shader defs to the vertex stage and the
/// attached per-copy vertex buffer ([`attached_buffer_layout`]).
///
/// Call it from the material's `specialize` when its pipeline key asks for
/// the attached variant (the key change you give
/// [`InstancedMaterialPlugin::attached`]). With `ATTACHED` defined, the
/// vertex shader reads, besides `instance` at location 12,
/// `@location(13) attached_place: vec4<f32>`,
/// `@location(14) attached_turn: vec4<f32>` and
/// `@location(15) attached_wind: vec4<f32>`. It moves each vertex into the
/// host's coordinates with `attached_position` and `attached_direction`
/// from `procgena_core_bevy::shaders::instanced`, then applies the host
/// record's transform as for any copy. [`INSTANCED_MESH_SHADER`] and
/// [`INSTANCED_PREPASS_SHADER`] do this.
pub fn attached_vertex(descriptor: &mut RenderPipelineDescriptor) {
    descriptor.vertex.shader_defs.push("INSTANCED".into());
    descriptor.vertex.shader_defs.push("ATTACHED".into());
    descriptor.vertex.buffers.push(attached_buffer_layout());
}

/// Bevy's standard mesh vertex shader for the main pass, without skinning
/// or morph targets, that also draws instanced copies. Return it from a
/// material's `vertex_shader` when the material has no vertex shader of its
/// own.
pub const INSTANCED_MESH_SHADER: &str = "embedded://procgena_core_bevy/shaders/instanced_mesh.wesl";

/// Bevy's standard prepass vertex shader, without skinning or morph
/// targets, that also draws instanced copies. Return it from a material's
/// `prepass_vertex_shader` and `deferred_vertex_shader` alongside
/// [`INSTANCED_MESH_SHADER`].
pub const INSTANCED_PREPASS_SHADER: &str =
    "embedded://procgena_core_bevy/shaders/instanced_prepass.wesl";

/// Bevy's [`StandardMaterial`] that the instanced renderer can draw.
///
/// It draws exactly as its `base` does, through [`INSTANCED_MESH_SHADER`]
/// and [`INSTANCED_PREPASS_SHADER`], so it suits meshes without skinning or
/// morph targets. [`InstancedPlugin`] registers it; parts can use it
/// directly, and so can ordinary entities with a `MeshMaterial3d`.
pub type InstancedStandardMaterial = ExtendedMaterial<StandardMaterial, InstancedStandard>;

/// The (empty) extension of an [`InstancedStandardMaterial`].
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(InstancedStandardKey)]
pub struct InstancedStandard {}

/// The pipeline variant an [`InstancedStandardMaterial`] is drawn with. Bevy
/// builds one pipeline per distinct key; you never need to create one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InstancedStandardKey {
    /// Whether the pipeline draws copies of the instanced renderer.
    instanced: bool,
    /// Whether the pipeline draws attached copies.
    attached: bool,
}

impl From<&InstancedStandard> for InstancedStandardKey {
    fn from(_: &InstancedStandard) -> Self {
        Self {
            instanced: false,
            attached: false,
        }
    }
}

impl MaterialExtension for InstancedStandard {
    fn vertex_shader() -> ShaderRef {
        INSTANCED_MESH_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        INSTANCED_PREPASS_SHADER.into()
    }

    fn deferred_vertex_shader() -> ShaderRef {
        INSTANCED_PREPASS_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.attached {
            attached_vertex(descriptor);
        } else if key.bind_group_data.instanced {
            instanced_vertex(descriptor);
        }
        Ok(())
    }
}

/// Adds [`InstancedStandardMaterial`] and registers it with the renderer,
/// for copies and attached copies.
pub(crate) fn add_standard_material(app: &mut App) {
    if app.get_sub_app(RenderApp).is_some() {
        app.add_plugins(MaterialPlugin::<InstancedStandardMaterial>::default());
    } else {
        app.init_asset::<InstancedStandardMaterial>();
    }
    type Key = CombinedBindGroupData<StandardMaterialKey, InstancedStandardKey>;
    register::<InstancedStandardMaterial, Key>(app, |key| key.extension.instanced = true, false);
    register::<InstancedStandardMaterial, Key>(app, |key| key.extension.attached = true, true);
}

/// Turns a material's pipeline key into its instanced variant, by material
/// type.
pub(crate) type InstancedKeyFn = Arc<dyn Fn(&ErasedMaterialKey) -> ErasedMaterialKey + Send + Sync>;

/// Every registered material's key changes, in the render world: to its
/// instanced variant, and to its attached variant.
#[derive(Resource, Default, Clone)]
pub(crate) struct InstancedMaterials {
    pub copies: HashMap<TypeId, InstancedKeyFn>,
    pub attached: HashMap<TypeId, InstancedKeyFn>,
}

/// Lets the instanced renderer draw parts with material `M`.
///
/// `K` is `M`'s pipeline key type (the type of `M`'s bind group data, for an
/// `ExtendedMaterial<StandardMaterial, E>` that is
/// `CombinedBindGroupData<StandardMaterialKey, E::Data>`). The function given
/// to [`new`](Self::new) changes a key into the key of the instanced
/// variant, whose `specialize` calls [`instanced_vertex`]. Add the plugin
/// after `MaterialPlugin::<M>`. Adds [`InstancedPlugin`].
pub struct InstancedMaterialPlugin<M, K> {
    set_instanced: fn(&mut K),
    /// Whether `set_instanced` makes the attached variant.
    attached: bool,
    material: PhantomData<fn() -> M>,
}

impl<M, K> InstancedMaterialPlugin<M, K> {
    /// Registers `M` for copies; its instanced key is made by
    /// `set_instanced`.
    pub fn new(set_instanced: fn(&mut K)) -> Self {
        Self {
            set_instanced,
            attached: false,
            material: PhantomData,
        }
    }

    /// Registers `M` for attached copies (see
    /// [`InstancedAttached`](super::InstancedAttached)); its attached key is
    /// made by `set_attached`, and its `specialize` calls
    /// [`attached_vertex`] for that key. Add it beside the plugin from
    /// [`new`](Self::new) when `M` draws both.
    pub fn attached(set_attached: fn(&mut K)) -> Self {
        Self {
            set_instanced: set_attached,
            attached: true,
            material: PhantomData,
        }
    }
}

impl<M, K> Plugin for InstancedMaterialPlugin<M, K>
where
    M: Material,
    K: Clone + Hash + PartialEq + Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        app.add_plugins(InstancedPlugin);
        register::<M, K>(app, self.set_instanced, self.attached);
    }

    fn is_unique(&self) -> bool {
        false
    }
}

/// Records in the render world how `M`'s key `K` becomes its instanced key,
/// or with `attached`, its attached key.
fn register<M, K>(app: &mut App, set: fn(&mut K), attached: bool)
where
    M: Material,
    K: Clone + Hash + PartialEq + Send + Sync + 'static,
{
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    let change: InstancedKeyFn = Arc::new(move |key: &ErasedMaterialKey| {
        let mut key: K = key.to_key();
        set(&mut key);
        ErasedMaterialKey::new(key)
    });
    let mut materials = render_app
        .world_mut()
        .get_resource_or_init::<InstancedMaterials>();
    let table = if attached {
        &mut materials.attached
    } else {
        &mut materials.copies
    };
    table.insert(TypeId::of::<M>(), change);
}
