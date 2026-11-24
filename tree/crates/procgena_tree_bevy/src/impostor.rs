//! Tree impostors: each generated tree is baked into an impostor (see
//! [`procgena_impostor`]) and drawn as one quad, the last level of the
//! tree's levels of detail.

use bevy::asset::UntypedAssetId;
use bevy::prelude::*;
use procgena_core::{LevelForm, LodLevel};
use procgena_impostor::{
    BakeOutput, BakePart, BakedImpostor, Impostor, ImpostorKey, ImpostorMaterial, ImpostorQuad,
    ImpostorReady, ImpostorRequest,
};

use crate::material::{TreeMaterial, bake_copy};
use crate::spawn::{
    CacheState, Generated, SpawnSettings, TreeCache, TreeGenerating, TreeKey, TreeLevel, TreePart,
    TreePartKind, part_range,
};

/// Draws trees far away as impostors: one quad showing a baked view of the
/// tree, beyond its farthest mesh level. Set on
/// [`TreePlugin::impostor`](crate::TreePlugin::impostor).
///
/// Each distinct tree is baked once, in the background, the first time it
/// is generated; until then its farthest mesh level is drawn at every
/// distance beyond. To keep bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`TreePlugin`](crate::TreePlugin).
///
/// Impostors don't sway in the wind, so keep `distance` well beyond where
/// sway is noticeable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeImpostor {
    /// Camera distance where the impostor takes over from the farthest mesh
    /// level, in metres.
    pub distance: f32,
    /// Width of the crossfade with the farthest mesh level, as a fraction
    /// of `distance` on each side.
    pub hysteresis: f32,
    /// The mesh level the impostor is baked from, counting from 0 for the
    /// nearest. It should be a level with real geometry, not camera-facing
    /// cards.
    pub baked_level: usize,
}

impl Default for TreeImpostor {
    fn default() -> Self {
        Self {
            distance: 120.0,
            hysteresis: 0.1,
            baked_level: 1,
        }
    }
}

impl TreeImpostor {
    /// The impostor as the last level of a tree's levels of detail, after
    /// the mesh levels. It has no mesh settings of its own.
    pub(crate) fn level(&self) -> LodLevel<Option<TreeLevel>> {
        LodLevel::new(self.distance, self.hysteresis, None).with_form(LevelForm::Impostor)
    }
}

/// A tree's impostor, once baked, with the quad and material that draw it.
#[derive(Clone, Debug)]
pub(crate) struct TreeImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) quad: Handle<Mesh>,
    pub(crate) material: Handle<ImpostorMaterial>,
}

/// On an impostor request spawned for a generated tree.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ImpostorFor(pub(crate) TreeKey);

/// On a tree entity once its impostor part is spawned.
#[derive(Component, Debug, Default)]
pub(crate) struct ImpostorShown;

/// Asks for the impostor of every newly generated tree.
pub(crate) fn request_impostors(
    mut commands: Commands,
    settings: Res<SpawnSettings>,
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
        let request = tree_request(key, &generated, impostor, &settings, &mut materials);
        let entity = commands.spawn((request, ImpostorFor(key))).id();
        cache.impostor_requested(key, entity);
    }
}

/// The impostor request for the tree `generated` under `key`: its baked
/// level's parts with still, unlit copies of their materials, waiting for
/// those materials' textures.
fn tree_request(
    key: TreeKey,
    generated: &Generated,
    impostor: TreeImpostor,
    settings: &SpawnSettings,
    materials: &mut Assets<TreeMaterial>,
) -> ImpostorRequest {
    let mut parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    for (lod, _, mesh, material) in generated.parts() {
        if lod != impostor.baked_level {
            continue;
        }
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
        parts.push(BakePart::new(mesh.mesh.clone(), albedo, normal));
    }

    // Everything that changes the picture: the tree, the baked level's
    // detail and textures, and this crate's version.
    let level = format!("{:?}", settings.lod.levels.get(impostor.baked_level));
    let textures = format!(
        "{:?} {:?}",
        settings.texture_root, settings.texture_compression
    );
    ImpostorRequest {
        key: ImpostorKey::new(
            "procgena_tree",
            key.value(),
            &[
                level.as_bytes(),
                textures.as_bytes(),
                env!("CARGO_PKG_VERSION").as_bytes(),
            ],
        ),
        parts,
        wait_for,
    }
}

/// Keeps each tree's impostor when its bake is ready.
pub(crate) fn keep_impostor(
    event: On<ImpostorReady>,
    requests: Query<(&ImpostorFor, &BakedImpostor)>,
    settings: Res<SpawnSettings>,
    quad: Res<ImpostorQuad>,
    mut cache: ResMut<TreeCache>,
    mut materials: ResMut<Assets<ImpostorMaterial>>,
) {
    let (Ok((ImpostorFor(key), BakedImpostor(impostor))), Some(_)) =
        (requests.get(event.entity), settings.impostor)
    else {
        return;
    };
    let material = materials.add(impostor.material());
    cache.impostor_ready(
        *key,
        TreeImpostorAssets {
            impostor: impostor.clone(),
            quad: quad.0.clone(),
            material,
        },
    );
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
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            if part.kind == TreePartKind::Trellis {
                continue;
            }
            if let Some(range) = part_range(&ranges[part.lod]) {
                commands.entity(*child).insert(range);
            }
        }
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

    use procgena_core::{Lod, Quality};
    use procgena_tree::TextureCompression;

    use crate::default_tree_lod;

    /// Spawn settings with `levels` (distance and hysteresis of each mesh
    /// level, all with the default nearest level's materials) and
    /// `impostor`.
    fn settings(levels: &[(f32, f32)], impostor: Option<TreeImpostor>) -> SpawnSettings {
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
        let impostor = TreeImpostor {
            distance: 100.0,
            hysteresis: 0.1,
            baked_level: 0,
        };

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
            impostor: Some(TreeImpostor::default()),
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
}
