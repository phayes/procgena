//! The CPU's whole steady-state job: describing the fields.
//!
//! When a field, its plants, its mask or its heightmap changes, the field
//! table, the chunk table and the packed masks and heights are rebuilt and
//! handed to the render world. Nothing here runs per blade or per frame; the
//! GPU decides which blades exist (`shaders/generate.wesl`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::app::{App, PostUpdate};
use bevy::asset::{Asset, AssetEvent, AssetEventSystems, AssetId, Assets};
use bevy::camera::visibility::{InheritedVisibility, RenderLayers, VisibilitySystems};
use bevy::color::{ColorToComponents, LinearRgba};
use bevy::ecs::message::MessageReader;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::log::warn;
use bevy::math::{EulerRot, Quat, UVec2, UVec4, Vec2, Vec3, Vec4};
use bevy::platform::time::Instant;
use bevy::transform::TransformSystems;
use bevy::transform::components::GlobalTransform;

use crate::api::{
    FieldPlant, GroundTint, PlantArrangement, PlantField, PlantGround, PlantParams, PlantVariation,
};
use crate::edit::{self, EditLayers};
use crate::gpu_layout::{ChunkGpu, FieldGpu, LevelGpu, MAX_BRANCHED_CLASSES, NO_CLASS, NO_MAP};
use crate::ground::{HOLE, MeshGrounds, bake_mesh_grounds};
use crate::look::{MAX_BRANCHES, MAX_PLANTS, MAX_STEMS_PER_PLANT};
use crate::packed::{self, PackedImages};
use crate::shader_model::{self, ChunkLayout, ShapeClass, drawn_children};
use crate::stats::TableStats;
use crate::{PlantStats, PlantSystems};

/// Side of a chunk, in metres.
///
/// One size for every view, because the chunk table is shared. T2 in the plan
/// suggested 8 m chunks for orthographic views; per-blade frustum culling
/// rejects what a larger chunk lets through, so this stays 16 m until a
/// measurement says otherwise.
pub(crate) const CHUNK_SIZE: f32 = 16.0;

/// Entry limit imposed by each instance's 8-bit field index.
/// Each plant and each enabled accent uses one entry.
const MAX_ENTRIES: usize = 256;

impl FieldGpu {
    /// The field's X axis in world XZ. An entry built by hand, with no axis,
    /// is unturned.
    pub(crate) fn x_axis(&self) -> Vec2 {
        let axis = Vec2::new(self.axis.x, self.axis.y);
        if axis == Vec2::ZERO { Vec2::X } else { axis }
    }

    /// A point of the field, in metres along its axes from its minimum
    /// corner, in world XZ. Twin of `field_to_world` in `generate.wesl`.
    /// Only the debug gizmos and the tests draw from the GPU table.
    #[cfg(any(test, feature = "debug"))]
    pub(crate) fn world_of(&self, local: Vec2) -> Vec2 {
        Vec2::new(self.min_corner.x, self.min_corner.z) + turn(self.x_axis(), local)
    }

    /// The point of the field at `point` (world XZ), in metres along its axes
    /// from its minimum corner.
    pub(crate) fn local_of(&self, point: Vec2) -> Vec2 {
        let axis = self.x_axis();
        let offset = point - Vec2::new(self.min_corner.x, self.min_corner.z);
        Vec2::new(offset.dot(axis), offset.dot(axis.perp()))
    }
}

/// `v`, given along the axes of a field whose X axis is `axis`, in world XZ.
/// Unturned (`axis` = X) it is `v` exactly.
fn turn(axis: Vec2, v: Vec2) -> Vec2 {
    axis * v.x + axis.perp() * v.y
}

/// Everything the GPU needs to know about the fields. Replaced, never edited,
/// so the render world can keep the previous version while a new one builds.
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct FieldTables {
    /// Bumped on every change; the render world uploads when it changes.
    pub version: u64,
    /// Bumped when the chunks, masks or heightmaps change, which is most of
    /// the upload. A field only shown or hidden bumps `version` alone.
    pub structure_version: u64,
    pub fields: Arc<Vec<FieldGpu>>,
    /// Where each field's entries are in `fields`, and what they were built
    /// from.
    pub slots: Arc<Vec<FieldSlot>>,
    /// What each mask in `masks` is, in order.
    pub mask_ids: Arc<Vec<MaskKey>>,
    /// The image of each heightmap in `heights`, in order.
    pub height_ids: Arc<Vec<AssetId<Image>>>,
    pub chunks: Arc<Vec<ChunkGpu>>,
    /// Every mask, two R8G8 texels per word, low half first.
    pub masks: Arc<Vec<u32>>,
    /// Every heightmap, one normalized height per texel.
    pub heights: Arc<Vec<f32>>,
    /// The optional shading terms some field uses (`LookFeatures`).
    pub features: LookFeatures,
    /// Sorted branched mesh classes. Entry `i` corresponds to class `i + 1` in
    /// `FieldGpu::access.y`; sorting makes class assignment deterministic.
    pub shape_classes: Arc<Vec<ShapeClass>>,
}

impl FieldTables {
    /// The highest ground under `point` (world X and Z) in the chunk of the
    /// field that holds it, or `None` outside every field. A flat field is
    /// exact; on a heightmap it is the top of the 16 m chunk.
    pub(crate) fn highest_ground_at(&self, point: Vec2) -> Option<f32> {
        let mut first_chunk = 0usize;
        for slot in self.slots.iter() {
            let field = self.fields.get(slot.first)?;
            let size = Vec2::new(field.extent.x, field.extent.y);
            let counts = (size / CHUNK_SIZE).ceil().as_uvec2().max(UVec2::ONE);
            let local = field.local_of(point);
            if local.cmpge(Vec2::ZERO).all() && local.cmplt(size).all() {
                let cell = (local / CHUNK_SIZE).as_uvec2().min(counts - UVec2::ONE);
                let index = first_chunk + (cell.y * counts.x + cell.x) as usize;
                return self.chunks.get(index).map(|chunk| chunk.bounds.w);
            }
            first_chunk += (counts.x * counts.y) as usize;
        }
        None
    }
}

impl FieldTables {
    /// The ground below `point` (world X and Z) as [`highest_ground_at`]
    /// finds it, or, outside every field, the first field's height, or 0.
    ///
    /// [`highest_ground_at`]: FieldTables::highest_ground_at
    pub(crate) fn ground_below(&self, point: Vec2) -> f32 {
        self.highest_ground_at(point)
            .or_else(|| self.fields.first().map(|field| field.min_corner.y))
            .unwrap_or(0.0)
    }
}

