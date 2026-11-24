//! The structure of one cluster: its stalks and the place, size and
//! direction of each item, in the cluster's own frame.

use std::f32::consts::TAU;

use glam::{Quat, Vec3};
use procgena_core::Rng;
use procgena_core::hash::{derive_seed, salt};

use crate::{ClusterKind, Facing, FruitOptions};

/// Angle between successive items around a stem, about 137.5°.
/// This keeps items from lining up in repeated rows.
const GOLDEN_ANGLE: f32 = 2.399_963;
/// Number of straight sections used to approximate a curved item or side stalk.
const STALK_SEGMENTS: usize = 4;
/// Number of straight sections used to approximate a cluster's main stem.
const AXIS_SEGMENTS: usize = 8;
/// Fraction of the main stem used to turn from the branch toward the cluster direction.
const AXIS_TURN: f32 = 0.25;
/// The random stream for the per-item variation, kept apart from the
/// structure's stream so that changing [`Variation`](crate::Variation)
/// never moves an item.
const VARIATION_SALT: u32 = salt("fruit.skeleton.variation");

/// The stalks and items of one cluster, in the cluster's own frame.
///
/// The cluster attaches to its host at the origin and faces local +Y. For
/// [`Facing::Hang`] and [`Facing::Up`] its host's surface lies toward local
/// −Z, so stalks start out along +Z; for [`Facing::Outward`] they start
/// out along +Y. Lengths are in metres.
///
/// Both lists are flat and stable: index `k` of [`stalks`](Self::stalks)
/// is part `stalk k`, and index `k` of [`items`](Self::items) is part
/// `item k`, at every level of detail.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FruitSkeleton {
    /// Supporting stalks and main stems. Each appears after any stalk it
    /// grows from.
    pub stalks: Vec<ClusterStalk>,
    /// The individual fruit or flowers.
    pub items: Vec<ClusterItem>,
}

/// A supporting stalk or main stem, described by points from its base to its tip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterStalk {
    /// Points along the stalk in the cluster frame, from base to tip. A
    /// built stalk has at least two points.
    pub points: Vec<Vec3>,
    /// Thickness of the stalk, as a radius in metres.
    pub radius: f32,
    /// Distance from the cluster origin to this stalk's base, following the
    /// supporting stalks rather than measuring in a straight line.
    pub base_distance: f32,
    /// The supporting stalk's index in [`FruitSkeleton::stalks`], which
    /// comes earlier in that list. `None` when this stalk starts at the
    /// cluster origin.
    pub parent: Option<usize>,
}

/// Position, size, direction and individual differences of one fruit or flower.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterItem {
    /// Attachment point of the item, in the cluster frame.
    pub origin: Vec3,
    /// Rotation from item coordinates to the cluster frame. Local +Y points
    /// away from the attachment into the item.
    pub orientation: Quat,
    /// Size in metres after random variation and cluster taper, before
    /// maturity changes it.
    pub size: f32,
    /// Index of the supporting stalk in [`FruitSkeleton::stalks`]. `None`
    /// when the item attaches directly at the cluster origin.
    pub stalk: Option<usize>,
    /// Distance from the cluster origin to the item, measured along its
    /// supporting stalks.
    pub distance: f32,
    /// How this item differs from the cluster's settings.
    pub variation: ItemVariation,
}

/// How one item differs from its cluster's settings, drawn from
/// [`Variation`](crate::Variation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemVariation {
    /// Added to the cluster's maturity, then clamped to 0–1.
    pub maturity: f32,
    /// Turn of the item's colours around the colour wheel, in fractions of
    /// a whole turn.
    pub hue: f32,
    /// Multiplies the fruit's blush strength.
    pub blush: f32,
    /// Lopsidedness from −1 to 1 for each of three shape traits, already
    /// scaled by [`Variation::shape_jitter`](crate::Variation::shape_jitter).
    pub shape: [f32; 3],
    /// Turn of the fruit's stem bend around the stem, in radians.
    pub stem_turn: f32,
}

impl ItemVariation {
    /// No difference from the cluster's settings.
    pub const NONE: Self = Self {
        maturity: 0.0,
        hue: 0.0,
        blush: 1.0,
        shape: [0.0; 3],
        stem_turn: 0.0,
    };
}

