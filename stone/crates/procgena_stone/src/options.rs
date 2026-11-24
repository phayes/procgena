//! Settings for a rock's shape, roughness, world size, and mesh detail.
//!
//! Start with [`StoneOptions::default()`], a [`crate::StonePreset`], or a
//! `.stone.yml` file read with [`crate::StonePreset::from_yaml`]. Creating a
//! [`crate::Stone`] validates the settings;
//! use [`StoneOptions::validate`] to check edits before generating a rock.

use std::fmt;

use procgena_core::scatter::{Placement, Rest};
use procgena_core::{LodError, OptionsHash, PoolId, SpecimenId};
use serde::{Deserialize, Serialize};

use crate::content::STONE_KIND;
use crate::detail::{DetailLevel, StoneDetail};

/// Minimum allowed number of grid cells per axis in [`DetailLevel::resolution`].
pub const MIN_RESOLUTION: u32 = 8;
/// Maximum allowed number of grid cells per axis in [`DetailLevel::resolution`].
pub const MAX_RESOLUTION: u32 = 512;
/// Maximum number of levels in [`MeshOptions::detail`].
pub const MAX_DETAIL_LEVELS: u32 = 8;
/// Maximum allowed number of cuts in [`CarveOptions::steps`].
pub const MAX_CARVE_STEPS: u32 = 72;
/// Maximum allowed number of noise layers in [`SurfaceOptions::octaves`].
pub const MAX_OCTAVES: u32 = 8;

/// Settings used to generate a repeatable rock shape and mesh.
///
/// Reuse the same settings to reproduce a rock, or change
/// [`default_seed`](Self::default_seed) for a variation. Start with
/// [`Self::default()`] or a [`crate::StonePreset`] and edit fields.
///
/// Floating-point settings must be finite and within their documented ranges.
/// Assigning fields does not validate them; call
/// [`validate`](Self::validate) or create a [`crate::Stone`] to check them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoneOptions {
    /// Seed for the shape and surface pattern of a rock built on its own.
    /// Reusing a seed with the same settings reproduces the same field. A
    /// rock in a Scatter or pool takes its seed from there instead (see
    /// [`variant`](Self::variant)).
    pub default_seed: u32,
    /// The rock's width: the width of the containing cube, in metres. Must be
    /// finite and positive.
    /// [`crate::StoneField`] uses a unit-width cube and does not apply this scale.
    pub scale: f32,
    /// Relative dimensions along x, y, and z; each must be finite and positive.
    /// The largest component sets the reference size, and the other axes shrink
    /// in proportion. For example, `[1.0, 0.5, 1.0]` halves the y dimension.
    /// Multiplying all three components by the same factor preserves the shape.
    /// Surface roughness keeps its spacing rather than stretching with the shape.
    pub stretch: [f32; 3],
    /// Ground orientation and suggested burial depth; burial is not applied automatically.
    pub placement: PlacementOptions,
    /// Cuts and edge rounding that define the rock’s basic shape.
    pub carve: CarveOptions,
    /// Surface roughness applied to the carved shape.
    pub surface: SurfaceOptions,
    /// Meshing method and detail level used when creating a [`crate::Stone`].
    pub mesh: MeshOptions,
    /// Surface appearance for renderers and exporters. It does not affect the shape.
    pub material: MaterialOptions,
}

/// Settings for orienting a rock on flat ground and suggesting how far to bury it.
///
/// Placement lifts the rock to `y = 0` without changing its shape. Ground
/// contact is approximate; see [`crate::StonePlacement`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementOptions {
    /// Turns the rock onto an estimated resting side, as if dropped onto flat
    /// ground. Defaults to true. Set false to keep its generated orientation,
    /// for example for a standing stone. Either way, the rock is lifted to ground level.
    pub settle: bool,
    /// Limits suggested burial to this fraction of the placed rock's height.
    /// Must be finite and between 0 and 1, inclusive. The suggestion selects
    /// the lowest slice that is nearly as wide as the widest horizontal slice
    /// within that lower portion of the rock.
    /// Defaults to 1 (the whole height); 0.25 allows at most a quarter of its
    /// height, and 0 disables burial. The caller applies the suggested depth.
    pub max_embed_fraction: f32,
    /// Smallest suggested burial, as a fraction of the placed rock's height.
    /// Must be finite, from 0 through [`max_embed_fraction`](Self::max_embed_fraction).
    /// Rocks with straight sides, which do not get wider above the ground,
    /// are sunk by this much so their contact with the ground is hidden.
    /// Defaults to 0.05.
    pub min_embed_fraction: f32,
}

