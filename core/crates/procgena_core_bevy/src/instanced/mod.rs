//! One renderer for many copies of a few specimens.
//!
//! A Scatter draws through this module instead of spawning an entity per
//! copy. Put an [`InstancedScatter`] on one entity: it lists the Scatter's
//! slots (one specimen each, with its parts at every step of the shared
//! drawing chain, [`procgena_core::chain`]) and its copies (a slot, a
//! transform, a size and a wind phase each). Every camera and shadow map
//! then picks, on the GPU, the copies it sees and the step each is drawn
//! at, by its size on screen, and draws each part once for all its visible
//! copies.
//!
//! - Steps fade into each other by dithering in the main pass, the prepass
//!   and the deferred G-buffer pass, with the shared dither of
//!   [`LodPlugin`]. Shadow maps draw one step per copy, switching in the
//!   middle of each fade band, and only the mesh steps: impostors cast no
//!   shadows.
//! - Each camera measures size on screen through its own field of view, so
//!   a narrow lens keeps detail further away. Orthographic cameras use
//!   [`ViewScale::DEFAULT`](procgena_core::chain::ViewScale::DEFAULT).
//!   Shadow maps follow the camera they are drawn for.
//! - Distances follow [`LodQuality`](crate::LodQuality) as it changes.
//! - Parts are drawn with their own materials. Each material type must be
//!   registered with [`InstancedMaterialPlugin`], and its vertex shaders must
//!   read the per-copy input described there.
//! - Parts are usually Bevy [`Mesh`] assets, but a plugin that builds its
//!   meshes on the GPU can draw them too: see [`PartMesh::External`].
//! - [`SpecimenCache`] holds built parts by [`SpecimenId`] and step,
//!   so Scatters that share a specimen share its meshes. Entries no
//!   [`InstancedScatter`] uses are dropped after a short while.
//!
//! The renderer needs Bevy's render app. Without one (in headless apps with
//! no renderer), [`InstancedPlugin`] only keeps the cache.

mod chain;
mod material;
mod render;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::app::{App, Last, Plugin};
use bevy::asset::UntypedHandle;
use bevy::camera::visibility::Visibility;
use bevy::ecs::prelude::*;
use bevy::math::{Affine3A, Quat, Vec3};
use bevy::mesh::Mesh;
use bevy::prelude::{Handle, Transform};
use procgena_core::SpecimenId;
use procgena_core::chain::{DistanceChain, Step};

pub use material::{
    ATTACHED_SIZE, INSTANCE_LOCATION, INSTANCE_SIZE, INSTANCED_MESH_SHADER,
    INSTANCED_PREPASS_SHADER, InstancedMaterialPlugin, InstancedStandard, InstancedStandardKey,
    InstancedStandardMaterial, attached_buffer_layout, attached_vertex, instance_buffer_layout,
    instanced_vertex,
};
pub use render::{
    CullTimes, ExternalMesh, ExternalMeshes, InstanceCounts, InstancedSystems, InstancedTimings,
    read_instance_counts,
};

use crate::LodPlugin;

/// Adds the shared instanced renderer and the [`SpecimenCache`].
///
/// Asset plugins add it themselves; adding it more than once is harmless.
/// Register each material a part uses with [`InstancedMaterialPlugin`].
pub struct InstancedPlugin;

impl Plugin for InstancedPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LodPlugin);
        if app.world().contains_resource::<SpecimenCache>() {
            return;
        }
        crate::load_instanced_shaders(app);
        material::add_standard_material(app);
        app.init_resource::<SpecimenCache>()
            .add_systems(Last, prune_cache);
        render::build(app);
    }

    fn is_unique(&self) -> bool {
        false
    }
}

/// Draws many copies of a few specimens, relative to this entity.
///
/// Copies move with the entity's transform, are hidden with it, and are
/// drawn only by cameras that share its render layers. Replace the whole
/// component (or either list) to change what is drawn; changing it uploads
/// every copy again, so it suits Scatters that change rarely.
///
/// A copy whose slot has no parts draws nothing. A copy whose `slot` is not
/// below the number of slots is skipped.
#[derive(Component, Clone, Debug)]
#[require(Transform, Visibility)]
pub struct InstancedScatter {
    /// The specimens copies are drawn from, by slot.
    pub slots: Arc<[InstancedSlot]>,
    /// Every copy.
    pub copies: Arc<[InstancedCopy]>,
}

