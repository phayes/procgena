//! Settings for a piece of dead wood: its kind, size, shape, ends, decay,
//! colours and mesh detail.
//!
//! Start with [`DeadwoodOptions::default()`], a [`crate::DeadwoodPreset`], or
//! a partial JSON object loaded with [`DeadwoodOptions::from_json`]. Creating
//! a [`crate::Piece`] validates the settings; call
//! [`DeadwoodOptions::validate`] to check edits before that.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::colour::parse_hex;
use crate::detail::DeadwoodDetail;
use crate::texture::{BARK_TEXTURE_SETS, END_TEXTURE_SETS};

/// Highest decay class. Decay runs from 0 (just fallen) to this value (a
/// soft, crumbling mound).
pub const MAX_DECAY: f32 = 5.0;
/// Largest allowed [`StubOptions::per_metre`].
pub const MAX_STUBS_PER_METRE: f32 = 20.0;
/// Largest allowed [`BranchingOptions::children`] and
/// [`BranchingOptions::twigs`].
pub const MAX_CHILDREN: u32 = 24;
/// Largest allowed [`BranchingOptions::levels`].
pub const MAX_LEVELS: u32 = 2;
/// Largest allowed [`RootOptions::lobes`].
pub const MAX_LOBES: u32 = 12;
/// Largest allowed [`RootOptions::count`].
pub const MAX_ROOTS: u32 = 16;
/// Largest allowed [`EndKind::Snapped`] `splinters`.
pub const MAX_SPLINTERS: u32 = 24;
/// Largest allowed segment or side count in a [`DeadwoodDetail`].
pub const MAX_SEGMENTS: u32 = 256;
/// Largest share of the main length that both ends together take up. A
/// cut or break reaches into the wood and must leave some of it whole, so
/// ends that would take up more are shrunk to fit when the piece is built.
pub const MAX_END_SHARE: f32 = 0.8;

/// What kind of wood a piece is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceKind {
    /// A section of trunk lying on the ground, with optional branch stubs
    /// and a hollow core.
    #[default]
    Log,
    /// A fallen branch, from a single stick up to a limb with side branches
    /// and twigs.
    Branch,
    /// An upright stump standing in the ground, with a flared base and
    /// surface roots.
    Stump,
}

/// Everything that describes one piece of dead wood.
///
/// Lengths are in metres where the field says so; others are fractions or
/// multiples of the piece's diameter or radius, so presets keep their
/// proportions when you change the size.
///
/// Setting fields or parsing JSON does not check the values. Call
/// [`validate`](Self::validate), or create a [`crate::Piece`], which checks
/// them for you.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadwoodOptions {
    /// Number that picks the random details. The same seed and settings
    /// always give the same piece; change it for a variation.
    pub seed: u32,
    /// Log, branch or stump.
    pub kind: PieceKind,
    /// How decayed the wood is, from 0 through [`MAX_DECAY`].
    ///
    /// Foresters sort fallen wood into five decay classes, from 1 (recently
    /// fallen and still sound) to 5 (soft and crumbling into the soil). This
    /// value follows those classes, with 0 meaning just fallen. Fractions
    /// blend between neighbouring classes.
    ///
    /// - 1: bark intact, fine twigs still on, resting on its stubs.
    /// - 2: twigs falling off, starting to sag.
    /// - 3: bark mostly gone, many branches lost, sagging to the ground.
    /// - 4: no bark, stubs loose, lying fully on the ground, slightly flattened.
    /// - 5: soft, flattened and sunk into the forest floor.
    ///
    /// Decay controls bark loss, twig and stub loss, flattening, sag, burial
    /// and colour. Most of these also have their own optional setting, such
    /// as [`BarkOptions::stripped`]; when that setting is given, it is used
    /// in place of the value from decay.
    pub decay: f32,
    /// Length of the main stem in metres: along a log or branch, or the
    /// height of a stump above the ground. Must be positive.
    pub length: f32,
    /// Diameter at the base of the main stem in metres. Must be positive.
    pub diameter: f32,
    /// Diameter at the tip as a fraction of the base diameter, from 0.05
    /// through 1.
    pub taper: f32,
    /// Bends, bumps and flattening of the main stem.
    pub shape: ShapeOptions,
    /// How each end of the main stem finishes: a saw cut, a break and so on.
    pub ends: EndsOptions,
    /// Short broken-off branches sticking out of the main stem.
    pub stubs: StubOptions,
    /// Side branches and twigs. Used by [`PieceKind::Branch`] only.
    pub branching: BranchingOptions,
    /// Bark thickness, peeling and cracks.
    pub bark: BarkOptions,
    /// A hollow core. Used by logs and stumps.
    pub hollow: HollowOptions,
    /// The flared base and surface roots. Used by [`PieceKind::Stump`] only.
    pub roots: RootOptions,
    /// Colours and surface appearance.
    pub material: MaterialOptions,
    /// How the piece comes to rest on the ground.
    pub settle: SettleOptions,
    /// Mesh detail that [`crate::Piece::mesh`] uses. Other methods take
    /// their detail as an argument.
    pub detail: DeadwoodDetail,
}

