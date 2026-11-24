//! Settings for a mushroom's shape, colours, grouping and mesh detail.
//!
//! Start with [`MushroomOptions::default()`], a [`crate::MushroomPreset`], or
//! a partial JSON object loaded with [`MushroomOptions::from_json`]. Creating
//! a [`crate::Mushroom`] validates the settings; call
//! [`MushroomOptions::validate`] to check edits before that.

use std::fmt;

use glam::Vec2;
use procgena_core::LodError;
use serde::{Deserialize, Serialize};

use crate::cluster::ClusterOptions;
use crate::colour::parse_hex;
use crate::detail::MushroomDetail;
use crate::profile::{CapFamily, Profile};
use crate::texture::TexturePart;

/// Largest allowed [`CapOptions::lobes`].
pub const MAX_LOBES: u32 = 12;
/// Largest allowed [`WartOptions::count`].
pub const MAX_WARTS: u32 = 400;
/// Largest allowed [`MaterialOptions::gill_lines`].
pub const MAX_GILL_LINES: u32 = 256;
/// Largest allowed segment count in a [`MushroomDetail`].
pub const MAX_SEGMENTS: u32 = 256;

/// Everything that describes one mushroom, or a group of them: shape,
/// colours, how many grow together, and the default mesh detail.
///
/// Only [`size`](Self::size) is in metres. Other lengths are fractions or
/// multiples of the cap or stem, as each field says, so a mushroom keeps its
/// proportions when you change its size.
///
/// Setting fields or parsing JSON does not check the values. Call
/// [`validate`](Self::validate), or create a [`crate::Mushroom`], which
/// checks them for you.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MushroomOptions {
    /// Number that picks the random details: surface bumps, rim waves, wart
    /// positions, the annulus's ragged edge, and where the members of a
    /// cluster stand. The same seed and settings always give the same
    /// mushroom; change it for a variation.
    ///
    /// The seed is not used directly: it stands in for the World seed of
    /// the mushroom's [`specimen_id`](Self::specimen_id), which seeds the
    /// shape. It is left out of the options hash.
    pub seed: u32,
    /// Cap diameter in metres. Must be positive. Defaults to 0.08 (8 cm).
    pub size: f32,
    /// Cap outline and surface.
    pub cap: CapOptions,
    /// The cap's underside, where a real mushroom has its gills.
    pub underside: UndersideOptions,
    /// Stem shape above the ground.
    pub stem: StemOptions,
    /// The swollen foot of the stem and its closed, rounded bottom.
    pub bulb: BulbOptions,
    /// The annulus: a thin skirt some species have around the stem.
    pub annulus: AnnulusOptions,
    /// Raised warts or scales on top of the cap.
    pub warts: WartOptions,
    /// Colours and surface appearance.
    pub material: MaterialOptions,
    /// How many mushrooms grow together, and how they are arranged.
    pub cluster: ClusterOptions,
    /// Mesh detail that [`crate::Mushroom::mesh`] uses. Other methods take
    /// their detail as an argument.
    pub detail: MushroomDetail,
}

/// Settings for the cap's outline and surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapOptions {
    /// Side-view outline, such as a rounded dome or a funnel.
    pub family: CapFamily,
    /// Multiplies every height in the outline: below 1 flattens the cap,
    /// above 1 makes it taller. Must be positive. Defaults to 1.
    pub height: f32,
    /// Number of gentle waves around the rim, from 0 (a round rim) up to
    /// [`MAX_LOBES`]. Only has an effect when
    /// [`lobe_depth`](Self::lobe_depth) is above 0.
    pub lobes: u32,
    /// Depth of the rim waves as a fraction of the cap radius, from 0
    /// through 0.5. The waves are deepest at the rim and fade out toward the
    /// centre.
    pub lobe_depth: f32,
    /// How lopsided the cap is, from 0 (round) through 1, where each side
    /// can be up to 30% wider or narrower than the others.
    pub asymmetry: f32,
    /// How far the cap tips away from the stem's direction, in degrees, from
    /// 0 through 45. The direction of the tilt comes from the seed.
    pub tilt: f32,
    /// Height of the bumps on the cap surface as a fraction of the cap
    /// radius, from 0 (smooth) through 0.2.
    pub roughness: f32,
    /// Roughly how many bumps fit across the cap. Must be positive.
    pub roughness_scale: f32,
}

