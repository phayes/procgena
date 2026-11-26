//! A region's border field: how its ground meets the height profiles
//! agreed along its shared edges.
//!
//! Each shared edge that needs agreeing (an *active* edge) carries a
//! height target along it and the layer scales its relief fades to. A
//! point near the border mixes the nearby edge segments' targets and
//! scales by weights that fall steeply with the distance to each segment
//! and reach zero at a fixed reach, so a segment entering or leaving the
//! mix changes nothing. On an edge the target is returned exactly, so
//! both regions on the edge give the same height there, bit for bit.

use glam::DVec2;

use crate::converge::{EdgeTarget, LayerScales, LayerSpreads, level_for, scales_at};
use crate::region::{EdgeId, Segment};
use crate::seam::{smootherstep, smootherstep_slope};
use crate::terrain::ON_EDGE;

/// How much farther than the widest of the fade and the strip a segment's
/// weight reaches. At 1.25 the clip changes the soft distance by under
/// 0.1% within the strip.
const REACH_FACTOR: f64 = 1.25;

/// The base spacing of a [`BorderProfile`]'s knots, in metres (at most;
/// an edge is split into equal steps).
const PROFILE_STEP: f64 = 32.0;

/// The most a profile's straight pieces may miss the height they follow
/// at their middles, in metres, before they are halved.
const PROFILE_TOLERANCE: f64 = 0.1;

/// The shortest piece a profile is halved into, in metres.
const PROFILE_MIN_STEP: f64 = 1.0;

/// The spacing of the layer scales sampled along an edge, in metres (at
/// most). The scales change only towards corners, over hundreds of metres.
const SCALE_STEP: f64 = 16.0;

/// How far from a corner, in metres, an edge's profile bends to meet the
/// corner's agreed height (at most half the edge).
const PIN_RAMP: f64 = 250.0;

/// The height an edge asks for along it, the same for both regions on the
/// edge.
///
/// Each terrain works it out for its edges from its neighbours' settings
/// (see [`TerrainGenerator::border`](crate::TerrainGenerator::border)); a
/// [`Terrain`](crate::Terrain) meets it exactly on the edge and fades into
/// its own ground within a strip beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderProfile {
    pub(crate) shape: ProfileShape,
    /// The agreed heights at the edge's start and end corners, in metres.
    pub(crate) corners: [f32; 2],
}

/// What a [`BorderProfile`] follows between its corners.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProfileShape {
    /// A height above E along the edge, in metres, linear between knots; E
    /// at the point is added.
    Shared(Knots),
    /// The two regions' shared relief itself, read at the point, scaled by
    /// the edge's layer scales, plus E, plus an offset at each corner
    /// (metres) that fades to nothing over `ramp` metres.
    Relative { offsets: [f64; 2], ramp: f64 },
}

impl BorderProfile {
    /// The agreed heights at the edge's start and end corners, in metres.
    pub fn corner_heights(&self) -> [f32; 2] {
        self.corners
    }

    /// Whether the edge follows the two regions' shared relief (both
    /// regions have the same relief settings), rather than a profile of
    /// its own.
    pub fn is_relative(&self) -> bool {
        matches!(self.shape, ProfileShape::Relative { .. })
    }
}

/// A height along an edge: knots `step` apart, each interval halved where
/// a straight line would miss by more than [`PROFILE_TOLERANCE`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Knots {
    step: f64,
    length: f64,
    /// The first knot of each base interval, then the last knot.
    starts: Vec<u32>,
    along: Vec<f64>,
    heights: Vec<f64>,
}

impl Knots {
    /// Samples `height` along an edge `length` metres long, with its first
    /// and last knots exactly `ends`.
    pub(crate) fn sample(length: f64, ends: [f64; 2], height: impl Fn(f64) -> f64) -> Self {
        let intervals = (length / PROFILE_STEP).ceil().max(1.0) as usize;
        let step = length / intervals as f64;
        let mut knots = Self {
            step,
            length,
            starts: Vec::with_capacity(intervals + 1),
            along: Vec::new(),
            heights: Vec::new(),
        };
        let at = |i: usize| match i {
            0 => (0.0, ends[0]),
            i if i == intervals => (length, ends[1]),
            i => {
                let s = i as f64 * step;
                (s, height(s))
            }
        };

        // Each interval's start knot, then its halvings.
        let mut start = at(0);
        for i in 0..intervals {
            let end = at(i + 1);
            knots.starts.push(knots.along.len() as u32);
            knots.push(start);
            knots.halve(start, end, &height);
            start = end;
        }
        knots.starts.push(knots.along.len() as u32);
        knots.push(start);
        knots
    }