/// Bends, bumps and flattening of the main stem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeOptions {
    /// How far the middle of the stem bows sideways, as a fraction of its
    /// length, from 0 (straight) through 0.5.
    pub bend: f32,
    /// Small random kinks along the stem, as a fraction of the diameter,
    /// from 0 through 1. On short stems such as stumps, the kinks are kept
    /// small enough that the stem never bends sharply.
    pub wobble: f32,
    /// Height of broad bumps on the surface, as a fraction of the radius,
    /// from 0 (smooth) through 0.4.
    pub lumps: f32,
    /// Roughly how many bumps fit around the stem. Must be positive.
    pub lump_scale: f32,
    /// How much the cross-section is squashed toward the ground, from 0
    /// (round) through 0.6 (the height is 40% of the width). `None` takes it
    /// from [`DeadwoodOptions::decay`]: round until class 3, then flattening
    /// toward class 5. Stumps are never flattened.
    pub ovality: Option<f32>,
    /// How far a stump leans from upright, in degrees, from 0 through 45.
    /// The direction comes from the seed. Logs and branches ignore it.
    pub lean: f32,
}

/// How each end of the main stem finishes.
///
/// When the two ends together would take up more than [`MAX_END_SHARE`] of
/// the stem (see [`EndKind::span`]), the piece is built with both shrunk
/// by the same amount until they fit: less tilt, step, jaggedness and
/// slant, and a blunter axe wedge. An axe cut or natural tip that still
/// doesn't fit a very short stem becomes a flat saw cut. The options keep
/// the values given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndsOptions {
    /// The base end: the thick end of a log or branch. Stumps ignore it,
    /// since their base is under the ground.
    pub base: EndKind,
    /// The tip end: the thin end of a log or branch, or the top of a stump.
    pub tip: EndKind,
}

/// The shape of one end of a stem.
///
/// In JSON, an end is an object with a `"kind"` field, such as
/// `{"kind": "saw", "tilt": 10}`. With [`DeadwoodOptions::from_json`], the
/// "defaults in JSON" noted on each field apply when the JSON gives an end a
/// different kind from the default end and leaves that field out. If the
/// JSON names the same kind as the default end, a field left out keeps the
/// default end's value instead. For example, the default base is a saw cut
/// tilted 6°, so `{"ends": {"base": {"kind": "saw"}}}` keeps the 6° tilt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndKind {
    /// A flat saw cut, showing the growth rings in the cut face.
    Saw {
        /// Angle of the cut away from square (straight across the stem), in
        /// degrees, from 0 through 45. Defaults to 0 in JSON.
        #[serde(default)]
        tilt: f32,
    },
    /// A chainsaw felling cut: two flat faces at slightly different
    /// heights, with a small step between them. The step is where the last
    /// strip of uncut wood (the hinge) tore as the tree fell.
    Chainsaw {
        /// Angle of the cut away from square, in degrees, from 0 through 30.
        /// Defaults to 0 in JSON.
        #[serde(default)]
        tilt: f32,
        /// Height of the step as a fraction of the diameter, from 0 through
        /// 0.3. Defaults to 0.1 in JSON.
        #[serde(default = "default_step")]
        step: f32,
    },
    /// An axe cut: two faces meeting in a ridge, like a wedge.
    Axe {
        /// Angle between the two faces, in degrees, from 30 through 150.
        /// Smaller angles give a longer, sharper point. Defaults to 60 in
        /// JSON.
        #[serde(default = "default_axe_angle")]
        angle: f32,
    },
    /// A snapped, jagged break with splinters.
    Snapped {
        /// How rough the break is, as a fraction of the diameter, from 0
        /// through 1. Defaults to 0.35 in JSON.
        #[serde(default = "default_jaggedness")]
        jaggedness: f32,
        /// Number of long splinters sticking out of the break, from 0 up to
        /// [`MAX_SPLINTERS`]. Defaults to 6 in JSON.
        #[serde(default = "default_splinters")]
        splinters: u32,
        /// How much the break slants to one side, from 0 (even) through 1.
        /// Defaults to 0.4 in JSON.
        #[serde(default = "default_one_sided")]
        one_sided: f32,
    },
    /// A natural, rounded tip covered in bark, as at the end of a twig.
    Taper,
}

