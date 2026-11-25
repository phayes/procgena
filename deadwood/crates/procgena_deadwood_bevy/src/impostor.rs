//! Dead wood impostors: each built piece is baked into an impostor (see
//! [`procgena_impostor`]) from its game-level mesh, and drawn as one quad
//! when it is small on screen, the last step of the shared drawing chain.

use bevy::asset::UntypedAssetId;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step};
use procgena_core_bevy::instanced::{InstancedScatter, InstancedStandardMaterial};
use procgena_core_bevy::{LodQuality, visibility_range};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, BakeStandardMaterial, Impostor, ImpostorKey,
    ImpostorMaterial, ImpostorQuad, ImpostorRequest, Impostors, ViewCoverage, bake_standard,
    frame_size_for,
};

use crate::material::{DeadwoodMaterial, bake_copy};
use crate::spawn::{
    Building, DeadwoodCache, DeadwoodGenerating, DeadwoodImpostorShown, DeadwoodParams,
    DeadwoodPart, DeadwoodPartKind, DrawnVariants, SpawnSettings, VariantAssets, VariantKey,
    chain_ranges, with_impostor,
};

/// How dead wood impostors are baked unless told otherwise: 8 × 8 views
/// covering viewers from straight above down to 45° below level, as for a
/// log seen up a hillside, each large enough for the biggest size on screen
/// the shared [`DistanceChain`] shows an impostor at (256 pixels with the
/// default chain). Set it on
/// [`DeadwoodPlugin::impostor`](crate::DeadwoodPlugin::impostor).
///
/// Each distinct piece is baked once, from its game-level mesh, in the
/// background the first time it is built; until then its lower mesh is
/// drawn until the piece is culled. Impostors cast no shadows. To keep
/// bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`DeadwoodPlugin`](crate::DeadwoodPlugin).
pub fn default_deadwood_impostor() -> BakeSettings {
    let frame = frame_size_for(DistanceChain::DEFAULT.largest_impostor_size());
    BakeSettings::new(8, frame).with_coverage(ViewCoverage::from_lowest_elevation(-45.0))
}

/// A piece's impostor, once baked, with the quad and material that draw
/// it.
#[derive(Clone, Debug)]
pub(crate) struct DeadwoodImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) quad: Handle<Mesh>,
    pub(crate) material: Handle<ImpostorMaterial>,
}

/// Asks the shared impostor cache for the impostor of every uploaded
/// variant that has none yet, and keeps each one once it is baked.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    quad: Res<ImpostorQuad>,
    mut impostors: ResMut<Impostors>,
    mut cache: ResMut<DeadwoodCache>,
    mut materials: ResMut<Assets<DeadwoodMaterial>>,
    standard: Res<Assets<InstancedStandardMaterial>>,
    mut bake_materials: ResMut<Assets<BakeStandardMaterial>>,
) {
    let Some(bake) = settings.impostor else {
        return;
    };
    let waiting: Vec<(VariantKey, std::sync::Arc<VariantAssets>)> = cache
        .variants
        .iter()
        .filter(|(key, _)| !cache.impostors.contains_key(key))
        .map(|(key, variant)| (*key, variant.clone()))
        .collect();
    for (key, variant) in waiting {
        let impostor_key = variant_key(key, bake, &settings);
        let ready = impostors
            .request(&mut commands, impostor_key, || {
                bake_request(
                    impostor_key,
                    &variant,
                    &mut materials,
                    &standard,
                    &mut bake_materials,
                )
            })
            .cloned();
        let (Some(ready), Some(material)) = (ready, impostors.material(impostor_key)) else {
            continue;
        };
        let assets = DeadwoodImpostorAssets {
            impostor: ready,
            quad: quad.0.clone(),
            material: material.clone(),
        };
        cache.impostors.insert(key, assets);
    }
}

