//! Impostors of the fruit and flower clusters trees grow: each cluster
//! specimen is baked once, and far enough away its clusters are drawn as
//! single quads, still in place on their branches and swaying with them.

use bevy::asset::UntypedAssetId;
use bevy::material::OpaqueRendererMethod;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use procgena_core::SpecimenId;
use procgena_core_bevy::instanced::attached_vertex;
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, IMPOSTOR_PREPASS_SHADER, IMPOSTOR_SHADER, Impostor,
    ImpostorKey, ImpostorLevel, ImpostorRequest, ImpostorUniform,
};
use procgena_tree::ClusterPart;
use procgena_tree::procgena_fruit::default_lod as fruit_lod;

use crate::clusters::TreeClusters;
use crate::material::{TreeMaterial, WindUniform, bake_copy};

/// The impostor tree fruit and flower clusters use unless told otherwise:
/// from 20 m for a cluster 10 cm across (scaled with each cluster's size,
/// like its other levels), crossfading over 10% on each side, baked from
/// fruit level 1 as 6 × 6 views of 32 pixels seen from every direction. It
/// casts no shadows, like the fruit level before it. Set it on
/// [`TreePlugin::fruit_impostor`](crate::TreePlugin::fruit_impostor).
pub fn default_fruit_impostor() -> ImpostorLevel {
    ImpostorLevel {
        bake: BakeSettings::new(6, 32),
        ..ImpostorLevel::new(20.0, 0.1, 1)
    }
}

/// Vertex shader of attached cluster impostors in the forward pass.
const FRUIT_IMPOSTOR_SHADER: &str = "embedded://procgena_tree_bevy/shaders/fruit_impostor.wesl";
/// Vertex shader of attached cluster impostors in depth, normal and shadow
/// passes.
const FRUIT_IMPOSTOR_PREPASS_SHADER: &str =
    "embedded://procgena_tree_bevy/shaders/fruit_impostor_prepass.wesl";

/// The material a cluster's impostor is drawn with on trees: the baked
/// atlases, and the trees' wind.
pub(crate) type FruitImpostorMaterial = ExtendedMaterial<StandardMaterial, FruitImpostorExtension>;

/// The atlases, bounds and wind of a [`FruitImpostorMaterial`].
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(FruitImpostorKey)]
pub(crate) struct FruitImpostorExtension {
    /// The baked cluster's bounding sphere and atlas layout.
    #[uniform(100)]
    pub(crate) params: ImpostorUniform,
    /// Base colour and coverage of every view.
    #[texture(101)]
    #[sampler(102)]
    pub(crate) albedo: Handle<Image>,
    /// Normals of every view.
    #[texture(103)]
    pub(crate) normal: Handle<Image>,
    /// The trees' wind, kept in step with [`Wind`](crate::Wind).
    #[uniform(104)]
    pub(crate) wind: WindUniform,
}

/// The pipeline variant a [`FruitImpostorMaterial`] is drawn with: only the
/// attached variant draws anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FruitImpostorKey {
    /// Whether the pipeline draws clusters attached to trees.
    pub(crate) attached: bool,
}

impl From<&FruitImpostorExtension> for FruitImpostorKey {
    fn from(_: &FruitImpostorExtension) -> Self {
        Self::default()
    }
}

impl MaterialExtension for FruitImpostorExtension {
    fn vertex_shader() -> ShaderRef {
        FRUIT_IMPOSTOR_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        IMPOSTOR_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        FRUIT_IMPOSTOR_PREPASS_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        IMPOSTOR_PREPASS_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.attached {
            attached_vertex(descriptor);
        }
        Ok(())
    }
}

/// A cluster specimen's baked impostor, as the material that draws it on
/// trees.
#[derive(Clone, Debug)]
pub(crate) struct FruitImpostor {
    pub(crate) material: Handle<FruitImpostorMaterial>,
}

/// A material drawing `impostor` attached to trees, swaying with `wind`.
pub(crate) fn fruit_impostor_material(
    impostor: &Impostor,
    wind: WindUniform,
) -> FruitImpostorMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            opaque_render_method: OpaqueRendererMethod::Forward,
            ..default()
        },
        extension: FruitImpostorExtension {
            params: ImpostorUniform {
                center: impostor.center,
                radius: impostor.radius,
                frames: impostor.frames,
                lowest_elevation: impostor.coverage.lowest_elevation_deg(),
            },
            albedo: impostor.albedo.clone(),
            normal: impostor.normal.clone(),
            wind,
        },
    }
}

/// The impostor key of cluster specimen `specimen`, one of a tree's
/// `part` clusters, baked with `impostor`'s settings from its baked fruit
/// level's detail, and this crate's version. The cluster's look on trees
/// comes from the part's material, so the part is in the key.
pub(crate) fn fruit_key(
    specimen: SpecimenId,
    part: ClusterPart,
    impostor: ImpostorLevel,
) -> ImpostorKey {
    let part = match part {
        ClusterPart::Fruit => "fruit",
        ClusterPart::Flowers => "flowers",
    };
    let level = format!("{:?}", fruit_lod().levels.get(impostor.baked_level));
    ImpostorKey::specimen(
        specimen,
        impostor.bake,
        &[
            part.as_bytes(),
            level.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for slot `slot` of `clusters` under key `key`: the
/// specimen's mesh at `impostor`'s baked fruit level, with bake copies of
/// the clusters' material. `None` when that level has no mesh or the
/// material is missing.
pub(crate) fn fruit_request(
    key: ImpostorKey,
    clusters: &TreeClusters,
    slot: usize,
    impostor: ImpostorLevel,
    materials: &mut Assets<TreeMaterial>,
) -> Option<ImpostorRequest> {
    let mesh = clusters
        .specimens
        .get(slot)?
        .levels
        .get(impostor.baked_level)?
        .clone()?;
    let source = materials.get(&clusters.material)?.clone();
    let wait_for: Vec<UntypedAssetId> = source
        .base
        .base_color_texture
        .iter()
        .map(|t| t.id().untyped())
        .collect();
    let albedo = materials.add(bake_copy(&source, BakeOutput::Albedo));
    let normal = materials.add(bake_copy(&source, BakeOutput::Normal));
    Some(ImpostorRequest {
        key,
        parts: vec![BakePart::new(mesh, albedo, normal)],
        wait_for,
    })
}

/// Keeps every [`FruitImpostorMaterial`]'s wind in step with the trees'.
pub(crate) fn sync_fruit_wind(
    wind: Res<crate::Wind>,
    mut materials: ResMut<Assets<FruitImpostorMaterial>>,
) {
    let uniform = WindUniform::from(wind.0);
    let stale: Vec<_> = materials
        .iter()
        .filter(|(_, material)| material.extension.wind != uniform)
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut material) = materials.get_mut(id) {
            material.extension.wind = uniform;
        }
    }
}
