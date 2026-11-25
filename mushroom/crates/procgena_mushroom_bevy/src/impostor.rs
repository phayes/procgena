//! Mushroom impostors: each distinct mushroom, cluster or scattered body is
//! baked into an impostor (see [`procgena_impostor`]) from its game-level
//! mesh, and drawn as one quad when it is small on screen, the last step of
//! the shared drawing chain.

use bevy::asset::UntypedAssetId;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use procgena_core::chain::{DistanceChain, Step};
use procgena_core::{LodRange, SpecimenId};
use procgena_core_bevy::instanced::{InstancedPart, InstancedScatter, InstancedSlot, PartMesh};
use procgena_impostor::{
    BakeOutput, BakePart, BakeSettings, ImpostorKey, ImpostorQuad, ImpostorRequest, Impostors,
    ViewCoverage, frame_size_for,
};

use crate::material::{MushroomMaterial, bake_copy};
use crate::spawn::{MushroomPart, SpawnSettings, bevy_range};

/// How mushrooms' impostors are baked unless told otherwise: 8 × 8 views
/// covering viewers from straight above down to 45° below level, as for a
/// mushroom seen up a slope, each large enough for the biggest size on
/// screen the shared [`DistanceChain`] shows an impostor at (256 pixels with
/// the default chain). Set it on
/// [`MushroomPlugin::impostor`](crate::MushroomPlugin::impostor).
///
/// Each distinct mushroom or cluster, and each distinct body in a scatter,
/// is baked once, from its game-level mesh, in the background the first time
/// it is built; until then its lower mesh is drawn until it is culled.
/// Impostors cast no shadows. To keep bakes between runs, or ship them, add
/// [`ImpostorPlugin`](procgena_impostor::ImpostorPlugin) yourself with a
/// cache folder before [`MushroomPlugin`](crate::MushroomPlugin).
pub fn default_mushroom_impostor() -> BakeSettings {
    let frame = frame_size_for(DistanceChain::DEFAULT.largest_impostor_size());
    BakeSettings::new(8, frame).with_coverage(ViewCoverage::from_lowest_elevation(-45.0))
}

/// What a mushroom or scatter is baked as, which keeps the impostors of a
/// whole cluster and of one scattered body apart.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BakedAs {
    /// A single mushroom or cluster, every member included.
    Single,
    /// Piece `piece` of a scattered specimen: one body.
    Piece(u32),
}

/// The impostor key of `specimen` baked as `baked_as` with `bake`, from a
/// game level whose content hash is `game_level`, with the textures in
/// `settings` and this crate's version.
pub(crate) fn impostor_key(
    specimen: SpecimenId,
    baked_as: BakedAs,
    game_level: u64,
    bake: BakeSettings,
    settings: &SpawnSettings,
) -> ImpostorKey {
    let baked_as = match baked_as {
        BakedAs::Single => "single".to_owned(),
        BakedAs::Piece(piece) => format!("piece {piece}"),
    };
    let textures = format!("{:?}", settings.textures);
    ImpostorKey::specimen(
        specimen,
        bake,
        &[
            baked_as.as_bytes(),
            &game_level.to_le_bytes(),
            textures.as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
        ],
    )
}

/// On a single mushroom or cluster until its impostor is drawn: what to
/// bake it from, and where each step is drawn once the impostor is there.
#[derive(Component)]
pub(crate) struct SingleImpostor {
    /// The impostor to ask the shared cache for.
    pub(crate) key: ImpostorKey,
    /// For each variant, the game-level meshes of the members drawn with it,
    /// placed as in the cluster, with the material that draws them.
    pub(crate) parts: Vec<(Mesh, Handle<MushroomMaterial>)>,
    /// Each step's fade band by [`Step::index`], the impostor included.
    pub(crate) ranges: [LodRange; 3],
}

/// On a scatter until every body's impostor is drawn: each slot's impostor
/// key, by slot, or `None` once that slot draws its impostor or has nothing
/// to bake.
#[derive(Component)]
pub(crate) struct ScatterImpostors {
    waiting: Vec<Option<ImpostorKey>>,
}

impl ScatterImpostors {
    /// Waits for the impostor of every slot in `slots`, where slot `i` was
    /// built from a game level with content hash `game_levels[i]`.
    pub(crate) fn new(
        slots: &[InstancedSlot],
        game_levels: &[u64],
        bake: BakeSettings,
        settings: &SpawnSettings,
    ) -> Self {
        let waiting = slots
            .iter()
            .zip(game_levels)
            .map(|(slot, &game_level)| {
                let baked_as = BakedAs::Piece(slot.piece);
                Some(impostor_key(
                    slot.specimen,
                    baked_as,
                    game_level,
                    bake,
                    settings,
                ))
            })
            .collect();
        Self { waiting }
    }
}

