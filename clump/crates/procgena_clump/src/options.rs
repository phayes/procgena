//! Settings for one clump generation pass.
//!
//! Lengths are in world units, the same units as the input surface, and are
//! multiplied by [`ClumpOptions::scale`].

use std::fmt;

use crate::surface::SurfaceError;

/// Settings for one generation pass, such as a snow cap.
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpOptions {
    /// Seed for every random choice. The same seed and settings give the same clumps.
    pub seed: u32,
    /// Directions the material arrives from, such as falling snow or wind.
    /// Each one covers the surfaces that face into it and are open toward it.
    /// Their coverage adds up, capped at full thickness, and becomes one
    /// mesh. Must hold from 1 to [`MAX_DIRECTIONS`] directions.
    pub directions: Vec<Direction>,
    /// Multiplies every length in these settings. Must be greater than 0.
    pub scale: f32,
    /// Where on the surface clumps may grow.
    pub mask: MaskOptions,
    /// Size and profile of the clump.
    pub shape: ShapeOptions,
    /// How the clump is turned into triangles.
    pub mesh: MeshOptions,
}

impl Default for ClumpOptions {
    /// The [`ClumpPreset::SnowCap`](crate::ClumpPreset::SnowCap) settings.
    fn default() -> Self {
        Self {
            seed: 0,
            directions: vec![Direction::default()],
            scale: 1.0,
            mask: MaskOptions::default(),
            shape: ShapeOptions::default(),
            mesh: MeshOptions::default(),
        }
    }
}

/// Most entries allowed in [`ClumpOptions::directions`].
pub const MAX_DIRECTIONS: usize = 8;

/// A direction material arrives from, and how much it contributes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    /// The way the material travels, in world space. `[0, -1, 0]` is falling
    /// straight down; `[1, 0, 0]` is wind blowing toward +X, which coats
    /// surfaces facing -X. Doesn't need to be normalized, but must not be zero.
    pub vector: [f32; 3],
    /// How much this direction contributes, where 1 is full coverage on
    /// surfaces facing straight into it. Must be 0 or more.
    pub strength: f32,
}

impl Direction {
    /// Material falling straight down at full strength. This is the default.
    pub const DOWN: Self = Self::new([0.0, -1.0, 0.0]);

    /// Creates a direction travelling along `vector`, at full strength (1).
    pub const fn new(vector: [f32; 3]) -> Self {
        Self {
            vector,
            strength: 1.0,
        }
    }
}

impl Default for Direction {
    /// [`Direction::DOWN`]: straight down, strength 1.
    fn default() -> Self {
        Self::DOWN
    }
}

/// Where on the surface clumps may grow.
///
/// For each [`Direction`], the mask is a facing term (how directly the surface
/// faces into the direction) times an exposure term (how open the surface is
/// toward it), times the direction's strength. The directions' masks add up.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskOptions {
    /// Facing value (cosine of the angle between the surface normal and the
    /// direction the material comes from) below which nothing grows. From -1
    /// to 1, and less than `facing_full`.
    pub facing_min: f32,
    /// Facing value at and above which the facing term is 1. From -1 to 1.
    pub facing_full: f32,
    /// Number of rays cast from each surface vertex toward each direction's
    /// source, to measure how open the vertex is to it. 0 skips the test and
    /// treats every point as fully open.
    pub sky_rays: u32,
    /// Exponent applied to exposure (0 to 1). Higher values push clumps
    /// further out of sheltered spots, and 0 ignores shelter. Must be 0 or more.
    pub sky_power: f32,
}

impl Default for MaskOptions {
    fn default() -> Self {
        Self {
            facing_min: 0.3,
            facing_full: 0.7,
            sky_rays: 64,
            sky_power: 1.0,
        }
    }
}

/// Size and profile of a single clump.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeOptions {
    /// Centre of the clump. `None` picks the most open, most up-facing vertex.
    pub at: Option<[f32; 3]>,
    /// Distance from the centre at which the clump ends. `None` covers the
    /// whole surface. Must be greater than 0.
    pub radius: Option<f32>,
    /// Fraction of `radius`, at its outer edge, over which the clump thins to
    /// nothing. From 0 to 1.
    pub edge_width: f32,
    /// Height of the clump above the surface where the mask is 1. Must be greater than 0.
    pub thickness: f32,
    /// Depth below the surface that the clump's edges sink to, so no seam
    /// shows where the clump ends. Must be 0 or more.
    pub sink: f32,
    /// Steepest the clump's sides may be, as rise over run: 1 is 45°, and
    /// larger values allow steeper, cliff-like edges. Heights are lowered
    /// where they would be steeper. Must be greater than 0.
    pub max_slope: f32,
    /// How much the thickness varies with noise, from 0 (smooth) to 1.
    pub lumpiness: f32,
    /// Typical width of a lump. Must be greater than 0.
    pub lump_size: f32,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            at: None,
            radius: None,
            edge_width: 0.3,
            thickness: 0.12,
            sink: 0.03,
            max_slope: 1.0,
            lumpiness: 0.3,
            lump_size: 0.25,
        }
    }
}

