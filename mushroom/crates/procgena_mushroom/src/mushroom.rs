//! The [`Mushroom`] type: a checked set of options, ready to be meshed, and
//! the single mushrooms ([`MushroomBody`]) it is made of.

use std::fmt;

use glam::{Affine3A, Vec3};
use procgena_core::SpecimenId;

use crate::cluster::{ClusterOptions, MushroomInstance, arrange, body_seed, make_bodies};
use crate::colour::Palette;
use crate::detail::{LodLevel, MushroomDetail, MushroomLod};
use crate::mesh::{MushroomMesh, build, combine};
use crate::options::{MushroomOptions, OptionsError};
use crate::presets::MushroomPreset;
use crate::shape::Shape;

/// A generated mushroom, or a cluster of them, that can be meshed at any
/// detail.
///
/// Creating one checks the options, works out the shape and, when
/// [`ClusterOptions::stems`](crate::ClusterOptions::stems) is above 1,
/// places the members of the cluster. Meshing is then quick, and every
/// detail level follows the same shape, so the mushroom doesn't change form
/// as it switches levels.
///
/// A cluster is made of a few different [`bodies`](Self::bodies), each a
/// single mushroom, drawn at the places given by
/// [`members`](Self::members). Renderers that can draw many copies of one
/// mesh should mesh each body once and draw every member from it. The
/// methods on `Mushroom` itself return all members combined into one mesh.
///
/// The mushroom stands on the ground at the origin, growing up the +Y axis,
/// in metres. A closed stem bottom reaches below the ground.
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
    id: SpecimenId,
    bodies: Vec<MushroomBody>,
    members: Vec<MushroomInstance>,
}

/// One single mushroom, without any cluster: the shape that the members of
/// a [`Mushroom`] are copies of.
///
/// Its options are the [`Mushroom`]'s, with this body's own seed, small
/// differences in height, and a single stem. It stands on the ground at the
/// origin, growing up the +Y axis, in metres.
///
/// A body is named by the specimen it belongs to and its index there:
/// bodies with the same [`specimen`](Self::specimen) and
/// [`index`](Self::index) are identical.
#[derive(Debug, Clone)]
pub struct MushroomBody {
    options: MushroomOptions,
    specimen: SpecimenId,
    index: u32,
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
    /// Creates a mushroom, or a cluster, from `options`, after checking
    /// them. It is specimen 0 of the options' pool, with
    /// [`MushroomOptions::seed`] standing in for the World seed; see
    /// [`MushroomOptions::specimen_id`].
    ///
    /// # Errors
    ///
    /// Returns the first setting that is out of range.
    pub fn new(options: MushroomOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let id = options.specimen_id(0);

        // A single mushroom is exactly what the options describe; a
        // cluster's members are variations of it.
        let cluster = &options.cluster;
        let (bodies, members) = if cluster.stems == 1 {
            let body = MushroomBody::single(&options, id)?;
            let member = MushroomInstance {
                variant: 0,
                translation: Vec3::ZERO,
                rotation: glam::Quat::IDENTITY,
                scale: 1.0,
            };
            (vec![body], vec![member])
        } else {
            let bodies = make_bodies(&options, id, cluster.variants.min(cluster.stems))?;
            let members = arrange(&bodies, cluster, id.seed());
            (bodies, members)
        };
        Ok(Self {
            options,
            id,
            bodies,
            members,
        })
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

    /// Returns the id of this specimen. Mushrooms with equal ids are
    /// identical.
    pub fn id(&self) -> SpecimenId {
        self.id
    }

    /// Returns the different single mushrooms the members are copies of.
    /// A mushroom with one stem has exactly one body.
    pub fn bodies(&self) -> &[MushroomBody] {
        &self.bodies
    }

    /// Returns the placed mushrooms, largest first. A mushroom with one stem
    /// has one member, at the origin and unscaled.
    pub fn members(&self) -> &[MushroomInstance] {
        &self.members
    }

    /// Builds the mesh of every member at the detail in the options
    /// ([`MushroomOptions::detail`]).
    pub fn mesh(&self) -> MushroomMesh {
        self.combined(&self.options.detail)
    }

    /// Builds the mesh of every member at `detail`, such as
    /// [`MushroomDetail::FLAT`] for a cheaper version.
    ///
    /// # Errors
    ///
    /// Returns an error if a count in `detail` is out of range.
    pub fn mesh_with_detail(&self, detail: &MushroomDetail) -> Result<MushroomMesh, OptionsError> {
        detail.validate()?;
        Ok(self.combined(detail))
    }

    /// Builds one mesh of every member for each level of `lod`, nearest
    /// level first, paired with the level it belongs to.
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
        Ok(lod
            .levels
            .iter()
            .map(|level| (*level, self.combined(&level.detail)))
            .collect())
    }

