//! The look of the grass: plants, and the field-wide settings that colour them.
//!
//! Use [`PlantParams`] to choose a plant's size, colour, shape, branches and
//! tufting, and how densely it grows. A [`PlantScatter`](crate::PlantScatter)
//! lists its plants as [`ScatterPlant`]s, each a plant with a scale on its
//! density in that field, and adds the settings shared by the whole field:
//! [`PlantVariation`] and [`GroundTint`].
//!
//! Plants are built from [`PlantSegment`]s: curved strips that represent blades, stalks
//! or leaves. [`PlantBranches`] places smaller segments along a parent segment, such as
//! a fern's leaflets along its stalk.

use bevy::color::Color;
use bevy::math::Vec2;
use bevy::reflect::Reflect;
use bevy::reflect::std_traits::ReflectDefault;
use serde::{Deserialize, Serialize};

use crate::PlantSettings;
use crate::presets::PlantScatterPreset;
use crate::scatters::level_entries;
use crate::shader_model::ShapeClass;

/// Maximum number of plant kinds drawn in one field.
///
/// Only the first 16 entries in [`PlantScatter::plants`](crate::PlantScatter::plants)
/// are used. Accents do not count towards this limit.
pub const MAX_PLANTS: usize = 16;

/// Maximum stems sharing one root; bounds [`PlantParams::stems_per_plant`].
pub(crate) const MAX_STEMS_PER_PLANT: u32 = 8;

/// Maximum children per segment; bounds [`PlantBranches::count`].
pub(crate) const MAX_BRANCHES: u32 = 32;

/// The size, appearance and branching of one kind of plant.
///
/// Add a plant to [`PlantScatter::plants`](crate::PlantScatter::plants) with
/// [`ScatterPlant::new`]. It grows at its own [`density`](PlantParams::density),
/// which the field can scale.
///
/// Each stem starts with a main [`segment`](PlantParams::segment), such as a grass blade
/// or fern stalk. Up to two [`levels`](PlantParams::levels) add branches to that segment
/// and then to its children. Disable both levels for an unbranched blade.
///
/// Start from a plant preset such as [`PlantParams::MEADOW_GRASS`],
/// [`PlantParams::SEED_HEADS`] or [`PlantParams::FERN`].
///
/// With serde, a plant and each of its parts read and write the same
/// settings as the `plant:` part of a `.plant.yml` file. Settings that are
/// missing come from the type's default ([`PlantParams::MEADOW_GRASS`] for a
/// plant), and a value out of range is an error.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::PlantParamsFile",
    into = "crate::files::PlantParamsFile"
)]
pub struct PlantParams {
    /// The seed used when this plant is built on its own, rather than inside
    /// a Scatter that gives it a seed.
    ///
    /// Not used yet: plants are placed and shaped from their position alone,
    /// so changing it changes nothing that is drawn.
    pub default_seed: u32,
    /// The plant's natural density, in plants per square metre.
    ///
    /// For example, meadow grass uses 60 and ferns use 2. Adjust how densely
    /// it grows in a field with [`ScatterPlant::density_scale`] and
    /// [`PlantScatter::density_scale`](crate::PlantScatter::density_scale); both
    /// scales multiply this value. Each plant counts once, regardless of its
    /// [`stems_per_plant`](Self::stems_per_plant).
    pub density: f32,
    /// The two limits for main stem length in metres, before bending. Each stem
    /// gets a length between these values; either limit may be written first.
    pub height: Vec2,
    /// Main-segment width scale, in metres, with ±25% variation per stem. The segment's
    /// profile determines its outline.
    pub width: f32,
    /// Resting tip lean as a fraction of the main segment's length.
    pub lean: f32,
    /// Stems sharing one root, from 1 to 8. Use 1 for grass or 5–6 for a fern's ring of
    /// outward-leaning fronds. Density counts the whole plant once, regardless of its
    /// stem count.
    pub stems_per_plant: u32,
    /// Shape, colour and movement of each main stem, such as a blade or fern stalk.
    pub segment: PlantSegment,
    /// Two layers of branches: `levels[0]` grows from the main stem, and
    /// `levels[1]` grows from those branches, like leaves on twigs. Set a layer's
    /// [`count`](PlantBranches::count) to 0 to hide it. The second layer is also
    /// hidden when the first is disabled, but its settings are kept.
    pub levels: [PlantBranches; 2],
    /// Darkening near the ground, from 0 (none) to 1 (black), to suggest shade from
    /// surrounding plants.
    pub root_occlusion: f32,
    /// Sunlight transmitted through a backlit segment, from 0 (none) to 1 (as bright as
    /// its sunlit side). Backlit grass glows
    /// instead of going black.
    pub translucency: f32,
    /// Broad highlights on the plant's upper half, from 0 (matte) to 1, so
    /// wind waves show as a moving sheen.
    pub specular: f32,
    /// Length of the seed head or flower relative to the main stem: 0.2 means
    /// one fifth of the stem length, and 0 hides it. The head faces the camera.
    /// Enabling branches in `levels[0]` hides the head without clearing its settings.
    pub head_length: f32,
    /// Width of the head, in metres.
    pub head_width: f32,
    /// Colour of the head.
    pub head_color: Color,
    /// How strongly plants gather into tufts, from 0 (evenly spread) to 1
    /// (tight groups with gaps between them). Stems lean away from each
    /// tuft centre. Tufting moves plants without changing their number.
    pub tufts: f32,
    /// Distance between tufts, in metres.
    pub tuft_size: f32,
    /// Stems that grow differently from the rest, such as plumes rising from a
    /// grass tussock or kelp fronds that carry a float. They grow in place of
    /// some main stems, in the plant's own tufts.
    pub accent: PlantAccent,
}