/// What one mask in the tables is. An image keeps its key when it is edited,
/// so a field's first edit does not move anything in the tables; a field
/// without an image gets a mask of its own on its first edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MaskKey {
    /// The field's density map, edited or not.
    Image(AssetId<Image>),
    /// The edit layer of a field that has no density map.
    Edited(Entity),
}

/// The entries of one field in [`FieldTables::fields`], and what they were
/// built from, so a rebuild can tell what has to be built again.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldSlot {
    pub entity: Entity,
    pub first: usize,
    pub entries: usize,
    entry: EntryKey,
    chunk: ChunkKey,
}

/// Everything a field's entries depend on besides the contents of its images:
/// while this is the same, so are the entries.
#[derive(Debug, Clone, PartialEq)]
struct EntryKey {
    look: FieldLook,
    size: Vec2,
    corner: Vec3,
    axis: Vec2,
    access: u32,
    /// The mask, and its offset in the mask table, in texels.
    mask: Option<(MaskKey, u32)>,
    /// The heightmap, its offset in the height table and its scale.
    height: Option<(AssetId<Image>, u32, f32)>,
}

/// Snapshot of the field settings used to build its entries.
///
/// Placement is tracked separately. Comparing this snapshot with the next
/// field value avoids rebuilding entries when their inputs are unchanged.
#[derive(Debug, Clone, PartialEq)]
struct FieldLook {
    /// The plants that fit, at most [`MAX_PLANTS`].
    plants: Vec<FieldPlant>,
    /// The field-wide multiplier applied to every plant entry's density.
    density_scale: f32,
    variation: PlantVariation,
    ground_tint: GroundTint,
    toon_bands: u32,
    wind_scale: f32,
}

impl FieldLook {
    fn of(field: &PlantField, plants: &[FieldPlant]) -> Self {
        Self {
            plants: plants.to_vec(),
            density_scale: field.density_scale,
            variation: field.variation,
            ground_tint: field.ground_tint,
            toon_bands: field.toon_bands,
            wind_scale: field.wind_scale,
        }
    }
}

/// Everything a field's chunks depend on besides the contents of its
/// heightmap.
#[derive(Debug, Clone, PartialEq)]
struct ChunkKey {
    size: Vec2,
    base: f32,
    height: Option<(AssetId<Image>, f32)>,
    /// The index of the field's first entry, which its chunks point at.
    first: usize,
}

/// Optional shading terms, compiled into the grass shaders only when some
/// field uses them, so the others cost nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LookFeatures(u8);

impl LookFeatures {
    pub(crate) const GROUND_TINT: Self = Self(1);
    pub(crate) const SHEEN: Self = Self(2);
    pub(crate) const TOON: Self = Self(4);
    /// Segment shape features: twist, droop, fold, profile and heads.
    pub(crate) const SHAPE: Self = Self(8);
    /// Colour variation: patches, dry tips and dead blades.
    pub(crate) const VARIATION: Self = Self(16);

    /// The terms `field` uses for one of its plants, `look`.
    pub(crate) fn of(field: &PlantField, look: &PlantParams) -> Self {
        let mut features = Self::default();
        if field.ground_tint.amount > 0.0 {
            features.0 |= Self::GROUND_TINT.0;
        }
        if look.specular > 0.0 {
            features.0 |= Self::SHEEN.0;
        }
        if field.toon_bands > 0 {
            features.0 |= Self::TOON.0;
        }
        let segment = &look.segment;
        if segment.twist > 0.0
            || segment.droop > 0.0
            || segment.fold > 0.0
            || segment.profile > 0.0
            || look.head_length > 0.0
        {
            features.0 |= Self::SHAPE.0;
        }
        let variation = &field.variation;
        if variation.patch_amount > 0.0 || variation.dry_tips > 0.0 || variation.dead_share > 0.0 {
            features.0 |= Self::VARIATION.0;
        }
        features
    }

    /// The terms that change where blades are, which is all a shadow map
    /// needs.
    pub(crate) fn geometry(self) -> Self {
        Self(self.0 & Self::SHAPE.0)
    }

    pub(crate) fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Every optional look term with the shader definition that compiles it
    /// in: the one list `shader_defs` and the tests both read, so a term
    /// cannot be given a definition in one place and not the other.
    pub(crate) const TERMS: [(Self, &'static str); 5] = [
        (Self::GROUND_TINT, "PLANT_GROUND_TINT"),
        (Self::SHEEN, "PLANT_SHEEN"),
        (Self::TOON, "PLANT_TOON"),
        (Self::SHAPE, "PLANT_SHAPE"),
        (Self::VARIATION, "PLANT_VARIATION"),
    ];

    /// The shader definitions that compile the terms in.
    pub(crate) fn shader_defs(self) -> impl Iterator<Item = &'static str> {
        Self::TERMS
            .into_iter()
            .filter(move |(feature, _)| self.contains(*feature))
            .map(|(_, def)| def)
    }
}

impl std::ops::BitOr for LookFeatures {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Registers the tables and the systems that keep them current, in
/// [`PlantSystems`] order.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<FieldTables>()
        .init_resource::<TablesDirty>()
        .init_resource::<MeshGrounds>()
        .init_resource::<PlantStats>()
        .init_resource::<PackedImages>()
        .configure_sets(
            PostUpdate,
            (
                PlantSystems::DetectChanges,
                PlantSystems::Edit,
                PlantSystems::Rebuild,
            )
                .chain()
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate)
                .after(AssetEventSystems),
        )
        .add_systems(
            PostUpdate,
            (
                (
                    warn_unsupported,
                    (bake_mesh_grounds, detect_changes).chain(),
                )
                    .in_set(PlantSystems::DetectChanges),
                (rebuild, edit::collect_patches)
                    .chain()
                    .in_set(PlantSystems::Rebuild),
            ),
        );
    edit::plugin(app);
}

/// What the tables need.
#[derive(Resource, Debug, Default)]
pub(crate) struct TablesDirty {
    /// A field was added, removed, changed or moved.
    fields: bool,
    /// Masks and heightmaps in use that changed.
    images: HashSet<AssetId<Image>>,
    /// Which fields are drawn changed, which only the entries show.
    access: bool,
}

impl TablesDirty {
    /// Marks the fields as changed: a field's ground was baked, say.
    pub(crate) fn mark_fields(&mut self) {
        self.fields = true;
    }

    /// Whether the tables have to be looked at again, not merely patched.
    fn rebuild(&self) -> bool {
        self.fields || !self.images.is_empty()
    }
}

