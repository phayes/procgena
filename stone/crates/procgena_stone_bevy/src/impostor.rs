//! Rock impostors: each generated rock is baked into an impostor (see
//! [`procgena_impostor`]) and drawn far away as one quad, the last level of
//! the rock's levels of detail.

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use procgena_core::{LodRange, Quality};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, Impostor, ImpostorKey, ImpostorLevel, ImpostorMaterial,
    ImpostorQuad, ImpostorRequest, Impostors, ViewCoverage,
};
use procgena_stone::StoneDetail;

use crate::material::{StoneMaterial, bake_copy};
use crate::spawn::{
    Generated, SpawnSettings, StoneCache, StoneGenerating, StoneKey, StonePart, bevy_range,
};

/// The impostor rocks use unless told otherwise: from 80 m for a rock of
/// the reference size (2.5 m across, scaled with each rock's size like its
/// other levels), crossfading over 10% on each side, baked from level 1 as
/// 8 × 8 views of 128 pixels covering viewers from straight above down to
/// 45° below level, as for a boulder seen up a hillside. Set it on
/// [`StonePlugin::impostor`](crate::StonePlugin::impostor).
///
/// Each distinct rock is baked once, in the background, the first time it
/// is generated; until then its farthest level is drawn at every distance
/// beyond. A rock with fewer levels than the baked level, or whose last
/// level reaches past the impostor's distance, has no impostor. To keep
/// bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`StonePlugin`](crate::StonePlugin).
pub fn default_stone_impostor() -> ImpostorLevel {
    ImpostorLevel {
        bake: BakeSettings::new(8, 128).with_coverage(ViewCoverage::from_lowest_elevation(-45.0)),
        ..ImpostorLevel::new(80.0, 0.1, 1)
    }
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

/// Each of `detail`'s levels' fade bands, then `impostor`'s, for a rock
/// `scale` metres across drawn at `quality`; `None` when the rock has no
/// impostor: it has no level `impostor.baked_level`, or the impostor
/// doesn't fit after its levels.
pub(crate) fn impostor_ranges(
    detail: &StoneDetail,
    scale: f32,
    quality: Quality,
    impostor: ImpostorLevel,
) -> Option<Vec<LodRange>> {
    let mut lod = detail.lod().map_detail(Some);
    if impostor.baked_level >= lod.levels.len() {
        return None;
    }
    lod.levels.push(impostor.level(None));
    lod.validate().ok()?;
    Some(lod.ranges(lod.factor(scale, quality)))
}

/// Asks the shared impostor cache for the impostor of every generated rock
/// that draws one, and keeps each one once it is baked.
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    mut impostors: ResMut<Impostors>,
    mut cache: ResMut<StoneCache>,
    mut materials: ResMut<Assets<StoneMaterial>>,
) {
    let Some(impostor) = settings.impostor else {
        return;
    };
    for (key, generated) in cache.ready_without_impostor() {
        let impostor_key = stone_key(&key, impostor, &settings);
        let ready = impostors
            .request(&mut commands, impostor_key, || {
                stone_request(impostor_key, &generated, impostor, &mut materials)
            })
            .cloned();
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
/// `impostor`'s settings from its baked level, its detail levels and
/// textures, and this crate's version.
fn stone_key(key: &StoneKey, impostor: ImpostorLevel, settings: &SpawnSettings) -> ImpostorKey {
    let level = (impostor.baked_level as u64).to_le_bytes();
    let detail = key.detail().to_le_bytes();
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorKey::specimen(
        key.interim_specimen_id(),
        impostor.bake,
        &[
            &level,
            &detail,
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for the rock `generated` under impostor key `key`:
/// its baked level's mesh with bake copies of its material, waiting for
/// that material's textures.
fn stone_request(
    key: ImpostorKey,
    generated: &Generated,
    impostor: ImpostorLevel,
    materials: &mut Assets<StoneMaterial>,
) -> ImpostorRequest {
    let level = &generated.levels[impostor.baked_level];
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
/// ends its farthest level where the impostor takes over.
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

        // Every level takes its bands with the impostor as the last level:
        // the farthest now fades out where the impostor fades in.
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            if let Some(range) = bevy_range(&ranges[part.lod]) {
                commands.entity(*child).insert(range);
            }
        }
        let lod = generated.levels.len();
        let mut impostor = commands.spawn((
            StonePart { lod },
            assets.impostor.bundle(&quad, assets.material.clone()),
            ChildOf(rock),
        ));
        if let Some(range) = bevy_range(&ranges[lod]) {
            impostor.insert(range);
        }
        commands.entity(rock).insert(StoneImpostorShown);
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;

    /// Each level's fade-in and fade-out bands.
    fn bands(ranges: &[LodRange]) -> Vec<(Range<f32>, Range<f32>)> {
        ranges
            .iter()
            .map(|r| (r.fade_in.clone(), r.fade_out.clone()))
            .collect()
    }

    #[test]
    fn the_impostor_takes_over_where_the_farthest_level_ends() {
        let impostor = default_stone_impostor();
        let ranges = impostor_ranges(&StoneDetail::MEDIUM, 2.5, Quality::High, impostor).unwrap();
        let inf = f32::INFINITY..f32::INFINITY;
        // The medium levels hand over at 15 and 40 m with 5% bands; the
        // farthest now ends over the impostor's band, which never ends.
        assert_eq!(
            bands(&ranges),
            vec![
                (0.0..0.0, 14.25..15.75),
                (14.25..15.75, 38.0..42.0),
                (38.0..42.0, 72.0..88.0),
                (72.0..88.0, inf),
            ]
        );

        // A rock twice the size switches twice as far away, the impostor
        // included, and so does a higher quality setting.
        let large = impostor_ranges(&StoneDetail::MEDIUM, 5.0, Quality::High, impostor).unwrap();
        assert_eq!(large[3].fade_in, 144.0..176.0);
        let ultra = impostor_ranges(&StoneDetail::MEDIUM, 2.5, Quality::Ultra, impostor).unwrap();
        assert_eq!(ultra[3].fade_in, 108.0..132.0);
    }

    #[test]
    fn rocks_without_room_for_the_impostor_have_none() {
        let impostor = default_stone_impostor();
        // No level 1 to bake from.
        let single = StoneDetail::single(32, procgena_stone::TextureSize::Size512);
        assert!(impostor_ranges(&single, 2.5, Quality::High, impostor).is_none());
        // The last level starts beyond the impostor.
        let near = ImpostorLevel::new(30.0, 0.1, 1);
        assert!(impostor_ranges(&StoneDetail::MEDIUM, 2.5, Quality::High, near).is_none());
    }
}
