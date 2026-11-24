//! Scatter: mushrooms spread over another generator's surface, such as the
//! ground or a rock.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use glam::{Affine3A, IVec2, Quat, Vec2, Vec3};
use procgena_core::hash::{derive_seed, seed_for};
use procgena_core::scatter::Placement;
use procgena_core::{Quality, Rng, SpecimenId};
use procgena_specimen::{Specimen, Surface, SurfaceError, TagFilter};

use crate::cluster::{MushroomInstance, arrange, make_bodies};
use crate::detail::MushroomLod;
use crate::mesh::{MushroomMesh, combine};
use crate::mushroom::MushroomBody;
use crate::options::{MushroomOptions, OptionsError, int_range, non_negative, positive, within};
use crate::rng::{RngExt, SCATTER_CLUSTER, SCATTER_SITES};
use crate::specimen::specimen_from_meshes;

/// The most growing spots a scatter makes, however large the surface or
/// high the density.
pub const MAX_SITES: u32 = 20_000;

/// Settings for [`MushroomScatter::on`] and [`MushroomScatter::new`].
///
/// Lengths are in metres; for [`on`](MushroomScatter::on), in the host
/// specimen's units. Each field says if only one of the two uses it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScatterOptions {
    /// Number that picks the growing spots, sizes, turns, which variant
    /// grows at each spot and how each spot's cluster is laid out. It also
    /// stands in for the World seed of the variants' specimen ids, so it
    /// picks their shapes too. The same seed, settings and host always give
    /// the same scatter. Used by [`MushroomScatter::on`];
    /// [`MushroomScatter::new`] uses its input's seed instead.
    pub seed: u32,
    /// Which parts of the host mushrooms grow on, chosen by their tags, such
    /// as ground or rock; see [`procgena_specimen::tags`]. Only used by
    /// [`MushroomScatter::on`].
    pub grows_on: TagFilter,
    /// Which way is up, in the host specimen's coordinates. Any length other
    /// than zero. Only used by [`MushroomScatter::on`].
    pub up: Vec3,
    /// Growing spots per square metre of suitable surface. Must be zero or
    /// positive. Fewer may fit because of
    /// [`min_spacing`](Self::min_spacing), and there are never more than
    /// [`MAX_SITES`].
    pub density: f32,
    /// Steepest surface mushrooms grow on, in degrees from level, from 0
    /// (flat only) through 180 (anywhere, even overhangs).
    pub max_slope: f32,
    /// Smallest distance between growing spots, in metres. Must be zero or
    /// positive. A spot that grows a cluster counts as one spot, however
    /// wide the cluster.
    pub min_spacing: f32,
    /// Smallest size of what grows at a spot, as a multiple of the species'
    /// size. Must be positive. A cluster, such as a fairy ring, is scaled as
    /// a whole.
    pub scale_min: f32,
    /// Largest size of what grows at a spot, as a multiple of the species'
    /// size; at least `scale_min`.
    pub scale_max: f32,
    /// How much mushrooms tilt with a sloping surface instead of growing
    /// straight up, from 0 (straight up) through 1 (square to the surface).
    /// Only used by [`MushroomScatter::on`]; a ground scatter keeps
    /// mushrooms upright.
    pub follow_surface: f32,
    /// How each mushroom is set on the ground, or `None` for the species'
    /// own [`MushroomOptions::ground_placement`]. Lengths in it are shares
    /// of each stem's radius at the ground. [`conform`](Placement::conform)
    /// must be from 0 through 1, and [`embed`](Placement::embed) and any
    /// [`max_bend`](Placement::max_bend) zero or more.
    ///
    /// [`MushroomScatter::new`] places every member of a copy with it.
    /// [`MushroomScatter::on`] uses only its `embed`, to sink each stem
    /// into the host's surface.
    pub placement: Option<Placement>,
    /// How many different whole mushrooms (or clusters) a ground scatter
    /// ([`MushroomScatter::new`]) draws its copies from, from 1 through
    /// [`MAX_SPECIMENS`]. Not used by [`MushroomScatter::on`], which uses
    /// [`ClusterOptions::variants`](crate::ClusterOptions::variants).
    pub specimens: u32,
}

/// Largest allowed [`ScatterOptions::specimens`].
pub const MAX_SPECIMENS: u32 = 256;