/// A field whose settings or position changed. `With<PlantField>` matters:
/// without it, every moving camera or character would rebuild the tables.
type FieldChanged = (
    With<PlantField>,
    Or<(Changed<PlantField>, Changed<GlobalTransform>)>,
);

/// A field shown, hidden, or moved to other render layers.
type AccessChanged = (
    With<PlantField>,
    Or<(Changed<InheritedVisibility>, Changed<RenderLayers>)>,
);

/// The number of render layers a view or field can name.
const LAYER_BITS: usize = 32;

/// `layers` as the bit mask the shaders test; a missing component is layer 0,
/// as for meshes. Layers beyond 31 do not fit and are left out.
pub(crate) fn layer_mask(layers: Option<&RenderLayers>) -> u32 {
    layers.map_or(1, |layers| {
        layers
            .iter()
            .filter(|&layer| layer < LAYER_BITS)
            .fold(0, |mask, layer| mask | 1 << layer)
    })
}

/// Whether `layers` names a layer the shaders cannot test.
pub(crate) fn has_unsupported_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_some_and(|layers| layers.iter().any(|layer| layer >= LAYER_BITS))
}

/// What a field's `access` word holds: its layers, or nothing while hidden.
fn access(visible: bool, layers: Option<&RenderLayers>) -> u32 {
    if visible { layer_mask(layers) } else { 0 }
}

/// A field's size on the ground: `size` times the transform's X and Z scale.
fn scaled_size(field: &PlantField, transform: &GlobalTransform) -> Vec2 {
    let scale = transform.scale();
    field.size * Vec2::new(scale.x, scale.z).abs()
}

/// Where a field lies on the ground, worked out from its components exactly
/// as the tables work it out, for code that cannot wait for a rebuild.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FieldFrame {
    /// The minimum corner (the field's own −X, −Z corner), in world XZ.
    pub corner: Vec2,
    /// The field's X axis in world XZ.
    pub axis: Vec2,
    /// Its size on the ground, scale applied.
    pub size: Vec2,
}

impl FieldFrame {
    pub(crate) fn of(field: &PlantField, transform: &GlobalTransform) -> Self {
        let size = scaled_size(field, transform);
        let axis = field_axis(transform);
        let origin = transform.translation();
        Self {
            corner: Vec2::new(origin.x, origin.z) + turn(axis, -size * 0.5),
            axis,
            size,
        }
    }

    /// A point of the field, in metres along its axes from its minimum
    /// corner, in world XZ.
    pub(crate) fn world_of(&self, local: Vec2) -> Vec2 {
        self.corner + turn(self.axis, local)
    }

    /// The point of the field at `point` (world XZ), in metres along its axes
    /// from its minimum corner.
    pub(crate) fn local_of(&self, point: Vec2) -> Vec2 {
        let offset = point - self.corner;
        Vec2::new(offset.dot(self.axis), offset.dot(self.axis.perp()))
    }
}

/// A field's X axis in world XZ: its transform's turn about Y. Grass grows
/// upright, so a tilt is left out.
fn field_axis(transform: &GlobalTransform) -> Vec2 {
    let (yaw, _, _) = transform.rotation().to_euler(EulerRot::YXZ);
    let (sin, cos) = yaw.sin_cos();
    // An unturned field gets exactly X, so it is placed exactly as before.
    if sin == 0.0 && cos > 0.0 {
        Vec2::X
    } else {
        Vec2::new(cos, -sin)
    }
}

/// Whether a transform tilts the field out of the XZ plane, which a field
/// does not follow.
fn is_tilted(transform: &GlobalTransform) -> bool {
    let rotation = transform.rotation();
    let (yaw, _, _) = rotation.to_euler(EulerRot::YXZ);
    rotation.angle_between(Quat::from_rotation_y(yaw)) > 1e-3
}

/// A field whose transform or layers changed, for `warn_unsupported`.
type TransformOrLayersChanged = (
    With<PlantField>,
    Or<(Changed<GlobalTransform>, Changed<RenderLayers>)>,
);

/// True the first time `unsupported` holds for `entity`, so each field is
/// warned about once; a field that fixes the problem and breaks it again is
/// warned about again.
fn first_time(seen: &mut HashSet<Entity>, entity: Entity, unsupported: bool) -> bool {
    if unsupported {
        seen.insert(entity)
    } else {
        seen.remove(&entity);
        false
    }
}

/// Warns, once per field, about the things a field's entity can ask for that
/// are not supported.
pub(crate) fn warn_unsupported(
    fields: Query<(Entity, &GlobalTransform, Option<&RenderLayers>), TransformOrLayersChanged>,
    mut tilted: Local<HashSet<Entity>>,
    mut layered: Local<HashSet<Entity>>,
) {
    for (entity, transform, layers) in &fields {
        if first_time(&mut tilted, entity, is_tilted(transform)) {
            warn!(
                "procgena_plant: the PlantField {entity} is tilted; grass grows upright, so only \
                 its turn about Y is used. For a slope, use PlantGround::Heightmap or \
                 PlantGround::Mesh."
            );
        }
        if first_time(&mut layered, entity, has_unsupported_layer(layers)) {
            warn!(
                "procgena_plant: the PlantField {entity} is on a render layer above 31; grass \
                 supports layers 0 to 31, so the others are ignored."
            );
        }
    }
}

/// The asset an event is about.
fn event_id<A: Asset>(event: &AssetEvent<A>) -> AssetId<A> {
    match event {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::LoadedWithDependencies { id }
        | AssetEvent::Removed { id }
        | AssetEvent::Unused { id } => *id,
    }
}

/// Marks the tables for a rebuild when anything they depend on changes.
pub(crate) fn detect_changes(
    mut dirty: ResMut<TablesDirty>,
    changed: Query<(), FieldChanged>,
    access_changed: Query<(), AccessChanged>,
    mut removed: RemovedComponents<PlantField>,
    mut images: MessageReader<AssetEvent<Image>>,
    fields: Query<&PlantField>,
) {
    // Only images some field uses matter.
    for event in images.read() {
        let id = event_id(event);
        let used = fields.iter().any(|field| {
            field
                .density_map
                .as_ref()
                .is_some_and(|mask| mask.id() == id)
                || matches!(&field.ground, PlantGround::Heightmap { image, .. } if image.id() == id)
        });
        if used {
            dirty.images.insert(id);
        }
    }
    if !changed.is_empty() || removed.read().count() > 0 {
        dirty.fields = true;
    }
    if !access_changed.is_empty() {
        dirty.access = true;
    }
}