/// Stems with a different appearance within one kind of plant.
///
/// Use an accent for seed stalks among grass blades, cream plumes above
/// pampas grass, or floats on kelp fronds. Accent stems replace ordinary
/// stems rather than adding to the field's density.
///
/// Accents are always unbranched. They keep the plant's root shading, sheen,
/// stem count and tuft settings, but have their own size, shape and colours.
/// Set [`share`](Self::share) to 0, or use [`PlantAccent::NONE`], to turn them off.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::PlantAccentFile",
    into = "crate::files::PlantAccentFile"
)]
pub struct PlantAccent {
    /// The share of the plant's stems that are accents, from 0 (none) to 1
    /// (every stem).
    pub share: f32,
    /// The two limits for an accent stem's length in metres; either may be
    /// written first.
    pub height: Vec2,
    /// Accent stem width, in metres, as [`PlantParams::width`].
    pub width: f32,
    /// Resting tip lean as a fraction of the stem's length.
    pub lean: f32,
    /// Sunlight transmitted through a backlit accent, from 0 to 1, as
    /// [`PlantParams::translucency`].
    pub translucency: f32,
    /// Shape, colour and movement of each accent stem.
    pub segment: PlantSegment,
    /// Length of the accent's head relative to its stem, as
    /// [`PlantParams::head_length`]; 0 hides it.
    pub head_length: f32,
    /// Width of the accent's head, in metres.
    pub head_width: f32,
    /// Colour of the accent's head.
    pub head_color: Color,
}

impl PlantAccent {
    /// An accent that replaces no stems.
    ///
    /// Use this to disable accents when constructing a [`PlantParams`].
    pub const NONE: Self = Self {
        share: 0.0,
        height: Vec2::new(0.3, 0.6),
        width: 0.02,
        lean: 0.15,
        translucency: 0.6,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.3,
            droop: 0.0,
            twist: 0.0,
            fold: 0.0,
            root_color: Color::srgb(0.06, 0.13, 0.03),
            tip_color: Color::srgb(0.38, 0.55, 0.16),
        },
        head_length: 0.0,
        head_width: 0.03,
        head_color: Color::srgb(0.85, 0.80, 0.60),
    };

    /// Returns whether the accent share is greater than zero.
    ///
    /// An enabled accent replaces that share of the plant's stems.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.share > 0.0
    }
}

impl Default for PlantAccent {
    fn default() -> Self {
        Self::NONE
    }
}