impl FruitSkeleton {
    /// Grows the cluster that `options` describe from `seed`. The same
    /// options and seed always grow the same cluster.
    ///
    /// Pass validated options.
    pub fn generate(options: &FruitOptions, seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let mut skeleton = ClusterBuilder::new(options, &mut rng).build();

        // Each item's differences come from a stream of its own.
        let variation_seed = derive_seed(seed, VARIATION_SALT);
        for (k, item) in (0u32..).zip(&mut skeleton.items) {
            let mut rng = Rng::new(derive_seed(variation_seed, k));
            item.variation = draw_variation(options, &mut rng);
        }
        skeleton
    }

    /// The distance from the cluster origin to the farthest point of its
    /// stalks and items, each item counted as a ball of its size, in metres.
    pub fn radius(&self) -> f32 {
        let stalks = self
            .stalks
            .iter()
            .flat_map(|stalk| &stalk.points)
            .map(|point| point.length());
        let items = self
            .items
            .iter()
            .map(|item| item.origin.length() + item.size);
        stalks.chain(items).fold(0.0, f32::max)
    }

    /// The distance from the cluster origin to the farthest stalk tip or
    /// item, measured along the stalks. Zero for a cluster with no length.
    pub fn reach(&self) -> f32 {
        let stalk_reach = self
            .stalks
            .iter()
            .map(|s| s.base_distance + polyline_length(&s.points));
        let item_reach = self.items.iter().map(|i| i.distance);
        stalk_reach.chain(item_reach).fold(0.0, f32::max)
    }
}

/// Draws one item's differences. Always draws the same number of values,
/// so a zero spread changes nothing else.
fn draw_variation(options: &FruitOptions, rng: &mut Rng) -> ItemVariation {
    let v = &options.variation;
    let maturity = rng.range(-1.0, 1.0) * v.maturity_spread;
    let hue = rng.range(-1.0, 1.0) * v.hue_spread * 0.25;
    let blush = 1.0 + rng.range(-1.0, 1.0) * v.blush_spread;
    let shape = [
        rng.range(-1.0, 1.0) * v.shape_jitter,
        rng.range(-1.0, 1.0) * v.shape_jitter,
        rng.range(-1.0, 1.0) * v.shape_jitter,
    ];
    let stem_turn = rng.range(-1.0, 1.0) * v.stem_curve_jitter * std::f32::consts::PI;
    ItemVariation {
        maturity,
        hue,
        blush,
        shape,
        stem_turn,
    }
}

/// The directions of a cluster frame for one facing: away from the host
/// surface, and the ground's downward direction.
struct Frame {
    out: Vec3,
    down: Vec3,
}

impl Frame {
    fn new(facing: Facing) -> Self {
        match facing {
            Facing::Hang => Self {
                out: Vec3::Z,
                down: Vec3::Y,
            },
            Facing::Up => Self {
                out: Vec3::Z,
                down: Vec3::NEG_Y,
            },
            Facing::Outward => Self {
                out: Vec3::Y,
                down: Vec3::NEG_Z,
            },
        }
    }
}

/// Collects stalks and items for one cluster using shared settings and random choices.
struct ClusterBuilder<'a> {
    options: &'a FruitOptions,
    rng: &'a mut Rng,
    frame: Frame,
    stalks: Vec<ClusterStalk>,
    items: Vec<ClusterItem>,
}

