//! The Scatters an Ecotope can hold today, and one small adapter per kind
//! that places it over a [`ScatterInput`] and measures its copies'
//! footprints.

use std::fmt;
use std::sync::Arc;

use glam::{Affine3A, Vec2, Vec3, Vec3Swizzles};
use procgena_core::scatter::ScatterInput;
use procgena_deadwood::{
    Piece, ScatterError as DeadwoodScatterError, ScatterOptions as DeadwoodScatterOptions,
    StickScatter,
};
use procgena_mushroom::{
    MushroomOptions, MushroomScatter, ScatterError as MushroomScatterError,
    ScatterOptions as MushroomScatterOptions,
};
use procgena_plant::{PlantBake, PlantScatterOptions, PlantSettingsError};
use procgena_stone::{StoneScatter, StoneScatterError, StoneScatterOptions};
use procgena_tree::{TreeScatter, TreeScatterError, TreeScatterPlacement};

use super::footprints::{FootprintMask, Footprints, Shape};

/// The width of a plant Scatter's baked texels, in metres: the width of the
/// footprint masks every Scatter leaves.
pub const PLANT_CELL: f32 = 0.5;

/// What one Scatter in an Ecotope spreads, with its settings.
///
/// Each kind's own `default_seed` is not used inside an Ecotope: the
/// Ecotope gives every Scatter a seed of its own.
#[derive(Debug, Clone, PartialEq)]
pub enum ScatterKind {
    /// Trees; see [`TreeScatterPlacement::new`].
    Tree(TreeScatter),
    /// Boulders, rocks and pebbles; see [`StoneScatter::new`].
    Stone(StoneScatterOptions),
    /// Logs, sticks, twigs or stumps; see [`StickScatter::new`].
    Deadwood(DeadwoodScatterOptions),
    /// Whole mushrooms or clusters of one species on the ground; see
    /// [`MushroomScatter::new`].
    Mushroom {
        /// The species, or cluster, that grows.
        mushroom: MushroomOptions,
        /// Where and how densely it grows.
        scatter: MushroomScatterOptions,
    },
    /// Grass and other small plants, placed where they are drawn; see
    /// [`PlantScatterOptions::bake`].
    Plant(PlantScatterOptions),
}

impl ScatterKind {
    /// The kind's name as a preset file writes it: `tree`, `stone`,
    /// `deadwood`, `mushroom` or `plant`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Tree(_) => "tree",
            Self::Stone(_) => "stone",
            Self::Deadwood(_) => "deadwood",
            Self::Mushroom { .. } => "mushroom",
            Self::Plant(_) => "plant",
        }
    }

    /// Checks the settings, as far as can be done without placing.
    pub(crate) fn validate(&self) -> Result<(), ScatterFailure> {
        match self {
            Self::Tree(scatter) => scatter.validate().map_err(ScatterFailure::Tree),
            Self::Stone(options) => options.validate().map_err(ScatterFailure::Stone),
            Self::Deadwood(options) => options
                .validate()
                .and_then(|()| options.piece.validate())
                .map_err(|error| ScatterFailure::Deadwood(error.into())),
            Self::Mushroom { mushroom, scatter } => scatter
                .validate()
                .and_then(|()| mushroom.validate())
                .map_err(|error| ScatterFailure::Mushroom(error.into())),
            Self::Plant(options) => options.validate().map_err(ScatterFailure::Plant),
        }
    }

    /// Places the Scatter over `input`, and measures where its copies stand,
    /// each widened by `clearance` metres.
    /// `above` is the footprints of the Scatters above: `input`'s mask
    /// already keeps out of them, point by point, and a plant Scatter also
    /// clears every texel they touch.
    pub(crate) fn place(
        &self,
        input: &ScatterInput<'_>,
        above: &FootprintMask,
        clearance: f32,
    ) -> Result<(ScatterCopies, Footprints), ScatterFailure> {
        let mut footprints = Footprints::new(input.area.centre);
        let copies = match self {
            Self::Tree(scatter) => {
                let placed =
                    TreeScatterPlacement::new(scatter, input).map_err(ScatterFailure::Tree)?;
                for copy in placed.instances() {
                    let trunk = scatter.trees[copy.tree].tree.branch.radius[0];
                    footprints.insert(Shape::Disc {
                        centre: copy.position.xz(),
                        radius: trunk * scale_of(&copy.transform) + clearance,
                    });
                }
                ScatterCopies::Tree(placed)
            }
            Self::Stone(options) => {
                let placed = StoneScatter::new(options, input).map_err(ScatterFailure::Stone)?;
                for stone in placed.instances() {
                    footprints.insert(Shape::Disc {
                        centre: stone.position.xz(),
                        radius: stone.radius + clearance,
                    });
                }
                ScatterCopies::Stone(placed)
            }
            Self::Deadwood(options) => {
                let placed = StickScatter::new(options, input).map_err(ScatterFailure::Deadwood)?;
                let spans: Vec<Span> = placed.variants().iter().map(span_of).collect();
                for copy in placed.instances() {
                    let span = spans[copy.variant as usize];
                    let to_ground = |p: Vec2| {
                        copy.transform
                            .transform_point3(Vec3::new(p.x, 0.0, p.y))
                            .xz()
                    };
                    footprints.insert(Shape::Capsule {
                        a: to_ground(span.a),
                        b: to_ground(span.b),
                        radius: span.radius * scale_of(&copy.transform) + clearance,
                    });
                }
                ScatterCopies::Deadwood(placed)
            }
            Self::Mushroom { mushroom, scatter } => {
                let placed = MushroomScatter::new(mushroom, scatter, input)
                    .map_err(ScatterFailure::Mushroom)?;
                for member in placed.instances() {
                    footprints.insert(Shape::Disc {
                        centre: member.translation.xz(),
                        radius: clearance,
                    });
                }
                ScatterCopies::Mushroom(placed)
            }
            Self::Plant(options) => {
                // Plants are placed where they are drawn, from this bake,
                // and leave no footprints.
                let exclude = above.discs();
                let input = ScatterInput {
                    exclude: Some(&exclude),
                    ..*input
                };
                ScatterCopies::Plant(Arc::new(options.bake(&input, PLANT_CELL)))
            }
        };
        Ok((copies, footprints))
    }
}