/// Settings for the underside of the cap.
///
/// The underside is only built at solid detail levels; open levels show the
/// cap's top surface alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndersideOptions {
    /// Thickness of the cap at its rim, as a fraction of the cap radius,
    /// from 0 through 0.5. 0 lets the underside meet the top in a sharp edge.
    pub rim_thickness: f32,
    /// How far the underside bulges downward halfway between rim and stem,
    /// as a fraction of the cap radius, from -0.5 through 0.5. Negative
    /// values curve it upward into a hollow.
    pub sag: f32,
    /// How far the underside runs down the outside of the stem, as a
    /// fraction of the cap radius, from 0 through 1. Mushrooms such as
    /// chanterelles have these "decurrent" gills; 0 stops the underside
    /// where it meets the stem.
    pub decurrent: f32,
}

/// Settings for the stem above the ground. Its foot is set by
/// [`BulbOptions`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemOptions {
    /// Stem height as a multiple of the cap diameter. Must be positive.
    pub height: f32,
    /// Radius at the top of the stem as a fraction of the cap diameter,
    /// from 0.01 through 0.4.
    pub radius: f32,
    /// Radius at the ground as a multiple of the top radius, before any
    /// bulb. Must be positive. Above 1 widens the stem toward the ground.
    pub base: f32,
    /// Extra radius just under the cap, as a multiple of the top radius,
    /// from 0 through 1. It flares the stem where it joins the cap.
    pub collar: f32,
    /// How far the top of the stem is offset sideways, as a multiple of the
    /// stem height, from 0 (upright) through 1.
    pub lean: f32,
    /// How the lean is spread along the stem, from 1 through 4. 1 gives a
    /// straight, slanted stem; larger values keep the base upright and bend
    /// the stem near the top.
    pub curve: f32,
    /// How much a leaning stem turns back toward vertical near the top, from
    /// 0 through 1. At 0 the stem keeps bending outward to the cap; at 1 it
    /// sweeps out and then curves back up, so the cap faces upward again,
    /// like the long stalks of a clustered mushroom. Has no effect without
    /// [`lean`](Self::lean).
    pub upturn: f32,
    /// Direction of the lean in degrees, measured around the vertical axis
    /// from +X toward +Z. Any finite value is allowed. Ignored for the
    /// members of a cluster, which lean in directions the cluster picks.
    pub lean_direction: f32,
    /// Height of the bumps on the stem surface as a fraction of its radius,
    /// from 0 (smooth) through 0.3.
    pub roughness: f32,
}

/// Settings for the stem's foot: a swelling just above the ground (the
/// basal bulb), and a rounded bottom that closes the stem below the ground.
///
/// The ground is at height 0, where the mushroom stands. A closed bottom
/// hangs below it, so a mushroom placed on a surface looks rooted, and one
/// lifted off the ground has no hole underneath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulbOptions {
    /// Whether the stem is closed underneath by a rounded bottom below the
    /// ground. Turn it off for mushrooms planted in a landscape, where the
    /// bottom would be buried: the stem then ends, open, at the ground,
    /// which saves triangles.
    pub closed: bool,
    /// Extra radius of the swelling at the ground, as a multiple of the
    /// stem's top radius, from 0 (none) through 3.
    pub size: f32,
    /// Fraction of the stem height, from the ground up, over which the
    /// swelling narrows back into the stem, from 0.01 through 1.
    pub height: f32,
    /// How far the closed bottom reaches below the ground, as a multiple of
    /// the stem's radius at the ground, from 0.05 through 2. Only used when
    /// [`closed`](Self::closed) is on.
    pub depth: f32,
}

/// Settings for the annulus: a thin skirt hanging from the stem, as on a
/// fly agaric. It is what is left of a veil that covered the gills while
/// the mushroom was young.
///
/// The annulus is built as a single sheet with no thickness, so it is cheap
/// and never closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnulusOptions {
    /// Whether the mushroom has an annulus.
    pub enabled: bool,
    /// Where the annulus is attached, as a fraction of the stem height from
    /// the ground, from 0.1 through 1.
    pub position: f32,
    /// How far the skirt hangs down, as a fraction of the cap diameter, from
    /// 0.01 through 0.5.
    pub length: f32,
    /// Radius of the skirt's lower edge as a multiple of the stem radius
    /// where it hangs, from 1 (hugging the stem) through 4.
    pub flare: f32,
    /// How torn the lower edge looks, from 0 (even) through 1.
    pub raggedness: f32,
}

