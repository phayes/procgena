//! Tree impostors: each generated tree is baked into an impostor (see
//! [`procgena_impostor`]) and drawn as one quad, the last level of the
//! tree's levels of detail.

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use procgena_core::SpecimenId;
use procgena_impostor::{
    BakeOutput, BakePart, Impostor, ImpostorKey, ImpostorLevel, ImpostorMaterial, ImpostorQuad,
    ImpostorRequest, Impostors,
};

use crate::material::{TreeMaterial, bake_copy};
use crate::spawn::{
    CacheState, Generated, SpawnSettings, TreeCache, TreeGenerating, TreeKey, TreePart,
    TreePartKind, part_range, spawn_clusters,
};

/// The impostor trees use unless told otherwise: from 120 m, crossfading
/// over 10% on each side, baked from level 1 with the default
/// [`BakeSettings`](procgena_impostor::BakeSettings). Set it on
/// [`TreePlugin::impostor`](crate::TreePlugin::impostor).
///
/// Each distinct tree is baked once, in the background, the first time it
/// is generated; until then its farthest mesh level is drawn at every
/// distance beyond. To keep bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`TreePlugin`](crate::TreePlugin).
pub fn default_tree_impostor() -> ImpostorLevel {
    ImpostorLevel::new(120.0, 0.1, 1)
}

/// A tree's impostor, once baked, with the quad and material that draw it.
#[derive(Clone, Debug)]
pub(crate) struct TreeImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) quad: Handle<Mesh>,
    pub(crate) material: Handle<ImpostorMaterial>,
}

/// On a tree entity once its impostor part is spawned.
#[derive(Component, Debug, Default)]
pub(crate) struct ImpostorShown;

/// Asks the shared impostor cache for the impostor of every generated tree
/// that has none yet, and keeps each one once it is baked.
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    quad: Res<ImpostorQuad>,
    mut impostors: ResMut<Impostors>,
    mut cache: ResMut<TreeCache>,
    mut materials: ResMut<Assets<TreeMaterial>>,
) {
    let Some(impostor) = settings.impostor else {
        return;
    };
    for (key, generated) in cache.ready_without_impostor() {
        if impostor.baked_level >= generated.level_count() {
            continue;
        }
        let impostor_key = tree_key(cache.specimen_id(key), impostor, &settings);
        let ready = impostors
            .request(&mut commands, impostor_key, || {
                tree_request(impostor_key, &generated, impostor, &mut materials)
            })
            .cloned();
        let (Some(ready), Some(material)) = (ready, impostors.material(impostor_key)) else {
            continue;
        };
        let assets = TreeImpostorAssets {
            impostor: ready,
            quad: quad.0.clone(),
            material: material.clone(),
        };
        cache.impostor_ready(key, assets);
    }
}

