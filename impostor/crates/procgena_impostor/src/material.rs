//! The material impostors are drawn with, and the bake mode generators'
//! materials switch to while an impostor is baked.

use bevy::asset::uuid_handle;
use bevy::material::OpaqueRendererMethod;
use bevy::mesh::{MeshVertexBufferLayoutRef, VertexBufferLayout};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    VertexAttribute, VertexFormat, VertexStepMode,
};
use bevy::shader::{Shader, ShaderRef};

/// Vertex and fragment shader of impostors in the forward pass.
///
/// A material that draws impostors with a vertex shader of its own can use
/// this as its fragment shader; see the crate's "Drawing impostors with
/// your own transform" section for what the material must bind.
pub const IMPOSTOR_SHADER: &str = "embedded://procgena_impostor/shaders/impostor.wesl";
/// Vertex and fragment shader of impostors in depth, normal and shadow
/// passes. A material with its own vertex shader can use this as its
/// prepass fragment shader, like [`IMPOSTOR_SHADER`].
pub const IMPOSTOR_PREPASS_SHADER: &str =
    "embedded://procgena_impostor/shaders/impostor_prepass.wesl";

/// The fragment shader of bake copies: base colour, or the normal with
/// `BAKE_NORMAL`, instead of lit colour.
///
/// A generator whose material builds on [`StandardMaterial`] with a
/// [`MaterialExtension`] makes its bake copies by setting this as the
/// forward fragment shader in its `specialize` (and pushing `BAKE_NORMAL` to
/// its fragment shader definitions for the normal atlas), keeping its own
/// vertex shader. Materials without a vertex shader of their own can use
/// [`BakeStandardMaterial`] instead.
pub const BAKE_SHADER: Handle<Shader> = uuid_handle!("3b0e9f1c-5d6a-4c2e-8f7b-1a9d4e6c2b57");

/// What a bake copy of a material draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum BakeOutput {
    /// Lit colour, as usual: not a bake copy.
    #[default]
    None,
    /// The material's base colour, unlit, for the albedo atlas.
    Albedo,
    /// The surface normal and depth for the normal atlas: the normal in the
    /// object's own coordinates, encoded as `n * 0.5 + 0.5`, and depth in
    /// alpha (see [`Impostor::normal`](crate::Impostor::normal)).
    Normal,
}

/// A [`StandardMaterial`] drawn for an impostor bake. Make one with
/// [`bake_standard`].
pub type BakeStandardMaterial = ExtendedMaterial<StandardMaterial, BakeExtension>;

/// Switches a [`BakeStandardMaterial`] to drawing base colour or normals.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(BakeKey)]
pub struct BakeExtension {
    /// What the material draws.
    #[reflect(ignore)]
    pub output: BakeOutput,
}

/// The pipeline variant of a [`BakeStandardMaterial`]: what it draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BakeKey(BakeOutput);

impl From<&BakeExtension> for BakeKey {
    fn from(extension: &BakeExtension) -> Self {
        Self(extension.output)
    }
}

impl MaterialExtension for BakeExtension {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        use_bake_shader(descriptor, key.bind_group_data.0);
        Ok(())
    }
}

/// Switches a forward pipeline's fragment shader to [`BAKE_SHADER`] for
/// `output`, leaving prepass pipelines and `BakeOutput::None` alone. Call it
/// from a [`MaterialExtension::specialize`] to give a material a bake mode.
pub fn use_bake_shader(descriptor: &mut RenderPipelineDescriptor, output: BakeOutput) {
    let prepass = descriptor
        .vertex
        .shader_defs
        .contains(&"PREPASS_PIPELINE".into());
    if output == BakeOutput::None || prepass {
        return;
    }
    if let Some(fragment) = &mut descriptor.fragment {
        fragment.shader = BAKE_SHADER;
        if output == BakeOutput::Normal {
            fragment.shader_defs.push("BAKE_NORMAL".into());
        }
    }
}

