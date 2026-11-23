//! Tree impostors: each generated tree is baked into an impostor (see
//! [`procgena_impostor`]) and drawn as one quad beyond its farthest mesh
//! level.

use std::ops::Range;

use bevy::asset::UntypedAssetId;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_impostor::{
    BakeOutput, BakePart, BakedImpostor, Impostor, ImpostorKey, ImpostorMaterial, ImpostorQuad,
    ImpostorReady, ImpostorRequest,
};

use crate::material::{TreeMaterial, bake_copy};
use crate::spawn::{
    CacheState, Generated, SpawnSettings, TreeCache, TreeGenerating, TreeKey, TreePart,
    TreePartKind,
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
    /// The camera distances over which the impostor fades in, which are also
    /// where the farthest mesh level fades out.
    pub(crate) fn band(&self) -> Range<f32> {
        let half = self.distance * self.hysteresis;
        (self.distance - half).max(0.0)..self.distance + half
    }

    /// The distances the impostor is drawn at.
    pub(crate) fn range(&self) -> VisibilityRange {
        VisibilityRange {
            start_margin: self.band(),
            end_margin: f32::INFINITY..f32::INFINITY,
            use_aabb: false,
        }
    }

    /// The distance bands of a tree's levels once the impostor is drawn
    /// beyond them: `levels` (the mesh levels' bands, nearest first) with
    /// the farthest level ending where the impostor fades in, then the
    /// impostor's own band.
    pub(crate) fn level_ranges(
        &self,
        mut levels: Vec<Option<VisibilityRange>>,
    ) -> Vec<Option<VisibilityRange>> {
        if let Some(last) = levels.last_mut() {
            *last = Some(ended_at(last.take(), self.band()));
        }
        levels.push(Some(self.range()));
        levels
    }
}

/// A tree's impostor, once baked, with the quad and material that draw it
/// and the distances it is drawn at.
#[derive(Clone, Debug)]
pub(crate) struct TreeImpostorAssets {
    pub(crate) impostor: Impostor,
    pub(crate) quad: Handle<Mesh>,
    pub(crate) material: Handle<ImpostorMaterial>,
    pub(crate) setting: TreeImpostor,
}

/// `range` (or, for a level without one, every distance from the camera)
/// ending over `band`.
fn ended_at(range: Option<VisibilityRange>, band: Range<f32>) -> VisibilityRange {
    match range {
        Some(range) => VisibilityRange {
            end_margin: band,
            ..range
        },
        None => VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: band,
            use_aabb: false,
        },
    }
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
    let level = format!("{:?}", settings.lod_levels.get(impostor.baked_level));
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
    let (Ok((ImpostorFor(key), BakedImpostor(impostor))), Some(setting)) =
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
            setting,
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
    parts: Query<(&TreePart, Option<&VisibilityRange>)>,
) {
    let Some(setting) = settings.impostor else {
        return;
    };
    for (tree, key, children) in &trees {
        let (CacheState::Ready(generated), Some(assets)) =
            (cache.state(*key), cache.impostor(*key))
        else {
            continue;
        };

        // The farthest mesh level now fades out where the impostor fades in.
        // A tree with a single level had no range: it starts at the camera.
        let last = generated.level_count().saturating_sub(1);
        for child in children.into_iter().flatten() {
            let Ok((part, range)) = parts.get(*child) else {
                continue;
            };
            if part.lod != last || part.kind == TreePartKind::Trellis {
                continue;
            }
            commands
                .entity(*child)
                .insert(ended_at(range.cloned(), setting.band()));
        }
        commands.spawn((
            TreePart {
                lod: last + 1,
                kind: TreePartKind::Impostor,
            },
            assets.impostor.bundle(&quad, assets.material.clone()),
            setting.range(),
            ChildOf(tree),
        ));
        commands.entity(tree).insert(ImpostorShown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level faded in over `start` and out over `end`.
    fn level(start: Range<f32>, end: Range<f32>) -> Option<VisibilityRange> {
        Some(VisibilityRange {
            start_margin: start,
            end_margin: end,
            use_aabb: false,
        })
    }

    /// Each level's fade-in and fade-out bands, which can be printed.
    type Bands = Vec<Option<(Range<f32>, Range<f32>)>>;

    /// The bands of `levels`.
    fn bands(levels: &[Option<VisibilityRange>]) -> Bands {
        levels
            .iter()
            .map(|level| {
                level
                    .as_ref()
                    .map(|r| (r.start_margin.clone(), r.end_margin.clone()))
            })
            .collect()
    }

    #[test]
    fn the_impostor_takes_over_where_the_farthest_level_ends() {
        let impostor = TreeImpostor {
            distance: 100.0,
            hysteresis: 0.1,
            baked_level: 0,
        };
        assert_eq!(impostor.band(), 90.0..110.0);

        // The farthest level keeps its start and ends over the band; the
        // impostor starts over the same band and never ends.
        let ranges = impostor.level_ranges(vec![
            level(0.0..0.0, 18.0..22.0),
            level(18.0..22.0, f32::INFINITY..f32::INFINITY),
        ]);
        assert_eq!(
            bands(&ranges),
            vec![
                Some((0.0..0.0, 18.0..22.0)),
                Some((18.0..22.0, 90.0..110.0)),
                Some((90.0..110.0, f32::INFINITY..f32::INFINITY)),
            ]
        );

        // A tree with a single level, drawn at every distance, now ends.
        let ranges = impostor.level_ranges(vec![None]);
        assert_eq!(
            bands(&ranges),
            vec![
                Some((0.0..0.0, 90.0..110.0)),
                Some((90.0..110.0, f32::INFINITY..f32::INFINITY)),
            ]
        );
    }
}