/// Settings for warts or scales on top of the cap.
///
/// Warts are not modelled. A renderer such as `procgena_mushroom_bevy`
/// draws them as shaded, raised-looking spots, so they cost no triangles at
/// any distance. For renderers without that shader, such as files written
/// by [`crate::export`], meshes paint them into the cap's vertex colours;
/// see [`MushroomDetail::paint_warts`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WartOptions {
    /// Roughly how many warts cover the cap, from 0 (none) up to
    /// [`MAX_WARTS`]. Warts never overlap, so fewer may fit on a small or
    /// crowded cap.
    pub count: u32,
    /// Wart radius as a fraction of the cap radius, from 0.005 through 0.3.
    pub size: f32,
    /// How much wart sizes vary, from 0 (all the same) through 1.
    pub size_jitter: f32,
    /// How raised the warts look, as a multiple of their radius, from 0.05
    /// through 2. Low values give flat scales; high values give pointed
    /// warts.
    pub height: f32,
    /// How much of the cap carries warts, measured from the top centre, from
    /// 0.05 (a patch at the top) through 1 (out to the rim).
    pub coverage: f32,
}

/// Colours and surface appearance.
///
/// Colours are written as `"#rrggbb"` hex strings in the usual sRGB colour
/// space, as in image editors, such as `"#c8281e"` for a fly-agaric red.
/// They are stored in the mesh as per-vertex colours, which renderers
/// multiply with any texture. Gill lines, warts and the cap pattern are not
/// in the mesh: a renderer such as `procgena_mushroom_bevy` draws them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialOptions {
    /// Cap colour at the top centre.
    pub cap_centre: String,
    /// Cap colour at the rim. The cap blends from centre to rim colour.
    pub cap_rim: String,
    /// Colour of the underside (the gills).
    pub underside: String,
    /// Stem colour just under the cap.
    pub stem_top: String,
    /// Stem colour at the ground. The stem blends from base to top colour.
    pub stem_base: String,
    /// Annulus colour.
    pub annulus: String,
    /// Wart colour.
    pub wart: String,
    /// Whether a renderer draws thin lines on the underside, radiating from
    /// the stem, to suggest gills. Turn it off for a smooth underside, such
    /// as the pores of a bolete.
    pub gills: bool,
    /// Number of gill lines around the underside, from 1 up to
    /// [`MAX_GILL_LINES`]. Only used when [`gills`](Self::gills) is on.
    pub gill_lines: u32,
    /// How much the gill lines darken the underside, from 0 (invisible)
    /// through 1. Only used when [`gills`](Self::gills) is on.
    pub gill_strength: f32,
    /// Pattern a renderer draws on top of the cap.
    pub pattern: Pattern,
    /// Colour of the pattern.
    pub pattern_colour: String,
    /// Roughly how many pattern features (spots, cracked tiles or flecks)
    /// fit across the cap. Must be positive.
    pub pattern_scale: f32,
    /// How strongly the pattern colour shows, from 0 (invisible) through 1.
    pub pattern_strength: f32,
    /// How rough the surface looks, from 0 (glossy) through 1 (matt).
    pub roughness: f32,
    /// A texture set painted onto the top of the cap, one of
    /// [`CAP_TEXTURE_SETS`](crate::CAP_TEXTURE_SETS), or `None` for vertex
    /// colours only.
    ///
    /// The set's own colours replace the cap colours and the pattern on the
    /// cap's top; the rim, underside and stem keep theirs. Warts are drawn
    /// on top of it. Renderers that don't load textures ignore it.
    pub cap_texture: Option<String>,
    /// Width of the cap's top covered by one repeat of
    /// [`cap_texture`](Self::cap_texture), in metres. Must be positive.
    pub cap_texture_size: f32,
    /// A texture set painted onto the stem, one of
    /// [`STEM_TEXTURE_SETS`](crate::STEM_TEXTURE_SETS), or `None` for vertex
    /// colours only.
    ///
    /// The set adds surface detail while the stem keeps its own colours:
    /// renderers tint the texture so that its average colour matches
    /// [`stem_top`](Self::stem_top) and [`stem_base`](Self::stem_base) at
    /// each point. The annulus is not textured. Renderers that don't load
    /// textures ignore it.
    pub stem_texture: Option<String>,
    /// Length of stem covered by one repeat of
    /// [`stem_texture`](Self::stem_texture), in metres. Must be positive.
    pub stem_texture_size: f32,
}