/// Short broken-off branches sticking out of the main stem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StubOptions {
    /// Stubs per metre of main stem, from 0 (none) through
    /// [`MAX_STUBS_PER_METRE`].
    pub per_metre: f32,
    /// Stub length as a multiple of the main diameter, from 0.05 through 3.
    pub length: f32,
    /// Stub radius as a fraction of the main stem's radius where it grows,
    /// from 0.05 through 0.8.
    pub radius: f32,
    /// Angle between a stub and the main stem, in degrees, from 10 through
    /// 90.
    pub angle: f32,
    /// Fraction of the stubs still attached, from 0 through 1. `None` takes
    /// it from decay: all of them until class 2, most gone by class 4.
    pub keep: Option<f32>,
}

/// Side branches and twigs of a fallen [`PieceKind::Branch`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchingOptions {
    /// How many levels of side branches grow on each other, from 0 (a bare
    /// stick) through [`MAX_LEVELS`]. Twigs come on top of these.
    pub levels: u32,
    /// Side branches per branch, from 0 up to [`MAX_CHILDREN`].
    pub children: u32,
    /// Length of a side branch as a fraction of its parent's length, from
    /// 0.05 through 1.
    pub length: f32,
    /// Radius of a side branch as a fraction of its parent's radius where it
    /// grows, from 0.1 through 0.9.
    pub radius: f32,
    /// Angle between a side branch and its parent, in degrees, from 5
    /// through 90.
    pub angle: f32,
    /// Twigs on each of the thinnest branches, from 0 up to
    /// [`MAX_CHILDREN`].
    pub twigs: u32,
    /// Twig length as a fraction of the branch it grows on, from 0.02
    /// through 0.6.
    pub twig_length: f32,
    /// Fraction of side branches still attached, from 0 through 1. `None`
    /// takes it from decay: all of them until class 2, many gone by class 4.
    pub keep: Option<f32>,
    /// Fraction of twigs still attached, from 0 through 1. `None` takes it
    /// from decay: all of them when fresh, none by class 2.
    pub twig_keep: Option<f32>,
}

/// Bark thickness, peeling and cracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarkOptions {
    /// Bark thickness as a fraction of the radius, from 0.01 through 0.3.
    /// Where bark has peeled off, the surface sinks by this much.
    pub thickness: f32,
    /// Fraction of the bark that has peeled off, from 0 (all there) through
    /// 1 (bare wood). `None` takes it from decay: intact until class 1.5,
    /// gone by class 4.
    pub stripped: Option<f32>,
    /// Typical size of a peeled patch, in metres. Must be positive.
    pub patch_size: f32,
    /// Depth of the cracks between bark plates, from 0 (smooth bark) through
    /// 1 (cracks reach through the bark).
    pub cracks: f32,
    /// Typical width of a bark plate, in metres. Plates are about three
    /// times longer along the stem. Must be positive.
    pub plate_size: f32,
}

/// A hollow core, as left by rot inside an old log or stump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HollowOptions {
    /// Whether the piece is hollow. Branches are never hollow.
    pub enabled: bool,
    /// Radius of the hollow as a fraction of the outer radius, from 0.2
    /// through 0.85.
    pub radius: f32,
    /// Whether the hollow opens at the base end. Stumps ignore this.
    pub open_base: bool,
    /// Whether the hollow opens at the tip end (the top of a stump).
    pub open_tip: bool,
    /// How deep the hollow reaches from an open end, from 0.05 through 1,
    /// as a fraction of the stem between the two ends' cuts. At 1 it reaches
    /// as deep as it can while the far end stays closed. When both ends are
    /// open it runs all the way through, whatever this is.
    pub depth: f32,
    /// How uneven the hollow's wall is, from 0 (smooth) through 0.5.
    pub roughness: f32,
}

/// The flared base and surface roots of a stump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootOptions {
    /// How much wider the stump is at the ground than higher up, as a
    /// fraction of its radius, from 0 through 2.
    pub flare: f32,
    /// Height of the flare above the ground, as a multiple of the diameter,
    /// from 0.05 through 3.
    pub flare_height: f32,
    /// Number of buttresses: ridges where roots join the trunk, from 0 up
    /// to [`MAX_LOBES`].
    pub lobes: u32,
    /// How strongly the buttresses stand out, from 0 through 1.
    pub lobe_depth: f32,
    /// Number of roots running out over the ground, from 0 up to
    /// [`MAX_ROOTS`].
    pub count: u32,
    /// Root length as a multiple of the diameter, from 0.1 through 6.
    pub length: f32,
    /// Root radius at the trunk as a fraction of the trunk radius, from 0.05
    /// through 0.6.
    pub radius: f32,
    /// How steeply the roots dive into the soil, in degrees below level,
    /// from 0 through 60.
    pub pitch: f32,
    /// How far the trunk reaches below the ground, as a multiple of the
    /// diameter, from 0.05 through 2.
    pub depth: f32,
}