    fn push(&mut self, (s, h): (f64, f64)) {
        self.along.push(s);
        self.heights.push(h);
    }

    /// Adds the knots strictly between `a` and `b` that keep the line
    /// within the tolerance, in order.
    fn halve(&mut self, a: (f64, f64), b: (f64, f64), height: &impl Fn(f64) -> f64) {
        if b.0 - a.0 < 2.0 * PROFILE_MIN_STEP {
            return;
        }
        let s = 0.5 * (a.0 + b.0);
        let middle = (s, height(s));
        if (middle.1 - 0.5 * (a.1 + b.1)).abs() <= PROFILE_TOLERANCE {
            return;
        }
        self.halve(a, middle, height);
        self.push(middle);
        self.halve(middle, b, height);
    }

    /// The height at `along` metres from the start, and its change per
    /// metre along the edge.
    pub(crate) fn at(&self, along: f64) -> (f64, f64) {
        let s = along.clamp(0.0, self.length);
        let last = self.starts.len() - 2;
        let i = ((s / self.step) as usize).min(last);
        let (first, end) = (self.starts[i] as usize, self.starts[i + 1] as usize);
        // The knot at or before `s` in this interval, short of its end knot
        // (the next interval's first, or the last).
        let mut k = first;
        while k + 1 < end && self.along[k + 1] <= s {
            k += 1;
        }
        let (s0, s1) = (self.along[k], self.along[k + 1]);
        let (h0, h1) = (self.heights[k], self.heights[k + 1]);
        if s == s0 {
            return (h0, (h1 - h0) / (s1 - s0));
        }
        let slope = (h1 - h0) / (s1 - s0);
        (h0 + slope * (s - s0), slope)
    }

    /// How many knots there are.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.along.len()
    }
}

/// `1 − smootherstep(t)` from each end of an edge `length` metres long, at
/// `along`, over `ramp` metres, with its change along the edge: how much of
/// each corner's offset applies there.
pub(crate) fn pin_shares(along: f64, length: f64, ramp: f64) -> ([f64; 2], [f64; 2]) {
    let start = along / ramp;
    let end = (length - along) / ramp;
    (
        [1.0 - smootherstep(start), 1.0 - smootherstep(end)],
        [
            -smootherstep_slope(start) / ramp,
            smootherstep_slope(end) / ramp,
        ],
    )
}

/// How far from a corner an edge `length` metres long bends to its
/// corners' heights.
pub(crate) fn pin_ramp(length: f64) -> f64 {
    PIN_RAMP.min(0.5 * length)
}

/// A region's layer scales sampled along one edge.
#[derive(Debug, Clone)]
pub(crate) struct EdgeScales {
    step: f64,
    samples: Vec<[f64; 3]>,
}

impl EdgeScales {
    /// The scales of a terrain with layer `spreads` along an edge `length`
    /// metres long with `target`.
    pub(crate) fn new(spreads: LayerSpreads, target: &EdgeTarget, length: f64) -> Self {
        let count = (length / SCALE_STEP).ceil().max(1.0) as usize;
        let step = length / count as f64;
        let samples = (0..=count)
            .map(|i| {
                let amplitude = target.amplitude_at(i as f64 * step, length);
                scales_at(level_for(spreads, amplitude)).map(f64::from)
            })
            .collect();
        Self { step, samples }
    }

    /// The scales at `along` metres from the start, and their change per
    /// metre along the edge.
    pub(crate) fn at(&self, along: f64) -> ([f64; 3], [f64; 3]) {
        let last = self.samples.len() - 1;
        let t = (along / self.step).clamp(0.0, last as f64);
        let i = (t as usize).min(last - 1);
        let f = t - i as f64;
        let (a, b) = (self.samples[i], self.samples[i + 1]);
        (
            std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f),
            std::array::from_fn(|k| (b[k] - a[k]) / self.step),
        )
    }
}

/// One active edge: its pieces, its target and its layer scales along it.
#[derive(Debug, Clone)]
pub(crate) struct FieldEdge {
    pub(crate) id: EdgeId,
    pub(crate) segments: Vec<Segment>,
    pub(crate) length: f64,
    pub(crate) target: EdgeTarget,
    pub(crate) scales: EdgeScales,
}