impl<'a> ClusterBuilder<'a> {
    fn new(options: &'a FruitOptions, rng: &'a mut Rng) -> Self {
        Self {
            options,
            rng,
            frame: Frame::new(options.facing),
            stalks: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Builds the stalks and items in the chosen arrangement.
    fn build(mut self) -> FruitSkeleton {
        let count = self.item_count();
        let (origin, out, facing) = (Vec3::ZERO, self.frame.out, Vec3::Y);
        match self.options.cluster.kind {
            ClusterKind::Single => self.single(origin, out, facing),
            ClusterKind::Umbel => self.umbel(origin, out, count),
            ClusterKind::Raceme => self.raceme(origin, out, facing, count),
            ClusterKind::Panicle => self.panicle(origin, out, facing, count),
            ClusterKind::Catkin => self.catkin(origin, out, facing, count),
        }
        FruitSkeleton {
            stalks: self.stalks,
            items: self.items,
        }
    }

    /// Places one item on a curved stalk, or directly at `origin` if the
    /// stalk length is zero. The item points along `facing`.
    fn single(&mut self, origin: Vec3, out: Vec3, facing: Vec3) {
        let length = self.options.cluster.stalk_length;
        if length > 0.0 {
            let path = self.curve(origin, out, facing, 1.0, 0.0, length, STALK_SEGMENTS);
            let end = path.end();
            let stalk = self.add_stalk(path.points, 0.0, None);
            self.add_item(end, facing, 1.0, Some(stalk), length);
        } else {
            self.add_item(origin, facing, 1.0, None, 0.0);
        }
    }

    /// Builds an umbel: item stalks fan out from one point at the
    /// configured spread angle from the cluster direction.
    fn umbel(&mut self, origin: Vec3, out: Vec3, count: u32) {
        let spread = self.options.cluster.spread.to_radians();
        let length = self.options.cluster.stalk_length;
        let slot = TAU / count as f32;
        for j in 0..count {
            // Space stalks around the cluster direction with a small random angular offset.
            let azimuth = (j as f32 + self.rng.range(-0.25, 0.25)) * slot;
            let direction = tilted(Vec3::Y, Vec3::X, Vec3::Z, spread, azimuth);
            if length > 0.0 {
                let path = self.curve(origin, out, direction, 1.0, 0.0, length, STALK_SEGMENTS);
                let end = path.end();
                let stalk = self.add_stalk(path.points, 0.0, None);
                self.add_item(end, direction, 1.0, Some(stalk), length);
            } else {
                self.add_item(origin, direction, 1.0, None, 0.0);
            }
        }
    }

    /// Builds a raceme: individually stalked items along one main stem.
    fn raceme(&mut self, origin: Vec3, out: Vec3, facing: Vec3, count: u32) {
        let axis = self.axis(origin, out, facing, self.options.cluster.axis_length);
        let axis_stalk = self.add_stalk(axis.points.clone(), 0.0, None);
        let phase = TAU * self.rng.next_f32();
        for k in 0..count {
            let f = (k as f32 + 0.5) / count as f32;
            let (point, tangent) = axis.at(f);
            let shrink = 1.0 - self.options.cluster.taper * f;
            let azimuth = phase + k as f32 * GOLDEN_ANGLE;
            self.add_stalked_item(point, tangent, azimuth, shrink, axis_stalk, f * axis.length);
        }
    }

    /// Builds a panicle: items shared among side stems branching from a main stem.
    fn panicle(&mut self, origin: Vec3, out: Vec3, facing: Vec3, count: u32) {
        let options = self.options;
        let shape = &options.cluster;
        let axis = self.axis(origin, out, facing, shape.axis_length);
        let axis_stalk = self.add_stalk(axis.points.clone(), 0.0, None);
        let phase = TAU * self.rng.next_f32();
        let sides = shape.side_stalks.min(count);
        let spread = shape.spread.to_radians();

        for j in 0..sides {
            // Apply taper and droop to each side stem by its place on the main stem.
            let f = (j as f32 + 0.5) / sides as f32;
            let (point, tangent) = axis.at(f);
            let shrink = 1.0 - shape.taper * f;
            let (u, v) = tangent.any_orthonormal_pair();
            let direction = tilted(tangent, u, v, spread, phase + j as f32 * GOLDEN_ANGLE);
            let side_length = shape.side_length * shape.axis_length * shrink;
            let side = self.curve(
                point,
                direction,
                direction,
                1.0,
                shape.droop,
                side_length,
                STALK_SEGMENTS,
            );
            let base_distance = f * axis.length;
            let side_stalk = self.add_stalk(side.points.clone(), base_distance, Some(axis_stalk));

            // Share items evenly; give any extra items to stems nearer the cluster base.
            let here = count / sides + u32::from(j < count % sides);
            for k in 0..here {
                let g = (k as f32 + 0.5) / here as f32;
                let (point, tangent) = side.at(g);
                let azimuth = j as f32 + k as f32 * GOLDEN_ANGLE;
                let distance = base_distance + g * side.length;
                self.add_stalked_item(point, tangent, azimuth, shrink, side_stalk, distance);
            }
        }
    }

    /// Builds a catkin: items attach directly along the main stem, after a
    /// bare section whose length is the stalk length.
    fn catkin(&mut self, origin: Vec3, out: Vec3, facing: Vec3, count: u32) {
        let shape = &self.options.cluster;
        let (stalk_length, axis_length, taper) =
            (shape.stalk_length, shape.axis_length, shape.taper);
        let spread = shape.spread.to_radians();
        let total = stalk_length + axis_length;
        let axis = self.axis(origin, out, facing, total);
        let axis_stalk = self.add_stalk(axis.points.clone(), 0.0, None);
        let phase = TAU * self.rng.next_f32();
        for k in 0..count {
            let body = (k as f32 + 0.5) / count as f32;
            let distance = stalk_length + body * axis_length;
            let f = if total > 0.0 { distance / total } else { 0.0 };
            let (point, tangent) = axis.at(f);
            let (u, v) = tangent.any_orthonormal_pair();
            let direction = tilted(tangent, u, v, spread, phase + k as f32 * GOLDEN_ANGLE);
            let shrink = 1.0 - taper * body;
            self.add_item(point, direction, shrink, Some(axis_stalk), distance);
        }
    }

    /// Builds the main stem, curving away from the host toward `facing` and
    /// bending down by the configured droop.
    fn axis(&self, origin: Vec3, out: Vec3, facing: Vec3, length: f32) -> Path {
        let droop = self.options.cluster.droop;
        self.curve(origin, out, facing, AXIS_TURN, droop, length, AXIS_SEGMENTS)
    }

    /// A [`Path::curve`] that droops toward this cluster frame's down.
    #[allow(clippy::too_many_arguments)]
    fn curve(
        &self,
        start: Vec3,
        from: Vec3,
        to: Vec3,
        turn: f32,
        droop: f32,
        length: f32,
        segments: usize,
    ) -> Path {
        Path::curve(
            start,
            from,
            to,
            self.frame.down,
            turn,
            droop,
            length,
            segments,
        )
    }

    /// Adds an item on a straight stalk at the configured spread angle from
    /// `tangent`. `shrink` scales both stalk length and item size;
    /// `distance` is the path distance to `point`. With zero stalk length,
    /// the item attaches directly to the supporting stalk `parent`.
    fn add_stalked_item(
        &mut self,
        point: Vec3,
        tangent: Vec3,
        azimuth: f32,
        shrink: f32,
        parent: usize,
        distance: f32,
    ) {
        let (u, v) = tangent.any_orthonormal_pair();
        let spread = self.options.cluster.spread.to_radians();
        let direction = tilted(tangent, u, v, spread, azimuth);
        let length = self.options.cluster.stalk_length * shrink;
        if length > 0.0 {
            let end = point + direction * length;
            let stalk = self.add_stalk(vec![point, end], distance, Some(parent));
            self.add_item(end, direction, shrink, Some(stalk), distance + length);
        } else {
            self.add_item(point, direction, shrink, Some(parent), distance);
        }
    }

    /// Records a stalk with the configured radius and returns its index.
    fn add_stalk(&mut self, points: Vec<Vec3>, base_distance: f32, parent: Option<usize>) -> usize {
        self.stalks.push(ClusterStalk {
            points,
            radius: self.options.cluster.stalk_radius,
            base_distance,
            parent,
        });
        self.stalks.len() - 1
    }

    /// Records an item pointing along unit vector `direction`, with random
    /// size variation, the `shrink` factor and a random turn about that
    /// direction.
    fn add_item(
        &mut self,
        origin: Vec3,
        direction: Vec3,
        shrink: f32,
        stalk: Option<usize>,
        distance: f32,
    ) {
        let variance = self.options.size_variance;
        let size = self.options.size * (1.0 + self.rng.range(-variance, variance)) * shrink;
        let orientation = Quat::from_rotation_arc(Vec3::Y, direction)
            * Quat::from_rotation_y(TAU * self.rng.next_f32());
        self.items.push(ClusterItem {
            origin,
            orientation,
            size,
            stalk,
            distance,
            variation: ItemVariation::NONE,
        });
    }

    /// Chooses the item count after random variation and rounding, with a
    /// minimum of one. Single-item clusters always have exactly one.
    fn item_count(&mut self) -> u32 {
        let cluster = &self.options.cluster;
        if cluster.kind == ClusterKind::Single {
            return 1;
        }
        let variance = cluster.items_variance;
        let varied = cluster.items as f32 * (1.0 + self.rng.range(-variance, variance));
        (varied.round() as u32).max(1)
    }
}

/// Returns a unit direction tilted `angle` radians from `axis`. `azimuth`
/// chooses the direction of tilt in the plane of `u` and `v`; these and
/// `axis` must be mutually perpendicular unit vectors.
fn tilted(axis: Vec3, u: Vec3, v: Vec3, angle: f32, azimuth: f32) -> Vec3 {
    let side = u * azimuth.cos() + v * azimuth.sin();
    axis * angle.cos() + side * angle.sin()
}

/// Rotates unit vector `from` toward unit vector `to`: `t = 0` keeps
/// `from`, and `t = 1` reaches `to`.
fn turn_toward(from: Vec3, to: Vec3, t: f32) -> Vec3 {
    Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), t) * from
}