impl Default for ScatterOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            grows_on: TagFilter::any_of([procgena_specimen::tags::GROUND]),
            up: Vec3::Y,
            density: 4.0,
            max_slope: 50.0,
            min_spacing: 0.15,
            scale_min: 0.6,
            scale_max: 1.1,
            follow_surface: 0.5,
            placement: None,
            specimens: 3,
        }
    }
}

impl ScatterOptions {
    /// Checks every setting against the range given in its documentation.
    ///
    /// # Errors
    ///
    /// Returns the first setting found out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        if !self.up.is_finite() || self.up.length_squared() == 0.0 {
            return Err(OptionsError::FloatOutOfRange {
                field: "scatter.up",
                value: self.up.length(),
                requirement: "non-zero",
            });
        }
        non_negative("scatter.density", self.density)?;
        within("scatter.max_slope", self.max_slope, 0.0, 180.0)?;
        non_negative("scatter.min_spacing", self.min_spacing)?;
        positive("scatter.scale_min", self.scale_min)?;
        within(
            "scatter.scale_max",
            self.scale_max,
            self.scale_min,
            f32::MAX,
        )?;
        within("scatter.follow_surface", self.follow_surface, 0.0, 1.0)?;
        if let Some(p) = &self.placement {
            within("scatter.placement.conform", p.conform, 0.0, 1.0)?;
            non_negative("scatter.placement.embed", p.embed)?;
            if let Some(bend) = p.max_bend {
                non_negative("scatter.placement.max_bend", bend)?;
            }
        }
        int_range("scatter.specimens", self.specimens, 1, MAX_SPECIMENS)
    }

    /// Returns the placement to use for `mushroom`: [`placement`](Self::placement)
    /// if set, otherwise the species' own
    /// [`ground_placement`](MushroomOptions::ground_placement).
    pub fn placement_for(&self, mushroom: &MushroomOptions) -> Placement {
        self.placement
            .unwrap_or_else(|| mushroom.ground_placement())
    }
}

/// Why a scatter could not be made.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScatterError {
    /// No part of the host's most detailed level matched
    /// [`ScatterOptions::grows_on`].
    NoSurface,
    /// The host's matching parts are not a usable mesh, for example because
    /// a triangle refers to a missing vertex.
    Surface(SurfaceError),
    /// A setting is out of range.
    Options(OptionsError),
    /// A ground scatter's area holds more candidate spots than
    /// [`MAX_CELLS`](crate::MAX_CELLS): the area is too large for its
    /// [`min_spacing`](ScatterOptions::min_spacing).
    TooLarge {
        /// How many candidate spots the area holds.
        cells: u64,
    },
}

impl fmt::Display for ScatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScatterError::NoSurface => {
                write!(f, "no part of the host matches the scatter's tag filter")
            }
            ScatterError::Surface(e) => write!(f, "the host surface is invalid: {e}"),
            ScatterError::Options(e) => write!(f, "invalid scatter options: {e}"),
            ScatterError::TooLarge { cells } => write!(
                f,
                "the scatter area holds {cells} candidate spots, more than {}",
                crate::MAX_CELLS
            ),
        }
    }
}

impl std::error::Error for ScatterError {}

impl From<OptionsError> for ScatterError {
    fn from(e: OptionsError) -> Self {
        ScatterError::Options(e)
    }
}

/// Mushrooms spread over another generator's output: a
/// [`procgena_specimen::Specimen`] such as a rock or ground patch, called the
/// host.
///
/// The result is a list of placed [`instances`](Self::instances), each a
/// copy of one of a few [`bodies`](Self::bodies), in the host's
/// coordinates. A renderer can mesh each body once and draw every instance
/// from it, which is far cheaper than one big mesh. Use
/// [`specimen`](Self::specimen) if you do need a single mesh.
#[derive(Debug, Clone)]
pub struct MushroomScatter {
    pub(crate) bodies: Vec<MushroomBody>,
    pub(crate) instances: Vec<MushroomInstance>,
    pub(crate) copies: Vec<MushroomCopy>,
}