    /// Returns the smallest and largest corners of a box that roughly holds
    /// every member, in metres: each member's stem from its lowest point to
    /// the top of its cap, widened by its cap radius.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for member in &self.members {
            let body = &self.bodies[member.variant as usize];
            let at = member.transform();
            let reach = Vec3::splat(body.cap_radius() * member.scale);
            for point in [body.bottom(), body.cap_centre(), body.apex()] {
                let p = at.transform_point3(point);
                min = min.min(p - reach);
                max = max.max(p + reach);
            }
        }
        (min, max)
    }

    /// Meshes each body at `detail`, which must be valid, and combines the
    /// members. A single unmoved member is returned as it is.
    fn combined(&self, detail: &MushroomDetail) -> MushroomMesh {
        let meshes: Vec<MushroomMesh> = self.bodies.iter().map(|b| b.build(detail)).collect();
        if let [member] = self.members.as_slice()
            && member.transform() == Affine3A::IDENTITY
        {
            return meshes[member.variant as usize].clone();
        }
        let placed: Vec<(&MushroomMesh, Affine3A)> = self
            .members
            .iter()
            .map(|m| (&meshes[m.variant as usize], m.transform()))
            .collect();
        combine(&placed)
    }
}

impl MushroomBody {
    /// Works out the shape of a single mushroom made from `options`, after
    /// checking them, as body `index` of `specimen`. The cluster settings
    /// are not used, and `options.seed` must already be the body's own seed.
    pub(crate) fn new(
        options: MushroomOptions,
        specimen: SpecimenId,
        index: u32,
    ) -> Result<Self, OptionsError> {
        options.validate()?;
        let shape = Shape::new(&options);
        Ok(Self {
            options,
            specimen,
            index,
            shape,
        })
    }

    /// Makes the one body of single-stem specimen `specimen`, grown from
    /// `options`: exactly the mushroom the options describe, with the body's
    /// own seed. The cluster settings are not used.
    pub(crate) fn single(
        options: &MushroomOptions,
        specimen: SpecimenId,
    ) -> Result<Self, OptionsError> {
        let options = MushroomOptions {
            seed: body_seed(specimen.seed(), 0),
            cluster: ClusterOptions::default(),
            ..options.clone()
        };
        Self::new(options, specimen, 0)
    }

    /// Returns the id of the specimen this body belongs to.
    pub fn specimen(&self) -> SpecimenId {
        self.specimen
    }

    /// Returns this body's index within its [`specimen`](Self::specimen).
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Returns this body's options. Its
    /// [`cluster`](MushroomOptions::cluster) settings are the defaults,
    /// with a single stem, so bodies from different clusters compare equal
    /// when they look the same.
    pub fn options(&self) -> &MushroomOptions {
        &self.options
    }

    /// Builds the mesh at `detail`, such as [`MushroomDetail::FLAT`].
    ///
    /// # Errors
    ///
    /// Returns an error if a count in `detail` is out of range.
    pub fn mesh_with_detail(&self, detail: &MushroomDetail) -> Result<MushroomMesh, OptionsError> {
        detail.validate()?;
        Ok(self.build(detail))
    }

    /// Builds one mesh for each level of `lod`, nearest level first, paired
    /// with the level it belongs to. The distances in `lod` are used as
    /// given.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`MushroomLod::validate`].
    pub fn lod_meshes(
        &self,
        lod: &MushroomLod,
    ) -> Result<Vec<(LodLevel, MushroomMesh)>, OptionsError> {
        lod.validate()?;
        Ok(lod
            .levels
            .iter()
            .map(|level| (*level, self.build(&level.detail)))
            .collect())
    }

    /// Returns the cap radius in metres.
    pub fn cap_radius(&self) -> f32 {
        self.shape.radius
    }

    /// Returns the position of the top centre of the cap.
    pub fn apex(&self) -> Vec3 {
        self.shape.cap_apex()
    }

    /// Returns the unit direction the cap faces (its axis).
    pub fn cap_axis(&self) -> Vec3 {
        self.shape.cap_axis()
    }

    /// Returns the top of the stem, where the cap sits.
    pub fn cap_centre(&self) -> Vec3 {
        self.shape.stem_centre(1.0)
    }

    /// Returns the stem's radius at the ground in metres, including any
    /// bulb.
    pub fn base_radius(&self) -> f32 {
        self.shape.stem_radius(0.0)
    }

    /// Returns the lowest point of the stem's centre line: the bottom of a
    /// closed stem, below the ground, or the origin for an open one.
    pub fn bottom(&self) -> Vec3 {
        let ground = self.shape.stem_centre(0.0);
        self.shape.bottom_depth.map_or(ground, |depth| {
            ground - self.shape.stem_tangent(0.0) * depth
        })
    }

    /// Builds the mesh at `detail`, which must already be validated.
    fn build(&self, detail: &MushroomDetail) -> MushroomMesh {
        build(&self.shape, detail, &Palette::new(&self.options.material))
    }
}