/// What `rebuild` reads of each field.
type FieldInputs<'a> = (
    Entity,
    &'a PlantField,
    &'a GlobalTransform,
    &'a InheritedVisibility,
    Option<&'a RenderLayers>,
);

/// Where a field is and which views draw it.
struct Placement {
    size: Vec2,
    /// The minimum corner (the field's own −X, −Z corner), at the field's
    /// base height.
    corner: Vec3,
    /// The field's X axis in world XZ.
    axis: Vec2,
    access: u32,
}

impl Placement {
    fn of(
        field: &PlantField,
        transform: &GlobalTransform,
        visibility: InheritedVisibility,
        layers: Option<&RenderLayers>,
    ) -> Self {
        let frame = FieldFrame::of(field, transform);
        Self {
            size: frame.size,
            corner: Vec3::new(frame.corner.x, transform.translation().y, frame.corner.y),
            axis: frame.axis,
            access: access(visibility.get(), layers),
        }
    }
}

/// Shows or hides the fields already in the tables, without touching their
/// chunks or images.
fn patch_access(tables: &mut FieldTables, fields: &Query<FieldInputs>) {
    let mut entries = tables.fields.as_ref().clone();
    let mut slots = tables.slots.as_ref().clone();
    for slot in &mut slots {
        let Ok((_, _, _, visibility, layers)) = fields.get(slot.entity) else {
            continue;
        };
        let bits = access(visibility.get(), layers);
        slot.entry.access = bits;
        for entry in &mut entries[slot.first..slot.first + slot.entries] {
            entry.access.x = bits;
        }
    }
    tables.fields = Arc::new(entries);
    tables.slots = Arc::new(slots);
    tables.version += 1;
}

/// What `rebuild` reads of each field, once its images have loaded.
struct Input<'a> {
    entity: Entity,
    placement: Placement,
    field: &'a PlantField,
    mask: Option<MaskInput<'a>>,
    height: Option<(AssetId<Image>, &'a Image, f32)>,
}

/// A field's mask: its image, its edit layer, or both, in which case the
/// layer's texels are the ones drawn.
struct MaskInput<'a> {
    key: MaskKey,
    size: UVec2,
    image: Option<&'a Image>,
    edited: Option<&'a [u32]>,
}

impl MaskInput<'_> {
    /// The image the mask was read from, if any.
    fn image_id(&self) -> Option<AssetId<Image>> {
        match self.key {
            MaskKey::Image(id) => Some(id),
            MaskKey::Edited(_) => None,
        }
    }

    /// The texels, packed (see `PackedImages::mask`).
    fn words(&self, cache: &mut PackedImages) -> Arc<Vec<u32>> {
        match (self.edited, self.image, self.key) {
            (Some(words), ..) => Arc::new(words.to_vec()),
            (None, Some(image), MaskKey::Image(id)) => cache.mask(id, image),
            _ => Arc::default(),
        }
    }
}

/// An input placed in the tables: which entries it gets, and where its images
/// sit in the mask and height tables.
struct Plan<'a> {
    input: &'a Input<'a>,
    /// The field's plants that fit, one entry each, then one more for each
    /// plant with an accent.
    plants: &'a [FieldPlant],
    /// The index of the field's first entry.
    first: usize,
    /// Offsets of its mask and heightmap, in texels.
    mask_offset: u32,
    height_offset: u32,
}

impl Plan<'_> {
    /// How many entries the field takes: its plants, then their accents.
    fn entry_count(&self) -> usize {
        entry_count(self.plants)
    }

    fn entry_key(&self) -> EntryKey {
        let input = self.input;
        EntryKey {
            look: FieldLook::of(input.field, self.plants),
            size: input.placement.size,
            corner: input.placement.corner,
            axis: input.placement.axis,
            access: input.placement.access,
            mask: input.mask.as_ref().map(|mask| (mask.key, self.mask_offset)),
            height: input
                .height
                .map(|(id, _, scale)| (id, self.height_offset, scale)),
        }
    }

    fn chunk_key(&self) -> ChunkKey {
        ChunkKey {
            size: self.input.placement.size,
            base: self.input.placement.corner.y,
            height: self.input.height.map(|(id, _, scale)| (id, scale)),
            first: self.first,
        }
    }

    /// Whether the field's heightmap is one of `changed`.
    fn uses_height(&self, changed: &HashSet<AssetId<Image>>) -> bool {
        self.input
            .height
            .is_some_and(|(id, ..)| changed.contains(&id))
    }

    /// Whether the field's mask or heightmap is one of `changed`.
    fn uses(&self, changed: &HashSet<AssetId<Image>>) -> bool {
        let mask = self.input.mask.as_ref().and_then(MaskInput::image_id);
        mask.is_some_and(|id| changed.contains(&id))
            || self
                .input
                .height
                .is_some_and(|(id, ..)| changed.contains(&id))
    }
}

/// Reads each field with its mask and heightmap, or `None` while any of
/// them has not loaded (or a mesh ground has not been baked), so a field
/// never appears at the wrong density or height and then jumps. An edited
/// field's mask is its edit layer.
fn gather<'a>(
    sorted: &[FieldInputs<'a>],
    images: &'a Assets<Image>,
    grounds: &MeshGrounds,
    edits: &'a EditLayers,
) -> Option<Vec<Input<'a>>> {
    let mut inputs = Vec::with_capacity(sorted.len());
    for (entity, field, transform, visibility, layers) in sorted {
        let layer = edits.layer_of(*entity, field);
        let mask = match &field.density_map {
            Some(handle) => {
                let image = images.get(handle)?;
                Some(MaskInput {
                    key: MaskKey::Image(handle.id()),
                    size: image.size(),
                    image: Some(image),
                    edited: layer
                        .filter(|layer| layer.size == image.size())
                        .map(|layer| layer.words.as_slice()),
                })
            }
            None => layer.map(|layer| MaskInput {
                key: MaskKey::Edited(*entity),
                size: layer.size,
                image: None,
                edited: Some(layer.words.as_slice()),
            }),
        };
        let height = match &field.ground {
            PlantGround::Flat => None,
            PlantGround::Heightmap {
                image,
                height_scale,
            } => Some((image.id(), images.get(image)?, *height_scale)),
            // A baked ground is in the mesh's units, which the transform's Y
            // scale turns into metres, as it does for the mesh itself.
            PlantGround::Mesh { .. } => {
                let image = grounds.image(*entity)?;
                Some((image.id(), images.get(image)?, transform.scale().y))
            }
        };
        inputs.push(Input {
            entity: *entity,
            placement: Placement::of(field, transform, **visibility, *layers),
            field,
            mask,
            height,
        });
    }
    Some(inputs)
}

