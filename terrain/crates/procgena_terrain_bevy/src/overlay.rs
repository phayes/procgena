//! Images drawn over terrain chunks and other ground with a terrain
//! material, for debugging and editing tools.

use bevy::prelude::*;

use crate::TerrainMaterial;

/// An image drawn over the ground of this entity's [`TerrainMaterial`],
/// such as a terrain chunk or an Ecotope's ground patch, mixed over the
/// ground's own colour.
///
/// The image is stretched over the mesh's UVs as the ground weights are:
/// for a chunk, its first texel's centre lies on the chunk's smallest
/// corner and its last texel's on the largest. Its alpha scales
/// `strength`. Remove the component to draw the ground alone again.
///
/// It changes the entity's material asset, so give entities that share a
/// material the same overlay. Terrain chunks each have their own.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TerrainOverlay {
    /// The image, sRGB or linear.
    pub image: Handle<Image>,
    /// How strongly it covers the ground's colour, from 0 to 1.
    pub strength: f32,
}

/// Entities whose overlay or material changed.
type OverlayChanged = Or<(
    Changed<TerrainOverlay>,
    Changed<MeshMaterial3d<TerrainMaterial>>,
)>;

/// Copies each entity's [`TerrainOverlay`] into its material, and clears
/// the overlay from materials whose entity no longer has one.
pub(crate) fn apply_overlays(
    changed: Query<(&MeshMaterial3d<TerrainMaterial>, Option<&TerrainOverlay>), OverlayChanged>,
    mut removed: RemovedComponents<TerrainOverlay>,
    bare: Query<&MeshMaterial3d<TerrainMaterial>, Without<TerrainOverlay>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    // Only materials whose overlay differs are touched, so the others are
    // not sent to the GPU again.
    let mut set = |handle: &Handle<TerrainMaterial>, overlay: Option<&TerrainOverlay>| {
        let (image, strength) =
            overlay.map_or((Handle::default(), 0.0), |o| (o.image.clone(), o.strength));
        let same = materials.get(handle).is_none_or(|m| {
            m.extension.overlay == image && m.extension.surface.overlay == strength
        });
        if !same && let Some(mut material) = materials.get_mut(handle) {
            material.extension.overlay = image;
            material.extension.surface.overlay = strength;
        }
    };
    for (material, overlay) in &changed {
        set(&material.0, overlay);
    }
    for entity in removed.read() {
        if let Ok(material) = bare.get(entity) {
            set(&material.0, None);
        }
    }
}
