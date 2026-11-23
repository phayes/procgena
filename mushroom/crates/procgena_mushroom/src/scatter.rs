//! Scatter: mushrooms spread over another generator's surface, such as the
//! ground or a rock.

use std::collections::HashMap;
use std::fmt;

use glam::{Affine3A, Quat, Vec2, Vec3};
use procgena_mesh::{Object, Surface, SurfaceError, TagFilter};

use crate::cluster::{MushroomInstance, arrange, make_variants};
use crate::detail::MushroomLod;
use crate::mesh::{MushroomMesh, combine};
use crate::mushroom::MushroomBody;
use crate::object::object_from_meshes;
use crate::options::{MushroomOptions, OptionsError, non_negative, positive, within};
use crate::rng::Rng;

/// The most growing spots a scatter makes, however large the surface or
/// high the density.
pub const MAX_SITES: u32 = 20_000;

/// Random-number stream for growing spots, sizes and turns.
const STREAM_SITES: u32 = 21;

/// Settings for [`MushroomScatter::on`].
///
/// Lengths are in the host object's units, which are metres for procgena
/// generators.
#[derive(Debug, Clone, PartialEq)]
pub struct ScatterOptions {
    /// Number that picks the growing spots, sizes and turns. The same seed,
    /// settings and host always give the same scatter.
    pub seed: u32,
    /// Which parts of the host mushrooms grow on, chosen by their tags, such
    /// as ground or rock; see [`procgena_mesh::tags`].
    pub grows_on: TagFilter,
    /// Which way is up, in the host object's coordinates. Any length other
    /// than zero.
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
    pub follow_surface: f32,
    /// How deep each stem base sits in the surface, as a multiple of its
    /// radius, from 0 through 2. A little depth hides gaps on uneven ground.
    pub sink: f32,
}

impl Default for ScatterOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            grows_on: TagFilter::any_of([procgena_mesh::tags::GROUND]),
            up: Vec3::Y,
            density: 4.0,
            max_slope: 50.0,
            min_spacing: 0.15,
            scale_min: 0.6,
            scale_max: 1.1,
            follow_surface: 0.5,
            sink: 0.3,
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
        within("scatter.sink", self.sink, 0.0, 2.0)
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
}

impl fmt::Display for ScatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScatterError::NoSurface => {
                write!(f, "no part of the host matches the scatter's tag filter")
            }
            ScatterError::Surface(e) => write!(f, "the host surface is invalid: {e}"),
            ScatterError::Options(e) => write!(f, "invalid scatter options: {e}"),
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
/// [`procgena_mesh::Object`] such as a rock or ground patch, called the
/// host.
///
/// The result is a list of placed [`instances`](Self::instances), each a
/// copy of one of a few [`bodies`](Self::bodies), in the host's
/// coordinates. A renderer can mesh each body once and draw every instance
/// from it, which is far cheaper than one big mesh. Use
/// [`object`](Self::object) if you do need a single mesh.
#[derive(Debug, Clone)]
pub struct MushroomScatter {
    bodies: Vec<MushroomBody>,
    instances: Vec<MushroomInstance>,
}

impl MushroomScatter {
    /// Spreads mushrooms like `mushroom` over `host`.
    ///
    /// Each growing spot grows the mushroom as its options describe: a
    /// single mushroom, or a whole cluster such as a tuft or a fairy ring
    /// when [`ClusterOptions::stems`](crate::ClusterOptions::stems) is more
    /// than one. Every spot gets its own variations, picked from
    /// [`ClusterOptions::variants`](crate::ClusterOptions::variants)
    /// different bodies.
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
        host: &Object,
        mushroom: &MushroomOptions,
        options: &ScatterOptions,
    ) -> Result<Self, ScatterError> {
        options.validate()?;
        let cluster = &mushroom.cluster;
        let bodies = make_variants(mushroom, cluster.variants, options.seed)?;
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
            return Ok(Self { bodies, instances });
        }
        // Clusters drop their members onto the gentle faces.
        let columns = (cluster.stems > 1).then(|| Columns::new(&surface, &faces, up));

        // Try random spots and keep those far enough from earlier ones. A
        // grid of cells, each `min_spacing` wide, means only nearby spots
        // need checking.
        let mut rng = Rng::new(options.seed, STREAM_SITES);
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
                let sink = bodies[variant as usize].base_radius() * scale * options.sink;
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
            let seed = options.seed ^ sites.wrapping_mul(0x85eb_ca6b);
            for member in arrange(&bodies, cluster, seed) {
                let planned = point + rotation * (member.translation * scale);
                let Some(ground) = columns.drop(planned) else {
                    continue;
                };
                let scale = member.scale * scale;
                let sink = bodies[member.variant as usize].base_radius() * scale * options.sink;
                instances.push(MushroomInstance {
                    variant: member.variant,
                    translation: ground - grow * sink,
                    rotation: rotation * member.rotation,
                    scale,
                });
            }
        }
        Ok(Self { bodies, instances })
    }

    /// Returns the different single mushrooms the instances are copies of.
    pub fn bodies(&self) -> &[MushroomBody] {
        &self.bodies
    }

    /// Returns the placed mushrooms, in the host's coordinates.
    pub fn instances(&self) -> &[MushroomInstance] {
        &self.instances
    }

    /// Combines every instance into one [`Object`], with one detail level
    /// for each level of `lod` and parts of the same kind merged. This can
    /// be a large mesh; drawing [`instances`](Self::instances) is usually
    /// cheaper.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`MushroomLod::validate`].
    pub fn object(&self, lod: &MushroomLod) -> Result<Object, OptionsError> {
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
        Ok(object_from_meshes(&levels))
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