/// A bake copy of `material` that draws `output`.
pub fn bake_standard(material: &StandardMaterial, output: BakeOutput) -> BakeStandardMaterial {
    ExtendedMaterial {
        base: material.clone(),
        extension: BakeExtension { output },
    }
}

/// The material an impostor quad is drawn with. Make one with
/// [`Impostor::material`](crate::Impostor::material).
///
/// The base material's colour tints the atlas and its roughness, metallic
/// and other lighting settings apply to the whole impostor; its textures are
/// not used.
pub type ImpostorMaterial = ExtendedMaterial<StandardMaterial, ImpostorExtension>;

/// The atlases and bounds an [`ImpostorMaterial`] draws.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(ImpostorPipelineKey)]
pub struct ImpostorExtension {
    /// The baked object's bounding sphere and the atlas size.
    #[uniform(100)]
    pub params: ImpostorUniform,
    /// Base colour and coverage of every view.
    #[texture(101)]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    /// Normals and depth of every view; see
    /// [`Impostor::normal`](crate::Impostor::normal).
    #[texture(103)]
    pub normal: Handle<Image>,
}

/// An impostor's bounds and atlas size, in the layout the shaders read.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct ImpostorUniform {
    /// Centre of the baked object's bounding sphere, in its own coordinates.
    pub center: Vec3,
    /// Radius of the bounding sphere.
    pub radius: f32,
    /// Views along each side of the atlas.
    pub frames: u32,
    /// The lowest viewer elevation the views cover, in degrees; -90 covers
    /// every direction. See [`ViewCoverage`](crate::ViewCoverage).
    pub lowest_elevation: f32,
}

/// The pipeline variant an [`ImpostorMaterial`] is drawn with.
///
/// Materials always make the ordinary variant, which draws one impostor per
/// entity. A renderer that draws many impostors in one instanced draw asks
/// for its pipelines with `instanced` set; see [`instance_buffer_layout`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ImpostorPipelineKey {
    /// Whether each instance names its own mesh record and dither level
    /// through a second vertex buffer.
    pub instanced: bool,
}

impl From<&ImpostorExtension> for ImpostorPipelineKey {
    fn from(_: &ImpostorExtension) -> Self {
        Self::default()
    }
}

/// Shader location of the per-instance input of instanced pipelines.
pub const INSTANCE_LOCATION: u32 = 12;
/// Bytes per instance in the per-instance vertex buffer: two `u32`s.
pub const INSTANCE_SIZE: u64 = 8;

/// The second vertex buffer of an instanced impostor pipeline (see
/// [`ImpostorPipelineKey::instanced`]): per instance, two `u32`s at
/// [`INSTANCE_LOCATION`].
///
/// - The first is the instance's index in the mesh records (Bevy's
///   `MeshUniform`s) bound as the mesh bind group, which hold its transform.
/// - The second is its visibility-range dither level, an `i32` from -16
///   (faded out, fading in) through 0 (fully drawn) to 16 (faded out,
///   fading out), stored as its bits. It is read only when the pipeline's
///   mesh key has `VISIBILITY_RANGE_DITHER`.
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

impl MaterialExtension for ImpostorExtension {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.instanced {
            descriptor.vertex.shader_defs.push("INSTANCED".into());
            descriptor.vertex.buffers.push(instance_buffer_layout());
        }
        Ok(())
    }

    fn vertex_shader() -> ShaderRef {
        IMPOSTOR_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        IMPOSTOR_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        IMPOSTOR_PREPASS_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        IMPOSTOR_PREPASS_SHADER.into()
    }
}

/// The base material of a new impostor: white, alpha-tested, drawn forward
/// from both sides.
pub(crate) fn impostor_base() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Mask(0.5),
        perceptual_roughness: 0.9,
        double_sided: true,
        cull_mode: None,
        opaque_render_method: OpaqueRendererMethod::Forward,
        ..default()
    }
}