/// Settings for shaping a sphere by removing material with spherical cuts.
///
/// Distances use carve space: coordinates where the containing cube runs from
/// -1 to 1 on each axis before stretching. Large cutting spheres can leave
/// nearly flat facets. All floating-point settings must be finite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarveOptions {
    /// Number of cuts, from 0 through [`MAX_CARVE_STEPS`]. Zero leaves the starting
    /// sphere uncut; surface roughness still applies.
    pub steps: u32,
    /// Width of the rounded transitions between cuts, in carve-space units.
    /// Larger values soften edges and remove more material. Must be positive.
    pub smoothness: f32,
    /// Radius of the starting sphere in carve-space units; must be positive.
    /// A radius of 1 reaches the cube faces before cuts and roughness.
    /// [`crate::StoneField`] clips any remaining shape to its containing cube.
    pub base_radius: f32,
    /// Minimum distance from the origin to a cutting sphere’s centre, in carve-space
    /// units; must be positive. Its radius is this distance times [`depth`](Self::depth).
    pub radius_min: f32,
    /// Maximum random addition to [`radius_min`](Self::radius_min) for each cut’s
    /// centre distance; must be zero or positive. Zero makes all cuts the same size.
    pub radius_jitter: f32,
    /// Ratio of a cutting sphere’s radius to its distance from the origin; must
    /// be positive. Larger values cut deeper and can remove the entire rock.
    pub depth: f32,
    /// How strongly cut directions turn toward the nearest positive or negative
    /// x, y, or z axis, from 0 through 1. Defaults to 0, which keeps the original
    /// random directions. Higher values make the rock more box-like; at 1, all
    /// cuts come from these six directions. The rock’s proportions follow
    /// [`StoneOptions::stretch`]; for example, `[1.0, 0.5, 1.0]` makes it shorter
    /// along y. Cuts remain spherical, so sides can still curve. With few
    /// [`steps`](Self::steps), some sides may receive no cuts and stay rounded.
    pub blockiness: f32,
}

/// Settings for surface roughness. All floating-point values must be finite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceOptions {
    /// Strength of the inward surface displacement; must be zero or positive.
    /// Zero disables roughness. This is a strength factor, not a distance in metres.
    pub displacement_scale: f32,
    /// Frequency of the surface pattern; must be positive. Larger values produce
    /// smaller, more closely spaced details within the stone’s unit-width cube.
    pub displacement_spread: f32,
    /// Number of noise layers (octaves), from 1 through [`MAX_OCTAVES`].
    /// Each additional layer contributes finer, weaker detail and takes more work to sample.
    pub octaves: u32,
}

/// Controls how a rock surface is approximated by triangles and how much detail is captured.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshOptions {
    /// Surface-to-triangle conversion method; see [`Mesher`] for the tradeoffs.
    pub mesher: Mesher,
    /// Mesh levels to build, most detailed first: a preset such as
    /// [`StoneDetail::MEDIUM`] (the default) or your own levels. In a file,
    /// a list of levels that replaces the whole list.
    pub detail: StoneDetail,
}

/// Surface appearance: a texture set and adjustments to it.
///
/// Textures are projected onto the rock along its three axes, so meshes need
/// no texture coordinates. All floating-point values must be finite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialOptions {
    /// Name of the texture set, usually one of [`crate::TEXTURE_SETS`]. Use
    /// [`crate::texture_path`] to find its images. The name is not validated.
    pub texture: String,
    /// Linear RGB colour that multiplies the colour texture, with each channel
    /// zero or positive. Without textures this is the rock's colour. Defaults to
    /// white, which keeps the texture's colours unchanged.
    pub color: [f32; 3],
    /// Roughness from 0 (polished) to 1 (fully matte). Multiplies the roughness
    /// texture; without textures this is the rock's roughness. Defaults to 1.
    pub roughness: f32,
    /// Metalness from 0 (stone and other non-metals) to 1 (bare metal). Applies
    /// to the whole surface. Defaults to 0.
    pub metallic: f32,
    /// Metres covered by one repeat of the texture; must be positive.
    /// Larger values make the texture's features larger on the rock.
    pub texture_size: f32,
}