/// A corner of an active edge: where it is, and which edge end it is.
#[derive(Debug, Clone, Copy)]
struct FieldCorner {
    position: DVec2,
    edge: usize,
    end: usize,
}

/// What an active edge asks for at one point: the target height, its
/// change per metre along the edge, and its change with the point at a
/// fixed place along the edge (zero unless gradients were asked for).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Target {
    pub(crate) height: f64,
    pub(crate) along_slope: f64,
    pub(crate) gradient: DVec2,
}

/// The height targets of a field's edges at a point.
pub(crate) trait Targets {
    /// What edge `edge` asks for at `along` metres along it, where its
    /// layer scales are `scales` (with their change along the edge).
    fn target(&self, edge: usize, along: f64, scales: &([f64; 3], [f64; 3])) -> Target;
}

/// No height targets: for reading the layer scales alone.
pub(crate) struct NoTargets;

impl Targets for NoTargets {
    fn target(&self, _: usize, _: f64, _: &([f64; 3], [f64; 3])) -> Target {
        Target::default()
    }
}

/// Where a point lies against a field.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Place {
    /// Beyond every edge's reach: the region's own relief, unscaled.
    Beyond,
    /// On an active edge, `along` metres from its start; `direction` is how
    /// `along` changes with the point there.
    OnEdge {
        edge: usize,
        along: f64,
        direction: DVec2,
    },
    /// On the given end (0 start, 1 end) of an active edge: a corner.
    OnCorner { edge: usize, end: usize },
    /// Near the border.
    Near(Mix),
}

/// The field's mix at a point near the border. Gradients are zero unless
/// asked for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mix {
    /// How much of each relief layer the region keeps.
    pub(crate) scales: [f64; 3],
    pub(crate) scale_gradients: [DVec2; 3],
    /// The nearby edges' targets, mixed.
    pub(crate) border: f64,
    pub(crate) border_gradient: DVec2,
    /// How much of the region's own (scaled) ground to keep over the mixed
    /// targets: 0 on the border, 1 from the strip's width out.
    pub(crate) own: f64,
    pub(crate) own_gradient: DVec2,
}

/// A region's active edges, ready to mix at any point.
#[derive(Debug, Clone)]
pub(crate) struct BorderField {
    edges: Vec<FieldEdge>,
    corners: Vec<FieldCorner>,
    /// How far the layer scales fade in, in metres.
    fade: f64,
    /// How far the targets give way to the region's own ground, in metres.
    strip: f64,
    /// How far a segment's weight reaches, in metres.
    reach: f64,
    index: SegmentIndex,
}

/// Where a point is against one segment: its distance and how that
/// changes, how far along the edge its nearest point is and how that
/// changes.
#[derive(Debug, Clone, Copy)]
struct Foot {
    distance: f64,
    distance_slope: DVec2,
    along: f64,
    along_slope: DVec2,
}

fn foot(segment: &Segment, p: DVec2) -> Foot {
    let offset = p - segment.start;
    let t = offset.dot(segment.direction);
    let end = |t: f64, q: DVec2| {
        let away = p - q;
        let distance = away.length();
        Foot {
            distance,
            distance_slope: if distance > 0.0 {
                away / distance
            } else {
                DVec2::ZERO
            },
            along: segment.offset + t,
            along_slope: DVec2::ZERO,
        }
    };
    if t <= 0.0 {
        end(0.0, segment.start)
    } else if t >= segment.length {
        end(
            segment.length,
            segment.start + segment.direction * segment.length,
        )
    } else {
        let across = offset.dot(segment.normal);
        Foot {
            distance: across.abs(),
            distance_slope: segment.normal * across.signum(),
            along: segment.offset + t,
            along_slope: segment.direction,
        }
    }
}

/// `x^-16`, for `x` above zero.
fn inverse_16(x: f64) -> f64 {
    let x2 = x * x;
    let x4 = x2 * x2;
    let x8 = x4 * x4;
    1.0 / (x8 * x8)
}