/// Colours and surface appearance.
///
/// Colours are written as `"#rrggbb"` hex strings (the `#` is optional) in
/// the usual sRGB colour space, as in image editors. They end up in the mesh
/// as per-vertex colours, which renderers multiply with any texture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialOptions {
    /// Bark colour of freshly fallen wood.
    pub bark: String,
    /// Bark colour after weathering. Bark blends toward it with decay.
    pub bark_weathered: String,
    /// Colour of the outer wood under the bark, where bark has peeled off
    /// and in the outer rings of a cut end.
    pub sapwood: String,
    /// Colour of the core of a cut end.
    pub heartwood: String,
    /// Colour of rotten wood. Everything blends toward it with decay, and
    /// the inside of a hollow uses it.
    pub rot: String,
    /// Name of the bark texture set a renderer should use, one of
    /// [`crate::BARK_TEXTURE_SETS`] such as `"bark_brown_02"`, or `None`
    /// for vertex colours only.
    pub bark_texture: Option<String>,
    /// Width of bark, in metres, that one repeat of the bark texture
    /// covers on the main stem. Must be positive. The texture wraps a whole
    /// number of times around each stem, so the actual width is rounded to
    /// fit, and thinner side stems get proportionally finer bark.
    pub bark_texture_size: f32,
    /// Name of the end-grain texture set a renderer should draw on sawn
    /// ends, one of [`crate::END_TEXTURE_SETS`] such as `"TreeEnd005"`, or
    /// `None` for drawn growth rings only. The photo is stretched to fit
    /// each sawn end; broken and axe-cut ends don't use it.
    pub end_texture: Option<String>,
    /// Surface roughness for lighting, from 0 (glossy) through 1 (matte).
    pub roughness: f32,
}

/// How a piece comes to rest on the ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleOptions {
    /// Turns the piece onto a stable resting position, as if dropped on flat
    /// ground, so it lies on its stubs, branches or bumps. When false, the
    /// piece keeps its generated position and is only lifted to the ground.
    /// Stumps never settle.
    pub settle: bool,
    /// How far the piece sags between its supports, from 0 (stiff) through
    /// 1 (lies fully on the ground). `None` takes it from decay: stiff until
    /// class 1, fully sagged by class 4. Stumps ignore it.
    pub sag: Option<f32>,
    /// Suggested burial depth as a fraction of the diameter, from 0 through
    /// 0.5. `None` takes it from decay: none until class 3.5, up to 0.3 at
    /// class 5. The depth is only reported, in
    /// [`crate::Placement::burial`]; lower the piece by it to sink it.
    /// Stumps ignore it and always report 0.
    pub burial: Option<f32>,
}

impl Default for DeadwoodOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            kind: PieceKind::Log,
            decay: 1.0,
            length: 3.0,
            diameter: 0.35,
            taper: 0.75,
            shape: ShapeOptions::default(),
            ends: EndsOptions::default(),
            stubs: StubOptions::default(),
            branching: BranchingOptions::default(),
            bark: BarkOptions::default(),
            hollow: HollowOptions::default(),
            roots: RootOptions::default(),
            material: MaterialOptions::default(),
            settle: SettleOptions::default(),
            detail: DeadwoodDetail::FULL,
        }
    }
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            bend: 0.04,
            wobble: 0.15,
            lumps: 0.06,
            lump_scale: 3.0,
            ovality: None,
            lean: 4.0,
        }
    }
}

impl Default for EndsOptions {
    fn default() -> Self {
        Self {
            base: EndKind::Saw { tilt: 6.0 },
            tip: EndKind::Snapped {
                jaggedness: 0.35,
                splinters: 6,
                one_sided: 0.4,
            },
        }
    }
}

impl Default for StubOptions {
    fn default() -> Self {
        Self {
            per_metre: 1.5,
            length: 0.35,
            radius: 0.25,
            angle: 55.0,
            keep: None,
        }
    }
}

impl Default for BranchingOptions {
    fn default() -> Self {
        Self {
            levels: 1,
            children: 4,
            length: 0.4,
            radius: 0.5,
            angle: 40.0,
            twigs: 4,
            twig_length: 0.15,
            keep: None,
            twig_keep: None,
        }
    }
}

impl Default for BarkOptions {
    fn default() -> Self {
        Self {
            thickness: 0.08,
            stripped: None,
            patch_size: 0.25,
            cracks: 0.4,
            plate_size: 0.05,
        }
    }
}

impl Default for HollowOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 0.6,
            open_base: true,
            open_tip: false,
            depth: 0.5,
            roughness: 0.2,
        }
    }
}

impl Default for RootOptions {
    fn default() -> Self {
        Self {
            flare: 0.5,
            flare_height: 0.6,
            lobes: 5,
            lobe_depth: 0.5,
            count: 4,
            length: 1.5,
            radius: 0.25,
            pitch: 18.0,
            depth: 0.4,
        }
    }
}