/// What the limits left out.
#[derive(Debug, Default, Clone, Copy)]
struct Dropped {
    fields: usize,
    plants: usize,
}

/// Assigns table entries and image offsets to fields that can be drawn.
///
/// Each plant and enabled accent uses one of the 256 entries. A field that
/// does not fit, and all fields after it, are omitted. Fields with no positive
/// density among their first [`MAX_PLANTS`] plants need no entries or chunks.
fn plan<'a>(inputs: &'a [Input<'a>]) -> (Vec<Plan<'a>>, Dropped) {
    let mut plans = Vec::with_capacity(inputs.len());
    let mut dropped = Dropped::default();
    let (mut entry_cursor, mut mask_words, mut height_texels) = (0usize, 0u32, 0u32);
    for (index, input) in inputs.iter().enumerate() {
        // Apply the per-field plant limit before checking whether anything grows.
        let all = &input.field.plants;
        let fitting = &all[..all.len().min(MAX_PLANTS)];
        dropped.plants += all.len() - fitting.len();
        if fitting.len() < all.len() {
            warn!(
                "procgena_plant: a PlantField grows at most {MAX_PLANTS} plants; {} of the {} \
                 plants of {} are drawn",
                fitting.len(),
                all.len(),
                input.entity
            );
        }
        if !fitting.iter().any(|plant| plant.density() > 0.0) {
            continue;
        }
        let entries = entry_count(fitting);
        if entry_cursor + entries > MAX_ENTRIES {
            warn!(
                "procgena_plant draws at most {MAX_ENTRIES} fields, each plant of a field, \
                 and each plant's accent, counting as one; some fields are not drawn"
            );
            dropped.fields = inputs.len() - index;
            break;
        }
        plans.push(Plan {
            input,
            plants: fitting,
            first: entry_cursor,
            mask_offset: mask_words * 2,
            height_offset: height_texels,
        });
        entry_cursor += entries;
        if let Some(mask) = &input.mask {
            mask_words += packed::mask_words(mask.size.x * mask.size.y);
        }
        if let Some((_, image, _)) = input.height {
            let size = image.size();
            height_texels += size.x * size.y;
        }
    }
    (plans, dropped)
}

/// The masks and heightmaps of the tables, kept while the same images are used
/// in the same order and none changed, and otherwise joined from the packed
/// images, each read at most once (see `PackedImages`), and the edit layers.
struct Images {
    masks: Arc<Vec<u32>>,
    heights: Arc<Vec<f32>>,
    mask_ids: Arc<Vec<MaskKey>>,
    height_ids: Arc<Vec<AssetId<Image>>>,
    masks_same: bool,
    heights_same: bool,
}

/// `edits_ahead` says the edit layers changed since the masks were last
/// joined. Edits reach the GPU as patches, so stale masks are kept while
/// nothing else is uploaded; but if the chunks or heightmaps are uploaded
/// (`chunks_same` false), the masks go with them and are joined afresh.
fn images_for(
    plans: &[Plan],
    old: &FieldTables,
    changed: &HashSet<AssetId<Image>>,
    cache: &mut PackedImages,
    (chunks_same, edits_ahead): (bool, bool),
) -> Images {
    let mask_ids: Vec<_> = plans
        .iter()
        .filter_map(|plan| plan.input.mask.as_ref().map(|mask| mask.key))
        .collect();
    let height_ids: Vec<_> = plans
        .iter()
        .filter_map(|plan| plan.input.height.map(|(id, ..)| id))
        .collect();
    let untouched = |ids: &[AssetId<Image>]| !ids.iter().any(|id| changed.contains(id));
    let mask_images: Vec<_> = plans
        .iter()
        .filter_map(|plan| plan.input.mask.as_ref().and_then(MaskInput::image_id))
        .collect();
    let heights_same = *old.height_ids == height_ids && untouched(&height_ids);
    let refresh = edits_ahead && !(chunks_same && heights_same);
    let masks_same = *old.mask_ids == mask_ids && untouched(&mask_images) && !refresh;

    let masks = if masks_same {
        old.masks.clone()
    } else {
        let parts: Vec<_> = plans
            .iter()
            .filter_map(|plan| plan.input.mask.as_ref())
            .map(|mask| mask.words(cache))
            .collect();
        packed::join(&parts)
    };
    let heights = if heights_same {
        old.heights.clone()
    } else {
        let parts: Vec<_> = plans
            .iter()
            .filter_map(|plan| plan.input.height)
            .map(|(id, image, _)| cache.heights(id, image))
            .collect();
        packed::join(&parts)
    };
    cache.retain(&mask_images, &height_ids);
    Images {
        masks,
        heights,
        mask_ids: if masks_same {
            old.mask_ids.clone()
        } else {
            Arc::new(mask_ids)
        },
        height_ids: if heights_same {
            old.height_ids.clone()
        } else {
            Arc::new(height_ids)
        },
        masks_same,
        heights_same,
    }
}

/// The chunks, kept (and the second value true) while no field resized, moved
/// up or down, changed heightmap or shifted in the tables.
fn chunks_for(
    plans: &[Plan],
    old: &FieldTables,
    changed: &HashSet<AssetId<Image>>,
    cache: &mut PackedImages,
) -> (Arc<Vec<ChunkGpu>>, bool) {
    let same =
        old.slots.len() == plans.len()
            && old.slots.iter().zip(plans).all(|(slot, plan)| {
                slot.entity == plan.input.entity && slot.chunk == plan.chunk_key()
            })
            && !plans.iter().any(|plan| plan.uses_height(changed));
    if same {
        return (old.chunks.clone(), true);
    }
    let mut chunks = Vec::new();
    for plan in plans {
        let heights = plan
            .input
            .height
            .map(|(id, image, _)| cache.heights(id, image));
        field_chunks(plan, heights.as_ref().map(|h| h.as_slice()), &mut chunks);
    }
    (Arc::new(chunks), false)
}

/// The field table, with the slots that say what each entry was built from.
struct Built {
    fields: Vec<FieldGpu>,
    slots: Vec<FieldSlot>,
    features: LookFeatures,
    /// Entries worked out again, not kept.
    rewritten: u64,
}