/// One copy in a ground scatter ([`MushroomScatter::new`]): one whole
/// mushroom, or one whole cluster such as a fairy ring, whose members were
/// each set on the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct MushroomCopy {
    /// Which of the scatter's specimens this copy is, from 0 up to
    /// [`ScatterOptions::specimens`].
    pub slot: u32,
    /// The lattice cell the copy grew from. Equal cells in two scatters with
    /// the same input seed are the same copy.
    pub cell: IVec2,
    /// Where the copy's centre stands, relative to the area's centre: X and
    /// Z across, Y the ground height there.
    pub translation: Vec3,
    /// The copy's turn about the vertical, in radians.
    pub yaw: f32,
    /// The copy's size multiplier.
    pub scale: f32,
    /// The copy's entries in [`MushroomScatter::instances`].
    pub instances: Range<usize>,
    /// For each of those instances, its member's index in the specimen's
    /// [`Mushroom::members`](crate::Mushroom::members). A member that
    /// couldn't be set on the ground is missing, and the others keep their
    /// indices.
    pub members: Vec<u32>,
}

impl MushroomScatter {
    /// Spreads mushrooms like `mushroom` over `host`.
    ///
    /// Each growing spot grows the mushroom as its options describe: a
    /// single mushroom, or a whole cluster such as a tuft or a fairy ring
    /// when [`ClusterOptions::stems`](crate::ClusterOptions::stems) is more
    /// than one. Every spot gets its own variations, picked from
    /// [`ClusterOptions::variants`](crate::ClusterOptions::variants)
    /// different bodies. For a single mushroom these are the specimens in
    /// slots 0, 1, … of the mushroom's pool; for a cluster they are the
    /// bodies of the specimen in slot 0. Each specimen is seeded from its
    /// [`SpecimenId`], with [`ScatterOptions::seed`] as the World seed, and the
    /// mushroom's own [`seed`](MushroomOptions::seed) is not used.
    ///
    /// Mushrooms grow only on the parts of the host's most detailed level
    /// that [`ScatterOptions::grows_on`] picks, and only where the surface is
    /// no steeper than [`ScatterOptions::max_slope`]. A cluster's members are
    /// dropped straight down onto that surface, so a fairy ring follows
    /// uneven ground; a member with no such surface below or above it is
    /// left out. If no surface is gentle enough, the scatter is empty rather
    /// than an error. Mushrooms in a cluster are kept apart from each
    /// other, but not from neighbouring spots, so a cluster can touch a
    /// nearby mushroom.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::NoSurface`] if no part matches,
    /// [`ScatterError::Surface`] if the matching parts are malformed, and
    /// [`ScatterError::Options`] if a setting in `mushroom` or `options` is
    /// out of range.
    pub fn on(
        host: &Specimen,
        mushroom: &MushroomOptions,
        options: &ScatterOptions,
    ) -> Result<Self, ScatterError> {
        options.validate()?;
        mushroom.validate()?;
        let cluster = &mushroom.cluster;
        let pool = mushroom.pool();
        let specimen_seed = |slot| SpecimenId::new(options.seed, pool, slot).seed();
        let bodies = if cluster.stems == 1 {
            (0..cluster.variants)
                .map(|slot| MushroomBody::single(mushroom, specimen_seed(slot)))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            make_bodies(mushroom, specimen_seed(0), cluster.variants)?
        };
        let part = host
            .merged(0, &options.grows_on)
            .ok_or(ScatterError::NoSurface)?;
        let surface = part.surface().map_err(ScatterError::Surface)?;
        let up = options.up.normalize();

        // List the faces gentle enough to grow on, with a running total of
        // their area, so spots can be picked evenly by area.
        let min_facing = options.max_slope.to_radians().cos();
        let mut faces = Vec::new();
        let mut total = 0.0;
        for t in 0..surface.triangle_count() {
            let [a, b, c] = surface.triangle_positions(t);
            let cross = (b - a).cross(c - a);
            let area = cross.length() * 0.5;
            if area > 0.0 && cross.normalize().dot(up) >= min_facing - 1e-6 {
                total += area;
                faces.push((t, total));
            }
        }
        let mut instances = Vec::new();
        if faces.is_empty() {
            return Ok(Self {
                bodies,
                instances,
                copies: Vec::new(),
            });
        }
        // Clusters drop their members onto the gentle faces.
        let columns = (cluster.stems > 1).then(|| Columns::new(&surface, &faces, up));

        // Try random spots and keep those far enough from earlier ones. A
        // grid of cells, each `min_spacing` wide, means only nearby spots
        // need checking.
        let mut rng = Rng::new(seed_for(options.seed, SCATTER_SITES));
        let embed = options.placement_for(mushroom).embed;
        let cluster_seeds = seed_for(options.seed, SCATTER_CLUSTER);
        let wanted = ((options.density * total).round() as u32).min(MAX_SITES);
        let cell = options.min_spacing.max(1e-4);
        let mut grid: HashMap<[i32; 3], Vec<Vec3>> = HashMap::new();
        let key = |p: Vec3| (p / cell).floor().as_ivec3().to_array();
        let mut sites = 0;
        for _ in 0..wanted * 8 {
            if sites >= wanted {
                break;
            }
            // Pick a face, with larger faces more likely, then a random point
            // on it.
            let pick = rng.next_f32() * total;
            let index = faces
                .partition_point(|&(_, cumulative)| cumulative < pick)
                .min(faces.len() - 1);
            let t = faces[index].0;
            let [a, b, c] = surface.triangle_positions(t);
            let (mut u, mut v) = (rng.next_f32(), rng.next_f32());
            if u + v > 1.0 {
                (u, v) = (1.0 - u, 1.0 - v);
            }
            let point = a + (b - a) * u + (c - a) * v;

            let k = key(point);
            let crowded = (-1..=1).any(|x| {
                (-1..=1).any(|y| {
                    (-1..=1).any(|z| {
                        grid.get(&[k[0] + x, k[1] + y, k[2] + z])
                            .is_some_and(|cell| {
                                cell.iter().any(|q| q.distance(point) < options.min_spacing)
                            })
                    })
                })
            });
            if crowded {
                continue;
            }
            grid.entry(k).or_default().push(point);
            sites += 1;

            // Grow in a direction between straight up and square to the
            // surface, turned randomly about that direction.
            let [ia, ib, ic] = surface.triangle(t);
            let normal = (surface.normal(ia) * (1.0 - u - v)
                + surface.normal(ib) * u
                + surface.normal(ic) * v)
                .try_normalize()
                .unwrap_or(up);
            let grow = up
                .lerp(normal, options.follow_surface)
                .try_normalize()
                .unwrap_or(up);
            let rotation = Quat::from_rotation_arc(Vec3::Y, grow)
                * Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU));
            let scale = rng.range(options.scale_min, options.scale_max);