/// A pattern on top of the cap, drawn by the renderer rather than stored in
/// the mesh, so it stays sharp at every detail level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// No pattern.
    #[default]
    None,
    /// Fine dark or light flecks.
    Speckle,
    /// A network of cracks, as on some boletes.
    Cracks,
    /// Round blotches, as on a stylised toadstool.
    Spots,
}

impl Default for MushroomOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            size: 0.08,
            cap: CapOptions::default(),
            underside: UndersideOptions::default(),
            stem: StemOptions::default(),
            bulb: BulbOptions::default(),
            annulus: AnnulusOptions::default(),
            warts: WartOptions::default(),
            material: MaterialOptions::default(),
            cluster: ClusterOptions::default(),
            detail: MushroomDetail::FULL,
        }
    }
}

impl Default for CapOptions {
    fn default() -> Self {
        Self {
            family: CapFamily::Convex,
            height: 1.0,
            lobes: 0,
            lobe_depth: 0.0,
            asymmetry: 0.15,
            tilt: 4.0,
            roughness: 0.015,
            roughness_scale: 6.0,
        }
    }
}

impl Default for UndersideOptions {
    fn default() -> Self {
        Self {
            rim_thickness: 0.05,
            sag: 0.05,
            decurrent: 0.0,
        }
    }
}

impl Default for StemOptions {
    fn default() -> Self {
        Self {
            height: 0.9,
            radius: 0.09,
            base: 1.2,
            collar: 0.1,
            lean: 0.08,
            curve: 1.6,
            upturn: 0.0,
            lean_direction: 0.0,
            roughness: 0.03,
        }
    }
}

impl Default for BulbOptions {
    fn default() -> Self {
        Self {
            closed: true,
            size: 0.2,
            height: 0.2,
            depth: 0.5,
        }
    }
}

impl Default for AnnulusOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            position: 0.8,
            length: 0.12,
            flare: 1.8,
            raggedness: 0.3,
        }
    }
}

impl Default for WartOptions {
    fn default() -> Self {
        Self {
            count: 0,
            size: 0.06,
            size_jitter: 0.5,
            height: 0.6,
            coverage: 0.85,
        }
    }
}

impl Default for MaterialOptions {
    fn default() -> Self {
        Self {
            cap_centre: "#6b4a2e".into(),
            cap_rim: "#a07a52".into(),
            underside: "#d8c7a8".into(),
            stem_top: "#e6dcc6".into(),
            stem_base: "#b9a888".into(),
            annulus: "#ece4d2".into(),
            wart: "#f2ece0".into(),
            gills: true,
            gill_lines: 72,
            gill_strength: 0.35,
            pattern: Pattern::None,
            pattern_colour: "#3a2616".into(),
            pattern_scale: 12.0,
            pattern_strength: 0.0,
            roughness: 0.75,
            cap_texture: None,
            cap_texture_size: 0.08,
            stem_texture: None,
            stem_texture_size: 0.04,
        }
    }
}