/// Reuses each field's entries while its appearance, placement and images match.
/// Rebuilds entries only for fields whose inputs changed.
fn entries_for(
    plans: &[Plan],
    old: &FieldTables,
    changed_images: &HashSet<AssetId<Image>>,
) -> Built {
    let previous: HashMap<Entity, &FieldSlot> =
        old.slots.iter().map(|slot| (slot.entity, slot)).collect();
    let mut built = Built {
        fields: Vec::with_capacity(old.fields.len()),
        slots: Vec::with_capacity(plans.len()),
        features: LookFeatures::default(),
        rewritten: 0,
    };
    for plan in plans {
        let (entry, chunk) = (plan.entry_key(), plan.chunk_key());
        let count = plan.entry_count();
        let kept = previous.get(&plan.input.entity).filter(|slot| {
            slot.entry == entry && slot.entries == count && !plan.uses(changed_images)
        });
        if let Some(slot) = kept {
            built
                .fields
                .extend_from_slice(&old.fields[slot.first..slot.first + slot.entries]);
        } else {
            built.fields.extend(field_entries(plan));
            built.rewritten += count as u64;
        }
        built.slots.push(FieldSlot {
            entity: plan.input.entity,
            first: plan.first,
            entries: count,
            entry,
            chunk,
        });
        for plant in plan.plants {
            built.features = built.features | LookFeatures::of(plan.input.field, &plant.plant);
            if plant.plant.accent.is_on() {
                built.features = built.features
                    | LookFeatures::of(plan.input.field, &plant.plant.accent_plant());
            }
        }
    }
    built
}

/// Brings the tables up to date with what changed, building only what that
/// needs, once every input has loaded.
///
/// - A mask or heightmap is read from its image only when that image changed
///   (`PackedImages`).
/// - A field's entries are rewritten only when its look changed, or it moved,
///   or its images did.
/// - The chunks, masks and heightmaps are kept (and not uploaded again) while
///   no field resized, moved up or down, or changed images. A look change
///   alone, a colour changing every frame for a season or the time of day,
///   rewrites the entries of that field and nothing else.
/// - A pass that finds the tables unchanged changes nothing.
pub(crate) fn rebuild(
    mut dirty: ResMut<TablesDirty>,
    mut tables: ResMut<FieldTables>,
    fields: Query<FieldInputs>,
    (images, grounds): (Res<Assets<Image>>, Res<MeshGrounds>),
    (mut cache, mut edits): (ResMut<PackedImages>, ResMut<EditLayers>),
    mut stats: ResMut<PlantStats>,
) {
    if !dirty.rebuild() {
        if dirty.access {
            dirty.access = false;
            patch_access(&mut tables, &fields);
        }
        return;
    }
    let mut sorted: Vec<_> = fields.iter().collect();
    sorted.sort_by_key(|(entity, ..)| *entity);
    let edits_ahead = edits.ahead;
    let Some(inputs) = gather(&sorted, &images, &grounds, &edits) else {
        return;
    };
    // This pass reads every field's visibility too, and takes care of
    // everything that changed.
    let changed_images = std::mem::take(&mut dirty.images);
    dirty.fields = false;
    dirty.access = false;

    let started = Instant::now();
    cache.forget(&changed_images);
    let old = (*tables).clone();
    let (plans, dropped) = plan(&inputs);
    let (chunks, chunks_same) = chunks_for(&plans, &old, &changed_images, &mut cache);
    let images = images_for(
        &plans,
        &old,
        &changed_images,
        &mut cache,
        (chunks_same, edits_ahead),
    );
    let mut built = entries_for(&plans, &old, &changed_images);
    // Class numbering depends on all plants, so retained entries must be reassigned
    // alongside rebuilt entries.
    let (shape_classes, unclassed) = assign_shape_classes(&mut built.fields);
    if unclassed > 0 {
        warn!(
            "procgena_plant draws at most {MAX_BRANCHED_CLASSES} kinds of branched plant (by how \
             many branches they have), each with at most {} vertices a stem; {unclassed} \
             plants beyond that are not drawn. Give more of them the same branch \
             counts, or fewer branches.",
            shader_model::BRANCHED_VERTEX_STRIDE
        );
    }
    drop(plans);
    drop(inputs);
    if !images.masks_same {
        // The masks were joined from the layers as they are now.
        edits.ahead = false;
    }

    let layout_changed = !(images.masks_same && images.heights_same && chunks_same);
    let mut counted = stats.tables;
    counted.entries_rewritten += built.rewritten;
    counted.images_read = cache.reads;
    if !layout_changed && built.fields == *old.fields && built.slots == *old.slots {
        // Something changed that no field shows: a field set to what it
        // already was, say. There is nothing to upload.
        if counted != stats.tables {
            stats.tables = counted;
        }
        return;
    }
    let (fields_len, chunks_len) = (built.slots.len(), chunks.len());
    *tables = FieldTables {
        version: old.version + 1,
        structure_version: old.structure_version + u64::from(layout_changed),
        fields: Arc::new(built.fields),
        slots: Arc::new(built.slots),
        chunks,
        masks: images.masks,
        heights: images.heights,
        mask_ids: images.mask_ids,
        height_ids: images.height_ids,
        features: built.features,
        shape_classes: Arc::new(shape_classes),
    };
    stats.tables = TableStats {
        fields: fields_len,
        chunks: chunks_len,
        fields_dropped: dropped.fields,
        plants_dropped: dropped.plants,
        branched_dropped: unclassed,
        rebuilds: counted.rebuilds + 1,
        layout_rebuilds: counted.layout_rebuilds + u64::from(layout_changed),
        last_rebuild: started.elapsed(),
        ..counted
    };
}