impl Default for MaterialOptions {
    fn default() -> Self {
        Self {
            bark: "#5a4a3c".into(),
            bark_weathered: "#7d776e".into(),
            sapwood: "#bfae94".into(),
            heartwood: "#9c7350".into(),
            rot: "#5b3a24".into(),
            bark_texture: Some("bark_brown_02".into()),
            bark_texture_size: 0.5,
            end_texture: Some("TreeEnd005".into()),
            roughness: 0.85,
        }
    }
}

impl Default for SettleOptions {
    fn default() -> Self {
        Self {
            settle: true,
            sag: None,
            burial: None,
        }
    }
}

/// A setting that is out of its allowed range.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OptionsError {
    /// A whole-number setting is outside `min..=max`.
    IntOutOfRange {
        /// The setting's name, such as `"roots.count"` or
        /// `"ends.tip.splinters"`.
        field: &'static str,
        /// The value given.
        value: u32,
        /// Smallest allowed value.
        min: u32,
        /// Largest allowed value.
        max: u32,
    },
    /// A decimal setting is not a finite number, or breaks a rule such as
    /// "positive".
    FloatOutOfRange {
        /// The setting's name.
        field: &'static str,
        /// The value given.
        value: f32,
        /// What the value must be, such as `"positive"`.
        requirement: &'static str,
    },
    /// A decimal setting is outside `min..=max`, or not a finite number.
    FloatNotInRange {
        /// The setting's name, such as `"decay"` or `"ends.base.tilt"`.
        field: &'static str,
        /// The value given.
        value: f32,
        /// Smallest allowed value.
        min: f32,
        /// Largest allowed value.
        max: f32,
    },
    /// A colour is not a `"#rrggbb"` hex string.
    Colour {
        /// The setting's name.
        field: &'static str,
        /// The text given.
        value: String,
    },
    /// [`MaterialOptions::bark_texture`] names a set that isn't in
    /// [`crate::BARK_TEXTURE_SETS`].
    BarkTexture {
        /// The name given.
        value: String,
    },
    /// [`MaterialOptions::end_texture`] names a set that isn't in
    /// [`crate::END_TEXTURE_SETS`].
    EndTexture {
        /// The name given.
        value: String,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionsError::IntOutOfRange {
                field,
                value,
                min,
                max,
            } => write!(
                f,
                "{field} is {value}, but must be from {min} through {max}"
            ),
            OptionsError::FloatOutOfRange {
                field,
                value,
                requirement,
            } => write!(
                f,
                "{field} is {value}, but must be finite and {requirement}"
            ),
            OptionsError::FloatNotInRange {
                field,
                value,
                min,
                max,
            } => write!(
                f,
                "{field} is {value}, but must be from {min} through {max}"
            ),
            OptionsError::Colour { field, value } => write!(
                f,
                "{field} is {value:?}, but must be a colour such as \"#a0522d\""
            ),
            OptionsError::BarkTexture { value } => write!(
                f,
                "material.bark_texture is {value:?}, but must be one of the names in \
                 BARK_TEXTURE_SETS"
            ),
            OptionsError::EndTexture { value } => write!(
                f,
                "material.end_texture is {value:?}, but must be one of the names in \
                 END_TEXTURE_SETS"
            ),
        }
    }
}

impl std::error::Error for OptionsError {}