/// One specimen as a Scatter draws it: its parts at every step, and where
/// each step takes over.
#[derive(Clone, Debug)]
pub struct InstancedSlot {
    /// Which specimen this is. It keeps the slot's parts in the
    /// [`SpecimenCache`].
    pub specimen: SpecimenId,
    /// Which piece of the specimen the slot draws, for specimens whose
    /// pieces are placed one by one (such as the members of a mushroom
    /// cluster, each set on the ground where it stands); 0 for specimens
    /// drawn whole.
    pub piece: u32,
    /// Where each step takes over, by a copy's size on screen; usually
    /// [`DistanceChain::DEFAULT`]. Must pass [`DistanceChain::validate`];
    /// the renderer skips slots that don't.
    pub chain: DistanceChain,
    /// Whether copies stop being drawn once they shrink below the chain's
    /// [`cull_below`](DistanceChain::cull_below). Usually true. Set it to
    /// false for a slot that [`InstancedAttached`] rows draw when the host
    /// decides how far they are drawn (through
    /// [`AttachedRow::host_steps`]): its last step is then drawn at any
    /// distance its host draws it.
    pub cull: bool,
    /// Whether the slot has an impostor step. Without one, the lower mesh
    /// is drawn until the cull, so a slot can be shown while its impostor
    /// bakes; set it once the impostor's part is added.
    pub impostor: bool,
    /// Every part, at every step. A step with no parts draws nothing, such
    /// as a host whose own meshes are drawn elsewhere and whose slot only
    /// carries its rows.
    pub parts: Vec<InstancedPart>,
    /// A sphere around every part at every level, in the specimen's own
    /// coordinates, as a centre and radius. Copies are culled with it, so
    /// leave room for anything that moves vertices, such as wind.
    pub bounds: (Vec3, f32),
}

/// One mesh and material of a specimen at one step.
#[derive(Clone, Debug, PartialEq)]
pub struct InstancedPart {
    /// The step the part is drawn at.
    pub step: Step,
    /// The mesh.
    pub mesh: PartMesh,
    /// A handle to a material asset whose type was registered with
    /// [`InstancedMaterialPlugin`]. Parts with other materials are not drawn.
    pub material: UntypedHandle,
}

/// Where a part's vertices come from.
#[derive(Clone, Debug, PartialEq)]
pub enum PartMesh {
    /// A mesh asset.
    Mesh(Handle<Mesh>),
    /// A mesh another plugin keeps on the GPU, which it lists in the render
    /// world's [`ExternalMeshes`] under this id each frame. The part is
    /// drawn only while it is listed.
    External(ExternalMeshId),
}

/// Names a mesh in the render world's [`ExternalMeshes`]. The plugin that
/// builds the mesh chooses the number; different meshes need different
/// numbers, so derive it from what the mesh is built from (for example a
/// content hash of the specimen and step).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExternalMeshId(pub u64);

/// One placed copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstancedCopy {
    /// Which of the Scatter's [`slots`](InstancedScatter::slots) to draw.
    pub slot: u32,
    /// Where the copy is, relative to the [`InstancedScatter`] entity.
    pub transform: Affine3A,
    /// The copy's size for choosing steps: its bounding-sphere diameter in
    /// metres, scale included. A copy twice as large switches steps twice
    /// as far away. Must be positive.
    pub size: f32,
    /// A number from 0 to 1 that offsets the copy's wind sway, so copies of
    /// one specimen don't sway in step. Shaders read it with
    /// `copy_wind_phase` from `procgena_core_bevy::shaders::instanced`.
    pub wind_phase: f32,
}

/// Attached copies drawn on the copies of an [`InstancedScatter`] on the
/// same entity, such as the fruit a tree grows on its branches.
///
/// Each host slot (a tree specimen) has a table of rows. Every copy of that
/// slot draws every row of its table: the row's own slot (a fruit
/// specimen), placed relative to the copy, moving with it. Rows are stored
/// once per host slot, so a thousand copies of one tree cost nothing more
/// per copy than the tree itself.
///
/// Each camera and shadow map picks the rows it draws and the step each
/// is drawn at on the GPU, in two stages. First, once per host copy: copies
/// out of view, or too far for any row to be drawn, draw no rows, and in a
/// camera, copies whose rows all fall inside one step (outside its fade
/// bands) draw every row at that step without testing each one. Then, for
/// the other host copies, each row is tested against the view on its own
/// and drawn at the step its own size on screen gives, fading as copies
/// do.
///
/// A slot that rows draw is drawn only by rows: copies of it in
/// [`InstancedScatter::copies`] are skipped, and its own table, if any, is
/// ignored. Its parts' materials must be registered with
/// [`InstancedMaterialPlugin::attached`], whose pipelines read the attached
/// input described at [`attached_vertex`]. Rows whose slot is out of range
/// are skipped.
#[derive(Component, Clone, Debug)]
pub struct InstancedAttached {
    /// The rows of each host slot, by the slot's index in
    /// [`InstancedScatter::slots`]. Slots past the end, and empty tables,
    /// have no attached copies.
    pub tables: Arc<[Arc<[AttachedRow]>]>,
}