/// How the clump is turned into triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshOptions {
    /// Longest triangle edge in the clump mesh. Larger input triangles are
    /// split until they fit. Smaller values give smoother rims and lumps, and
    /// cost more: halving it gives about four times the triangles. Must be
    /// greater than 0.
    pub edge_length: f32,
    /// Whether to close the clump into a watertight solid. When true, each
    /// clump gets an underside, buried `shape.sink` plus a quarter of
    /// `shape.thickness` below the input surface, joined to the top by side
    /// walls along its border. Use a closed clump for physics bodies, volume
    /// and mass, or voxelizing. When false, the clump is only its visible
    /// top: an open sheet with about half the triangles, enough for drawing
    /// and static collision.
    pub closed: bool,
    /// Whether to make sure no part of the clump passes through another part.
    /// When true, the finished mesh is checked for triangles that cross each
    /// other, and the clump is thinned around any crossing until none remain.
    /// Turn it on before signed distance fields, booleans (CSG) or other tools
    /// that need a clean solid. It costs extra build time. Crossings between
    /// parts of the input that themselves overlap, such as a branch entering
    /// a trunk, may not be removable; [`BuildStats::crossings`] reports any
    /// that remain.
    ///
    /// [`BuildStats::crossings`]: crate::BuildStats::crossings
    pub strict: bool,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            edge_length: 0.04,
            closed: true,
            strict: false,
        }
    }
}

impl ClumpOptions {
    /// Checks that every setting is in range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        let check = |ok: bool, field: &'static str, requirement: &'static str| {
            if ok {
                Ok(())
            } else {
                Err(OptionsError { field, requirement })
            }
        };
        let finite_positive = |v: f32| v.is_finite() && v > 0.0;

        // Directions.
        check(
            (1..=MAX_DIRECTIONS).contains(&self.directions.len()),
            "directions",
            "must hold from 1 to MAX_DIRECTIONS (8) directions",
        )?;
        for d in &self.directions {
            let v = glam::Vec3::from(d.vector);
            check(
                v.is_finite() && v.length_squared() > 0.0,
                "directions.vector",
                "must be finite and not zero",
            )?;
            check(
                d.strength.is_finite() && d.strength >= 0.0,
                "directions.strength",
                "must be 0 or more",
            )?;
        }
        check(
            finite_positive(self.scale),
            "scale",
            "must be greater than 0",
        )?;

        // Mask.
        let m = &self.mask;
        check(
            (-1.0..=1.0).contains(&m.facing_min),
            "mask.facing_min",
            "must be from -1 to 1",
        )?;
        check(
            (-1.0..=1.0).contains(&m.facing_full),
            "mask.facing_full",
            "must be from -1 to 1",
        )?;
        check(
            m.facing_min < m.facing_full,
            "mask.facing_min",
            "must be less than mask.facing_full",
        )?;
        check(
            m.sky_power >= 0.0 && m.sky_power.is_finite(),
            "mask.sky_power",
            "must be 0 or more",
        )?;

        // Shape.
        let s = &self.shape;
        check(
            s.radius.is_none_or(finite_positive),
            "shape.radius",
            "must be greater than 0",
        )?;
        check(
            (0.0..=1.0).contains(&s.edge_width),
            "shape.edge_width",
            "must be from 0 to 1",
        )?;
        check(
            finite_positive(s.thickness),
            "shape.thickness",
            "must be greater than 0",
        )?;
        check(
            (0.0..=1.0).contains(&s.lumpiness),
            "shape.lumpiness",
            "must be from 0 to 1",
        )?;
        check(
            finite_positive(s.lump_size),
            "shape.lump_size",
            "must be greater than 0",
        )?;
        check(
            s.sink.is_finite() && s.sink >= 0.0,
            "shape.sink",
            "must be 0 or more",
        )?;
        check(
            finite_positive(s.max_slope),
            "shape.max_slope",
            "must be greater than 0",
        )?;

        // Mesh.
        check(
            finite_positive(self.mesh.edge_length),
            "mesh.edge_length",
            "must be greater than 0",
        )?;
        Ok(())
    }
}

/// A setting that is out of range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsError {
    /// Dotted path of the setting, such as `"shape.thickness"`.
    pub field: &'static str,
    /// What the setting must satisfy.
    pub requirement: &'static str,
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.field, self.requirement)
    }
}

impl std::error::Error for OptionsError {}

/// Why a clump couldn't be generated.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ClumpError {
    /// The input surface is invalid.
    Surface(SurfaceError),
    /// A setting is out of range.
    Options(OptionsError),
}

impl fmt::Display for ClumpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Surface(e) => write!(f, "invalid surface: {e}"),
            Self::Options(e) => write!(f, "invalid options: {e}"),
        }
    }
}

impl std::error::Error for ClumpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Surface(e) => Some(e),
            Self::Options(e) => Some(e),
        }
    }
}

impl From<SurfaceError> for ClumpError {
    fn from(e: SurfaceError) -> Self {
        Self::Surface(e)
    }
}

impl From<OptionsError> for ClumpError {
    fn from(e: OptionsError) -> Self {
        Self::Options(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        ClumpOptions::default().validate().unwrap();
    }

    #[test]
    fn out_of_range_settings_are_named() {
        let mut options = ClumpOptions::default();
        options.shape.sink = -0.01;
        assert_eq!(options.validate().unwrap_err().field, "shape.sink");

        let mut options = ClumpOptions::default();
        options.mesh.edge_length = 0.0;
        assert_eq!(options.validate().unwrap_err().field, "mesh.edge_length");
    }
}