/// Combines one plant's appearance with the field's placement and shared look.
/// `field` supplies density, toon shading and colour variation; `look`
/// supplies the plant's shape, colours and tufting.
fn look_entry(
    field_entry: &FieldGpu,
    field: &PlantField,
    look: &PlantParams,
    mix: Vec4,
) -> FieldGpu {
    let segment = &look.segment;
    let (root, tip) = (linear(segment.root_color), linear(segment.tip_color));
    let patch = linear(field.variation.patch_color).truncate();
    let (shortest, tallest) = look.height_range();
    FieldGpu {
        shape: Vec4::new(shortest, tallest, look.width, field.density()),
        motion: Vec4::new(
            look.lean,
            segment.stiffness,
            1.0 - field.wind_scale.max(0.0),
            look.root_occlusion,
        ),
        root_color: root,
        // The tip colour's alpha carries the translucency.
        tip_color: tip.truncate().extend(look.translucency),
        look: Vec4::new(
            look.specular.clamp(0.0, 1.0),
            field.toon_bands as f32,
            segment.profile.clamp(0.0, 1.0),
            0.0,
        ),
        mix,
        form: Vec4::new(
            segment.twist.max(0.0),
            // Above 1, a main stem's droop lowers its tip onto the ground.
            segment.droop.clamp(0.0, 2.0),
            segment.fold.clamp(0.0, 1.0),
            look.head_length.clamp(0.0, 0.9),
        ),
        head: linear(look.head_color)
            .truncate()
            .extend(look.head_width.max(0.0)),
        // In a patch the whole blade is scaled by how far its tip colour
        // moves toward the patch colour.
        patches: (patch / tip.truncate().max(Vec3::splat(1e-3)))
            .extend(field.variation.patch_amount.clamp(0.0, 1.0)),
        // The plant's own tufts; the bare patches belong to the field.
        tufts: Vec4::new(
            look.tufts.clamp(0.0, 1.0),
            look.tuft_size.max(0.01),
            field_entry.tufts.z,
            field_entry.tufts.w,
        ),
        // Assign the shape class after all entries exist, in `assign_shape_classes`.
        access: UVec4::new(
            field_entry.access.x,
            0,
            field_entry.access.z,
            look.stems_per_plant.clamp(1, MAX_STEMS_PER_PLANT),
        ),
        levels: level_entries(look),
        ..*field_entry
    }
}

/// Packs branch settings for the GPU. Disabled levels, including children of a disabled
/// first level, become zeroed entries.
pub(crate) fn level_entries(plant: &PlantParams) -> [LevelGpu; 2] {
    let first_on = plant.levels[0].is_on();
    std::array::from_fn(|index| {
        let level = &plant.levels[index];
        if !level.is_on() || !first_on {
            return LevelGpu::default();
        }
        let opposite = level.arrangement == PlantArrangement::Opposite;
        let children = drawn_children(level.count.min(MAX_BRANCHES), opposite);
        let segment = &level.segment;
        let (root, tip) = (linear(segment.root_color), linear(segment.tip_color));
        LevelGpu {
            attach: Vec4::new(
                children as f32,
                if opposite { 0.0 } else { 1.0 },
                level.attachment_start.clamp(0.0, 0.99),
                level.length_envelope.clamp(0.0, 1.0),
            ),
            size: Vec4::new(
                level.length.max(0.0),
                level.width.max(0.0),
                level.angle.clamp(0.0, 1.0),
                segment.stiffness.clamp(0.0, 1.0),
            ),
            form: Vec4::new(
                segment.profile.clamp(0.0, 1.0),
                segment.droop.clamp(0.0, 1.0),
                segment.twist.max(0.0),
                segment.fold.clamp(0.0, 1.0),
            ),
            root_color: root.truncate().extend(level.spread.clamp(0.0, 1.0)),
            tip_color: tip.truncate().extend(1.0),
        }
    })
}

/// Conservative reach from the root, as a multiple of stem height. Unbranched plants
/// use 1; branches extend the culling bound. The WESL twin is `common::reach_factor`.
pub(crate) fn reach_factor(levels: &[LevelGpu; 2]) -> f32 {
    1.0 + levels[0].size.x * (1.0 + levels[1].size.x)
}

/// Assigns mesh classes in `access.y` across the whole field table. Class 0 is
/// unbranched; up to `MAX_BRANCHED_CLASSES` sorted classes cover branching plants.
/// Entries exceeding the class or vertex limits receive `NO_CLASS` and are not drawn.
/// Returns the active classes and the number of skipped entries.
fn assign_shape_classes(fields: &mut [FieldGpu]) -> (Vec<ShapeClass>, usize) {
    let class_of = |entry: &FieldGpu| ShapeClass {
        children: entry.levels.map(|level| level.attach.x as u32),
    };
    let mut wanted: Vec<ShapeClass> = fields
        .iter()
        .map(class_of)
        .filter(|class| class.children[0] > 0)
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let classes: Vec<ShapeClass> = wanted
        .into_iter()
        .filter(|class| class.fits())
        .take(MAX_BRANCHED_CLASSES)
        .collect();
    let mut unclassed = 0;
    for entry in fields {
        let class = class_of(entry);
        entry.access.y = if class.children[0] == 0 {
            0
        } else if let Some(at) = classes.iter().position(|known| *known == class) {
            at as u32 + 1
        } else {
            unclassed += 1;
            NO_CLASS
        };
    }
    (classes, unclassed)
}

/// Builds a field's plant entries, followed by its enabled accent entries.
/// Every entry includes the field's placement so either kind can be drawn.
fn field_entries(plan: &Plan) -> Vec<FieldGpu> {
    let Input {
        placement,
        field,
        mask,
        height,
        ..
    } = plan.input;
    let Placement {
        size,
        corner,
        axis,
        access,
    } = *placement;
    let layout = ChunkLayout::for_density(CHUNK_SIZE, field.density());
    let (mask_offset, mask_size) = match mask {
        Some(mask) => (plan.mask_offset, mask.size),
        None => (NO_MAP, UVec2::ZERO),
    };
    let (height_offset, height_size, height_scale) = match height {
        Some((_, image, scale)) => (plan.height_offset, image.size(), *scale),
        None => (NO_MAP, UVec2::ZERO, 0.0),
    };

    let variation = &field.variation;
    let field_entry = FieldGpu {
        min_corner: corner.extend(0.0),
        extent: Vec4::new(size.x, size.y, height_scale, layout.tile_size),
        // The seed comes from where the field is, not from its entity, so
        // the same field looks the same however the scene was spawned.
        placement: UVec4::new(
            shader_model::hash2(corner.x.to_bits(), corner.z.to_bits()),
            layout.tiles_per_side,
            mask_offset,
            height_offset,
        ),
        map_sizes: UVec4::new(mask_size.x, mask_size.y, height_size.x, height_size.y),
        ground: linear(field.ground_tint.color)
            .truncate()
            .extend(field.ground_tint.amount.clamp(0.0, 1.0)),
        variation: Vec4::new(
            variation.patch_size.max(0.01),
            variation.dry_tips.clamp(0.0, 1.0),
            variation.dead_share.clamp(0.0, 1.0),
            tuft_spread(plan.plants),
        ),
        // Each plant's entry fills in its own tufts in x and y.
        tufts: Vec4::new(
            0.0,
            0.0,
            if variation.bare_share > 0.0 {
                shader_model::bare_threshold(variation.bare_share)
            } else {
                -1.0
            },
            variation.bare_patch_size.max(0.01),
        ),
        access: UVec4::new(access, 0, 0, 0),
        axis: axis.extend(0.0).extend(0.0),
        ..FieldGpu::default()
    };
    let mut entries = plant_entries(&field_entry, field, plan.plants);
    // Culling uses the whole field's tallest plant and widest branch reach. Without
    // branches, the bound remains exactly the maximum plant height.
    let branched = entries.iter().any(|entry| entry.levels[0].attach.x > 0.0);
    let reach = entries
        .iter()
        .map(|entry| entry.shape.y * reach_factor(&entry.levels))
        .fold(0.0, f32::max);
    for entry in &mut entries {
        entry.min_corner.w = reach;
        entry.access.z = u32::from(branched);
    }
    entries
}