/// The shape, colour and movement of a curved strip of plant material.
///
/// A segment can represent a grass blade, fern stalk or leaflet. Its dimensions come
/// from [`PlantParams`] or [`PlantBranches`]. Set its shape controls to 0 for a
/// straight, pointed strip.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::PlantSegmentFile",
    into = "crate::files::PlantSegmentFile"
)]
pub struct PlantSegment {
    /// Outline from 0 (a grass blade tapering to a point) to 1 (an oval leaf, narrow at
    /// both ends and widest in the middle).
    pub profile: f32,
    /// Wind resistance, from 0 (soft) to 1 (stiff). Branches inherit their parent's
    /// movement and add their own sway, reduced by this setting.
    pub stiffness: f32,
    /// Downward arch, from 0 (straight) to 1 (a low-hanging tip). Main stems vary from
    /// no droop up to this amount, with taller stems drooping more.
    ///
    /// A main stem also takes values from 1 to 2, which lower every stem's tip
    /// further, until at 2 each stem arches over and its tip rests on the ground, like
    /// a bramble cane. The arch needs some
    /// [`lean`](PlantParams::lean) to pick its direction. The arch is low: each stem
    /// leaves its root at 45° or less, and its top is at most a quarter of its span.
    /// Both ends stay rooted: wind and trampling sway only the top of the arch.
    /// Stems still vary: the less a stem would have drooped, the taller its arch.
    /// Branches clamp their droop to 1.
    pub droop: f32,
    /// Maximum twist from root to tip, in full turns. Each segment varies in twist
    /// amount and direction.
    pub twist: f32,
    /// Fold along the centre line, from 0 (open, with rounded shading) to 1 (V-shaped).
    /// Folding narrows the visible strip, changes how each half catches light and tilts
    /// its branches.
    pub fold: f32,
    /// Segment colour at its base.
    pub root_color: Color,
    /// Segment colour at its tip.
    pub tip_color: Color,
}

impl PlantParams {
    /// Returns the stem length limits in ascending order, without changing the plant.
    pub(crate) fn height_range(&self) -> (f32, f32) {
        (self.height.min_element(), self.height.max_element())
    }

    /// Builds the unbranched plant used to draw accent stems.
    ///
    /// Copies the accent's appearance while retaining the parent plant's tufting,
    /// stem count and shared shading. The result has no accent of its own.
    pub(crate) fn accent_plant(&self) -> Self {
        let accent = &self.accent;
        Self {
            height: accent.height,
            width: accent.width,
            lean: accent.lean,
            translucency: accent.translucency,
            segment: accent.segment,
            head_length: accent.head_length,
            head_width: accent.head_width,
            head_color: accent.head_color,
            levels: [PlantBranches::NONE, PlantBranches::NONE],
            accent: PlantAccent::NONE,
            ..*self
        }
    }

    /// Returns the size of one plant's mesh at a level of detail.
    ///
    /// `lod` is 0 for near, 1 for mid and 2 (or more) for far detail, as
    /// [`PlantSettings::forced_lod`](crate::PlantSettings::forced_lod) counts
    /// them. The size covers all of the plant's
    /// [`stems_per_plant`](Self::stems_per_plant). Unbranched plants use
    /// [`PlantSettings::blade_rows`](crate::PlantSettings::blade_rows) from
    /// `settings`. Accent stems are separate plants and are not included;
    /// size them with an accent's own plant. Shadow passes draw simpler
    /// geometry and are not counted either.
    ///
    /// Returns `None` for a branched plant whose near-detail stem has too
    /// many vertices to be drawn at all.
    #[must_use]
    pub fn mesh_size(&self, lod: usize, settings: &PlantSettings) -> Option<PlantMeshSize> {
        let stems = self.stems_per_plant.clamp(1, MAX_STEMS_PER_PLANT);

        // Branched plants share a mesh class with others of the same branch counts.
        let class = ShapeClass {
            children: level_entries(self).map(|level| level.attach.x as u32),
        };
        let stem = if class.children[0] == 0 {
            let rows = settings.blade_rows[lod.min(2)].max(1);
            PlantMeshSize {
                vertices: 2 * rows + 1,
                triangles: 2 * rows - 1,
            }
        } else if class.fits() {
            PlantMeshSize {
                vertices: class.vertices(lod.min(2)),
                triangles: (class.indices(lod.min(2)).len() / 3) as u32,
            }
        } else {
            return None;
        };
        Some(PlantMeshSize {
            vertices: stem.vertices * stems,
            triangles: stem.triangles * stems,
        })
    }
}

/// The number of vertices and triangles in a plant's mesh.
///
/// Returned by [`PlantParams::mesh_size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlantMeshSize {
    /// Vertices drawn.
    pub vertices: u32,
    /// Triangles drawn.
    pub triangles: u32,
}

