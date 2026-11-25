//! The field and chunk tables' GPU layout: the contract between the field
//! logic and `shaders/common.wesl`.
//!
//! The field logic (`fields.rs`, `edit.rs`) builds its tables directly in
//! this layout and the render world uploads them as they are, shared as
//! `Arc`s: converting from a layout of the logic's own would cost a copy of
//! every table on every rebuild for nothing a port needs. So the logic
//! depends on the GPU layout, but only here: a change to the layout, or to
//! how Bevy derives `ShaderType`, is a change to this file and its WESL
//! twins, `Field`, `Chunk` and `NO_MAP` in `common.wesl`.

use bevy::math::{UVec4, Vec4};
use bevy::render::render_resource::ShaderType;

/// Marks a field without a mask or heightmap. The WESL twin is `NO_MAP` in
/// `common.wesl`.
pub(crate) const NO_MAP: u32 = u32::MAX;

/// Maximum branched mesh classes drawn together. Grouping by branch counts gives every
/// instance in a draw the same vertex count. Unbranched class 0 is additional.
pub(crate) const MAX_BRANCHED_CLASSES: usize = 32;

/// Marks an entry that cannot be assigned a drawable mesh class. Stored in
/// `FieldGpu::access.y`; these plants are skipped. The WESL twin is `common::NO_CLASS`.
pub(crate) const NO_CLASS: u32 = u32::MAX;

/// Packed branch settings for one level. Mirrors `Level` in `common.wesl`.
#[derive(Debug, Clone, Copy, PartialEq, Default, ShaderType)]
pub(crate) struct LevelGpu {
    /// x: rendered children per parent, rounded up for opposite pairs (0 disables the
    /// level); y: arrangement (0 opposite, 1 alternate); z: bare fraction at the
    /// parent's base; w: position of the longest children within the leafy part.
    pub attach: Vec4,
    /// x: maximum child length relative to its parent; y: child width relative to its
    /// own length; z: direction from perpendicular (0) to along the parent (1); w: wind
    /// stiffness.
    pub size: Vec4,
    /// x: outline profile; y: downward droop; z: twist in turns; w: centre-line fold.
    pub form: Vec4,
    /// rgb: base colour after field tint; w: rotation between attachment points, in
    /// half-turns.
    pub root_color: Vec4,
    /// rgb: tip colour after applying the field tint.
    pub tip_color: Vec4,
}

/// One plant or accent entry for rendering a field.
///
/// Mirrors `Field` in `common.wesl`. A field stores its plant entries in list
/// order, followed by enabled accents. Chunks point to the first entry; each
/// blade points to the entry it uses. All entries repeat the field's placement.
#[derive(Debug, Clone, Copy, PartialEq, Default, ShaderType)]
pub(crate) struct FieldGpu {
    /// xyz: minimum field corner at base height. On the first entry, w is the maximum
    /// root-to-tip reach, including branches, used for culling; without branches it
    /// equals `mix.w`.
    pub min_corner: Vec4,
    /// x, y: size along X and Z; z: heightmap scale; w: tile size.
    pub extent: Vec4,
    /// x: shortest blade, y: tallest blade, z: root width, w: blades per m².
    pub shape: Vec4,
    /// x: lean, y: stiffness, z: how much less the wind moves the field's
    /// blades, `1 - PlantScatter::wind_scale` (so 0, an entry's default, is the
    /// full wind), w: root occlusion.
    pub motion: Vec4,
    pub root_color: Vec4,
    pub tip_color: Vec4,
    /// x: seed, y: tiles per chunk side, z: mask offset, w: heightmap offset
    /// (offsets in texels; `NO_MAP` when absent). On the GPU the heightmaps
    /// follow the masks in one buffer, so the render world adds the masks'
    /// length in words to `w` when it uploads the table.
    pub placement: UVec4,
    /// Mask width and height, heightmap width and height, in texels.
    pub map_sizes: UVec4,
    /// rgb: the ground's colour; w: how far roots blend toward it.
    pub ground: Vec4,
    /// x: specular strength; y: toon bands (0 uses physical shading); z: main-segment
    /// outline profile.
    pub look: Vec4,
    /// x: chance of selecting this plant after earlier plants declined a root
    /// (1 for the last plant with a positive density, 0 for accents); y: clustering;
    /// z: plant count, excluding accents; w: maximum stem length across all
    /// plants and accents. The count and maximum are repeated on every entry.
    pub mix: Vec4,
    /// x: twist, in turns; y: droop; z: fold; w: head, a share of the length.
    pub form: Vec4,
    /// rgb: head colour; w: head width.
    pub head: Vec4,
    /// rgb: patch colour over tip colour; w: patch amount.
    pub patches: Vec4,
    /// x: patch size; y: dry tips; z: dead blades; w: how far tufts can pull
    /// a root, the widest of any of the field's tufted plants (1.5 tuft
    /// sizes), or 0, for culling.
    pub variation: Vec4,
    /// x: this plant's tuft pull; y: its tuft size; z: the field's bare-patch
    /// noise threshold, or -1 for none; w: the field's bare patch size.
    pub tufts: Vec4,
    /// x: the render layers the field is drawn on, one bit each; 0 while the
    /// field is hidden. A view draws the field when its own layers share a bit.
    /// y: mesh class (0 unbranched, `NO_CLASS` skipped); z: 1 for every entry of a
    /// field containing branched plants, otherwise 0; w: stems sharing each plant root.
    pub access: UVec4,
    /// x, y: the field's own X axis in world XZ, which turns with the field
    /// about Y; its Z axis is the perpendicular (`Vec2::perp`). Positions in
    /// the field are along these axes from `min_corner`. z: the field's fill
    /// (`shader_model::placement_fill`), a whole number.
    pub axis: Vec4,
    /// x: accent share (0 disables it); y: offset from this entry to its
    /// accent entry. A relative offset stays valid when the field moves
    /// within the table.
    pub accent: Vec4,
    /// Packed settings for the first and second branch levels.
    pub levels: [LevelGpu; 2],
}

/// One chunk. Mirrors `Chunk` in `common.wesl`: 32 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Default, ShaderType)]
pub(crate) struct ChunkGpu {
    /// x, y: the chunk's minimum corner in world XZ, in metres;
    /// z, w: lowest and highest ground of the field in the chunk, world Y.
    pub bounds: Vec4,
    /// x: field index; y, z: the World chunk's coordinates along X and Z,
    /// as `i32` bits.
    pub ids: UVec4,
}
