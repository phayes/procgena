//! Where one Scatter's copies stand, as discs and capsules on the ground,
//! and the mask that keeps lower-priority Scatters out of them.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DVec2, Vec2};
use procgena_core::scatter::{Disc, Mask};

/// Width of a lookup cell, in metres.
const CELL: f32 = 2.0;

/// The ground one Scatter's copies take up: a disc around each upright copy,
/// and a capsule along each lying one, widened by the Scatter's clearance.
///
/// Get one from [`PlacedScatter::footprints`](crate::PlacedScatter::footprints).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Footprints {
    /// World position the shapes are measured from.
    origin: DVec2,
    shapes: Vec<Shape>,
    /// Indexes into `shapes`, by the lookup cells each shape's bounds touch.
    cells: HashMap<(i32, i32), Vec<u32>>,
}

/// One copy's footprint, relative to [`Footprints::origin`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    /// Everything within `radius` of `centre`.
    Disc { centre: Vec2, radius: f32 },
    /// Everything within `radius` of the segment from `a` to `b`.
    Capsule { a: Vec2, b: Vec2, radius: f32 },
}

impl Shape {
    /// The smallest and largest corner of the shape's bounds.
    fn bounds(self) -> (Vec2, Vec2) {
        match self {
            Self::Disc { centre, radius } => (centre - radius, centre + radius),
            Self::Capsule { a, b, radius } => (a.min(b) - radius, a.max(b) + radius),
        }
    }

    /// Adds discs that cover the shape to `out`, as World positions from
    /// `origin`: the disc itself, or a chain along a capsule.
    ///
    /// A capsule's discs stand one radius apart along its length, each
    /// widened so the chain covers the capsule's whole width between them.
    fn cover(self, origin: DVec2, out: &mut procgena_core::scatter::Footprints) {
        let at = |p: Vec2| origin + p.as_dvec2();
        match self {
            Self::Disc { centre, radius } => out.insert(Disc::new(at(centre), radius)),
            Self::Capsule { a, b, radius } => {
                if radius <= 0.0 {
                    return;
                }
                let steps = (a.distance(b) / radius).ceil().max(1.0) as u32;
                let spacing = a.distance(b) / steps as f32;
                let widened = radius.hypot(0.5 * spacing);
                for i in 0..=steps {
                    let centre = a.lerp(b, i as f32 / steps as f32);
                    out.insert(Disc::new(at(centre), widened));
                }
            }
        }
    }

    /// Whether `p` lies inside the shape or on its edge.
    fn contains(self, p: Vec2) -> bool {
        match self {
            Self::Disc { centre, radius } => p.distance_squared(centre) <= radius * radius,
            Self::Capsule { a, b, radius } => {
                p.distance_squared(nearest_on(a, b, p)) <= radius * radius
            }
        }
    }
}

/// The point of the segment from `a` to `b` nearest to `p`.
fn nearest_on(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let ab = b - a;
    let length = ab.length_squared();
    let t = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    a + ab * t
}

impl Footprints {
    /// Footprints measured from World position `origin`, with no shapes yet.
    pub(crate) fn new(origin: DVec2) -> Self {
        Self {
            origin,
            shapes: Vec::new(),
            cells: HashMap::new(),
        }
    }

    /// Adds one shape, given relative to the origin. Shapes with a size or
    /// position that isn't finite are skipped.
    pub(crate) fn insert(&mut self, shape: Shape) {
        let (lo, hi) = shape.bounds();
        if !lo.is_finite() || !hi.is_finite() {
            return;
        }
        let index = self.shapes.len() as u32;
        self.shapes.push(shape);
        let (lo, hi) = (cell_of(lo), cell_of(hi));
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                self.cells.entry((x, z)).or_default().push(index);
            }
        }
    }

    /// The number of copies' footprints held.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Whether no footprint is held.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Whether World position `p` lies inside any footprint, or on its edge.
    pub fn contains(&self, p: DVec2) -> bool {
        let local = (p - self.origin).as_vec2();
        self.cells.get(&cell_of(local)).is_some_and(|shapes| {
            shapes
                .iter()
                .any(|&i| self.shapes[i as usize].contains(local))
        })
    }

}

/// The lookup cell holding `p`.
fn cell_of(p: Vec2) -> (i32, i32) {
    let cell = (p / CELL).floor();
    (cell.x as i32, cell.y as i32)
}

/// The mask a Scatter in an Ecotope was placed with, leaving out the
/// Ecotope's own coverage: 0 inside the footprints of every Scatter above
/// it, and 1 everywhere else.
///
/// Get one from [`EcotopePlacement::mask_above`](crate::EcotopePlacement::mask_above).
/// Multiply it by the coverage the Ecotope was placed with to get the whole
/// mask.
#[derive(Debug, Clone, Default)]
pub struct FootprintMask {
    above: Vec<Arc<Footprints>>,
}