impl DeadwoodOptions {
    /// Checks every setting, including [`detail`](Self::detail), and reports
    /// the first problem found.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] naming the first invalid setting. Ends
    /// too long for the stem are not an error; see [`EndsOptions`].
    pub fn validate(&self) -> Result<(), OptionsError> {
        within("decay", self.decay, 0.0, MAX_DECAY)?;
        positive("length", self.length)?;
        positive("diameter", self.diameter)?;
        within("taper", self.taper, 0.05, 1.0)?;

        let s = &self.shape;
        within("shape.bend", s.bend, 0.0, 0.5)?;
        within("shape.wobble", s.wobble, 0.0, 1.0)?;
        within("shape.lumps", s.lumps, 0.0, 0.4)?;
        positive("shape.lump_scale", s.lump_scale)?;
        optional_within("shape.ovality", s.ovality, 0.0, 0.6)?;
        within("shape.lean", s.lean, 0.0, 45.0)?;

        self.ends.base.validate(&BASE_FIELDS)?;
        self.ends.tip.validate(&TIP_FIELDS)?;

        let st = &self.stubs;
        within("stubs.per_metre", st.per_metre, 0.0, MAX_STUBS_PER_METRE)?;
        within("stubs.length", st.length, 0.05, 3.0)?;
        within("stubs.radius", st.radius, 0.05, 0.8)?;
        within("stubs.angle", st.angle, 10.0, 90.0)?;
        optional_within("stubs.keep", st.keep, 0.0, 1.0)?;

        let b = &self.branching;
        int_range("branching.levels", b.levels, 0, MAX_LEVELS)?;
        int_range("branching.children", b.children, 0, MAX_CHILDREN)?;
        within("branching.length", b.length, 0.05, 1.0)?;
        within("branching.radius", b.radius, 0.1, 0.9)?;
        within("branching.angle", b.angle, 5.0, 90.0)?;
        int_range("branching.twigs", b.twigs, 0, MAX_CHILDREN)?;
        within("branching.twig_length", b.twig_length, 0.02, 0.6)?;
        optional_within("branching.keep", b.keep, 0.0, 1.0)?;
        optional_within("branching.twig_keep", b.twig_keep, 0.0, 1.0)?;

        let k = &self.bark;
        within("bark.thickness", k.thickness, 0.01, 0.3)?;
        optional_within("bark.stripped", k.stripped, 0.0, 1.0)?;
        positive("bark.patch_size", k.patch_size)?;
        within("bark.cracks", k.cracks, 0.0, 1.0)?;
        positive("bark.plate_size", k.plate_size)?;

        let h = &self.hollow;
        within("hollow.radius", h.radius, 0.2, 0.85)?;
        within("hollow.depth", h.depth, 0.05, 1.0)?;
        within("hollow.roughness", h.roughness, 0.0, 0.5)?;

        let r = &self.roots;
        within("roots.flare", r.flare, 0.0, 2.0)?;
        within("roots.flare_height", r.flare_height, 0.05, 3.0)?;
        int_range("roots.lobes", r.lobes, 0, MAX_LOBES)?;
        within("roots.lobe_depth", r.lobe_depth, 0.0, 1.0)?;
        int_range("roots.count", r.count, 0, MAX_ROOTS)?;
        within("roots.length", r.length, 0.1, 6.0)?;
        within("roots.radius", r.radius, 0.05, 0.6)?;
        within("roots.pitch", r.pitch, 0.0, 60.0)?;
        within("roots.depth", r.depth, 0.05, 2.0)?;

        let m = &self.material;
        for (field, value) in [
            ("material.bark", &m.bark),
            ("material.bark_weathered", &m.bark_weathered),
            ("material.sapwood", &m.sapwood),
            ("material.heartwood", &m.heartwood),
            ("material.rot", &m.rot),
        ] {
            if parse_hex(value).is_none() {
                return Err(OptionsError::Colour {
                    field,
                    value: value.clone(),
                });
            }
        }
        if let Some(set) = &m.bark_texture
            && !BARK_TEXTURE_SETS.contains(&set.as_str())
        {
            return Err(OptionsError::BarkTexture { value: set.clone() });
        }
        if let Some(set) = &m.end_texture
            && !END_TEXTURE_SETS.contains(&set.as_str())
        {
            return Err(OptionsError::EndTexture { value: set.clone() });
        }
        positive("material.bark_texture_size", m.bark_texture_size)?;
        within("material.roughness", m.roughness, 0.0, 1.0)?;

        let se = &self.settle;
        optional_within("settle.sag", se.sag, 0.0, 1.0)?;
        optional_within("settle.burial", se.burial, 0.0, 0.5)?;

        self.detail.validate()
    }

    /// Reads options from JSON, filling anything left out with the
    /// defaults. Nested objects merge field by field, so
    /// `{"bark": {"cracks": 0.8}}` changes only the crack depth. An end that
    /// names a different `kind` from the default replaces it whole; see
    /// [`EndKind`].
    ///
    /// The values are not checked; see [`validate`](Self::validate).
    ///
    /// # Errors
    ///
    /// Returns the parse error if the text is not valid JSON or a value has
    /// the wrong type.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let source: serde_json::Value = serde_json::from_str(json)?;
        let mut merged = serde_json::to_value(Self::default())?;
        merge_json(&mut merged, source);
        serde_json::from_value(merged)
    }

    /// Returns every setting, including those at their defaults, as indented
    /// JSON that [`from_json`](Self::from_json) reads back.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("deadwood options always serialize to JSON")
    }

    /// Radius at the base of the main stem, in metres.
    pub fn base_radius(&self) -> f32 {
        self.diameter * 0.5
    }

    /// Radius at the tip of the main stem, in metres.
    pub fn tip_radius(&self) -> f32 {
        self.diameter * 0.5 * self.taper
    }
}

/// The setting names one end's values are reported under in errors.
struct EndFields {
    tilt: &'static str,
    step: &'static str,
    angle: &'static str,
    jaggedness: &'static str,
    splinters: &'static str,
    one_sided: &'static str,
}

