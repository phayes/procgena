//! Tree impostors: each generated tree is baked into an impostor (see
//! [`procgena_impostor`]) from its game-level mesh, and drawn as one quad
//! when it is small on screen, the last step of the shared drawing chain.

use std::collections::HashMap;

use bevy::asset::UntypedAssetId;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step};
use procgena_core::{ContentHasher, SpecimenId};
use procgena_core_bevy::LodQuality;
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, Impostor, ImpostorKey, ImpostorMaterial, ImpostorQuad,
    ImpostorRequest, Impostors, ViewCoverage, frame_size_for,
};

use crate::Wind;
use crate::clusters::TreeClusters;
use crate::fruit_impostor::{
    FruitImpostor, FruitImpostorMaterial, fruit_impostor_material, fruit_key, fruit_request,
};
use crate::material::{TreeMaterial, WindUniform, bake_copy};
use crate::spawn::{
    CacheState, Generated, SingleTree, SingleTreeClusters, SpawnSettings, TreeCache,
    TreeGenerating, TreeKey, TreePart, TreePartKind, TreeWindPhase, chain_ranges, part_range,
    spawn_clusters,
};

/// How trees' impostors are baked unless told otherwise: 8 × 8 views
/// covering viewers from straight above down to 45° below level, each large
/// enough for the biggest size on screen the shared [`DistanceChain`] shows
/// an impostor at (256 pixels with the default chain). Looking up a hillside
/// of up to about 40° at a tree is covered; from further below, the tree
/// shows its view from 45° below. Set it on
/// [`TreePlugin::impostor`](crate::TreePlugin::impostor).
///
/// Each distinct tree is baked once, from its game-level mesh with its fruit
/// and flowers, in the background the first time it is generated; until
/// then its lower mesh is drawn until the tree is culled. Impostors cast no
/// shadows. To keep bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`TreePlugin`](crate::TreePlugin).
pub fn default_tree_impostor() -> BakeSettings {
    let frame = frame_size_for(DistanceChain::DEFAULT.largest_impostor_size());
    BakeSettings::new(8, frame).with_coverage(ViewCoverage::from_lowest_elevation(-45.0))
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
    let Some(bake) = settings.impostor else {
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
                    // A specimen with an empty game-level mesh, or whose
                    // material is gone, keeps its lower mesh.
                    let bakeable = specimen.levels[Step::Game.index()].is_some()
                        && materials.contains(&part.material);
                    if !bakeable {
                        continue;
                    }
                    let fruit_key = fruit_key(specimen.id, part.part, fruit, specimen.detail);
                    let ready = impostors
                        .request(&mut commands, fruit_key, || {
                            fruit_request(fruit_key, part, slot, &mut materials)
                                .expect("the specimen was checked to have a game-level mesh")
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
        let impostor_key = tree_key(
            cache.specimen_id(key),
            bake,
            key.detail(),
            &settings,
            &cluster_specimens(&generated.clusters),
        );
        let ready = impostors
            .request(&mut commands, impostor_key, || {
                tree_request(impostor_key, &generated, &mut materials)
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

/// The impostor key of tree specimen `specimen`, baked with `bake` from its
/// game level (whose hash, with its fruit's, is `detail`) and textures, and
/// this crate's version. `clusters` names the fruit and flower specimens it
/// grows (see [`cluster_specimens`]): the tree's own specimen id leaves out
/// which fruit pool, seed and number of slots they come from, and the bake
/// shows them.
fn tree_key(
    specimen: SpecimenId,
    bake: BakeSettings,
    detail: u64,
    settings: &SpawnSettings,
    clusters: &[Vec<SpecimenId>],
) -> ImpostorKey {
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
        bake,
        &[
            &detail.to_le_bytes(),
            textures.as_bytes(),
            &fruit,
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// The impostor request for the tree `generated` under impostor key `key`:
/// its game-level parts and clusters with still, unlit copies of their
/// materials, waiting for those materials' textures.
fn tree_request(
    key: ImpostorKey,
    generated: &Generated,
    materials: &mut Assets<TreeMaterial>,
) -> ImpostorRequest {
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    let baked_parts = generated
        .parts()
        .filter(|(step, ..)| *step == Step::Game)
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
/// ends its lower mesh where the impostor takes over.
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
        let ranges = chain_ranges(copy.size, quality.0);

        // Every mesh step takes its bands with the impostor as the last
        // step: the lower mesh now fades out where the impostor fades in.
        // Clusters are rebuilt to stop where the impostor fades in.
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            match part.kind {
                TreePartKind::Trellis => {}
                TreePartKind::Clusters => commands.entity(*child).despawn(),
                _ => {
                    if let Some(range) = part_range(&ranges[part.step.index()]) {
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
        let mut impostor = commands.spawn((
            TreePart {
                step: Step::Impostor,
                kind: TreePartKind::Impostor,
            },
            assets.impostor.bundle(&quad, assets.material.clone()),
            NotShadowCaster,
            ChildOf(tree),
        ));
        if let Some(range) = part_range(&ranges[Step::Impostor.index()]) {
            impostor.insert(range);
        }
        commands.entity(tree).insert(ImpostorShown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use procgena_tree::TextureCompression;

    use crate::fruit_impostor::default_fruit_impostor;

    /// Spawn settings with the default tree and fruit impostors and no
    /// texture folder.
    fn settings() -> SpawnSettings {
        SpawnSettings {
            texture_root: None,
            texture_compression: TextureCompression::Bc7,
            impostor: Some(default_tree_impostor()),
            fruit_impostor: Some(default_fruit_impostor()),
        }
    }

    /// Both default bakes are large enough for the biggest size on screen
    /// the shared chain shows an impostor at, at any quality.
    #[test]
    fn the_default_bakes_fit_the_largest_impostor_on_screen() {
        let largest = DistanceChain::DEFAULT.largest_impostor_size();
        for bake in [default_tree_impostor(), default_fruit_impostor()] {
            assert_eq!(bake.frame_size, 256);
            assert!(
                bake.frame_size as f32
                    >= largest * procgena_impostor::REFERENCE_SCREEN_HEIGHT as f32
            );
        }
    }

    /// A Scatter's trees are keyed by their real specimen ids; a tree built
    /// on its own by its interim one.
    #[test]
    fn scatter_trees_key_their_impostors_by_their_real_specimens() {
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let settings = settings();
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
    /// bake shows them. So is the game level's detail.
    #[test]
    fn the_fruit_a_tree_grows_changes_its_impostor_key() {
        let settings = settings();
        let bake = default_tree_impostor();
        let apple = procgena_tree::TreePreset::apple().tree.clone();
        let ids = |options: &procgena_tree::TreeOptions| -> Vec<Vec<SpecimenId>> {
            let tree = procgena_tree::Tree::new(options.clone()).unwrap();
            let skeleton = tree.skeleton();
            let (_, slots) = skeleton.clusters(procgena_tree::ClusterPart::Fruit);
            vec![slots.iter().map(|fruit| fruit.id()).collect()]
        };
        let key = |options: &procgena_tree::TreeOptions| {
            let specimen = SpecimenId::new(options.default_seed as u32, options.pool_id(), 0);
            tree_key(specimen, bake, 0, &settings, &ids(options))
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

        let bare = SpecimenId::new(0, apple.pool_id(), 0);
        assert_ne!(
            tree_key(bare, bake, 0, &settings, &[]),
            tree_key(bare, bake, 0, &settings, &[Vec::new()]),
            "a tree without clusters differs from one whose clusters have no slots"
        );
        assert_ne!(
            tree_key(bare, bake, 0, &settings, &[]),
            tree_key(bare, bake, 1, &settings, &[]),
            "a different game level bakes differently"
        );
    }
}