/// Methods for approximating a rock surface with triangles.
///
/// Both generate outward surface normals. Resolution affects how much detail
/// is captured, and small features can disappear. Files name them
/// `marching_cubes` and `dual_contouring`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mesher {
    /// Approximates the surface with triangles whose vertices lie on sampling
    /// grid edges. This is the default method and generally produces softer
    /// corners than dual contouring.
    #[default]
    MarchingCubes,
    /// Fits vertices to local surface directions to better preserve corners
    /// and creases. Nearby surface parts can become joined at a vertex or edge,
    /// creating non-manifold geometry: connections that some collider and mesh
    /// processing tools cannot handle.
    DualContouring,
}

impl Default for StoneOptions {
    fn default() -> Self {
        Self {
            default_seed: 880,
            scale: 2.5,
            stretch: [1.0, 1.0, 1.0],
            placement: PlacementOptions::default(),
            carve: CarveOptions::default(),
            surface: SurfaceOptions::default(),
            mesh: MeshOptions::default(),
            material: MaterialOptions::default(),
        }
    }
}

impl Default for PlacementOptions {
    fn default() -> Self {
        Self {
            settle: true,
            max_embed_fraction: 1.0,
            min_embed_fraction: 0.05,
        }
    }
}

impl Default for MaterialOptions {
    fn default() -> Self {
        Self {
            texture: "rock_01".to_owned(),
            color: [1.0, 1.0, 1.0],
            roughness: 1.0,
            metallic: 0.0,
            texture_size: 1.0,
        }
    }
}

impl Default for CarveOptions {
    fn default() -> Self {
        Self {
            steps: 20,
            smoothness: 0.05,
            base_radius: 0.95,
            radius_min: 2.5,
            radius_jitter: 1.0,
            depth: 0.8,
            blockiness: 0.0,
        }
    }
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        Self {
            displacement_scale: 0.15,
            displacement_spread: 10.0,
            octaves: 6,
        }
    }
}

/// A setting that fails the range checks in [`StoneOptions::validate`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OptionsError {
    /// A whole-number setting is below its minimum or above its maximum.
    IntOutOfRange {
        /// Name of the invalid setting, for example `"mesh.resolution"`.
        field: &'static str,
        /// The rejected value.
        value: u32,
        /// Inclusive lower limit.
        min: u32,
        /// Inclusive upper limit.
        max: u32,
    },
    /// A floating-point setting is NaN, infinite, or outside its allowed range.
    FloatOutOfRange {
        /// Name of the invalid setting, for example `"carve.smoothness"`.
        field: &'static str,
        /// The rejected value, which may be NaN or infinite.
        value: f32,
        /// Allowed range in words, such as `"positive"`; the value must also be finite.
        requirement: &'static str,
    },
}

impl fmt::Display for OptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntOutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field} is {value}, but must be from {min} to {max}"),
            Self::FloatOutOfRange {
                field,
                value,
                requirement,
            } => write!(
                f,
                "{field} is {value}, but must be finite and {requirement}"
            ),
        }
    }
}

impl std::error::Error for OptionsError {}

impl StoneOptions {
    /// Checks all settings for allowed ranges and finite floating-point values.
    ///
    /// Passing validation does not guarantee a nonempty rock or that extreme
    /// values will remain finite during field evaluation.
    ///
    /// # Errors
    ///
    /// Returns the first invalid setting as [`OptionsError::IntOutOfRange`] or
    /// [`OptionsError::FloatOutOfRange`]. No settings are changed.
    pub fn validate(&self) -> Result<(), OptionsError> {
        positive("scale", self.scale)?;
        positive("stretch[0]", self.stretch[0])?;
        positive("stretch[1]", self.stretch[1])?;
        positive("stretch[2]", self.stretch[2])?;

        let carve = &self.carve;
        int_range("carve.steps", carve.steps, 0, MAX_CARVE_STEPS)?;
        positive("carve.smoothness", carve.smoothness)?;
        positive("carve.base_radius", carve.base_radius)?;
        positive("carve.radius_min", carve.radius_min)?;
        non_negative("carve.radius_jitter", carve.radius_jitter)?;
        positive("carve.depth", carve.depth)?;
        unit_interval("carve.blockiness", carve.blockiness)?;

        let surface = &self.surface;
        non_negative("surface.displacement_scale", surface.displacement_scale)?;
        positive("surface.displacement_spread", surface.displacement_spread)?;
        int_range("surface.octaves", surface.octaves, 1, MAX_OCTAVES)?;

        let placement = &self.placement;
        unit_interval("placement.max_embed_fraction", placement.max_embed_fraction)?;
        unit_interval("placement.min_embed_fraction", placement.min_embed_fraction)?;
        if placement.min_embed_fraction > placement.max_embed_fraction {
            return Err(OptionsError::FloatOutOfRange {
                field: "placement.min_embed_fraction",
                value: placement.min_embed_fraction,
                requirement: "at most placement.max_embed_fraction",
            });
        }

        check_detail(&self.mesh.detail)?;

        let material = &self.material;
        non_negative("material.color[0]", material.color[0])?;
        non_negative("material.color[1]", material.color[1])?;
        non_negative("material.color[2]", material.color[2])?;
        unit_interval("material.roughness", material.roughness)?;
        unit_interval("material.metallic", material.metallic)?;
        positive("material.texture_size", material.texture_size)
    }
}