            // A single mushroom, sunk a little into the surface.
            let Some(columns) = &columns else {
                let variant = rng.int(0, bodies.len() as u32 - 1);
                let sink = bodies[variant as usize].base_radius() * scale * embed;
                instances.push(MushroomInstance {
                    variant,
                    translation: point - grow * sink,
                    rotation,
                    scale,
                });
                continue;
            };

            // A cluster around the spot, each member dropped onto the
            // surface and sunk a little into it.
            for member in arrange(&bodies, cluster, derive_seed(cluster_seeds, sites)) {
                let planned = point + rotation * (member.translation * scale);
                let Some(ground) = columns.drop(planned) else {
                    continue;
                };
                let scale = member.scale * scale;
                let sink = bodies[member.variant as usize].base_radius() * scale * embed;
                instances.push(MushroomInstance {
                    variant: member.variant,
                    translation: ground - grow * sink,
                    rotation: rotation * member.rotation,
                    scale,
                });
            }
        }
        Ok(Self {
            bodies,
            instances,
            copies: Vec::new(),
        })
    }

    /// Returns the different single mushrooms the instances are copies of.
    pub fn bodies(&self) -> &[MushroomBody] {
        &self.bodies
    }

    /// Returns the placed mushrooms: in the host's coordinates for
    /// [`on`](Self::on), and relative to the area's centre for
    /// [`new`](Self::new) (X and Z across, Y the ground height).
    pub fn instances(&self) -> &[MushroomInstance] {
        &self.instances
    }

    /// Returns the copies of a ground scatter ([`new`](Self::new)), each one
    /// whole mushroom or cluster, in lattice order. Empty for
    /// [`on`](Self::on).
    pub fn copies(&self) -> &[MushroomCopy] {
        &self.copies
    }

    /// Combines every instance into one [`Specimen`], with one detail level
    /// for each level of `lod` and parts of the same kind merged. This can
    /// be a large mesh; drawing [`instances`](Self::instances) is usually
    /// cheaper.
    ///
    /// Each level starts at its distance in `lod` scaled to the species'
    /// size at [`Quality::High`], as for [`crate::Mushroom::specimen`].
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`crate::validate_lod`].
    pub fn specimen(&self, lod: &MushroomLod) -> Result<Specimen, OptionsError> {
        let size = self.bodies[0].options().size;
        let meshes: Vec<_> = self
            .bodies
            .iter()
            .map(|b| b.lod_meshes(lod))
            .collect::<Result<_, _>>()?;
        let levels: Vec<_> = lod
            .levels
            .iter()
            .enumerate()
            .map(|(l, level)| {
                let placed: Vec<(&MushroomMesh, Affine3A)> = self
                    .instances
                    .iter()
                    .map(|i| (&meshes[i.variant as usize][l].1, i.transform()))
                    .collect();
                (*level, combine(&placed))
            })
            .collect();
        Ok(specimen_from_meshes(
            &levels,
            lod.factor(size, Quality::High),
        ))
    }
}