/// A setting with a value outside its allowed range.
///
/// Every variant names the setting by its path in the options, such as
/// `"cap.lobes"`, so you can show it to a user or find it in a JSON file.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OptionsError {
    /// A whole-number setting is below its minimum or above its maximum.
    IntOutOfRange {
        /// Name of the setting, such as `"cap.lobes"`.
        field: &'static str,
        /// The rejected value.
        value: u32,
        /// Inclusive lower limit.
        min: u32,
        /// Inclusive upper limit.
        max: u32,
    },
    /// A decimal setting is not a number, is infinite, or breaks a rule
    /// described in words, such as "positive".
    FloatOutOfRange {
        /// Name of the setting, such as `"stem.height"`.
        field: &'static str,
        /// The rejected value.
        value: f32,
        /// The rule it breaks, such as `"positive"`.
        requirement: &'static str,
    },
    /// A decimal setting is not a number or lies outside a range whose
    /// limits are both allowed.
    FloatNotInRange {
        /// Name of the setting, such as `"cap.tilt"`.
        field: &'static str,
        /// The rejected value.
        value: f32,
        /// Inclusive lower limit.
        min: f32,
        /// Inclusive upper limit.
        max: f32,
    },
    /// A colour is not a `#rrggbb` hex string.
    Colour {
        /// Name of the setting, such as `"material.cap_rim"`.
        field: &'static str,
        /// The rejected text.
        value: String,
    },
    /// A texture set name is not one of the sets made for that part, such as
    /// [`CAP_TEXTURE_SETS`](crate::CAP_TEXTURE_SETS).
    UnknownTexture {
        /// Name of the setting, such as `"material.cap_texture"`.
        field: &'static str,
        /// The rejected name.
        value: String,
        /// The names the setting accepts.
        allowed: &'static [&'static str],
    },
    /// The levels of a [`MushroomLod`](crate::MushroomLod) are invalid, for
    /// example out of order or blending into each other.
    Lod(LodError),
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
            OptionsError::Colour { field, value } => {
                write!(
                    f,
                    "{field} is {value:?}, but must be a colour such as \"#a0522d\""
                )
            }
            OptionsError::UnknownTexture {
                field,
                value,
                allowed,
            } => write!(f, "{field} is {value:?}, but must be one of {allowed:?}"),
            OptionsError::Lod(error) => write!(f, "invalid levels of detail: {error}"),
        }
    }
}

impl std::error::Error for OptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OptionsError::Lod(error) => Some(error),
            _ => None,
        }
    }
}

impl MushroomOptions {
    /// Checks every setting, including [`cluster`](Self::cluster) and
    /// [`detail`](Self::detail), against the range given in its
    /// documentation.
    ///
    /// # Errors
    ///
    /// Returns the first setting found out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        positive("size", self.size)?;

        let c = &self.cap;
        positive("cap.height", c.height)?;
        int_range("cap.lobes", c.lobes, 0, MAX_LOBES)?;
        within("cap.lobe_depth", c.lobe_depth, 0.0, 0.5)?;
        within("cap.asymmetry", c.asymmetry, 0.0, 1.0)?;
        within("cap.tilt", c.tilt, 0.0, 45.0)?;
        within("cap.roughness", c.roughness, 0.0, 0.2)?;
        positive("cap.roughness_scale", c.roughness_scale)?;

        let u = &self.underside;
        within("underside.rim_thickness", u.rim_thickness, 0.0, 0.5)?;
        within("underside.sag", u.sag, -0.5, 0.5)?;
        within("underside.decurrent", u.decurrent, 0.0, 1.0)?;

        let s = &self.stem;
        positive("stem.height", s.height)?;
        within("stem.radius", s.radius, 0.01, 0.4)?;
        positive("stem.base", s.base)?;
        within("stem.collar", s.collar, 0.0, 1.0)?;
        within("stem.lean", s.lean, 0.0, 1.0)?;
        within("stem.curve", s.curve, 1.0, 4.0)?;
        within("stem.upturn", s.upturn, 0.0, 1.0)?;
        finite("stem.lean_direction", s.lean_direction)?;
        within("stem.roughness", s.roughness, 0.0, 0.3)?;

        let b = &self.bulb;
        within("bulb.size", b.size, 0.0, 3.0)?;
        within("bulb.height", b.height, 0.01, 1.0)?;
        within("bulb.depth", b.depth, 0.05, 2.0)?;

        let a = &self.annulus;
        within("annulus.position", a.position, 0.1, 1.0)?;
        within("annulus.length", a.length, 0.01, 0.5)?;
        within("annulus.flare", a.flare, 1.0, 4.0)?;
        within("annulus.raggedness", a.raggedness, 0.0, 1.0)?;

        let w = &self.warts;
        int_range("warts.count", w.count, 0, MAX_WARTS)?;
        within("warts.size", w.size, 0.005, 0.3)?;
        within("warts.size_jitter", w.size_jitter, 0.0, 1.0)?;
        within("warts.height", w.height, 0.05, 2.0)?;
        within("warts.coverage", w.coverage, 0.05, 1.0)?;

