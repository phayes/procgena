//! Let clumps grow on your own meshes.

use std::collections::HashSet;
use std::sync::Arc;

use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use procgena_specimen::{GeneratedSpecimen, Part, Specimen, SurfaceMesh, TagSet};

/// Lets clumps grow on this entity's [`Mesh3d`], described by these tags.
///
/// The plugin gives the entity a [`GeneratedSpecimen`] with one part: the
/// mesh, tagged with these tags, in the entity's own coordinates. Spawn a
/// [`ClumpParams`](crate::ClumpParams) child to grow a clump on it, with
/// [`ClumpOptions::grows_on`](procgena_clump::ClumpOptions::grows_on)
/// matching these tags.
///
/// The mesh needs triangle-list positions and normals, and its data must
/// stay in the main world ([`RenderAssetUsages::MAIN_WORLD`](bevy::asset::RenderAssetUsages::MAIN_WORLD),
/// the default); until the mesh is readable, clumps on it wait. A mesh that
/// can't carry clumps is reported once in the log. The specimen is replaced
/// when the tags, the [`Mesh3d`] or the mesh asset change, and removed with
/// this component. Don't add it to rocks or trees from the procgena
/// plugins: they describe themselves.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct ObjectTags(pub TagSet);

/// Marks a tagged entity whose mesh couldn't be described, so the problem
/// is reported once. Removed when the tags or mesh change.
#[derive(Component)]
pub(crate) struct Unreadable;

/// Gives tagged mesh entities an up-to-date [`GeneratedSpecimen`].
#[allow(clippy::type_complexity)]
pub(crate) fn describe_tagged_meshes(
    mut commands: Commands,
    tagged: Query<(
        Entity,
        Ref<ObjectTags>,
        Ref<Mesh3d>,
        Has<GeneratedSpecimen>,
        Has<Unreadable>,
    )>,
    mut removed: RemovedComponents<ObjectTags>,
    mut events: MessageReader<AssetEvent<Mesh>>,
    meshes: Res<Assets<Mesh>>,
) {
    // Forget the specimens of entities that lost their tags.
    for entity in removed.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(GeneratedSpecimen, Unreadable)>();
        }
    }

    // Describe meshes that are new, changed, or not yet readable.
    let modified: HashSet<AssetId<Mesh>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } => Some(*id),
            _ => None,
        })
        .collect();
    for (entity, tags, mesh, described, unreadable) in &tagged {
        let changed = tags.is_changed() || mesh.is_changed() || modified.contains(&mesh.id());
        if !changed && (described || unreadable) {
            continue;
        }
        let mut entity = commands.entity(entity);
        entity.remove::<(GeneratedSpecimen, Unreadable)>();
        let Some(asset) = meshes.get(&mesh.0) else {
            continue;
        };
        match tagged_specimen(asset, &tags.0) {
            Some(specimen) => {
                entity.insert(GeneratedSpecimen(Arc::new(specimen)));
            }
            None => {
                warn!(
                    "procgena_clump_bevy: the mesh of {} can't carry clumps: it needs \
                     triangles with positions and normals",
                    entity.id()
                );
                entity.insert(Unreadable);
            }
        }
    }
}

/// Describes `mesh` as a single-part specimen tagged `tags`, or returns `None`
/// if it has no triangles, positions or normals.
fn tagged_specimen(mesh: &Mesh, tags: &TagSet) -> Option<Specimen> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    )
    else {
        return None;
    };
    let indices: Vec<u32> = match mesh.indices() {
        Some(Indices::U16(i)) => i.iter().map(|&i| u32::from(i)).collect(),
        Some(Indices::U32(i)) => i.clone(),
        // An unindexed list draws its vertices in order.
        None => (0..positions.len() as u32).collect(),
    };
    let surface = SurfaceMesh {
        positions: positions.clone(),
        normals: normals.clone(),
        indices,
    };
    let closed = surface.surface().ok()?.is_closed();
    Some(Specimen::single(vec![Part {
        closed,
        ..Part::new(tags.clone(), surface)
    }]))
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use procgena_specimen::tags;

    use super::*;

    #[test]
    fn meshes_become_one_tagged_part() {
        let mesh = Mesh::from(Cuboid::default());
        let specimen = tagged_specimen(&mesh, &[tags::ROCK].into()).unwrap();
        specimen.validate().unwrap();
        let part = &specimen.levels[0].parts[0];
        assert_eq!(part.tags, [tags::ROCK].into());
        assert_eq!(part.triangle_count(), 12);
        // Bevy's cube repeats each corner per face, but welded it is closed.
        assert!(part.closed);
    }

    #[test]
    fn meshes_without_normals_are_rejected() {
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        assert!(tagged_specimen(&mesh, &TagSet::new()).is_none());
    }
}