/// The size factor of a copy's transform.
fn scale_of(transform: &Affine3A) -> f32 {
    transform.matrix3.x_axis.length()
}

/// The ground a lying piece of dead wood covers, in its own space: a
/// segment along its length and how far its sides reach from it.
#[derive(Debug, Clone, Copy)]
struct Span {
    a: Vec2,
    b: Vec2,
    radius: f32,
}

/// Measures the ground `piece` covers from its mesh's outline seen from
/// above: the longest direction through its middle, and its widest reach
/// across that.
fn span_of(piece: &Piece) -> Span {
    let points: Vec<Vec2> = piece
        .mesh()
        .surfaces
        .positions
        .iter()
        .map(|p| Vec2::new(p[0], p[2]))
        .collect();
    if points.is_empty() {
        return Span {
            a: Vec2::ZERO,
            b: Vec2::ZERO,
            radius: 0.0,
        };
    }

    // The longest direction of the outline, from its spread about the middle.
    let n = points.len() as f32;
    let mean = points.iter().copied().sum::<Vec2>() / n;
    let (mut xx, mut xz, mut zz) = (0.0, 0.0, 0.0);
    for p in &points {
        let d = *p - mean;
        xx += d.x * d.x;
        xz += d.x * d.y;
        zz += d.y * d.y;
    }
    let angle = 0.5 * (2.0 * xz).atan2(xx - zz);
    let axis = Vec2::from_angle(angle);

    // How far the outline reaches along that direction and across it.
    let (mut lo, mut hi, mut across) = (f32::MAX, f32::MIN, 0.0_f32);
    for p in &points {
        let d = *p - mean;
        let along = d.dot(axis);
        lo = lo.min(along);
        hi = hi.max(along);
        across = across.max(d.perp_dot(axis).abs());
    }

    // The capsule's rounded ends add its radius, so pull the ends in.
    let (lo, hi) = if hi - lo > 2.0 * across {
        (lo + across, hi - across)
    } else {
        let mid = 0.5 * (lo + hi);
        (mid, mid)
    };
    Span {
        a: mean + axis * lo,
        b: mean + axis * hi,
        radius: across,
    }
}

/// One Scatter's placed copies, as its own crate gives them. Positions are
/// relative to the centre of the area the Ecotope was placed over.
#[derive(Debug, Clone)]
pub enum ScatterCopies {
    /// Placed trees.
    Tree(TreeScatterPlacement),
    /// Placed stones, with the shape settings of each variant.
    Stone(StoneScatter),
    /// Placed dead wood, with its variants.
    Deadwood(StickScatter),
    /// Placed mushrooms, each whole copy and each member.
    Mushroom(MushroomScatter),
    /// A plant Scatter's ground and density over the area. Its plants are
    /// placed where they are drawn, so it holds no copies.
    Plant(Arc<PlantBake>),
}

impl ScatterCopies {
    /// The number of copies placed. A cluster of mushrooms, such as a fairy
    /// ring, counts as one copy, and a plant Scatter counts none: its
    /// plants are placed where they are drawn.
    pub fn len(&self) -> usize {
        match self {
            Self::Tree(placed) => placed.instances().len(),
            Self::Stone(placed) => placed.instances().len(),
            Self::Deadwood(placed) => placed.instances().len(),
            Self::Mushroom(placed) => placed.copies().len(),
            Self::Plant(_) => 0,
        }
    }

    /// Whether no copy was placed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why one Scatter of an Ecotope could not be placed: the error from its
/// own crate.
#[derive(Debug, Clone, PartialEq)]
pub enum ScatterFailure {
    /// From a tree Scatter.
    Tree(TreeScatterError),
    /// From a stone Scatter.
    Stone(StoneScatterError),
    /// From a dead wood Scatter.
    Deadwood(DeadwoodScatterError),
    /// From a mushroom Scatter.
    Mushroom(MushroomScatterError),
    /// From a plant Scatter's settings.
    Plant(PlantSettingsError),
}

impl fmt::Display for ScatterFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree(error) => error.fmt(f),
            Self::Stone(error) => error.fmt(f),
            Self::Deadwood(error) => error.fmt(f),
            Self::Mushroom(error) => error.fmt(f),
            Self::Plant(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ScatterFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tree(error) => Some(error),
            Self::Stone(error) => Some(error),
            Self::Deadwood(error) => Some(error),
            Self::Mushroom(error) => Some(error),
            Self::Plant(error) => Some(error),
        }
    }
}