impl FootprintMask {
    /// A mask that keeps out of each of `above`.
    pub(crate) fn new(above: Vec<Arc<Footprints>>) -> Self {
        Self { above }
    }

    /// Whether World position `p` lies inside any footprint the mask keeps
    /// out of.
    pub fn blocks(&self, p: DVec2) -> bool {
        self.above.iter().any(|footprints| footprints.contains(p))
    }

    /// Discs covering every footprint the mask keeps out of, for a Scatter
    /// that clears whole areas around them rather than testing points.
    pub(crate) fn discs(&self) -> procgena_core::scatter::Footprints {
        let mut discs = procgena_core::scatter::Footprints::new();
        for footprints in &self.above {
            for shape in &footprints.shapes {
                shape.cover(footprints.origin, &mut discs);
            }
        }
        discs
    }
}

impl Mask for FootprintMask {
    fn density(&self, p: DVec2) -> f32 {
        if self.blocks(p) { 0.0 } else { 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discs_and_capsules_block_what_they_cover() {
        let mut footprints = Footprints::new(DVec2::new(100.0, -50.0));
        footprints.insert(Shape::Disc {
            centre: Vec2::new(1.0, 1.0),
            radius: 0.5,
        });
        footprints.insert(Shape::Capsule {
            a: Vec2::new(-5.0, 0.0),
            b: Vec2::new(5.0, 0.0),
            radius: 0.25,
        });
        let at = |x: f64, z: f64| footprints.contains(DVec2::new(100.0 + x, -50.0 + z));
        assert!(at(1.0, 1.4));
        assert!(!at(1.0, 1.6));
        assert!(at(-4.9, 0.2));
        assert!(at(4.0, -0.2));
        assert!(!at(0.0, 0.3));
        assert!(!at(5.3, 0.0));
        assert_eq!(footprints.len(), 2);
    }

    #[test]
    fn shapes_that_are_not_finite_are_skipped() {
        let mut footprints = Footprints::new(DVec2::ZERO);
        footprints.insert(Shape::Disc {
            centre: Vec2::NAN,
            radius: 1.0,
        });
        assert!(footprints.is_empty());
    }

    #[test]
    fn the_mask_is_zero_inside_any_footprint_above() {
        let mut first = Footprints::new(DVec2::ZERO);
        first.insert(Shape::Disc {
            centre: Vec2::ZERO,
            radius: 1.0,
        });
        let mut second = Footprints::new(DVec2::ZERO);
        second.insert(Shape::Disc {
            centre: Vec2::new(10.0, 0.0),
            radius: 1.0,
        });
        let mask = FootprintMask::new(vec![Arc::new(first), Arc::new(second)]);
        assert_eq!(mask.density(DVec2::ZERO), 0.0);
        assert_eq!(mask.density(DVec2::new(10.0, 0.5)), 0.0);
        assert_eq!(mask.density(DVec2::new(5.0, 0.0)), 1.0);
    }

    #[test]
    fn the_discs_cover_every_footprint_above() {
        let mut shapes = Footprints::new(DVec2::new(5.0, -3.0));
        shapes.insert(Shape::Disc {
            centre: Vec2::new(1.0, 1.0),
            radius: 0.3,
        });
        shapes.insert(Shape::Capsule {
            a: Vec2::new(-2.0, 0.0),
            b: Vec2::new(2.0, 1.5),
            radius: 0.4,
        });
        let mask = FootprintMask::new(vec![Arc::new(shapes)]);
        let discs = mask.discs();

        // Every point inside a footprint is inside a disc, and points well
        // clear of the footprints are in none.
        let covered = |p: DVec2| {
            discs
                .discs()
                .iter()
                .any(|disc| disc.contains(procgena_core::scatter::lattice::Fixed2::from_world(p)))
        };
        for i in 0..200 {
            for j in 0..120 {
                let p = DVec2::new(2.0 + f64::from(i) * 0.03, -4.5 + f64::from(j) * 0.03);
                if mask.blocks(p) {
                    assert!(covered(p), "{p} is in a footprint but no disc");
                }
                let near = (0..10).any(|k| {
                    let angle = f64::from(k) * std::f64::consts::TAU / 10.0;
                    mask.blocks(p + DVec2::from_angle(angle) * 0.15)
                });
                if !near && !mask.blocks(p) {
                    assert!(!covered(p), "{p} is well clear of every footprint");
                }
            }
        }
    }
}
