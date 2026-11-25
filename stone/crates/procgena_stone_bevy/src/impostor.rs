//! Rock impostors: each generated rock is baked into an impostor (see
//! [`procgena_impostor`]) from its game-level mesh, and drawn as one quad
//! when it is small on screen, the last step of the shared drawing chain.

use std::collections::HashSet;

use bevy::asset::UntypedAssetId;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step};
use procgena_core_bevy::{BakeAhead, ImpostorGate};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, Impostor, ImpostorKey, ImpostorMaterial, ImpostorQuad,
    ImpostorRequest, Impostors, ViewCoverage, frame_size_for,
};

use crate::material::{StoneMaterial, bake_copy};
use crate::spawn::{
    Generated, SpawnSettings, StoneCache, StoneGenerating, StoneKey, StonePart, bevy_range,
};

/// How rocks' impostors are baked unless told otherwise: 8 × 8 views covering
/// viewers from straight above down to 45° below level, as for a boulder
/// seen up a hillside, each large enough for the biggest size on screen the
/// shared [`DistanceChain`] shows an impostor at (256 pixels with the
/// default chain). Set it on [`StonePlugin::impostor`](crate::StonePlugin::impostor).
///
/// Each distinct rock is baked once, from its game-level mesh, in the
/// background the first time it is generated; until then its lower mesh is
/// drawn until the rock is culled. A rock no camera can be far enough from
/// to show its impostor (see
/// [`FarthestViewDistance`](procgena_core_bevy::FarthestViewDistance)) is
/// not baked until one can. Impostors cast no shadows. To keep bakes
/// between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`StonePlugin`](crate::StonePlugin).
pub fn default_stone_impostor() -> BakeSettings {
    let frame = frame_size_for(DistanceChain::DEFAULT.largest_impostor_size());
    BakeSettings::new(8, frame).with_coverage(ViewCoverage::from_lowest_elevation(-45.0))
}

/// A rock's impostor, once baked, with the material that draws it.
#[derive(Clone, Debug)]
pub(crate) struct StoneImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) material: Handle<ImpostorMaterial>,
}

/// On a rock entity once its impostor part is spawned.
#[derive(Component, Debug, Default)]
pub(crate) struct StoneImpostorShown;

/// Asks the shared impostor cache for the impostor of every generated rock
/// that draws one and can be far enough away to show it (see
/// [`ImpostorGate`]), and keeps each one once it is baked. Rocks held back
/// still take an impostor that is already baked.
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    gate: ImpostorGate,
    ahead: Query<&StoneKey, With<BakeAhead>>,
    mut impostors: ResMut<Impostors>,
    mut cache: ResMut<StoneCache>,
    mut materials: ResMut<Assets<StoneMaterial>>,
) {
    let Some(bake) = settings.impostor else {
        return;
    };
    let ahead: HashSet<&StoneKey> = ahead.iter().collect();
    for (key, generated) in cache.ready_without_impostor() {
        let impostor_key = stone_key(&key, bake, &settings);
        let ready = if gate.can_show(generated.size) || ahead.contains(&key) {
            impostors
                .request(&mut commands, impostor_key, || {
                    stone_request(impostor_key, &generated, &mut materials)
                })
                .cloned()
        } else {
            impostors.get(impostor_key).cloned()
        };
        let (Some(ready), Some(material)) = (ready, impostors.material(impostor_key)) else {
            continue;
        };
        let assets = StoneImpostorAssets {
            impostor: ready,
            material: material.clone(),
        };
        cache.impostor_ready(key, assets);
    }
}

/// The impostor key of the rock under `key`: its specimen, baked with
/// `bake`, its game level and textures, and this crate's version.
fn stone_key(key: &StoneKey, bake: BakeSettings, settings: &SpawnSettings) -> ImpostorKey {
    let detail = key.detail().to_le_bytes();
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorKey::specimen(
        key.interim_specimen_id(),
        bake,
        &[
            &detail,
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for the rock `generated` under impostor key `key`:
/// its game-level mesh with bake copies of its material, waiting for that
/// material's textures.
fn stone_request(
    key: ImpostorKey,
    generated: &Generated,
    materials: &mut Assets<StoneMaterial>,
) -> ImpostorRequest {
    let level = &generated.levels[Step::Game.index()];
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    if let (Some(mesh), Some(source)) = (&level.mesh, materials.get(&level.material).cloned()) {
        let extension = &source.extension;
        wait_for.extend(
            [&extension.color, &extension.normal, &extension.orh]
                .into_iter()
                .flatten()
                .map(|t| t.id().untyped()),
        );
        let albedo = materials.add(bake_copy(&source, BakeOutput::Albedo));
        let normal = materials.add(bake_copy(&source, BakeOutput::Normal));
        parts.push(BakePart::new(mesh.clone(), albedo, normal));
    }
    ImpostorRequest {
        key,
        parts,
        wait_for,
    }
}

/// Gives every finished rock whose impostor is ready an impostor part, and
/// ends its lower mesh where the impostor takes over.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn show_impostors(
    mut commands: Commands,
    cache: Res<StoneCache>,
    quad: Res<ImpostorQuad>,
    rocks: Query<
        (Entity, &StoneKey, Option<&Children>),
        (Without<StoneGenerating>, Without<StoneImpostorShown>),
    >,
    parts: Query<&StonePart>,
) {
    for (rock, key, children) in &rocks {
        let Some((generated, assets)) = cache.with_impostor(key) else {
            continue;
        };
        let Some(ranges) = &generated.impostor_ranges else {
            continue;
        };

        // Every step takes its bands with the impostor as the last step:
        // the lower mesh now fades out where the impostor fades in.
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            if let Some(range) = bevy_range(&ranges[part.step.index()]) {
                commands.entity(*child).insert(range);
            }
        }
        let mut impostor = commands.spawn((
            StonePart {
                step: Step::Impostor,
            },
            assets.impostor.bundle(&quad, assets.material.clone()),
            NotShadowCaster,
            ChildOf(rock),
        ));
        if let Some(range) = bevy_range(&ranges[Step::Impostor.index()]) {
            impostor.insert(range);
        }
        commands.entity(rock).insert(StoneImpostorShown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_bake_fits_the_largest_impostor_on_screen() {
        let bake = default_stone_impostor();
        assert_eq!(bake.frame_size, 256);
        let largest = DistanceChain::DEFAULT.largest_impostor_size();
        assert!(
            bake.frame_size as f32 >= largest * procgena_impostor::REFERENCE_SCREEN_HEIGHT as f32
        );
    }
}