/// Checks the inclusive [`MIN_RESOLUTION`] to [`MAX_RESOLUTION`] range.
///
/// Returns [`OptionsError::IntOutOfRange`] on failure, using `field` as the
/// setting name in the error.
pub(crate) fn check_resolution(field: &'static str, resolution: u32) -> Result<(), OptionsError> {
    int_range(field, resolution, MIN_RESOLUTION, MAX_RESOLUTION)
}

/// Checks the level count, each level's resolution, distance and hysteresis,
/// and that distances strictly increase.
fn check_detail(detail: &StoneDetail) -> Result<(), OptionsError> {
    let levels = detail.levels();
    let count = u32::try_from(levels.len()).unwrap_or(u32::MAX);
    int_range("mesh.detail", count, 1, MAX_DETAIL_LEVELS)?;

    let mut previous: Option<&DetailLevel> = None;
    for level in levels {
        check_resolution("mesh.detail.resolution", level.resolution)?;
        non_negative("mesh.detail.distance", level.distance)?;
        if previous.is_some_and(|p| level.distance <= p.distance) {
            return Err(OptionsError::FloatOutOfRange {
                field: "mesh.detail.distance",
                value: level.distance,
                requirement: "greater than the previous level's distance",
            });
        }
        if !(0.0..1.0).contains(&level.hysteresis) {
            return Err(OptionsError::FloatOutOfRange {
                field: "mesh.detail.hysteresis",
                value: level.hysteresis,
                requirement: "from 0 up to but not including 1",
            });
        }
        previous = Some(level);
    }

    // The shared level-of-detail rules also need the first level at 0 and
    // fade bands that don't overlap.
    match detail.lod().validate() {
        Ok(()) => Ok(()),
        Err(LodError::FirstDistance(distance)) => Err(OptionsError::FloatOutOfRange {
            field: "mesh.detail.distance",
            value: distance,
            requirement: "0 for the first level",
        }),
        Err(LodError::BandsOverlap { level }) => Err(OptionsError::FloatOutOfRange {
            field: "mesh.detail.hysteresis",
            value: levels[level].hysteresis,
            requirement: "narrow enough that the level's fade band doesn't overlap the previous level's",
        }),
        Err(other) => unreachable!("checked above: {other}"),
    }
}

fn int_range(field: &'static str, value: u32, min: u32, max: u32) -> Result<(), OptionsError> {
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

fn positive(field: &'static str, value: f32) -> Result<(), OptionsError> {
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

fn non_negative(field: &'static str, value: f32) -> Result<(), OptionsError> {
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

fn unit_interval(field: &'static str, value: f32) -> Result<(), OptionsError> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(OptionsError::FloatOutOfRange {
            field,
            value,
            requirement: "from 0 to 1",
        })
    }
}

impl StoneOptions {
    /// How a rock with these settings stands on uneven ground, when
    /// scattered:
    /// - a rock that [settles](PlacementOptions::settle) onto a resting side
    ///   tilts halfway to the slope, like a boulder that has rolled to rest;
    /// - a rock that keeps its generated orientation, such as a standing
    ///   stone, stays upright.
    ///
    /// Either way it rests on the lowest ground under it, so it never
    /// floats, is not sunk any further, and is left out where the ground
    /// under it bends by more than a tenth of its size. Its suggested burial
    /// is applied separately.
    pub fn ground_placement(&self) -> Placement {
        Placement {
            conform: if self.placement.settle { 0.5 } else { 0.0 },
            rest: Rest::Lowest,
            embed: 0.0,
            max_bend: Some(0.1),
        }
    }