impl BorderField {
    /// The field of `edges`, with the layer scales fading in over `fade`
    /// metres and the targets giving way over `strip` metres. `None` with
    /// no edges.
    pub(crate) fn new(edges: Vec<FieldEdge>, fade: f64, strip: f64) -> Option<Self> {
        if edges.is_empty() {
            return None;
        }
        let reach = REACH_FACTOR * fade.max(strip);
        let mut corners: Vec<FieldCorner> = Vec::new();
        for (e, edge) in edges.iter().enumerate() {
            let last = edge.segments[edge.segments.len() - 1];
            let ends = [
                edge.segments[0].start,
                last.start + last.direction * last.length,
            ];
            for (end, position) in ends.into_iter().enumerate() {
                if !corners.iter().any(|c| c.position == position) {
                    corners.push(FieldCorner {
                        position,
                        edge: e,
                        end,
                    });
                }
            }
        }
        let index = SegmentIndex::new(edges.iter().map(|e| e.segments.as_slice()), reach);
        Some(Self {
            edges,
            corners,
            fade,
            strip,
            reach,
            index,
        })
    }

    /// The active edges.
    pub(crate) fn edges(&self) -> &[FieldEdge] {
        &self.edges
    }

    /// How far the layer scales fade in from the border, in metres.
    pub(crate) fn fade(&self) -> f64 {
        self.fade
    }

    /// How far the targets give way to the region's own ground, in metres.
    pub(crate) fn strip(&self) -> f64 {
        self.strip
    }

    /// The layer scales of edge `edge` at `along`, with their change along
    /// it.
    pub(crate) fn edge_scales(&self, edge: usize, along: f64) -> ([f64; 3], [f64; 3]) {
        self.edges[edge].scales.at(along)
    }

    /// Where `p` lies against the field, mixing the edges' targets from
    /// `targets` near the border, with gradients if `slope`.
    pub(crate) fn at(&self, p: DVec2, slope: bool, targets: &impl Targets) -> Place {
        let near = self.index.near(p);
        if near.is_empty() {
            return Place::Beyond;
        }
        if let Some(c) = self
            .corners
            .iter()
            .find(|c| c.position.distance(p) < ON_EDGE)
        {
            return Place::OnCorner {
                edge: c.edge,
                end: c.end,
            };
        }

        // The nearest segment, or the edge `p` is on.
        let mut nearest: Option<(f64, usize, Foot)> = None;
        for &(e, k) in near {
            let at = foot(&self.edges[e].segments[k], p);
            if at.distance < ON_EDGE {
                return Place::OnEdge {
                    edge: e,
                    along: at.along,
                    direction: at.along_slope,
                };
            }
            if at.distance < self.reach && nearest.is_none_or(|(d, _, _)| at.distance < d) {
                nearest = Some((at.distance, e, at));
            }
        }
        let Some((_, reference_edge, reference)) = nearest else {
            return Place::Beyond;
        };

        // Each segment's weight, and the targets and scales mixed by them,
        // as changes from the nearest segment's, which keeps the gradients
        // exact however much one segment outweighs the rest.
        let reference_scales = self.edges[reference_edge].scales.at(reference.along);
        let reference_target = targets.target(reference_edge, reference.along, &reference_scales);
        let (sigma_ref, t_ref) = (reference_scales.0, reference_target.height);
        let mut total = 0.0;
        let mut total_slope = DVec2::ZERO;
        let mut border = 0.0;
        let mut border_slope = DVec2::ZERO;
        let mut scales = [0.0; 3];
        let mut scale_slopes = [DVec2::ZERO; 3];
        for &(e, k) in near {
            let at = foot(&self.edges[e].segments[k], p);
            if at.distance >= self.reach {
                continue;
            }
            let whole = inverse_16(at.distance / self.reach);
            let weight = whole - 1.0;
            let edge_scales = self.edges[e].scales.at(at.along);
            let target = targets.target(e, at.along, &edge_scales);
            total += weight;
            border += weight * (target.height - t_ref);
            for i in 0..3 {
                scales[i] += weight * (edge_scales.0[i] - sigma_ref[i]);
            }
            if slope {
                let weight_slope = at.distance_slope * (-16.0 * whole / at.distance);
                total_slope += weight_slope;
                let target_slope = target.gradient + at.along_slope * target.along_slope;
                border_slope += weight_slope * (target.height - t_ref) + target_slope * weight;
                for i in 0..3 {
                    scale_slopes[i] += weight_slope * (edge_scales.0[i] - sigma_ref[i])
                        + at.along_slope * (edge_scales.1[i] * weight);
                }
            }
        }
        if total <= 0.0 {
            return Place::Beyond;
        }

        // The soft distance, and the fade and strip from it.
        let distance = self.reach * total.powf(-1.0 / 16.0);
        if distance >= self.fade && distance >= self.strip {
            return Place::Beyond;
        }
        let distance_slope = total_slope * (-distance / (16.0 * total));
        let border_offset = border / total;
        let mixed = t_ref + border_offset;
        let fade_t = distance / self.fade;
        let keep_scales = 1.0 - smootherstep(fade_t);
        let keep_slope = distance_slope * (-smootherstep_slope(fade_t) / self.fade);
        let mut out_scales = [0.0; 3];
        let mut out_scale_slopes = [DVec2::ZERO; 3];
        for i in 0..3 {
            let offset = scales[i] / total;
            let mixed_scale = sigma_ref[i] + offset;
            out_scales[i] = 1.0 + keep_scales * (mixed_scale - 1.0);
            if slope {
                let mixed_slope = (scale_slopes[i] - total_slope * offset) / total;
                out_scale_slopes[i] = keep_slope * (mixed_scale - 1.0) + mixed_slope * keep_scales;
            }
        }
        let strip_t = distance / self.strip;
        let own = smootherstep(strip_t);
        Place::Near(Mix {
            scales: out_scales,
            scale_gradients: out_scale_slopes,
            border: mixed,
            border_gradient: if slope {
                (border_slope - total_slope * border_offset) / total
            } else {
                DVec2::ZERO
            },
            own,
            own_gradient: distance_slope * (smootherstep_slope(strip_t) / self.strip),
        })
    }

