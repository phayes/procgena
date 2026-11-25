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
use procgena_core::chain::{DistanceChain, Step};
use procgena_core_bevy::instanced::attached_vertex;
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, IMPOSTOR_PREPASS_SHADER, IMPOSTOR_SHADER, Impostor,
    ImpostorKey, ImpostorRequest, ImpostorUniform, frame_size_for,
};
use procgena_tree::ClusterPart;

use crate::clusters::TreeClusters;
use crate::material::{TreeMaterial, WindUniform, bake_copy};

/// How tree fruit and flower clusters' impostors are baked unless told
/// otherwise: from each cluster's game-level mesh, as 6 × 6 views seen from
/// every direction, each large enough for the biggest size on screen the
/// shared [`DistanceChain`] shows an impostor at (256 pixels with the
/// default chain). A cluster is drawn as its impostor when it is small on
/// screen, until its tree's impostor takes over. It casts no shadows. Set it
/// on [`TreePlugin::fruit_impostor`](crate::TreePlugin::fruit_impostor).
pub fn default_fruit_impostor() -> BakeSettings {
    let frame = frame_size_for(DistanceChain::DEFAULT.largest_impostor_size());
    BakeSettings::new(6, frame)
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
/// `part` clusters, baked with `bake` from its game level, whose hash is
/// `detail`, and this crate's version. The cluster's look on trees comes
/// from the part's material, so the part is in the key.
pub(crate) fn fruit_key(
    specimen: SpecimenId,
    part: ClusterPart,
    bake: BakeSettings,
    detail: u64,
) -> ImpostorKey {
    let part = match part {
        ClusterPart::Fruit => "fruit",
        ClusterPart::Flowers => "flowers",
    };
    ImpostorKey::specimen(
        specimen,
        bake,
        &[
            part.as_bytes(),
            &detail.to_le_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for slot `slot` of `clusters` under key `key`: the
/// specimen's game-level mesh, with bake copies of the clusters' material.
/// `None` when that mesh is empty or the material is missing.
pub(crate) fn fruit_request(
    key: ImpostorKey,
    clusters: &TreeClusters,
    slot: usize,
    materials: &mut Assets<TreeMaterial>,
) -> Option<ImpostorRequest> {
    let mesh = clusters.specimens.get(slot)?.levels[Step::Game.index()].clone()?;
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