/// Setting names for [`EndsOptions::base`].
const BASE_FIELDS: EndFields = EndFields {
    tilt: "ends.base.tilt",
    step: "ends.base.step",
    angle: "ends.base.angle",
    jaggedness: "ends.base.jaggedness",
    splinters: "ends.base.splinters",
    one_sided: "ends.base.one_sided",
};

/// Setting names for [`EndsOptions::tip`].
const TIP_FIELDS: EndFields = EndFields {
    tilt: "ends.tip.tilt",
    step: "ends.tip.step",
    angle: "ends.tip.angle",
    jaggedness: "ends.tip.jaggedness",
    splinters: "ends.tip.splinters",
    one_sided: "ends.tip.one_sided",
};

impl EndKind {
    /// Checks this end's settings, reporting a bad value under its name in
    /// `names`.
    fn validate(&self, names: &EndFields) -> Result<(), OptionsError> {
        match *self {
            EndKind::Saw { tilt } => within(names.tilt, tilt, 0.0, 45.0),
            EndKind::Chainsaw { tilt, step } => {
                within(names.tilt, tilt, 0.0, 30.0)?;
                within(names.step, step, 0.0, 0.3)
            }
            EndKind::Axe { angle } => within(names.angle, angle, 30.0, 150.0),
            EndKind::Snapped {
                jaggedness,
                splinters,
                one_sided,
            } => {
                within(names.jaggedness, jaggedness, 0.0, 1.0)?;
                int_range(names.splinters, splinters, 0, MAX_SPLINTERS)?;
                within(names.one_sided, one_sided, 0.0, 1.0)
            }
            EndKind::Taper => Ok(()),
        }
    }

    /// Returns the furthest this end's surface can lie from the end's
    /// reference cross-section (a flat slice across the stem where the end
    /// is placed), measured along the stem in metres, for a stem of the
    /// given `radius`.
    ///
    /// Depending on its kind, an end can stick out past that slice or dip
    /// in behind it, so the end can take up twice this much of the stem's
    /// length; see [`span`](Self::span).
    pub fn reach(&self, radius: f32) -> f32 {
        let diameter = 2.0 * radius;
        match *self {
            EndKind::Saw { tilt } => tilt.to_radians().tan() * radius,
            EndKind::Chainsaw { tilt, step } => {
                tilt.to_radians().tan() * radius + 0.5 * step * diameter
            }
            EndKind::Axe { angle } => 0.5 * radius / (0.5 * angle.to_radians()).tan() * 1.1,
            EndKind::Snapped {
                jaggedness,
                one_sided,
                ..
            } => jaggedness * diameter * (SNAP_BASE + SNAP_SPIKE + one_sided),
            EndKind::Taper => radius,
        }
    }

    /// Returns how much of the stem's length, in metres, this end can take
    /// up for a stem of the given `radius`. This is twice its
    /// [`reach`](Self::reach).
    pub fn span(&self, radius: f32) -> f32 {
        2.0 * self.reach(radius)
    }
}

/// JSON default for [`EndKind::Chainsaw`]'s `step`.
fn default_step() -> f32 {
    0.1
}

/// JSON default for [`EndKind::Axe`]'s `angle`.
fn default_axe_angle() -> f32 {
    60.0
}

/// JSON default for [`EndKind::Snapped`]'s `jaggedness`.
fn default_jaggedness() -> f32 {
    0.35
}

/// JSON default for [`EndKind::Snapped`]'s `splinters`.
fn default_splinters() -> u32 {
    6
}

/// JSON default for [`EndKind::Snapped`]'s `one_sided`.
fn default_one_sided() -> f32 {
    0.4
}

/// Largest height of a snapped break's rough surface, before splinters and
/// slant, as a multiple of its jaggedness times the diameter.
pub(crate) const SNAP_BASE: f32 = 0.5;
/// Largest splinter height, as a multiple of a break's jaggedness times the
/// diameter.
pub(crate) const SNAP_SPIKE: f32 = 1.5;

/// Copies the JSON values in `source` over the defaults in `target`.
/// Objects merge key by key, all the way down; arrays and other values
/// replace the old value whole.
///
/// An object whose `kind` differs from the one in `target`, such as a
/// saw-cut end replacing a snapped one, replaces it whole, so the old kind's
/// fields don't carry over. An object with the same `kind` merges as usual.
fn merge_json(target: &mut serde_json::Value, source: serde_json::Value) {
    use serde_json::Value;
    match (target, source) {
        (Value::Object(target), Value::Object(source))
            if source
                .get("kind")
                .is_some_and(|k| Some(k) != target.get("kind")) =>
        {
            *target = source;
        }
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(slot) => merge_json(slot, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, source) => *target = source,
    }
}

/// Checks that whole number `value` of setting `field` is in `min..=max`.
pub(crate) fn int_range(
    field: &'static str,
    value: u32,
    min: u32,
    max: u32,
) -> Result<(), OptionsError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(OptionsError::IntOutOfRange {
            field,
            value,
            min,
            max,
        })
    }
}