    /// The values `value` gives along the nearby edges, mixed by the same
    /// weights as [`at`](Self::at) and faded in over the fade width:
    /// `own` beyond it, the edge's value on an edge.
    pub(crate) fn blend<const N: usize>(
        &self,
        p: DVec2,
        own: [f64; N],
        value: impl Fn(&FieldEdge, f64) -> [f64; N],
    ) -> [f64; N] {
        let near = self.index.near(p);
        let mut total = 0.0;
        let mut mixed = [0.0; N];
        for &(e, k) in near {
            let edge = &self.edges[e];
            let at = foot(&edge.segments[k], p);
            if at.distance < ON_EDGE {
                return value(edge, at.along);
            }
            if at.distance >= self.reach {
                continue;
            }
            let weight = inverse_16(at.distance / self.reach) - 1.0;
            total += weight;
            for (m, v) in mixed.iter_mut().zip(value(edge, at.along)) {
                *m += weight * v;
            }
        }
        if total <= 0.0 {
            return own;
        }
        let distance = self.reach * total.powf(-1.0 / 16.0);
        let keep = 1.0 - smootherstep(distance / self.fade);
        std::array::from_fn(|i| own[i] + keep * (mixed[i] / total - own[i]))
    }
}

/// The layer scales of a terrain with `spreads` at an edge's middle, where
/// the target amplitude is `amplitude`.
pub(crate) fn middle_scales(spreads: LayerSpreads, amplitude: f32) -> LayerScales {
    scales_at(level_for(spreads, f64::from(amplitude)))
}

/// Which segments of a set of edges can reach each part of the plane, so
/// a point visits only those. Segments left out of a point's list are
/// farther than the reach and would add nothing, so the result is exactly
/// what visiting every segment gives.
#[derive(Debug, Clone)]
pub(crate) struct SegmentIndex {
    origin: DVec2,
    cell: f64,
    width: usize,
    depth: usize,
    /// For each cell, the (edge, segment) pairs within reach of it, in
    /// edge and then segment order.
    cells: Vec<Vec<(usize, usize)>>,
}