/// The impostor key of the variant under `key`: its pool, seed and game
/// level, baked with `bake`, with the textures in use and this crate's
/// version.
fn variant_key(key: VariantKey, bake: BakeSettings, settings: &SpawnSettings) -> ImpostorKey {
    let textures = format!(
        "{:?}",
        settings
            .textures
            .as_ref()
            .map(|t| (&t.root, t.size, t.compression))
    );
    ImpostorKey::pool_part(
        key.pool,
        "deadwood",
        bake,
        &[
            &key.seed.to_le_bytes(),
            &key.game_level.to_le_bytes(),
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The bake request for `variant` under impostor key `key`: its game-level
/// round stems, strips and leaf cards with bake copies of their materials,
/// waiting for those materials' textures.
fn bake_request(
    key: ImpostorKey,
    variant: &VariantAssets,
    materials: &mut Assets<DeadwoodMaterial>,
    standard: &Assets<InstancedStandardMaterial>,
    bake_materials: &mut Assets<BakeStandardMaterial>,
) -> ImpostorRequest {
    let level = &variant.levels[Step::Game.index()];
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    let wood = level
        .surfaces
        .iter()
        .map(|(mesh, material)| (mesh, material))
        .chain(
            level
                .ribbon
                .iter()
                .map(|(mesh, material, _)| (mesh, material)),
        );
    for (mesh, material) in wood {
        let Some(source) = materials.get(material).cloned() else {
            continue;
        };
        let extension = &source.extension;
        wait_for.extend(
            [
                &extension.bark_color,
                &extension.bark_normal,
                &extension.bark_roughness,
                &extension.end_color,
                &extension.end_normal,
            ]
            .into_iter()
            .flatten()
            .map(|t| t.id().untyped()),
        );
        let albedo = materials.add(bake_copy(&source, BakeOutput::Albedo));
        let normal = materials.add(bake_copy(&source, BakeOutput::Normal));
        parts.push(BakePart::new(mesh.clone(), albedo, normal));
    }
    if let Some((mesh, material)) = &level.leaves
        && let Some(source) = standard.get(material)
    {
        let base = &source.base;
        wait_for.extend(
            [&base.base_color_texture, &base.normal_map_texture]
                .into_iter()
                .flatten()
                .map(|t| t.id().untyped()),
        );
        let albedo = bake_materials.add(bake_standard(base, BakeOutput::Albedo));
        let normal = bake_materials.add(bake_standard(base, BakeOutput::Normal));
        parts.push(BakePart::new(mesh.clone(), albedo, normal));
    }
    ImpostorRequest {
        key,
        parts,
        wait_for,
    }
}

/// Gives every built single piece whose impostor is ready an impostor part,
/// and ends its lower mesh where the impostor takes over.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn show_impostors(
    mut commands: Commands,
    cache: Res<DeadwoodCache>,
    quad: Res<ImpostorQuad>,
    quality: Res<LodQuality>,
    pieces: Query<
        (Entity, &DrawnVariants, Option<&Children>),
        (
            With<DeadwoodParams>,
            Without<DeadwoodGenerating>,
            Without<Building>,
            Without<DeadwoodImpostorShown>,
        ),
    >,
    parts: Query<&DeadwoodPart>,
) {
    for (piece, drawn, children) in &pieces {
        let Some(key) = drawn.0.first() else {
            continue;
        };
        let (Some(variant), Some(assets)) = (cache.variants.get(key), cache.impostors.get(key))
        else {
            continue;
        };
        let ranges = chain_ranges(variant.size, quality.0).map(|r| visibility_range(&r));

        // Every step takes its bands with the impostor as the last step:
        // the lower mesh now fades out where the impostor fades in.
        for child in children.into_iter().flatten() {
            if let Ok(part) = parts.get(*child) {
                let range: VisibilityRange = ranges[part.step.index()].clone();
                commands.entity(*child).insert(range);
            }
        }
        commands.spawn((
            DeadwoodPart {
                instance: 0,
                step: Step::Impostor,
                kind: DeadwoodPartKind::Impostor,
            },
            assets.impostor.bundle(&quad, assets.material.clone()),
            NotShadowCaster,
            ranges[Step::Impostor.index()].clone(),
            ChildOf(piece),
        ));
        commands.entity(piece).insert(DeadwoodImpostorShown);
    }
}

/// Gives each scatter's slots the impostors of their variants as they are
/// baked.
pub(crate) fn show_scatter_impostors(
    cache: Res<DeadwoodCache>,
    mut scatters: Query<(&DrawnVariants, &mut InstancedScatter)>,
) {
    if cache.impostors.is_empty() {
        return;
    }
    for (drawn, mut scatter) in &mut scatters {
        let missing =
            |(slot, key): (&procgena_core_bevy::instanced::InstancedSlot, &VariantKey)| {
                !slot.impostor && cache.impostors.contains_key(key)
            };
        if !scatter.slots.iter().zip(&drawn.0).any(missing) {
            continue;
        }
        let slots: Vec<_> = scatter
            .slots
            .iter()
            .zip(&drawn.0)
            .map(|(slot, key)| match cache.impostors.get(key) {
                Some(impostor) => with_impostor(slot.clone(), impostor),
                None => slot.clone(),
            })
            .collect();
        scatter.slots = slots.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_bake_fits_the_largest_impostor_on_screen() {
        let bake = default_deadwood_impostor();
        let largest = DistanceChain::DEFAULT.largest_impostor_size();
        assert!(
            bake.frame_size as f32 >= largest * procgena_impostor::REFERENCE_SCREEN_HEIGHT as f32
        );
    }
}