/// One level of branches attached along a parent segment.
///
/// Use this for leaflets along a fern stalk or leaves along twigs. See
/// [`PlantParams::levels`] for how the two levels connect.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::PlantBranchesFile",
    into = "crate::files::PlantBranchesFile"
)]
pub struct PlantBranches {
    /// Children per parent segment, from 0 (disabled) to 32.
    /// [`PlantArrangement::Opposite`] rounds odd counts up to complete pairs.
    pub count: u32,
    /// Whether children attach in pairs or alternate along the parent.
    pub arrangement: PlantArrangement,
    /// Fraction of the parent left bare at its base, from 0 up to but excluding 1. For
    /// example, 0.25 leaves the bottom quarter of a fern stalk bare.
    pub attachment_start: f32,
    /// Maximum child length as a fraction of its parent's length.
    pub length: f32,
    /// Position of the longest children within the leafy part, from 0 (base) to 1
    /// (tip). Children shorten towards either end; 0.3 gives a fern-like outline,
    /// widest near its base.
    pub length_envelope: f32,
    /// Child width scale as a fraction of its own length, before applying its profile.
    pub width: f32,
    /// Child direction, from 0 (perpendicular to the parent) to 1 (pointing along the
    /// parent towards its tip).
    pub angle: f32,
    /// Rotation between successive attachment points, from 0 to 1 half-turn.
    ///
    /// Use 0 for flat rows of fern leaflets. With opposite pairs, 0.5 rotates each pair
    /// a quarter-turn, as on a nettle. About 0.24 creates a spiral of alternate leaves.
    /// Positive values also orient leaves towards the sky.
    pub spread: f32,
    /// Shape, colour and wind response shared by the children at this level.
    pub segment: PlantSegment,
}

/// Placement of branches along their parent segment.
///
/// Choose paired attachments for fern leaflets or alternating attachments for leaves
/// along a twig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlantArrangement {
    /// Paired children attached at the same point on opposite sides.
    #[default]
    Opposite,
    /// Individual children attached successively on alternating sides.
    Alternate,
}

impl PlantBranches {
    /// A disabled branch level.
    ///
    /// Use this in [`PlantParams::levels`] wherever no branches should grow.
    pub const NONE: Self = Self {
        count: 0,
        arrangement: PlantArrangement::Opposite,
        attachment_start: 0.0,
        length: 0.0,
        length_envelope: 0.5,
        width: 0.0,
        angle: 0.0,
        spread: 0.0,
        segment: PlantSegment {
            profile: 0.0,
            stiffness: 0.5,
            droop: 0.0,
            twist: 0.0,
            fold: 0.0,
            root_color: Color::srgb(0.06, 0.13, 0.03),
            tip_color: Color::srgb(0.38, 0.55, 0.16),
        },
    };

    /// Whether this level has a non-zero branch count.
    ///
    /// A second level still needs an enabled first level to appear on the plant.
    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.count > 0
    }
}

/// How far the ground shows through at the foot of the grass.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::GroundTintFile",
    into = "crate::files::GroundTintFile"
)]
pub struct GroundTint {
    /// The colour of the ground the field grows on.
    pub color: Color,
    /// How far the lowest third of each blade blends toward `color`, 0 (not
    /// at all) to 1, so the field sits in the terrain instead of on it.
    pub amount: f32,
}

/// Colour changes and bare patches across a field.
///
/// Set all amounts and shares to 0 for an even field. Tufting belongs to
/// each [`PlantParams`], so different plants can form different-sized groups.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::PlantVariationFile",
    into = "crate::files::PlantVariationFile"
)]
pub struct PlantVariation {
    /// The colour the blades drift toward in large patches, as the tip
    /// colour; the rest of each blade shifts with it. Real fields are greener
    /// in hollows and yellower on dry rises.
    pub patch_color: Color,
    /// How far blades in a patch take on `patch_color`, 0 (no patches) to 1.
    pub patch_amount: f32,
    /// Size of the colour patches, in metres.
    pub patch_size: f32,
    /// How far the upper part of blades dries toward straw, 0 (not at all)
    /// to 1; some blades dry more than others.
    pub dry_tips: f32,
    /// Share of blades that are dead and straw-coloured, 0 to 1.
    pub dead_share: f32,
    /// Share of the ground left bare, in patches, 0 to 1. The grass thins
    /// out toward a patch's edge rather than stopping at a line.
    pub bare_share: f32,
    /// Size of the bare patches, in metres.
    pub bare_patch_size: f32,
}

