//! Tree impostors: each generated tree is baked into an impostor (see
//! [`procgena_impostor`]) and drawn as one quad, the last level of the
//! tree's levels of detail.

use std::collections::HashMap;

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use procgena_core::{ContentHasher, SpecimenId};
use procgena_core_bevy::LodQuality;
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, Impostor, ImpostorKey, ImpostorLevel, ImpostorMaterial,
    ImpostorQuad, ImpostorRequest, Impostors, ViewCoverage,
};

use crate::Wind;
use crate::clusters::TreeClusters;
use crate::fruit_impostor::{
    FruitImpostor, FruitImpostorMaterial, fruit_impostor_material, fruit_key, fruit_request,
};
use crate::material::{TreeMaterial, WindUniform, bake_copy};
use crate::spawn::{
    CacheState, Generated, SingleTree, SingleTreeClusters, SpawnSettings, TreeCache,
    TreeGenerating, TreeKey, TreePart, TreePartKind, TreeWindPhase, part_range, spawn_clusters,
};

/// The impostor trees use unless told otherwise: from 120 m for a tree of
/// the levels' reference height (60 m for one half as tall), crossfading
/// over 10% on each side, baked from level 1 as 10 × 10 views of 128
/// pixels covering viewers from straight above down to 45° below level.
/// That covers looking up a hillside of up to about 40° at a tree; from
/// further below, the tree shows its view from 45° below. Set it on
/// [`TreePlugin::impostor`](crate::TreePlugin::impostor).
///
/// Each distinct tree is baked once, in the background, the first time it
/// is generated; until then its farthest mesh level is drawn at every
/// distance beyond. To keep bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`TreePlugin`](crate::TreePlugin).
pub fn default_tree_impostor() -> ImpostorLevel {
    ImpostorLevel {
        bake: BakeSettings::new(10, 128).with_coverage(ViewCoverage::from_lowest_elevation(-45.0)),
        ..ImpostorLevel::new(120.0, 0.1, 1)
    }
}

/// A tree's impostor, once baked, with the quad and material that draw it,
/// and the impostors of its fruit and flower clusters.
#[derive(Clone, Debug)]
pub(crate) struct TreeImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) quad: Handle<Mesh>,
    pub(crate) material: Handle<ImpostorMaterial>,
    /// Each cluster specimen's impostor, by specimen; empty when cluster
    /// impostors are off.
    pub(crate) clusters: HashMap<SpecimenId, FruitImpostor>,
}

/// One shared [`FruitImpostorMaterial`] per baked cluster impostor.
#[derive(Resource, Default)]
pub(crate) struct FruitImpostorMaterials(HashMap<ImpostorKey, Handle<FruitImpostorMaterial>>);

/// On a tree entity once its impostor part is spawned.
#[derive(Component, Debug, Default)]
pub(crate) struct ImpostorShown;