/// The host's gentle faces sorted into columns along the up direction, for
/// finding the surface straight above or below a point.
struct Columns {
    /// Up, as a unit vector.
    up: Vec3,
    /// Two unit directions across `up`, at right angles to each other.
    across: [Vec3; 2],
    /// Width of each column.
    width: f32,
    /// Corner positions of each gentle face.
    triangles: Vec<[Vec3; 3]>,
    /// The faces whose footprint touches each column, by column.
    cells: HashMap<[i32; 2], Vec<u32>>,
}

impl Columns {
    /// Sorts `faces` of `surface`, given as (triangle number, running area
    /// total), into columns along the unit vector `up`.
    fn new(surface: &Surface<'_>, faces: &[(usize, f32)], up: Vec3) -> Self {
        let (a, b) = up.any_orthonormal_pair();
        let triangles: Vec<[Vec3; 3]> = faces
            .iter()
            .map(|&(t, _)| surface.triangle_positions(t))
            .collect();

        // Columns about twice as wide as an average face, so each face
        // touches only a few.
        let area = faces.last().map_or(0.0, |&(_, total)| total);
        let width = (area / faces.len() as f32).sqrt().max(1e-4) * 2.0;
        let mut columns = Self {
            up,
            across: [a, b],
            width,
            triangles,
            cells: HashMap::new(),
        };
        for (i, corners) in columns.triangles.iter().enumerate() {
            let flat = corners.map(|p| columns.flatten(p));
            let min = flat[0].min(flat[1]).min(flat[2]);
            let max = flat[0].max(flat[1]).max(flat[2]);
            let (lo, hi) = (columns.cell(min), columns.cell(max));
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    columns.cells.entry([x, y]).or_default().push(i as u32);
                }
            }
        }
        columns
    }

    /// Returns `p` seen from above: its position across the up direction.
    fn flatten(&self, p: Vec3) -> Vec2 {
        Vec2::new(p.dot(self.across[0]), p.dot(self.across[1]))
    }

    /// Returns the column holding flattened position `p`.
    fn cell(&self, p: Vec2) -> [i32; 2] {
        (p / self.width).floor().as_ivec2().to_array()
    }

    /// Returns the point of the gentle faces on the vertical line through
    /// `p` that is nearest to `p`, or `None` if the line misses them all.
    fn drop(&self, p: Vec3) -> Option<Vec3> {
        let candidates = self.cells.get(&self.cell(self.flatten(p)))?;
        candidates
            .iter()
            .filter_map(|&i| line_hit(p, self.up, self.triangles[i as usize]))
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .map(|t| p + self.up * t)
    }
}

/// Returns how far along `direction` from `origin`, forward or backward,
/// the line through them crosses triangle `[a, b, c]`, or `None` if it
/// misses or runs along it.
fn line_hit(origin: Vec3, direction: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let h = direction.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = origin - a;
    let u = s.dot(h) / det;
    let q = s.cross(e1);
    let v = direction.dot(q) / det;
    let inside = (-1e-6..=1.0 + 1e-6).contains(&u) && v >= -1e-6 && u + v <= 1.0 + 1e-6;
    inside.then(|| e2.dot(q) / det)
}