/// A plant and its density and clustering settings within a field.
///
/// Add entries to [`PlantScatter::plants`](crate::PlantScatter::plants). Each
/// plant grows at its own [`density`](PlantParams::density) times
/// [`density_scale`](Self::density_scale) times the field's
/// [`density_scale`](crate::PlantScatter::density_scale). For example, ferns
/// with a density of 2 and an entry scale of 0.5 contribute one fern per
/// square metre when the field scale is 1. Colour variation applies to
/// the whole field.
///
/// Order matters for clustering. The last plant with a positive density fills
/// the gaps left by earlier plants, so its own [`clustering`](Self::clustering)
/// has no effect. Put patch-forming plants first and the background grass or
/// moss last.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Default, Debug, PartialEq)]
#[serde(
    try_from = "crate::files::ScatterPlantFile",
    into = "crate::files::ScatterPlantFile"
)]
pub struct ScatterPlant {
    /// The plant's shape, appearance and natural density.
    ///
    /// Use a [`PlantParams`] preset or provide customised plant settings.
    pub plant: PlantParams,
    /// Multiplies this plant's natural density in this field.
    ///
    /// Use 1 for its [`density`](PlantParams::density), 0.5 for half as many
    /// plants, or 0 to remove it. Negative scales count as 0. The field's
    /// [`density_scale`](crate::PlantScatter::density_scale) also applies.
    pub density_scale: f32,
    /// How strongly this plant groups into patches, from 0 to 1.
    ///
    /// Use 0 to scatter plants throughout the field or 1 for patches about
    /// a metre across. Values outside this range are clamped. Density still
    /// sets how many plants grow. This setting has no effect on the last
    /// entry with a positive density, which fills the remaining gaps.
    pub clustering: f32,
}

impl ScatterPlant {
    /// Creates a field entry for a plant at its natural density.
    ///
    /// Pass the plant's shape and appearance settings. The entry starts
    /// with no clustering and a density scale of 1; the field's scale
    /// still applies.
    #[must_use]
    pub const fn new(plant: PlantParams) -> Self {
        Self {
            plant,
            density_scale: 1.0,
            clustering: 0.0,
        }
    }

    /// Sets this entry's density multiplier.
    ///
    /// Pass a non-negative scale: 1 keeps the plant's natural density,
    /// 0.5 halves it, and 0 removes the plant. The field's
    /// [`density_scale`](crate::PlantScatter::density_scale) also applies.
    #[must_use]
    pub const fn scaled(self, density_scale: f32) -> Self {
        Self {
            density_scale,
            ..self
        }
    }

    /// Sets how strongly this plant grows in patches.
    ///
    /// Pass 0 for scattered plants or 1 for patches about a metre across.
    /// Values outside this range are clamped when rendered. This does not
    /// change the density and has no effect on the last entry with a
    /// positive density; see [`ScatterPlant`].
    #[must_use]
    pub const fn clustered(self, clustering: f32) -> Self {
        Self { clustering, ..self }
    }

    /// Returns this entry's plants per square metre before the field's density scale.
    ///
    /// Multiplies the plant's natural density by
    /// [`density_scale`](Self::density_scale), returning 0 if the product
    /// is negative. The field's density scale is not included.
    #[must_use]
    pub fn density(&self) -> f32 {
        (self.plant.density * self.density_scale).max(0.0)
    }
}

impl Default for ScatterPlant {
    fn default() -> Self {
        Self::new(PlantParams::MEADOW_GRASS)
    }
}

impl Default for PlantParams {
    fn default() -> Self {
        Self::MEADOW_GRASS
    }
}

impl Default for PlantSegment {
    fn default() -> Self {
        PlantParams::MEADOW_GRASS.segment
    }
}

impl Default for PlantBranches {
    fn default() -> Self {
        Self::NONE
    }
}

impl Default for GroundTint {
    fn default() -> Self {
        PlantScatterPreset::MEADOW.plant_scatter.ground_tint
    }
}

impl Default for PlantVariation {
    fn default() -> Self {
        PlantScatterPreset::MEADOW.plant_scatter.variation
    }
}

#[cfg(test)]
mod tests;