/// One attached copy, placed relative to its host copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttachedRow {
    /// Which of the [`InstancedScatter::slots`] to draw.
    pub slot: u32,
    /// Where the attached copy is, in the host copy's own coordinates.
    pub position: Vec3,
    /// How it is turned, in the host copy's own coordinates.
    pub rotation: Quat,
    /// How much it is scaled, the same along every axis.
    pub scale: f32,
    /// Its size for choosing steps: its bounding-sphere diameter in metres,
    /// `scale` included, before the host copy's own scale. Must be
    /// positive.
    pub size: f32,
    /// Three numbers a material's vertex shader reads with
    /// `attached_wind`, such as how its host's branch sways where it is
    /// attached.
    pub wind: Vec3,
    /// The host's steps at which the row is drawn, one bit per step (bit
    /// [`Step::index`]). The host's step here is the one it switches to at
    /// the middle of each fade band, as in shadow maps, so rows appear and
    /// vanish there without fading. Use it to thin rows on far hosts (keep
    /// fewer rows at the lower mesh, perhaps as enlarged duplicates), or to
    /// stop drawing rows where the host's impostor already shows them.
    /// [`AttachedRow::ALL_STEPS`] draws the row at every step;
    /// [`AttachedRow::steps`] builds a mask.
    pub host_steps: u32,
}

impl AttachedRow {
    /// A [`host_steps`](Self::host_steps) mask with every step set.
    pub const ALL_STEPS: u32 = u32::MAX;

    /// A [`host_steps`](Self::host_steps) mask with `steps` set.
    pub fn steps(steps: impl IntoIterator<Item = Step>) -> u32 {
        steps
            .into_iter()
            .fold(0, |mask, step| mask | (1 << step.index()))
    }
}

/// Built parts by specimen, piece and step, shared by every Scatter.
///
/// Look a specimen up before building it, and insert what you build. An
/// entry stays while any [`InstancedScatter`] has a slot with its specimen,
/// and is dropped about a second after the last one stops using it. A
/// specimen's parts must not depend on anything but its id, piece and step
/// (an asset plugin's settings count as fixed for the app's run).
#[derive(Resource, Debug, Default)]
pub struct SpecimenCache {
    entries: HashMap<(SpecimenId, u32, Step), CacheEntry>,
}

#[derive(Debug)]
struct CacheEntry {
    parts: Arc<[InstancedPart]>,
    /// Frames in a row no Scatter has used the entry.
    unused_frames: u32,
}

/// Frames an unused cache entry is kept, so a Scatter that is rebuilt
/// finds its old specimens.
const PRUNE_FRAMES: u32 = 60;

impl SpecimenCache {
    /// The parts of piece `piece` of `specimen` at `step`, if cached. Use
    /// piece 0 for specimens drawn whole.
    pub fn get(
        &self,
        specimen: SpecimenId,
        piece: u32,
        step: Step,
    ) -> Option<Arc<[InstancedPart]>> {
        self.entries
            .get(&(specimen, piece, step))
            .map(|entry| entry.parts.clone())
    }

    /// Caches `parts` as the parts of piece `piece` of `specimen` at
    /// `step`, replacing any.
    pub fn insert(
        &mut self,
        specimen: SpecimenId,
        piece: u32,
        step: Step,
        parts: Vec<InstancedPart>,
    ) {
        self.entries.insert(
            (specimen, piece, step),
            CacheEntry {
                parts: parts.into(),
                unused_frames: 0,
            },
        );
    }

    /// The parts of piece `piece` of `specimen` at every one of `steps`, if
    /// all are cached.
    pub fn get_steps(
        &self,
        specimen: SpecimenId,
        piece: u32,
        steps: impl IntoIterator<Item = Step>,
    ) -> Option<Vec<InstancedPart>> {
        let mut parts = Vec::new();
        for step in steps {
            parts.extend(self.get(specimen, piece, step)?.iter().cloned());
        }
        Some(parts)
    }

    /// Number of cached (specimen, piece, step) entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Drops cache entries no Scatter has used for [`PRUNE_FRAMES`] frames.
fn prune_cache(scatters: Query<&InstancedScatter>, mut cache: ResMut<SpecimenCache>) {
    if cache.entries.is_empty() {
        return;
    }
    let used: std::collections::HashSet<SpecimenId> = scatters
        .iter()
        .flat_map(|scatter| scatter.slots.iter().map(|slot| slot.specimen))
        .collect();
    cache.entries.retain(|(specimen, _, _), entry| {
        if used.contains(specimen) {
            entry.unused_frames = 0;
        } else {
            entry.unused_frames += 1;
        }
        entry.unused_frames <= PRUNE_FRAMES
    });
}

#[cfg(test)]
mod tests;