    /// Returns the settings of shape `slot` in a set of related rocks built
    /// from these settings, in a World seeded with `world_seed`.
    ///
    /// The set is a pool: these settings plus `stretch_jitter`, whatever
    /// their own [`default_seed`](Self::default_seed). Slot `slot`'s seed is the
    /// [`SpecimenId`] seed of that pool and slot in `world_seed`'s World, so
    /// every World grows its own shapes, and each slot's shape is the same
    /// wherever it is used in one World. A set built on its own, outside any
    /// World, passes its own seed as `world_seed`.
    ///
    /// Each of the shape's [`stretch`](Self::stretch) axes is also reduced
    /// by a repeatable random share of up to `stretch_jitter`: 0.3 shrinks
    /// each axis to between 70% and 100%. Pass a `stretch_jitter` of at
    /// least 0 and below 1; this method does not check it.
    pub fn variant(&self, world_seed: u32, slot: u32, stretch_jitter: f32) -> StoneOptions {
        let mut options = self.clone();
        options.default_seed = self.variant_id(world_seed, slot, stretch_jitter).seed();
        for (axis, stretch) in options.stretch.iter_mut().enumerate() {
            *stretch *= 1.0 - stretch_jitter * unit_hash(options.default_seed, axis as u32);
        }
        options
    }

    /// The [`SpecimenId`] of shape `slot` in the set [`variant`](Self::variant)
    /// draws from, in a World seeded with `world_seed`.
    pub fn variant_id(&self, world_seed: u32, slot: u32, stretch_jitter: f32) -> SpecimenId {
        let pool = PoolId::new(STONE_KIND, OptionsHash::of(&(self, stretch_jitter)));
        SpecimenId::new(world_seed, pool, slot)
    }
}