/// Asks the shared impostor cache for the impostor of every single mushroom
/// or cluster waiting for one, and once it is baked, draws it after the
/// lower mesh. Does nothing until the impostor plugin's resources exist.
pub(crate) fn show_single_impostors(
    mut commands: Commands,
    impostors: Option<ResMut<Impostors>>,
    quad: Option<Res<ImpostorQuad>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MushroomMaterial>>,
    singles: Query<(Entity, &SingleImpostor, Option<&Children>)>,
    parts: Query<&MushroomPart>,
) {
    let (Some(mut impostors), Some(quad)) = (impostors, quad) else {
        return;
    };
    for (entity, waiting, children) in &singles {
        let key = waiting.key;
        let ready = impostors
            .request(&mut commands, key, || {
                let parts = waiting
                    .parts
                    .iter()
                    .map(|(mesh, material)| (meshes.add(mesh.clone()), material.clone()))
                    .collect();
                bake_request(key, parts, &mut materials)
            })
            .cloned();
        let (Some(ready), Some(material)) = (ready, impostors.material(key).cloned()) else {
            continue;
        };

        // Every step takes its band with the impostor as the last step: the
        // lower mesh now fades out where the impostor fades in.
        for child in children.into_iter().flatten() {
            let Ok(part) = parts.get(*child) else {
                continue;
            };
            if let Some(range) = bevy_range(&waiting.ranges[part.step.index()]) {
                commands.entity(*child).insert(range);
            }
        }
        let mut impostor = commands.spawn((
            MushroomPart {
                instance: 0,
                step: Step::Impostor,
            },
            ready.bundle(&quad, material),
            NotShadowCaster,
            ChildOf(entity),
        ));
        if let Some(range) = bevy_range(&waiting.ranges[Step::Impostor.index()]) {
            impostor.insert(range);
        }
        commands.entity(entity).remove::<SingleImpostor>();
    }
}

/// Asks the shared impostor cache for the impostor of every scattered body
/// waiting for one, and once it is baked, adds it to the body's slot as its
/// last level. Does nothing until the impostor plugin's resources exist.
pub(crate) fn show_scatter_impostors(
    mut commands: Commands,
    impostors: Option<ResMut<Impostors>>,
    quad: Option<Res<ImpostorQuad>>,
    mut materials: ResMut<Assets<MushroomMaterial>>,
    mut scatters: Query<(Entity, &mut InstancedScatter, &mut ScatterImpostors)>,
) {
    let (Some(mut impostors), Some(quad)) = (impostors, quad) else {
        return;
    };
    for (entity, mut scatter, mut pending) in &mut scatters {
        let mut changed: Option<Vec<InstancedSlot>> = None;
        for (index, waiting) in pending.waiting.iter_mut().enumerate() {
            let Some(key) = *waiting else {
                continue;
            };
            let game = scatter.slots[index]
                .parts
                .iter()
                .find(|part| part.step == Step::Game);
            let Some(InstancedPart {
                mesh: PartMesh::Mesh(mesh),
                material,
                ..
            }) = game
            else {
                // Nothing to bake.
                *waiting = None;
                continue;
            };
            let source = (mesh.clone(), material.clone().typed::<MushroomMaterial>());
            let ready = impostors
                .request(&mut commands, key, || {
                    bake_request(key, vec![source], &mut materials)
                })
                .is_some();
            let Some(material) = impostors.material(key).filter(|_| ready).cloned() else {
                continue;
            };

            // The impostor becomes the slot's last step, and the lower mesh
            // now ends where it takes over.
            let slots = changed.get_or_insert_with(|| scatter.slots.to_vec());
            slots[index].parts.push(InstancedPart {
                step: Step::Impostor,
                mesh: PartMesh::Mesh(quad.0.clone()),
                material: material.untyped(),
            });
            slots[index].impostor = true;
            *waiting = None;
        }
        if let Some(slots) = changed {
            scatter.slots = slots.into();
        }
        if pending.waiting.iter().all(Option::is_none) {
            commands.entity(entity).remove::<ScatterImpostors>();
        }
    }
}

/// The bake request under `key` for `parts`, each a mesh in the object's own
/// coordinates with the material that draws it: bake copies of each
/// material, waiting for its textures.
fn bake_request(
    key: ImpostorKey,
    parts: Vec<(Handle<Mesh>, Handle<MushroomMaterial>)>,
    materials: &mut Assets<MushroomMaterial>,
) -> ImpostorRequest {
    let mut bake_parts = Vec::new();
    let mut wait_for: Vec<UntypedAssetId> = Vec::new();
    for (mesh, source) in parts {
        let Some(source) = materials.get(&source).cloned() else {
            continue;
        };
        let extension = &source.extension;
        wait_for.extend(
            [
                &extension.cap_color,
                &extension.cap_normal,
                &extension.cap_orh,
                &extension.stem_color,
                &extension.stem_normal,
                &extension.stem_orh,
            ]
            .into_iter()
            .flatten()
            .map(|texture| texture.id().untyped()),
        );
        let albedo = materials.add(bake_copy(&source, BakeOutput::Albedo));
        let normal = materials.add(bake_copy(&source, BakeOutput::Normal));
        bake_parts.push(BakePart::new(mesh, albedo, normal));
    }
    ImpostorRequest {
        key,
        parts: bake_parts,
        wait_for,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_bake_fits_the_largest_impostor_on_screen() {
        let bake = default_mushroom_impostor();
        assert_eq!(bake.frame_size, 256);
        let largest = DistanceChain::DEFAULT.largest_impostor_size();
        assert!(
            bake.frame_size as f32 >= largest * procgena_impostor::REFERENCE_SCREEN_HEIGHT as f32
        );
    }
}
