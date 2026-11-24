//! The [`Mushroom`] type: a checked set of options, ready to be meshed, and
//! the single mushrooms ([`MushroomBody`]) it is made of.

use glam::{Affine3A, Vec3};
use procgena_core::PoolId;

use crate::cluster::{ClusterOptions, MushroomInstance, arrange, make_bodies};
use crate::colour::Palette;
use crate::detail::{MushroomDetail, MushroomLod, MushroomLodLevel, validate_lod};
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
/// let mushroom = Mushroom::from_preset(MushroomPreset::fly_agaric()).unwrap();
/// let close = mushroom.mesh();
/// let far = mushroom.mesh_with_detail(&MushroomDetail::FLAT).unwrap();
/// assert!(far.triangle_count() < close.triangle_count());
/// ```
#[derive(Debug, Clone)]
pub struct Mushroom {
    options: MushroomOptions,
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
/// Bodies with the same [`id`](Self::id) are identical.
#[derive(Debug, Clone)]
pub struct MushroomBody {
    options: MushroomOptions,
    id: BodyId,
    shape: Shape,
}

/// Names one [`MushroomBody`]: bodies with equal ids are identical, so a
/// renderer can mesh each id once and share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId {
    /// The pool of the options the body was grown from; see
    /// [`MushroomOptions::pool`].
    pub pool: PoolId,
    /// The seed the whole mushroom or cluster was grown with: the options'
    /// [`default_seed`](MushroomOptions::default_seed) for a [`Mushroom`],
    /// or a specimen seed in a scatter.
    pub seed: u32,
    /// The body's index among the bodies grown with that seed.
    pub index: u32,
}

impl Mushroom {
    /// Creates a mushroom, or a cluster, from `options`, after checking
    /// them. Its shape comes from [`MushroomOptions::default_seed`].
    ///
    /// # Errors
    ///
    /// Returns the first setting that is out of range.
    pub fn new(options: MushroomOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let seed = options.default_seed;

        // A single mushroom is exactly what the options describe; a
        // cluster's members are variations of it.
        let cluster = &options.cluster;
        let (bodies, members) = if cluster.stems == 1 {
            let body = MushroomBody::single(&options, seed)?;
            let member = MushroomInstance {
                variant: 0,
                translation: Vec3::ZERO,
                rotation: glam::Quat::IDENTITY,
                scale: 1.0,
            };
            (vec![body], vec![member])
        } else {
            let bodies = make_bodies(&options, seed, cluster.variants.min(cluster.stems))?;
            let members = arrange(&bodies, cluster, seed);
            (bodies, members)
        };
        Ok(Self {
            options,
            bodies,
            members,
        })
    }

    /// Creates a mushroom, or a cluster, from a preset's settings. To adjust
    /// settings first, clone [`MushroomPreset::mushroom`], edit it, and pass
    /// it to [`Self::new`] instead.
    ///
    /// # Errors
    ///
    /// Returns the first setting that is out of range. Built-in presets
    /// always pass; a preset read from a file was checked when it was read.
    pub fn from_preset(preset: &MushroomPreset) -> Result<Self, OptionsError> {
        Self::new(preset.mushroom.clone())
    }

    /// Returns the options this mushroom was made from.
    pub fn options(&self) -> &MushroomOptions {
        &self.options
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
    /// The levels are returned as written in `lod`, for its reference size;
    /// use [`Lod::factor`](procgena_core::Lod::factor) with this mushroom's
    /// size to scale their distances.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`validate_lod`].
    pub fn lod_meshes(
        &self,
        lod: &MushroomLod,
    ) -> Result<Vec<(MushroomLodLevel, MushroomMesh)>, OptionsError> {
        validate_lod(lod)?;
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
    /// checking them, named `id`. The cluster settings are not used, and
    /// `options.default_seed` must already be the body's own seed.
    pub(crate) fn new(options: MushroomOptions, id: BodyId) -> Result<Self, OptionsError> {
        options.validate()?;
        let shape = Shape::new(&options);
        Ok(Self { options, id, shape })
    }

    /// Makes the one body of a single-stem mushroom grown from `options`
    /// with `seed`: exactly the mushroom the options describe, with `seed`
    /// in place of theirs. The cluster settings are not used.
    pub(crate) fn single(options: &MushroomOptions, seed: u32) -> Result<Self, OptionsError> {
        let id = BodyId {
            pool: options.pool(),
            seed,
            index: 0,
        };
        let options = MushroomOptions {
            default_seed: seed,
            cluster: ClusterOptions::default(),
            ..options.clone()
        };
        Self::new(options, id)
    }

    /// Returns this body's id. Bodies with equal ids are identical.
    pub fn id(&self) -> BodyId {
        self.id
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
    /// with the level it belongs to, as written in `lod`.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`validate_lod`].
    pub fn lod_meshes(
        &self,
        lod: &MushroomLod,
    ) -> Result<Vec<(MushroomLodLevel, MushroomMesh)>, OptionsError> {
        validate_lod(lod)?;
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