/// Asks the shared impostor cache for the impostor of every generated tree
/// that has none yet, and for its clusters' impostors, and keeps them once
/// all of them are baked.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
    wind: Res<Wind>,
    quad: Res<ImpostorQuad>,
    mut impostors: ResMut<Impostors>,
    mut cache: ResMut<TreeCache>,
    mut materials: ResMut<Assets<TreeMaterial>>,
    mut fruit_materials: ResMut<Assets<FruitImpostorMaterial>>,
    mut shared: ResMut<FruitImpostorMaterials>,
) {
    let Some(impostor) = settings.impostor else {
        return;
    };
    for (key, generated) in cache.ready_without_impostor() {
        // The clusters' impostors, each baked once however many trees
        // grow it.
        let mut clusters = HashMap::new();
        let mut waiting = false;
        if let Some(fruit) = settings.fruit_impostor {
            for part in &generated.clusters {
                for (slot, specimen) in part.specimens.iter().enumerate() {
                    // A specimen with nothing at the baked level, or whose
                    // material is gone, keeps its last mesh level.
                    let bakeable = specimen
                        .levels
                        .get(fruit.baked_level)
                        .is_some_and(Option::is_some)
                        && materials.contains(&part.material);
                    if !bakeable {
                        continue;
                    }
                    let fruit_key = fruit_key(specimen.id, part.part, fruit);
                    let ready = impostors
                        .request(&mut commands, fruit_key, || {
                            fruit_request(fruit_key, part, slot, fruit, &mut materials)
                                .expect("the specimen was checked to have a baked level")
                        })
                        .cloned();
                    let Some(ready) = ready else {
                        waiting = true;
                        continue;
                    };
                    let material = shared
                        .0
                        .entry(fruit_key)
                        .or_insert_with(|| {
                            fruit_materials
                                .add(fruit_impostor_material(&ready, WindUniform::from(wind.0)))
                        })
                        .clone();
                    clusters.insert(specimen.id, FruitImpostor { material });
                }
            }
        }
        if impostor.baked_level >= generated.level_count() {
            continue;
        }
        let impostor_key = tree_key(
            cache.specimen_id(key),
            impostor,
            &settings,
            &cluster_specimens(&generated.clusters),
        );
        let ready = impostors
            .request(&mut commands, impostor_key, || {
                tree_request(impostor_key, &generated, impostor, &mut materials)
            })
            .cloned();
        let (Some(ready), Some(material)) = (ready, impostors.material(impostor_key)) else {
            continue;
        };
        if waiting {
            continue;
        }
        let assets = TreeImpostorAssets {
            impostor: ready,
            quad: quad.0.clone(),
            material: material.clone(),
            clusters,
        };
        cache.impostor_ready(key, assets);
    }
}

/// The fruit and flower specimens a tree draws, one list per kind of
/// cluster it grows, in slot order.
fn cluster_specimens(clusters: &[TreeClusters]) -> Vec<Vec<SpecimenId>> {
    clusters
        .iter()
        .map(|clusters| clusters.specimens.iter().map(|s| s.id).collect())
        .collect()
}