        let m = &self.material;
        for (field, value) in [
            ("material.cap_centre", &m.cap_centre),
            ("material.cap_rim", &m.cap_rim),
            ("material.underside", &m.underside),
            ("material.stem_top", &m.stem_top),
            ("material.stem_base", &m.stem_base),
            ("material.annulus", &m.annulus),
            ("material.wart", &m.wart),
            ("material.pattern_colour", &m.pattern_colour),
        ] {
            if parse_hex(value).is_none() {
                return Err(OptionsError::Colour {
                    field,
                    value: value.clone(),
                });
            }
        }
        int_range("material.gill_lines", m.gill_lines, 1, MAX_GILL_LINES)?;
        within("material.gill_strength", m.gill_strength, 0.0, 1.0)?;
        positive("material.pattern_scale", m.pattern_scale)?;
        within("material.pattern_strength", m.pattern_strength, 0.0, 1.0)?;
        within("material.roughness", m.roughness, 0.0, 1.0)?;
        // Texture sets must be ones made for their part.
        for (field, set, part) in [
            ("material.cap_texture", &m.cap_texture, TexturePart::Cap),
            ("material.stem_texture", &m.stem_texture, TexturePart::Stem),
        ] {
            if let Some(set) = set
                && !part.sets().contains(&set.as_str())
            {
                return Err(OptionsError::UnknownTexture {
                    field,
                    value: set.clone(),
                    allowed: part.sets(),
                });
            }
        }
        positive("material.cap_texture_size", m.cap_texture_size)?;
        positive("material.stem_texture_size", m.stem_texture_size)?;

        self.cluster.validate()?;
        self.detail.validate()
    }

    /// Reads options from JSON, using the default for every setting the JSON
    /// leaves out.
    ///
    /// Nested objects may also be partial: `{"seed": 7, "cap": {"lobes": 5}}`
    /// changes only those two settings. Names this crate doesn't know, such
    /// as misspellings, are ignored and the setting keeps its default.
    /// Values are not range-checked here; call [`validate`](Self::validate).
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, JSON that is not an object, or a
    /// value of the wrong type, such as `{"seed": "12"}`. `null` does not
    /// mean "use the default" and is rejected for numbers.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let source: serde_json::Value = serde_json::from_str(json)?;
        let mut merged = serde_json::to_value(Self::default())?;
        merge_json(&mut merged, source);
        serde_json::from_value(merged)
    }

    /// Returns every setting, including those at their defaults, as indented
    /// JSON that [`from_json`](Self::from_json) reads back.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("mushroom options always serialize to JSON")
    }

    /// Cap radius in metres: half of [`size`](Self::size).
    pub fn cap_radius(&self) -> f32 {
        self.size * 0.5
    }

    /// Returns the length of the cap's top surface measured along its
    /// side-view outline, from the rim up to the top centre, in metres.
    /// Rim waves, bumps and lopsidedness are not included.
    ///
    /// Renderers use it with the mesh's `v` texture coordinate, which runs
    /// from 0 at the rim to 1 at the top along this outline, to measure
    /// distances on the cap.
    pub fn cap_top_length(&self) -> f32 {
        let (anchors, sharp) = self.cap.family.top_anchors();
        let points: Vec<Vec2> = anchors
            .iter()
            .map(|&[r, y]| Vec2::new(r, y * self.cap.height))
            .collect();
        Profile::new(&points, sharp).length() * self.cap_radius()
    }
}

/// Copies supplied JSON values over the defaults in `target`. Objects merge
/// recursively; arrays and other values replace the whole slot.
fn merge_json(target: &mut serde_json::Value, source: serde_json::Value) {
    use serde_json::Value;
    match (target, source) {
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

/// Checks that `value` of setting `field` is finite and above zero.
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

/// Checks that `value` of setting `field` is finite and zero or above.
pub(crate) fn non_negative(field: &'static str, value: f32) -> Result<(), OptionsError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement: "zero or positive",
        })
    }
}

/// Checks that `value` of setting `field` is a finite number.
fn finite(field: &'static str, value: f32) -> Result<(), OptionsError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement: "a number",
        })
    }
}

/// Checks that `value` of setting `field` is in `min..=max`. NaN always
/// fails.
pub(crate) fn within(
    field: &'static str,
    value: f32,
    min: f32,
    max: f32,
) -> Result<(), OptionsError> {
    if (min..=max).contains(&value) {
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