/// Returns a value in `[0, 1)` that depends only on `seed` and `axis`.
fn unit_hash(seed: u32, axis: u32) -> f32 {
    // Mix the inputs with the "lowbias32" integer hash.
    let mut x = seed ^ axis.wrapping_mul(0x9e37_79b9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StonePreset, TextureSize};

    #[test]
    fn defaults_are_valid() {
        assert_eq!(StoneOptions::default().validate(), Ok(()));
    }

    /// Reads the options of a `.stone.yml` file with no base.
    fn read(text: &str) -> Result<StoneOptions, procgena_core::preset::PresetFileError> {
        StonePreset::from_yaml("test", text).map(|preset| preset.stone)
    }

    #[test]
    fn a_partial_file_keeps_nested_defaults() {
        let options =
            read("stone:\n  default_seed: 7\n  mesh:\n    mesher: dual_contouring\n").unwrap();
        let mut expected = StoneOptions {
            default_seed: 7,
            ..StoneOptions::default()
        };
        expected.mesh.mesher = Mesher::DualContouring;
        assert_eq!(options, expected);
    }

    #[test]
    fn a_file_detail_replaces_the_whole_list() {
        let options = read(
            "stone:\n  mesh:\n    detail:\n      - { resolution: 64, distance: 0.0, \
             hysteresis: 0.1, texture_size: 2k }\n",
        )
        .unwrap();
        let expected = StoneDetail::new(vec![DetailLevel {
            resolution: 64,
            distance: 0.0,
            hysteresis: 0.1,
            texture_size: TextureSize::Size2k,
        }]);
        assert_eq!(options.mesh.detail, expected);
    }

    #[test]
    fn presets_are_valid() {
        for (name, detail) in StoneDetail::PRESETS {
            let options = StoneOptions {
                mesh: MeshOptions {
                    mesher: Mesher::default(),
                    detail,
                },
                ..StoneOptions::default()
            };
            assert_eq!(options.validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn a_file_round_trips() {
        let mut options = StoneOptions {
            default_seed: 12345,
            stretch: [1.0, 0.5, 0.75],
            ..StoneOptions::default()
        };
        options.mesh.mesher = Mesher::DualContouring;
        options.surface.octaves = 3;
        options.material.texture = "lichen_rock_02".to_owned();
        let preset = StonePreset {
            stone: options.clone(),
            ..StonePreset::boulder().clone()
        };
        let text = preset.to_yaml(None);
        assert!(text.contains("mesher: dual_contouring"), "{text}");
        assert!(text.contains("texture: lichen_rock_02"), "{text}");
        assert_eq!(read(&text).unwrap(), options);
    }

    #[test]
    fn a_file_rejects_unknown_names() {
        for text in [
            "stone:\n  name: boulder\n",
            "stone:\n  carve:\n    note: 1\n",
            "tags: [a]\n",
        ] {
            assert!(read(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn a_file_rejects_bad_values() {
        for text in [
            "stone:\n  default_seed: twelve\n",
            "stone:\n  default_seed: -1\n",
            "stone:\n  mesh:\n    mesher: voxels\n",
            "stone:\n  carve: 3\n",
            "[1, 2]\n",
            "stone: {default_seed: 1\n",
            "stone:\n  scale: 0.0\n",
        ] {
            assert!(read(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn validate_reports_out_of_range_settings() {
        let check = |edit: fn(&mut StoneOptions), field: &str| {
            let mut options = StoneOptions::default();
            edit(&mut options);
            let error = options.validate().unwrap_err();
            let reported = match &error {
                OptionsError::IntOutOfRange { field, .. }
                | OptionsError::FloatOutOfRange { field, .. } => *field,
            };
            assert_eq!(reported, field, "{error}");
        };
        check(|o| o.scale = 0.0, "scale");
        check(|o| o.stretch[2] = -1.0, "stretch[2]");
        check(|o| o.carve.steps = MAX_CARVE_STEPS + 1, "carve.steps");
        check(|o| o.carve.smoothness = f32::NAN, "carve.smoothness");
        check(|o| o.carve.radius_jitter = -0.1, "carve.radius_jitter");
        check(|o| o.carve.blockiness = 1.5, "carve.blockiness");
        check(|o| o.carve.blockiness = f32::NAN, "carve.blockiness");
        check(
            |o| o.surface.displacement_scale = f32::INFINITY,
            "surface.displacement_scale",
        );
        check(|o| o.surface.octaves = 0, "surface.octaves");
        check(|o| o.mesh.detail = StoneDetail::new(vec![]), "mesh.detail");
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(32, 0.0); 9]),
            "mesh.detail",
        );
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(MIN_RESOLUTION - 1, 0.0)]),
            "mesh.detail.resolution",
        );
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(MAX_RESOLUTION + 1, 0.0)]),
            "mesh.detail.resolution",
        );
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(32, -1.0)]),
            "mesh.detail.distance",
        );
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(32, 10.0), level(16, 10.0)]),
            "mesh.detail.distance",
        );
        check(
            |o| {
                let mut bad = level(32, 0.0);
                bad.hysteresis = 1.0;
                o.mesh.detail = StoneDetail::new(vec![bad]);
            },
            "mesh.detail.hysteresis",
        );
        // The first level is the one drawn up close, so it starts at 0.
        check(
            |o| o.mesh.detail = StoneDetail::new(vec![level(32, 5.0), level(16, 10.0)]),
            "mesh.detail.distance",
        );
        // Fade bands 9..11 and 10.5..11.5 overlap.
        check(
            |o| {
                let mut wide = level(16, 10.0);
                wide.hysteresis = 0.1;
                let mut next = level(8, 11.0);
                next.hysteresis = 0.045;
                o.mesh.detail = StoneDetail::new(vec![level(32, 0.0), wide, next]);
            },
            "mesh.detail.hysteresis",
        );
        check(|o| o.material.color[1] = -0.5, "material.color[1]");
        check(|o| o.material.roughness = 1.5, "material.roughness");
        check(|o| o.material.roughness = f32::NAN, "material.roughness");
        check(|o| o.material.metallic = -0.1, "material.metallic");
        check(|o| o.material.metallic = 1.5, "material.metallic");
        check(|o| o.material.texture_size = 0.0, "material.texture_size");
    }

    #[test]
    fn validate_accepts_range_limits() {
        let mut options = StoneOptions::default();
        options.carve.steps = 0;
        options.carve.radius_jitter = 0.0;
        options.carve.blockiness = 1.0;
        options.surface.displacement_scale = 0.0;
        options.surface.octaves = MAX_OCTAVES;
        options.mesh.detail = StoneDetail::new(
            (0..MAX_DETAIL_LEVELS)
                .map(|i| level(MAX_RESOLUTION, i as f32))
                .collect(),
        );
        options.material.color = [0.0; 3];
        options.material.roughness = 0.0;
        options.material.metallic = 1.0;
        assert_eq!(options.validate(), Ok(()));
        options.mesh.detail = StoneDetail::new(vec![level(MIN_RESOLUTION, 0.0)]);
        assert_eq!(options.validate(), Ok(()));
    }

    /// A level at `resolution` starting at `distance`, with 512 textures.
    fn level(resolution: u32, distance: f32) -> DetailLevel {
        DetailLevel::new(resolution, distance, TextureSize::Size512)
    }
}