/// The impostor key of tree specimen `specimen`, baked with `impostor`'s
/// settings from its baked level's detail and textures, and this crate's
/// version. `clusters` names the fruit and flower specimens it grows (see
/// [`cluster_specimens`]): the tree's own specimen id leaves out which
/// fruit pool, seed and number of slots they come from, and the bake shows
/// them.
fn tree_key(
    specimen: SpecimenId,
    impostor: ImpostorLevel,
    settings: &SpawnSettings,
    clusters: &[Vec<SpecimenId>],
) -> ImpostorKey {
    let level = format!("{:?}", settings.lod.levels.get(impostor.baked_level));
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    let mut fruit = ContentHasher::new();
    fruit.write_usize(clusters.len());
    for specimens in clusters {
        fruit.write_usize(specimens.len());
        for id in specimens {
            fruit.write(id);
        }
    }
    let fruit = fruit.finish().to_le_bytes();
    ImpostorKey::specimen(
        specimen,
        impostor.bake,
        &[
            level.as_bytes(),
            textures.as_bytes(),
            &fruit,
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
    quality: Res<LodQuality>,
    cache: Res<TreeCache>,
    quad: Res<ImpostorQuad>,
    trees: Query<
        (
            Entity,
            &TreeKey,
            Option<&Children>,
            Option<&Transform>,
            Option<&TreeWindPhase>,
        ),
        (Without<TreeGenerating>, Without<ImpostorShown>),
    >,
    parts: Query<&TreePart>,
) {
    if settings.impostor.is_none() {
        return;
    }
    for (tree, key, children, transform, phase) in &trees {
        let (CacheState::Ready(generated), Some(assets)) =
            (cache.state(*key), cache.impostor(*key))
        else {
            continue;
        };
        let copy = SingleTree::new(generated, transform, phase);
        let Some(ranges) = settings.impostor_ranges(quality.0, copy.size) else {
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
        let clusters = SingleTreeClusters {
            specimen: cache.specimen_id(*key),
            impostor: Some(assets),
            copy,
        };
        spawn_clusters(&mut commands, tree, generated, &settings, clusters);
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
    use crate::fruit_impostor::default_fruit_impostor;

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
            fruit_impostor: Some(default_fruit_impostor()),
        }
    }

    /// Each level's fade-in and fade-out bands for a tree of the levels'
    /// reference size.
    fn bands(settings: &SpawnSettings) -> Vec<(Range<f32>, Range<f32>)> {
        let size = settings.lod.reference_size;
        let ranges = settings.impostor_ranges(Quality::High, size).unwrap();
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
        assert!(one.mesh_ranges(Quality::High, 10.0)[0].is_always());
        assert!(
            settings(&[(0.0, 0.0)], None)
                .impostor_ranges(Quality::High, 10.0)
                .is_none()
        );
    }

    /// The default levels and impostor keep, for a tree of the reference
    /// height, the bands trees had before they moved onto the shared levels
    /// of detail, and end at the cull distance.
    #[test]
    fn default_levels_keep_their_bands() {
        let settings = SpawnSettings {
            texture_root: None,
            texture_compression: TextureCompression::Bc7,
            lod: default_tree_lod(),
            impostor: Some(default_tree_impostor()),
            fruit_impostor: Some(default_fruit_impostor()),
        };
        settings.draw_lod().validate().unwrap();
        let mesh_bands = |size| -> Vec<_> {
            settings
                .mesh_ranges(Quality::High, size)
                .into_iter()
                .map(|r| (r.fade_in, r.fade_out))
                .collect()
        };
        assert_eq!(
            mesh_bands(10.0),
            vec![
                (0.0..0.0, 9.5..10.5),
                (9.5..10.5, 23.75..26.25),
                (23.75..26.25, 47.5..52.5),
                (47.5..52.5, 1425.0..1575.0),
            ]
        );
        assert_eq!(
            bands(&settings),
            vec![
                (0.0..0.0, 9.5..10.5),
                (9.5..10.5, 23.75..26.25),
                (23.75..26.25, 47.5..52.5),
                (47.5..52.5, 108.0..132.0),
                (108.0..132.0, 1350.0..1650.0),
            ]
        );

        // A tree half the reference height switches at half the distances.
        assert_eq!(
            mesh_bands(5.0),
            vec![
                (0.0..0.0, 4.75..5.25),
                (4.75..5.25, 11.875..13.125),
                (11.875..13.125, 23.75..26.25),
                (23.75..26.25, 712.5..787.5),
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

    /// The fruit a tree grows is part of its impostor's key: its number of
    /// slots and its seed leave the tree's own specimen id alone, but the
    /// bake shows them.
    #[test]
    fn the_fruit_a_tree_grows_changes_its_impostor_key() {
        let settings = settings(&[(0.0, 0.0)], Some(default_tree_impostor()));
        let apple = procgena_tree::TreePreset::apple().tree.clone();
        let ids = |options: &procgena_tree::TreeOptions| -> Vec<Vec<SpecimenId>> {
            let tree = procgena_tree::Tree::new(options.clone()).unwrap();
            let skeleton = tree.skeleton();
            let (_, slots) = skeleton.clusters(procgena_tree::ClusterPart::Fruit);
            vec![slots.iter().map(|fruit| fruit.id()).collect()]
        };
        let key = |options: &procgena_tree::TreeOptions| {
            let specimen = SpecimenId::new(options.default_seed as u32, options.pool_id(), 0);
            tree_key(specimen, default_tree_impostor(), &settings, &ids(options))
        };

        let mut more_slots = apple.clone();
        more_slots.fruit.as_mut().unwrap().default_slots += 1;
        let mut other_seed = apple.clone();
        other_seed.fruit.as_mut().unwrap().fruit.default_seed += 1;
        for other in [&more_slots, &other_seed] {
            assert_eq!(apple.pool_id(), other.pool_id());
            assert_ne!(key(&apple), key(other));
        }
        assert_eq!(key(&apple), key(&apple.clone()));
        assert_ne!(
            tree_key(
                SpecimenId::new(0, apple.pool_id(), 0),
                default_tree_impostor(),
                &settings,
                &[]
            ),
            tree_key(
                SpecimenId::new(0, apple.pool_id(), 0),
                default_tree_impostor(),
                &settings,
                &[Vec::new()]
            ),
            "a tree without clusters differs from one whose clusters have no slots"
        );
    }
}