/// Checks that `value` is finite and above zero.
pub(crate) fn positive(field: &'static str, value: f32) -> Result<(), OptionsError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement: "positive",
        })
    }
}

/// Checks that `value` is finite and zero or more.
pub(crate) fn non_negative(field: &'static str, value: f32) -> Result<(), OptionsError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement: "zero or more",
        })
    }
}

/// Checks that `value` is finite and in `min..=max`.
pub(crate) fn within(
    field: &'static str,
    value: f32,
    min: f32,
    max: f32,
) -> Result<(), OptionsError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(OptionsError::FloatNotInRange {
            field,
            value,
            min,
            max,
        })
    }
}

/// Checks an optional override with [`within`]; `None` is always valid.
fn optional_within(
    field: &'static str,
    value: Option<f32>,
    min: f32,
    max: f32,
) -> Result<(), OptionsError> {
    value.map_or(Ok(()), |v| within(field, v, min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        DeadwoodOptions::default().validate().unwrap();
    }

    #[test]
    fn json_merges_over_defaults() {
        let o = DeadwoodOptions::from_json(r#"{"bark": {"cracks": 0.8}, "decay": 3}"#).unwrap();
        assert_eq!(o.bark.cracks, 0.8);
        assert_eq!(o.decay, 3.0);
        assert_eq!(o.bark.thickness, BarkOptions::default().thickness);
        assert_eq!(DeadwoodOptions::from_json(&o.to_json()).unwrap(), o);
    }

    #[test]
    fn json_can_switch_end_kind() {
        let o = DeadwoodOptions::from_json(r#"{"ends": {"tip": {"kind": "saw"}}}"#).unwrap();
        assert_eq!(o.ends.tip, EndKind::Saw { tilt: 0.0 });
        let o =
            DeadwoodOptions::from_json(r#"{"ends": {"tip": {"kind": "snapped", "splinters": 2}}}"#)
                .unwrap();
        assert_eq!(
            o.ends.tip,
            EndKind::Snapped {
                jaggedness: 0.35,
                splinters: 2,
                one_sided: 0.4
            }
        );
        o.validate().unwrap();
    }

    #[test]
    fn rejects_out_of_range() {
        let mut o = DeadwoodOptions {
            decay: 6.0,
            ..Default::default()
        };
        assert!(matches!(
            o.validate(),
            Err(OptionsError::FloatNotInRange { field: "decay", .. })
        ));
        o.decay = 1.0;
        o.material.rot = "brown".into();
        assert!(matches!(o.validate(), Err(OptionsError::Colour { .. })));
    }

    #[test]
    fn end_errors_name_the_bad_value() {
        let mut o = DeadwoodOptions::default();
        o.ends.tip = EndKind::Snapped {
            jaggedness: 0.2,
            splinters: MAX_SPLINTERS + 1,
            one_sided: 0.2,
        };
        assert!(matches!(
            o.validate(),
            Err(OptionsError::IntOutOfRange {
                field: "ends.tip.splinters",
                ..
            })
        ));
        o.ends.tip = EndKind::Taper;
        o.ends.base = EndKind::Saw { tilt: 50.0 };
        assert!(matches!(
            o.validate(),
            Err(OptionsError::FloatNotInRange {
                field: "ends.base.tilt",
                ..
            })
        ));
    }

    #[test]
    fn bark_texture_must_be_a_known_set() {
        let mut o = DeadwoodOptions::default();
        o.material.bark_texture = Some("no_such_bark".into());
        assert!(matches!(
            o.validate(),
            Err(OptionsError::BarkTexture { value }) if value == "no_such_bark"
        ));
        o.material.bark_texture = None;
        o.validate().unwrap();
        o.material.bark_texture_size = 0.0;
        assert!(o.validate().is_err());
    }

    #[test]
    fn end_texture_must_be_a_known_set() {
        let mut o = DeadwoodOptions::default();
        o.material.end_texture = Some("no_such_end".into());
        assert!(matches!(
            o.validate(),
            Err(OptionsError::EndTexture { value }) if value == "no_such_end"
        ));
        o.material.end_texture = None;
        o.validate().unwrap();
    }

    #[test]
    fn accepts_ends_longer_than_the_stem() {
        // The piece shrinks them to fit when built; see the build tests.
        let o = DeadwoodOptions {
            length: 0.3,
            ends: EndsOptions {
                base: EndKind::Axe { angle: 30.0 },
                tip: EndKind::Snapped {
                    jaggedness: 1.0,
                    splinters: 4,
                    one_sided: 1.0,
                },
            },
            ..Default::default()
        };
        o.validate().unwrap();
    }
}