impl SegmentIndex {
    /// The index of `edges` (each a list of segments) for weights out to
    /// `reach` metres.
    pub(crate) fn new<'a>(edges: impl Iterator<Item = &'a [Segment]> + Clone, reach: f64) -> Self {
        // A grid of cells half the reach wide over every segment, widened
        // by the reach.
        let (lo, hi) = edges
            .clone()
            .flatten()
            .flat_map(|s| [s.start, s.start + s.direction * s.length])
            .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), q| {
                (lo.min(q), hi.max(q))
            });
        let origin = lo - DVec2::splat(reach);
        let cell = (0.5 * reach).max(1.0);
        let size = ((hi + DVec2::splat(reach) - origin) / cell).ceil();
        let (width, depth) = (size.x.max(1.0) as usize, size.y.max(1.0) as usize);
        let mut cells = vec![Vec::new(); width * depth];

        // Each segment in every cell it comes within reach of.
        for (e, segments) in edges.enumerate() {
            for (k, s) in segments.iter().enumerate() {
                let end = s.start + s.direction * s.length;
                let (a, b) = (s.start.min(end) - reach, s.start.max(end) + reach);
                let (x0, z0) = Self::cell_of(origin, cell, a);
                let (x1, z1) = Self::cell_of(origin, cell, b);
                for z in z0..=z1.min(depth - 1) {
                    for x in x0..=x1.min(width - 1) {
                        let min = origin + DVec2::new(x as f64, z as f64) * cell;
                        if box_distance(s, min, min + DVec2::splat(cell)) < reach {
                            cells[z * width + x].push((e, k));
                        }
                    }
                }
            }
        }
        Self {
            origin,
            cell,
            width,
            depth,
            cells,
        }
    }

    /// The cell holding `p`, clamped at zero.
    fn cell_of(origin: DVec2, cell: f64, p: DVec2) -> (usize, usize) {
        let q = ((p - origin) / cell).floor().max(DVec2::ZERO);
        (q.x as usize, q.y as usize)
    }

    /// The (edge, segment) pairs that can reach `p`, in edge and then
    /// segment order.
    pub(crate) fn near(&self, p: DVec2) -> &[(usize, usize)] {
        let q = (p - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 || q.x >= self.width as f64 || q.y >= self.depth as f64 {
            return &[];
        }
        &self.cells[q.y as usize * self.width + q.x as usize]
    }
}

/// The least distance between `segment` and the box from `min` to `max`:
/// 0 where they meet. Exact, not a bound from the box's corners: a segment
/// passing between two corners is as near as it really is.
pub(crate) fn box_distance(segment: &Segment, min: DVec2, max: DVec2) -> f64 {
    let a = segment.start;
    let b = segment.start + segment.direction * segment.length;
    let inside = |q: DVec2| q.cmpge(min).all() && q.cmple(max).all();
    if inside(a) || inside(b) {
        return 0.0;
    }
    let corners = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
    // The segment against each side, and its ends against the box.
    let mut best = f64::INFINITY;
    for i in 0..4 {
        let (c, d) = (corners[i], corners[(i + 1) % 4]);
        best = best.min(crate::warp::segment_distance(a, b, c, d));
    }
    for q in [a, b] {
        best = best.min((q - q.clamp(min, max)).length());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::segments;

    #[test]
    fn knots_meet_the_tolerance_and_the_ends() {
        let height = |s: f64| 40.0 * (s / 37.0).sin() + 3.0 * (s / 3.1).sin();
        let knots = Knots::sample(1000.0, [5.0, -7.0], height);
        assert_eq!(knots.at(0.0).0, 5.0);
        assert_eq!(knots.at(1000.0).0, -7.0);
        assert!(knots.len() > 32);
        // Between the pinned ends, a dense check against the function.
        let mut worst: f64 = 0.0;
        for i in 1..9990 {
            let s = 32.0 + f64::from(i) * 0.0937;
            worst = worst.max((knots.at(s).0 - height(s)).abs());
        }
        // Each piece meets the tolerance at its middle; within a piece the
        // curve may bend a little more.
        assert!(worst < 0.5, "{worst}");
        // Knots are hit exactly.
        for (s, h) in knots.along.iter().zip(&knots.heights) {
            assert_eq!(knots.at(*s).0, *h);
        }
    }

    #[test]
    fn box_distance_sees_a_segment_between_corners() {
        // A segment crossing near the middle of the box's side, nowhere
        // near its corners.
        let s = segments(&[DVec2::new(5.0, 12.0), DVec2::new(5.0, 30.0)]);
        let (min, max) = (DVec2::ZERO, DVec2::splat(10.0));
        assert!((box_distance(&s[0], min, max) - 2.0).abs() < 1e-12);
        let through = segments(&[DVec2::new(-5.0, 5.0), DVec2::new(15.0, 5.0)]);
        assert_eq!(box_distance(&through[0], min, max), 0.0);
        let corner_dist = segments(&[DVec2::new(13.0, 14.0), DVec2::new(20.0, 30.0)]);
        assert!((box_distance(&corner_dist[0], min, max) - 5.0).abs() < 1e-12);
    }
}