/// The impostor key of tree specimen `specimen`, baked with `impostor`'s
/// settings from its baked level's detail and textures, and this crate's
/// version.
fn tree_key(
    specimen: SpecimenId,
    impostor: ImpostorLevel,
    settings: &SpawnSettings,
) -> ImpostorKey {
    let level = format!("{:?}", settings.lod.levels.get(impostor.baked_level));
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorKey::specimen(
        specimen,
        impostor.bake,
        &[
            level.as_bytes(),
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for the tree `generated` under impostor key `key`:
/// its baked level's parts with still, unlit copies of their materials,
/// waiting for those materials' textures.
fn tree_request(
    key: ImpostorKey,
    generated: &Generated,
    impostor: ImpostorLevel,
    materials: &mut Assets<TreeMaterial>,
) -> ImpostorRequest {
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    let baked_parts = generated
        .parts()
        .filter(|(lod, ..)| *lod == impostor.baked_level)
        .map(|(_, _, mesh, material)| (&mesh.mesh, material));
    let clusters = generated
        .baked_clusters
        .iter()
        .map(|(mesh, material)| (mesh, material));
    for (mesh, material) in baked_parts.chain(clusters) {
        let Some(source) = materials.get(material).cloned() else {
            continue;
        };
        let base = &source.base;
        wait_for.extend(
            [
                &base.base_color_texture,
                &base.normal_map_texture,
                &base.metallic_roughness_texture,
                &base.occlusion_texture,
            ]
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

/// Gives every finished tree whose impostor is ready an impostor part, and
/// ends its farthest mesh level where the impostor takes over.
#[expect(clippy::type_complexity, reason = "Bevy system parameters")]
pub(crate) fn show_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    cache: Res<TreeCache>,
    quad: Res<ImpostorQuad>,
    trees: Query<
        (Entity, &TreeKey, Option<&Children>),
        (Without<TreeGenerating>, Without<ImpostorShown>),
    >,
    parts: Query<&TreePart>,
) {
    if settings.impostor.is_none() {
        return;
    }
    for (tree, key, children) in &trees {
        let (CacheState::Ready(generated), Some(assets)) =
            (cache.state(*key), cache.impostor(*key))
        else {
            continue;
        };
        let Some(ranges) = generated.impostor_ranges() else {
            continue;
        };

        // Every mesh level takes its bands with the impostor as the last
        // level: the farthest now fades out where the impostor fades in.
        // Clusters are rebuilt to stop where the impostor fades in.
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            match part.kind {
                TreePartKind::Trellis => {}
                TreePartKind::Clusters => commands.entity(*child).despawn(),
                _ => {
                    if let Some(range) = part_range(&ranges[part.lod]) {
                        commands.entity(*child).insert(range);
                    }
                }
            }
        }
        let specimen = cache.specimen_id(*key);
        spawn_clusters(&mut commands, tree, generated, specimen, &settings, true);
        let lod = generated.level_count();
        let mut impostor = commands.spawn((
            TreePart {
                lod,
                kind: TreePartKind::Impostor,
            },
            assets.impostor.bundle(&quad, assets.material.clone()),
            ChildOf(tree),
        ));
        if let Some(range) = part_range(&ranges[lod]) {
            impostor.insert(range);
        }
        commands.entity(tree).insert(ImpostorShown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Range;

    use procgena_core::{Lod, LodLevel, Quality};
    use procgena_tree::TextureCompression;

    use crate::default_tree_lod;

    /// Spawn settings with `levels` (distance and hysteresis of each mesh
    /// level, all with the default nearest level's materials) and
    /// `impostor`.
    fn settings(levels: &[(f32, f32)], impostor: Option<ImpostorLevel>) -> SpawnSettings {
        let near = default_tree_lod().levels[0].detail;
        let levels = levels
            .iter()
            .map(|&(distance, hysteresis)| LodLevel::new(distance, hysteresis, near))
            .collect();
        SpawnSettings {
            texture_root: None,
            texture_compression: TextureCompression::Bc7,
            lod: Lod::new(10.0, levels),
            impostor,
        }
    }

    /// Each level's fade-in and fade-out bands.
    fn bands(settings: &SpawnSettings) -> Vec<(Range<f32>, Range<f32>)> {
        let ranges = settings.impostor_ranges(Quality::High).unwrap();
        ranges
            .into_iter()
            .map(|r| (r.fade_in, r.fade_out))
            .collect()
    }

    #[test]
    fn the_impostor_takes_over_where_the_farthest_level_ends() {
        let impostor = ImpostorLevel::new(100.0, 0.1, 0);

        // The farthest level keeps its start and ends over the impostor's
        // band; the impostor starts over the same band and never ends.
        let two = settings(&[(0.0, 0.0), (20.0, 0.1)], Some(impostor));
        two.draw_lod().validate().unwrap();
        assert_eq!(
            bands(&two),
            vec![
                (0.0..0.0, 18.0..22.0),
                (18.0..22.0, 90.0..110.0),
                (90.0..110.0, f32::INFINITY..f32::INFINITY),
            ]
        );

        // A tree with a single level, drawn at every distance, now ends.
        let one = settings(&[(0.0, 0.0)], Some(impostor));
        assert_eq!(
            bands(&one),
            vec![
                (0.0..0.0, 90.0..110.0),
                (90.0..110.0, f32::INFINITY..f32::INFINITY),
            ]
        );
        assert!(one.mesh_ranges(Quality::High)[0].is_always());
        assert!(
            settings(&[(0.0, 0.0)], None)
                .impostor_ranges(Quality::High)
                .is_none()
        );
    }

    /// The default levels and impostor keep the bands trees had before
    /// they moved onto the shared levels of detail.
    #[test]
    fn default_levels_keep_their_bands() {
        let settings = SpawnSettings {
            texture_root: None,
            texture_compression: TextureCompression::Bc7,
            lod: default_tree_lod(),
            impostor: Some(default_tree_impostor()),
        };
        settings.draw_lod().validate().unwrap();
        let inf = f32::INFINITY..f32::INFINITY;
        let before: Vec<_> = settings
            .mesh_ranges(Quality::High)
            .into_iter()
            .map(|r| (r.fade_in, r.fade_out))
            .collect();
        assert_eq!(
            before,
            vec![
                (0.0..0.0, 9.5..10.5),
                (9.5..10.5, 23.75..26.25),
                (23.75..26.25, 47.5..52.5),
                (47.5..52.5, inf.clone()),
            ]
        );
        assert_eq!(
            bands(&settings),
            vec![
                (0.0..0.0, 9.5..10.5),
                (9.5..10.5, 23.75..26.25),
                (23.75..26.25, 47.5..52.5),
                (47.5..52.5, 108.0..132.0),
                (108.0..132.0, inf),
            ]
        );
    }

    /// A Scatter's trees are keyed by their real specimen ids; a tree built
    /// on its own by its interim one.
    #[test]
    fn scatter_trees_key_their_impostors_by_their_real_specimens() {
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let settings = settings(&[(0.0, 0.0)], Some(default_tree_impostor()));
        let species = procgena_tree::ScatterTree::new(procgena_tree::TreePreset::oak_small());
        let mut cache = TreeCache::default();

        let options = std::sync::Arc::new(species.variant(2));
        let real = species.specimen_id(2);
        let grove = cache.request_specimen(&options, real, &settings);
        assert_eq!(cache.specimen_id(grove), real);
        assert_ne!(real, grove.interim_specimen_id());

        let single = cache.request(&std::sync::Arc::new(species.tree.clone()), &settings);
        assert_eq!(cache.specimen_id(single), single.interim_specimen_id());
    }
}
