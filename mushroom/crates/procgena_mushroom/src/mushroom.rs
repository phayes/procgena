//! The [`Mushroom`] type: a checked set of options, ready to be meshed.

use std::fmt;

use glam::Vec3;

use crate::colour::Palette;
use crate::detail::{LodLevel, MushroomDetail, MushroomLod};
use crate::mesh::{MushroomMesh, build};
use crate::options::{MushroomOptions, OptionsError};
use crate::presets::MushroomPreset;
use crate::shape::Shape;

/// A generated mushroom that can be meshed at any detail.
///
/// Creating one checks the options and works out the shape. Meshing is then
/// quick, and every detail level follows the same shape, so the mushroom
/// doesn't change form as it switches levels.
///
/// The mushroom stands on the origin, growing up the +Y axis, in metres.
///
/// ```
/// use procgena_mushroom::{Mushroom, MushroomDetail, MushroomPreset};
///
/// let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
/// let close = mushroom.mesh();
/// let far = mushroom.mesh_with_detail(&MushroomDetail::FLAT).unwrap();
/// assert!(far.triangle_count() < close.triangle_count());
/// ```
#[derive(Debug, Clone)]
pub struct Mushroom {
    options: MushroomOptions,
    shape: Shape,
}

/// Why a [`Mushroom`] could not be created from JSON.
#[derive(Debug)]
#[non_exhaustive]
pub enum MushroomError {
    /// The JSON is malformed or has a value of the wrong type.
    Json(serde_json::Error),
    /// A setting is out of range.
    Options(OptionsError),
}

impl fmt::Display for MushroomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MushroomError::Json(e) => write!(f, "invalid mushroom JSON: {e}"),
            MushroomError::Options(e) => write!(f, "invalid mushroom options: {e}"),
        }
    }
}

impl std::error::Error for MushroomError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MushroomError::Json(e) => Some(e),
            MushroomError::Options(e) => Some(e),
        }
    }
}

impl Mushroom {
    /// Creates a mushroom from `options`, after checking them.
    ///
    /// # Errors
    ///
    /// Returns the first setting that is out of range.
    pub fn new(options: MushroomOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let shape = Shape::new(&options);
        Ok(Self { options, shape })
    }

    /// Creates a mushroom from a preset. Presets are always valid, so this
    /// can't fail.
    pub fn from_preset(preset: MushroomPreset) -> Self {
        Self::new(preset.options()).expect("presets are valid")
    }

    /// Creates a mushroom from JSON options, which may leave settings out to
    /// use their defaults; see [`MushroomOptions::from_json`].
    ///
    /// # Errors
    ///
    /// Returns [`MushroomError::Json`] if the JSON can't be read, and
    /// [`MushroomError::Options`] if a setting is out of range.
    pub fn from_json(json: &str) -> Result<Self, MushroomError> {
        let options = MushroomOptions::from_json(json).map_err(MushroomError::Json)?;
        Self::new(options).map_err(MushroomError::Options)
    }

    /// Returns the options this mushroom was made from.
    pub fn options(&self) -> &MushroomOptions {
        &self.options
    }

    /// Builds the mesh at the detail in the options
    /// ([`MushroomOptions::detail`]).
    pub fn mesh(&self) -> MushroomMesh {
        build(
            &self.shape,
            &self.options.detail,
            &Palette::new(&self.options.material),
        )
    }

    /// Builds the mesh at `detail`, such as [`MushroomDetail::FLAT`] for the
    /// cheapest version.
    ///
    /// # Errors
    ///
    /// Returns an error if a count in `detail` is out of range.
    pub fn mesh_with_detail(&self, detail: &MushroomDetail) -> Result<MushroomMesh, OptionsError> {
        detail.validate()?;
        Ok(build(
            &self.shape,
            detail,
            &Palette::new(&self.options.material),
        ))
    }

    /// Builds one mesh for each level of `lod`, nearest level first, paired
    /// with the level it belongs to.
    ///
    /// The distances in `lod` are used as given; use
    /// [`MushroomLod::for_size`] to suit them to this mushroom's size.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`MushroomLod::validate`].
    pub fn lod_meshes(
        &self,
        lod: &MushroomLod,
    ) -> Result<Vec<(LodLevel, MushroomMesh)>, OptionsError> {
        lod.validate()?;
        let palette = Palette::new(&self.options.material);
        Ok(lod
            .levels
            .iter()
            .map(|level| (*level, build(&self.shape, &level.detail, &palette)))
            .collect())
    }

    /// Returns the cap radius in metres.
    pub fn cap_radius(&self) -> f32 {
        self.shape.radius
    }

    /// Returns the position of the top centre of the cap, in the mushroom's
    /// coordinates.
    pub fn apex(&self) -> Vec3 {
        self.shape.cap_apex()
    }

    /// Returns the unit direction the cap faces (its axis).
    pub fn cap_axis(&self) -> Vec3 {
        self.shape.cap_axis()
    }

    /// Returns the top of the stem, where the cap sits, in the mushroom's
    /// coordinates.
    pub fn cap_centre(&self) -> Vec3 {
        self.shape.stem_centre(1.0)
    }

    /// Returns the stem's radius at the ground in metres, including any
    /// bulb.
    pub fn base_radius(&self) -> f32 {
        self.shape.stem_radius(0.0)
    }
}