/// Adds the distances between successive points. Zero for fewer than two points.
pub(crate) fn polyline_length(points: &[Vec3]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// A stalk path made of equal-length straight sections, each with a unit direction.
struct Path {
    points: Vec<Vec3>,
    headings: Vec<Vec3>,
    length: f32,
}

impl Path {
    /// Approximates a curved stalk with `segments` straight sections
    /// totalling `length`, beginning at `start`. Unit directions `from` and
    /// `to` are the first and target headings, and `down` the unit
    /// direction droop bends toward. `turn` is the fraction of the path
    /// over which the heading reaches the target; `droop` turns that target
    /// toward `down` progressively. Headings are taken at section midpoints.
    #[allow(clippy::too_many_arguments)]
    fn curve(
        start: Vec3,
        from: Vec3,
        to: Vec3,
        down: Vec3,
        turn: f32,
        droop: f32,
        length: f32,
        segments: usize,
    ) -> Self {
        let step = length / segments as f32;
        let mut points = Vec::with_capacity(segments + 1);
        let mut headings = Vec::with_capacity(segments);
        let mut point = start;
        points.push(point);
        for k in 0..segments {
            // Use the wanted direction at the midpoint as this section's heading.
            let f = (k as f32 + 0.5) / segments as f32;
            let target = turn_toward(to, down, droop * f);
            let heading = turn_toward(from, target, (f / turn).min(1.0));
            point += heading * step;
            points.push(point);
            headings.push(heading);
        }
        Self {
            points,
            headings,
            length,
        }
    }

    /// The point at the tip of the path.
    fn end(&self) -> Vec3 {
        *self.points.last().expect("a path has at least one point")
    }

    /// A position and unit heading along the path. `f = 0` is the base and
    /// `f = 1` the tip; values outside that range are clamped.
    fn at(&self, f: f32) -> (Vec3, Vec3) {
        let segments = self.headings.len();
        let scaled = f.clamp(0.0, 1.0) * segments as f32;
        let i = (scaled.floor() as usize).min(segments - 1);
        let point = self.points[i].lerp(self.points[i + 1], scaled - i as f32);
        (point, self.headings[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_turns_from_start_heading_to_end_heading() {
        let path = Path::curve(
            Vec3::ZERO,
            Vec3::X,
            Vec3::NEG_Y,
            Vec3::NEG_Y,
            1.0,
            0.0,
            4.0,
            8,
        );
        assert_eq!(path.points.len(), 9);
        assert!(path.headings[0].dot(Vec3::X) > 0.9);
        assert!(path.headings[7].dot(Vec3::NEG_Y) > 0.9);

        // The straight sections together must have the requested path length.
        let measured: f32 = path.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((measured - 4.0).abs() < 1e-4, "{measured}");
    }

    #[test]
    fn droop_bends_the_end_toward_down() {
        let curve = |down, droop| Path::curve(Vec3::ZERO, Vec3::X, Vec3::X, down, 1.0, droop, 1.0, 4);
        assert!(curve(Vec3::NEG_Y, 0.0).end().y.abs() < 1e-6);
        assert!(curve(Vec3::NEG_Y, 1.0).end().y < -0.2);
        assert!(curve(Vec3::Z, 1.0).end().z > 0.2);
    }

    #[test]
    fn path_at_interpolates_points() {
        let path = Path::curve(Vec3::ZERO, Vec3::Y, Vec3::Y, Vec3::NEG_Y, 1.0, 0.0, 2.0, 4);
        let (point, heading) = path.at(0.5);
        assert!(point.abs_diff_eq(Vec3::new(0.0, 1.0, 0.0), 1e-6));
        assert!(heading.abs_diff_eq(Vec3::Y, 1e-6));
        assert!(path.at(1.0).0.abs_diff_eq(path.end(), 1e-6));
    }

    #[test]
    fn tilted_keeps_unit_length_and_angle() {
        let (u, v) = Vec3::Y.any_orthonormal_pair();
        let d = tilted(Vec3::Y, u, v, 0.5, 1.3);
        assert!((d.length() - 1.0).abs() < 1e-6);
        assert!((d.dot(Vec3::Y) - 0.5_f32.cos()).abs() < 1e-6);
    }
}