/// Builds entries in plant order, followed by one entry per enabled accent.
/// Each plant stores a relative offset to its accent. `field_entry` supplies
/// the placement and settings shared by every entry.
fn plant_entries(
    field_entry: &FieldGpu,
    field: &PlantField,
    plants: &[FieldPlant],
) -> Vec<FieldGpu> {
    // The tallest stem of any plant or accent, for culling and the sampler.
    let tallest = plants
        .iter()
        .flat_map(|plant| {
            let accent = plant
                .plant
                .accent
                .is_on()
                .then(|| plant.plant.accent_plant());
            std::iter::once(plant.plant).chain(accent)
        })
        .map(|plant| plant.height_range().1)
        .fold(0.0, f32::max);
    let count = plants.len() as f32;

    // One entry per plant, in order; each accented plant points at its
    // accent's entry, which follows all the plants.
    let chances = plant_chances(plants);
    let mut accents = Vec::new();
    let mut entries: Vec<FieldGpu> = plants
        .iter()
        .zip(&chances)
        .enumerate()
        .map(|(index, (plant, chance))| {
            let mix = Vec4::new(*chance, plant.clustering.clamp(0.0, 1.0), count, tallest);
            let mut entry = look_entry(field_entry, field, &plant.plant, mix);
            if plant.plant.accent.is_on() {
                let at = plants.len() + accents.len();
                entry.accent = Vec4::new(
                    plant.plant.accent.share.min(1.0),
                    (at - index) as f32,
                    0.0,
                    0.0,
                );
                accents.push(plant.plant.accent_plant());
            }
            entry
        })
        .collect();

    // An accent's entry is never picked by the plant pick: its chance is 0.
    let mix = Vec4::new(0.0, 0.0, count, tallest);
    entries.extend(
        accents
            .iter()
            .map(|accent| look_entry(field_entry, field, accent, mix)),
    );
    entries
}

/// A field's chunks: the 16 m squares of it, each with the lowest and highest
/// ground in it. `heights` is the field's heightmap, if it has one.
fn field_chunks(plan: &Plan, heights: Option<&[f32]>, out: &mut Vec<ChunkGpu>) {
    let Placement { size, corner, .. } = plan.input.placement;
    let height_scale = plan.input.height.map_or(0.0, |(_, _, scale)| scale);
    let height_size = plan
        .input
        .height
        .map_or(UVec2::ZERO, |(_, image, _)| image.size());
    let counts = (size / CHUNK_SIZE).ceil().as_uvec2();
    for chunk_z in 0..counts.y {
        for chunk_x in 0..counts.x {
            let local = Vec2::new(chunk_x as f32, chunk_z as f32) * CHUNK_SIZE;
            let (low, high) = match heights {
                None => (corner.y, corner.y),
                Some(heights) => {
                    let (low, high) =
                        height_range(heights, height_size, local / size, CHUNK_SIZE / size);
                    (
                        corner.y + low * height_scale,
                        corner.y + high * height_scale,
                    )
                }
            };
            out.push(ChunkGpu {
                bounds: Vec4::new(local.x, local.y, low, high),
                ids: UVec4::new(plan.first as u32, chunk_x, chunk_z, 0),
            });
        }
    }
}

fn linear(color: bevy::color::Color) -> Vec4 {
    Vec4::from_array(LinearRgba::from(color).to_f32_array())
}

/// Returns each plant's chance of taking a root not assigned to an earlier entry.
///
/// Densities include each entry's scale. The field's scale affects every entry
/// equally, so it does not change these relative chances. The last entry with
/// positive density takes all remaining roots, avoiding gaps from rounding.
/// Entries with zero density have no chance of being picked.
fn plant_chances(plants: &[FieldPlant]) -> Vec<f32> {
    let total: f32 = plants.iter().map(FieldPlant::density).sum();
    let last = plants.iter().rposition(|plant| plant.density() > 0.0);
    let mut left = 1.0;
    plants
        .iter()
        .enumerate()
        .map(|(index, plant)| {
            let share = plant.density() / total;
            let chance = match last {
                Some(last) if index == last => 1.0,
                Some(last) if index < last && left > 1e-6 => (share / left).min(1.0),
                _ => 0.0,
            };
            left -= share;
            chance
        })
        .collect()
}

/// Counts one table entry per plant and one per enabled accent.
fn entry_count(plants: &[FieldPlant]) -> usize {
    plants.len()
        + plants
            .iter()
            .filter(|plant| plant.plant.accent.is_on())
            .count()
}

/// How far tufting can pull a plant root from where it would grow, for the
/// cull bound: the widest pull of any tufted plant, or 0 without tufts.
fn tuft_spread(plants: &[FieldPlant]) -> f32 {
    plants
        .iter()
        .filter(|plant| plant.plant.tufts > 0.0)
        .map(|plant| plant.plant.tuft_size.max(0.01) * 1.5)
        .fold(0.0, f32::max)
}

/// Lowest and highest normalized height over a UV rectangle, one texel wider
/// on every side so bilinear sampling at the edges is covered. Holes in a
/// mesh ground hold no blades, so they are left out.
fn height_range(heights: &[f32], size: UVec2, uv_min: Vec2, uv_size: Vec2) -> (f32, f32) {
    let texels = size.as_vec2();
    let lo = ((uv_min * texels).floor().as_ivec2() - 1)
        .max(bevy::math::IVec2::ZERO)
        .as_uvec2();
    let hi = (((uv_min + uv_size) * texels).ceil().as_ivec2() + 1)
        .min(size.as_ivec2() - 1)
        .max(bevy::math::IVec2::ZERO)
        .as_uvec2();
    let mut range = (f32::MAX, f32::MIN);
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let h = heights[(y * size.x + x) as usize];
            if h > HOLE * 0.5 {
                range = (range.0.min(h), range.1.max(h));
            }
        }
    }
    if range.0 > range.1 { (0.0, 0.0) } else { range }
}

#[cfg(test)]
mod tests;
