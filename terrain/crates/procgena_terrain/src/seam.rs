//! How two neighbouring terrains agree on their shared border.
//!
//! Before generating, each terrain makes an [`EdgeOffer`] for each border
//! edge and a [`CornerOffer`] for each corner. Offers depend only on the
//! terrain making them. [`negotiate_corner`] and [`negotiate`] then turn the
//! offers into agreements. Both are pure and give the same result whatever
//! order the offers come in, so every region computes identical agreements.
//! Each terrain then bends itself to meet the agreements near its border.

use std::fmt;

use glam::{DVec2, Vec2};

use crate::elevation::Elevation;
use crate::options::RoadGrades;
use crate::ports::{
    CORNER_CLEARANCE, FEATURE_CLEARANCE, INNER_DEPTHS, Port, PortWish, Side, SideWishes,
    agree_ports, wishes_valid,
};
use crate::region::{
    CornerId, EdgeId, RegionCorner, RegionEdge, RegionId, Segment, point_at, segments,
};

/// The spacing of an edge's profile samples, in metres (at most; edges are
/// split into equal steps).
pub const EDGE_STEP: f64 = 8.0;

/// The standard deviation of the smoothing applied along an edge before
/// mixing, in metres. Detail finer than this is left to each terrain.
const PROFILE_SMOOTHING: f64 = 48.0;

/// Height and cross-slope at one point on a border edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSample {
    /// Ground height, in metres.
    pub height: f32,
    /// Rate of height change across the edge, along the edge's canonical
    /// normal (the edge direction turned a quarter turn anticlockwise).
    pub slope: f32,
    /// Height in metres of the small irregularities added right at the
    /// border. 0 in offers; set by [`negotiate`].
    pub detail: f32,
}

/// What one terrain would like along one border edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeOffer {
    /// The edge this offer is for.
    pub edge: EdgeId,
    /// The terrain's region.
    pub region: RegionId,
    /// The terrain's own height and cross-slope at each of the edge's
    /// [`sample_count`] evenly spaced samples, in canonical order.
    pub natural: Vec<EdgeSample>,
    /// How strongly the terrain keeps its own shape. Positive.
    pub stiffness: f32,
    /// How far inside its region the terrain bends to meet the agreement, in
    /// metres. Positive.
    pub margin: f32,
    /// The height of border irregularities the terrain accepts, in metres.
    pub roughness: f32,
    /// Which side of the edge the terrain's region lies on.
    pub side: Side,
    /// The terrain's own rivers that would like to flow out through this
    /// edge.
    pub rivers: Vec<PortWish>,
    /// Where the terrain would like roads to cross this edge.
    pub roads: Vec<PortWish>,
    /// The terrain's own road grades, before any fading towards its
    /// neighbours' (see [`RoadOptions`](crate::RoadOptions)). Must be valid.
    pub road_grades: RoadGrades,
    /// The terrain's own height at [`INNER_DEPTHS`] depths inside its
    /// region (see [`inner_depth`](crate::inner_depth), out to
    /// [`FEATURE_CLEARANCE`]), straight
    /// in from each of the edge's [`sample_count`] samples. Crossings are
    /// placed only where this stays close to the border profile carried
    /// inward, and where a river's water would not sit above it.
    ///
    /// May be empty, together with `unfaded`, when neither offer on the
    /// edge has a river or road wish: no crossing is placed then.
    pub inner: Vec<[f32; INNER_DEPTHS]>,
    /// The terrain's height at each of the edge's [`sample_count`] samples
    /// with no fading towards its neighbours: the ground its roads must
    /// reach from a crossing. May be empty, as for `inner`.
    pub unfaded: Vec<f32>,
    /// How far from the edge the terrain fades towards its neighbours, in
    /// metres. Positive.
    pub fade_width: f32,
}

/// What one terrain would like at one corner.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerOffer {
    /// The corner this offer is for.
    pub corner: CornerId,
    /// The terrain's region.
    pub region: RegionId,
    /// The terrain's own height at the corner, in metres.
    pub height: f32,
    /// The terrain's own rate of height change along x and z at the corner.
    pub gradient: Vec2,
    /// How strongly the terrain keeps its own shape. Positive.
    pub stiffness: f32,
}

/// The agreed height and slope at a corner, which every edge ending there
/// meets.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerAgreement {
    /// The corner.
    pub corner: CornerId,
    /// The corner's position, in metres.
    pub position: DVec2,
    /// The agreed height, in metres.
    pub height: f32,
    /// The agreed rate of height change along x and z.
    pub gradient: Vec2,
}

/// The agreed height profile along a border edge, which both neighbours
/// meet exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeAgreement {
    /// The edge.
    pub edge: EdgeId,
    /// Agreed height and cross-slope at each of the edge's [`sample_count`]
    /// evenly spaced samples, in canonical order.
    pub profile: Vec<EdgeSample>,
    /// Agreed river and road crossings, in order along the edge.
    pub ports: Vec<Port>,
}

/// Offers that can't be negotiated together.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SeamError {
    /// An offer or corner agreement is for a different edge or corner.
    Mismatch,
    /// The edge doesn't have a neighbour, or the offers aren't from the two
    /// regions on it.
    WrongRegions,
    /// An offer doesn't have [`sample_count`] samples for the edge.
    SampleCount {
        /// The number of samples the edge needs.
        expected: usize,
        /// The number of samples the offer has.
        found: usize,
    },
    /// No offers, or offers from regions that don't touch the corner.
    BadCornerOffers,
    /// A stiffness, margin or roughness isn't positive and finite.
    BadWeight,
    /// A river or road wish is off the edge, not finite, or too wide.
    BadWish,
    /// Both offers claim the same side of the edge.
    SameSide,
}

impl fmt::Display for SeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch => write!(
                f,
                "an offer or corner agreement is for a different edge or corner"
            ),
            Self::WrongRegions => write!(f, "the offers aren't from the two regions on the edge"),
            Self::SampleCount { expected, found } => {
                write!(
                    f,
                    "an offer has {found} samples, but the edge needs {expected}"
                )
            }
            Self::BadCornerOffers => write!(
                f,
                "the corner offers don't match the regions touching the corner"
            ),
            Self::BadWeight => write!(
                f,
                "stiffness and margin must be positive, and roughness zero or more"
            ),
            Self::BadWish => write!(
                f,
                "a river or road wish is off the edge, not finite, or too wide"
            ),
            Self::SameSide => write!(f, "both offers claim the same side of the edge"),
        }
    }
}

impl std::error::Error for SeamError {}

/// The number of profile samples for an edge `length` metres long: evenly
/// spaced, at most [`EDGE_STEP`] apart, including both ends.
pub fn sample_count(length: f64) -> usize {
    (length / EDGE_STEP).ceil().max(1.0) as usize + 1
}

/// Agrees on a corner's height and gradient from the offers of every region
/// touching it.
///
/// The result is a stiffness-weighted mean. It is the same, bit for bit,
/// whatever order the offers come in.
pub fn negotiate_corner(
    corner: &RegionCorner,
    offers: &[CornerOffer],
) -> Result<CornerAgreement, SeamError> {
    // Check and sort, so the sums below always run in the same order.
    let mut sorted: Vec<&CornerOffer> = offers.iter().collect();
    sorted.sort_by_key(|o| o.region);
    let regions: Vec<RegionId> = sorted.iter().map(|o| o.region).collect();
    if sorted.is_empty() || regions != corner.incident {
        return Err(SeamError::BadCornerOffers);
    }
    if sorted.iter().any(|o| o.corner != corner.id) {
        return Err(SeamError::Mismatch);
    }
    if sorted
        .iter()
        .any(|o| !(o.stiffness > 0.0 && o.stiffness.is_finite()))
    {
        return Err(SeamError::BadWeight);
    }

    // Weighted mean.
    let total: f32 = sorted.iter().map(|o| o.stiffness).sum();
    let height = sorted.iter().map(|o| o.height * o.stiffness).sum::<f32>() / total;
    let gradient = sorted
        .iter()
        .map(|o| o.gradient * o.stiffness)
        .sum::<Vec2>()
        / total;
    Ok(CornerAgreement {
        corner: corner.id,
        position: corner.position,
        height,
        gradient,
    })
}

/// `6t⁵ − 15t⁴ + 10t³` for `t` clamped to 0..1: rises from 0 to 1 with zero
/// first and second derivatives at both ends.
pub(crate) fn smootherstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// The slope of [`smootherstep`] at `t`: zero outside 0 to 1.
pub(crate) fn smootherstep_slope(t: f64) -> f64 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    30.0 * t * t * (1.0 - t) * (1.0 - t)
}

/// Agrees on the height profile along a shared edge, from the two
/// neighbours' offers and the agreements at the edge's two corners.
///
/// The result is the same, bit for bit, if `a` and `b` are swapped. Along
/// the edge the profile is the stiffness-weighted mean of both terrains'
/// own heights and cross-slopes, smoothed along the edge. Near each end it
/// bends to meet that corner's agreed height and gradient. The border
/// irregularity height is the smaller of the two roughness values, falling
/// to 0 at the corners.
///
/// `elevation` is the world elevation both terrains stand on (see
/// [`TerrainGenerator::with_elevation`](crate::TerrainGenerator::with_elevation));
/// the smoothing and mixing act on heights above it, so the agreed profile
/// is E plus the agreed relief exactly. Offers, agreements and crossings
/// all hold heights above sea level.
pub fn negotiate(
    edge: &RegionEdge,
    a: &EdgeOffer,
    b: &EdgeOffer,
    start: &CornerAgreement,
    end: &CornerAgreement,
    elevation: &Elevation,
) -> Result<EdgeAgreement, SeamError> {
    // Check the inputs. The edge id covers both regions, so rebuilding it
    // from the offering regions confirms they are the two on this edge.
    if a.edge != edge.id
        || b.edge != edge.id
        || start.corner != edge.start
        || end.corner != edge.end
    {
        return Err(SeamError::Mismatch);
    }
    let (a, b) = if a.region <= b.region { (a, b) } else { (b, a) };
    let rebuilt = RegionEdge::new(a.region, Some(b.region), edge.polyline.iter().copied());
    if edge.neighbor.is_none() || rebuilt.map(|e| e.id) != Ok(edge.id) {
        return Err(SeamError::WrongRegions);
    }
    let length = edge.length();
    let count = sample_count(length);
    // The crossing samples may be left out when neither side wishes for a
    // crossing.
    let wished = [a, b]
        .iter()
        .any(|o| !o.rivers.is_empty() || !o.roads.is_empty());
    for offer in [a, b] {
        let crossing = [offer.inner.len(), offer.unfaded.len()];
        let given = wished || crossing != [0, 0];
        let lengths =
            std::iter::once(offer.natural.len()).chain(crossing.into_iter().filter(|_| given));
        for found in lengths {
            if found != count {
                return Err(SeamError::SampleCount {
                    expected: count,
                    found,
                });
            }
        }
        let weights_ok = offer.stiffness > 0.0
            && offer.stiffness.is_finite()
            && offer.margin > 0.0
            && offer.margin.is_finite()
            && offer.roughness >= 0.0
            && offer.roughness.is_finite()
            && offer.road_grades.is_valid()
            && offer.fade_width > 0.0
            && offer.fade_width.is_finite();
        if !weights_ok {
            return Err(SeamError::BadWeight);
        }
        if !wishes_valid(&offer.rivers, length) || !wishes_valid(&offer.roads, length) {
            return Err(SeamError::BadWish);
        }
    }
    if a.side == b.side {
        return Err(SeamError::SameSide);
    }

    // Shared settings.
    let segments = segments(&edge.polyline);
    let step = length / (count - 1) as f64;
    let total = a.stiffness + b.stiffness;
    let amplitude = a.roughness.min(b.roughness);
    let taper = f64::from(a.margin.max(b.margin)).min(length / 2.0);

    // Heights above E, smoothed. E itself is added back at the end.
    let e = elevation_samples(edge, count, elevation);
    let above = |o: &EdgeOffer| smooth(&relative_to(&o.natural, e.as_deref(), -1.0), step);
    let (above_a, above_b) = (above(a), above(b));
    // Each corner's agreement above E: its height and gradient there.
    let above_corner = |corner: &CornerAgreement| {
        if e.is_none() {
            return (corner.height, corner.gradient);
        }
        (
            corner.height - elevation.height(corner.position),
            corner.gradient - elevation.gradient(corner.position).as_vec2(),
        )
    };
    let corners = [(start, above_corner(start)), (end, above_corner(end))];

    // Mix and pin each sample.
    let above_profile = (0..count)
        .map(|i| {
            let along = i as f64 * step;
            let (sa, sb) = (above_a[i], above_b[i]);
            let mut height = (sa.height * a.stiffness + sb.height * b.stiffness) / total;
            let mut slope = (sa.slope * a.stiffness + sb.slope * b.stiffness) / total;
            let mut detail = amplitude;

            // Bend towards each corner's plane near that end.
            let (point, normal) = point_at(&segments, along);
            for ((corner, (corner_height, gradient)), distance) in
                corners.iter().zip([along, length - along])
            {
                let keep = smootherstep(distance / taper) as f32;
                let plane = corner_height + gradient.dot((point - corner.position).as_vec2());
                let plane_slope = gradient.dot(normal.as_vec2());
                height = plane + (height - plane) * keep;
                slope = plane_slope + (slope - plane_slope) * keep;
                detail *= keep;
            }
            EdgeSample {
                height,
                slope,
                detail,
            }
        })
        .collect::<Vec<_>>();
    let profile = relative_to(&above_profile, e.as_deref(), 1.0);
    let natural_a = relative_to(&above_a, e.as_deref(), 1.0);
    let natural_b = relative_to(&above_b, e.as_deref(), 1.0);

    // Crossings.
    fn side<'a>(o: &'a EdgeOffer, natural: &'a [EdgeSample]) -> SideWishes<'a> {
        SideWishes {
            region: o.region,
            side: o.side,
            rivers: &o.rivers,
            roads: &o.roads,
            natural,
            road_grades: o.road_grades,
            margin: o.margin,
            inner: &o.inner,
            unfaded: &o.unfaded,
            fade_width: o.fade_width,
        }
    }
    // Crossings stay clear of the edge's ends, where other edges meet, and
    // their pieces stay clear of the edge's other segments, which their
    // carving could otherwise reach near a bend.
    let usable = |along: f64| {
        if along < CORNER_CLEARANCE || along > length - CORNER_CLEARANCE {
            return false;
        }
        let (q, normal) = point_at(&segments, along);
        let ends = [
            q + normal * FEATURE_CLEARANCE,
            q - normal * FEATURE_CLEARANCE,
        ];
        segments
            .iter()
            .filter(|s| along < s.offset || along > s.offset + s.length)
            .all(|s| {
                ends.iter()
                    .all(|&p| s.project(p).distance >= FEATURE_CLEARANCE)
            })
    };
    let ports = agree_ports(
        &side(a, &natural_a),
        &side(b, &natural_b),
        &profile,
        step,
        &usable,
    );
    Ok(EdgeAgreement {
        edge: edge.id,
        profile,
        ports,
    })
}

/// The world elevation E's height and cross-slope at each of `count`
/// evenly spaced samples along `edge`, or `None` on flat land, where E is
/// 0. Negotiation smooths and mixes heights above E, so E passes through
/// agreements exactly.
pub(crate) fn elevation_samples(
    edge: &RegionEdge,
    count: usize,
    elevation: &Elevation,
) -> Option<Vec<EdgeSample>> {
    if elevation.is_flat() {
        return None;
    }
    let segments = segments(&edge.polyline);
    let step = edge.length() / (count - 1) as f64;
    Some(
        (0..count)
            .map(|i| {
                let (point, normal) = point_at(&segments, i as f64 * step);
                EdgeSample {
                    height: elevation.height(point),
                    slope: elevation.gradient(point).dot(normal) as f32,
                    detail: 0.0,
                }
            })
            .collect(),
    )
}

/// `samples` with E's samples `e` taken away (`sign` −1) or added back
/// (`sign` 1), or `samples` as they are on flat land.
pub(crate) fn relative_to(
    samples: &[EdgeSample],
    e: Option<&[EdgeSample]>,
    sign: f32,
) -> Vec<EdgeSample> {
    let Some(e) = e else {
        return samples.to_vec();
    };
    samples
        .iter()
        .zip(e)
        .map(|(s, e)| EdgeSample {
            height: s.height + sign * e.height,
            slope: s.slope + sign * e.slope,
            detail: s.detail,
        })
        .collect()
}

/// Smooths edge samples along the edge with a Gaussian of
/// [`PROFILE_SMOOTHING`], for samples `step` metres apart. The ends are
/// extended by repeating the end samples.
pub(crate) fn smooth(samples: &[EdgeSample], step: f64) -> Vec<EdgeSample> {
    let sigma = PROFILE_SMOOTHING / step;
    let radius = (3.0 * sigma).ceil() as isize;
    let kernel: Vec<f32> = (-radius..=radius)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp() as f32)
        .collect();
    let total: f32 = kernel.iter().sum();
    let last = samples.len() as isize - 1;
    (0..samples.len() as isize)
        .map(|i| {
            let mut height = 0.0;
            let mut slope = 0.0;
            for (k, w) in (-radius..=radius).zip(&kernel) {
                let s = samples[(i + k).clamp(0, last) as usize];
                height += s.height * w;
                slope += s.slope * w;
            }
            EdgeSample {
                height: height / total,
                slope: slope / total,
                detail: samples[i as usize].detail,
            }
        })
        .collect()
}

/// An agreed profile, ready to sample anywhere along its edge.
#[derive(Debug, Clone)]
pub(crate) struct Profile {
    segments: Vec<Segment>,
    samples: Vec<EdgeSample>,
    step: f64,
}

impl Profile {
    /// Prepares `agreement` for sampling along `edge`. The sample count must
    /// already match the edge.
    pub(crate) fn new(edge: &RegionEdge, agreement: &EdgeAgreement) -> Self {
        let segments = segments(&edge.polyline);
        let step = edge.length() / (agreement.profile.len() - 1) as f64;
        Self {
            segments,
            samples: agreement.profile.clone(),
            step,
        }
    }

    /// The edge's straight pieces.
    pub(crate) fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The agreed height and cross-slope at distance `along` the edge, by
    /// Catmull-Rom interpolation (smooth, and exact at the samples).
    pub(crate) fn sample(&self, along: f64) -> EdgeSample {
        let last = self.samples.len() - 1;
        let t = (along / self.step).clamp(0.0, last as f64);
        let i = (t.floor() as usize).min(last.saturating_sub(1));
        let f = (t - i as f64) as f32;
        let at = |k: isize| self.samples[k.clamp(0, last as isize) as usize];
        let i = i as isize;
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let spline = |a: f32, b: f32, c: f32, d: f32| {
            let f2 = f * f;
            let f3 = f2 * f;
            0.5 * (2.0 * b
                + (c - a) * f
                + (2.0 * a - 5.0 * b + 4.0 * c - d) * f2
                + (3.0 * b - a - 3.0 * c + d) * f3)
        };
        EdgeSample {
            height: spline(p0.height, p1.height, p2.height, p3.height),
            slope: spline(p0.slope, p1.slope, p2.slope, p3.slope),
            detail: spline(p0.detail, p1.detail, p2.detail, p3.detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge() -> RegionEdge {
        // Long enough that smoothing near the ends doesn't reach the middle.
        let points = [
            DVec2::ZERO,
            DVec2::new(300.0, 400.0),
            DVec2::new(1000.0, 400.0),
        ];
        RegionEdge::new(RegionId(4), Some(RegionId(9)), points).unwrap()
    }

    fn offer(edge: &RegionEdge, region: u64, base: f32, stiffness: f32) -> EdgeOffer {
        let n = sample_count(edge.length());
        EdgeOffer {
            edge: edge.id,
            region: RegionId(region),
            natural: (0..n)
                .map(|i| EdgeSample {
                    height: base + i as f32 * 0.3,
                    slope: 0.1 * base,
                    detail: 0.0,
                })
                .collect(),
            stiffness,
            margin: 50.0,
            roughness: 1.0,
            side: if region == 4 { Side::Left } else { Side::Right },
            rivers: Vec::new(),
            roads: Vec::new(),
            road_grades: RoadGrades {
                grade: 0.1,
                max_grade: 0.1,
            },
            inner: Vec::new(),
            unfaded: (0..n).map(|i| base + i as f32 * 0.3).collect(),
            fade_width: 50.0,
        }
        .with_fitting_inner()
    }

    impl EdgeOffer {
        /// Sets the inland heights to the offer's own profile carried
        /// inward, so crossings fit everywhere.
        fn with_fitting_inner(mut self) -> Self {
            let sign = self.side.sign();
            self.inner = self
                .natural
                .iter()
                .map(|s| {
                    std::array::from_fn(|k| {
                        let depth = crate::ports::inner_depth(k);
                        let reach = crate::ports::slope_reach(sign * depth);
                        s.height + (f64::from(s.slope) * reach) as f32
                    })
                })
                .collect();
            self
        }
    }

    fn corner(edge: &RegionEdge, at_start: bool, height: f32) -> CornerAgreement {
        let (id, position) = if at_start {
            (edge.start, edge.polyline[0])
        } else {
            (edge.end, *edge.polyline.last().unwrap())
        };
        CornerAgreement {
            corner: id,
            position,
            height,
            gradient: Vec2::new(0.2, -0.1),
        }
    }

    #[test]
    fn negotiation_is_symmetric() {
        let e = edge();
        let (a, b) = (offer(&e, 4, 10.0, 1.0), offer(&e, 9, 30.0, 3.0));
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        assert_eq!(
            negotiate(&e, &a, &b, &s, &t, &Elevation::flat()),
            negotiate(&e, &b, &a, &s, &t, &Elevation::flat())
        );
    }

    #[test]
    fn negotiation_with_crossings_is_symmetric() {
        let e = edge();
        let (mut a, mut b) = (offer(&e, 4, 20.0, 1.0), offer(&e, 9, 30.0, 3.0));
        // Region 9 is a little higher, and the ground falls gently away into
        // region 4 (on the left, so a negative slope along the normal).
        for s in &mut b.natural {
            s.slope = 0.0;
        }
        for s in &mut a.natural {
            s.slope = -0.2;
        }
        let (mut a, mut b) = (a.with_fitting_inner(), b.with_fitting_inner());
        // Margins wide enough that the ground under each crossing piece is
        // already bent close to the agreement, so crossings can fit.
        a.margin = 2000.0;
        b.margin = 2000.0;
        let wish = |along: f64, level: f32| PortWish {
            along,
            level,
            width: 6.0,
            ground: f32::INFINITY,
        };
        b.rivers = vec![wish(500.0, 80.0), wish(700.0, 80.0)];
        a.roads = vec![wish(300.0, 0.0)];
        b.roads = vec![wish(450.0, 0.0), wish(900.0, 0.0)];
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        let forward = negotiate(&e, &a, &b, &s, &t, &Elevation::flat()).unwrap();
        assert_eq!(
            Ok(forward.clone()),
            negotiate(&e, &b, &a, &s, &t, &Elevation::flat())
        );
        assert!(!forward.ports.is_empty());
    }

    #[test]
    fn bad_wishes_and_same_sides_are_rejected() {
        let e = edge();
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        let (a, mut b) = (offer(&e, 4, 10.0, 1.0), offer(&e, 9, 30.0, 3.0));
        b.roads = vec![PortWish {
            along: -1.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        assert_eq!(
            negotiate(&e, &a, &b, &s, &t, &Elevation::flat()),
            Err(SeamError::BadWish)
        );
        b.roads.clear();
        b.side = Side::Left;
        assert_eq!(
            negotiate(&e, &a, &b, &s, &t, &Elevation::flat()),
            Err(SeamError::SameSide)
        );
    }

    #[test]
    fn profile_meets_the_corners_and_favours_the_stiffer_side() {
        let e = edge();
        let (a, b) = (offer(&e, 4, 10.0, 1.0), offer(&e, 9, 30.0, 3.0));
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        let agreed = negotiate(&e, &a, &b, &s, &t, &Elevation::flat()).unwrap();
        assert_eq!(agreed.profile[0].height, 15.0);
        assert_eq!(agreed.profile.last().unwrap().height, 25.0);
        // Away from the corners: the stiffness-weighted mix of the (linear,
        // so unchanged by smoothing) offers, with the smaller roughness.
        let middle = agreed.profile.len() / 2;
        let mixed = (a.natural[middle].height + 3.0 * b.natural[middle].height) / 4.0;
        assert!((agreed.profile[middle].height - mixed).abs() < 1e-3);
        assert_eq!(agreed.profile[middle].detail, 1.0);
        assert_eq!(agreed.profile[0].detail, 0.0);
    }

    #[test]
    fn corner_negotiation_ignores_order() {
        let c = RegionCorner::new(
            DVec2::new(3.0, 4.0),
            [RegionId(2), RegionId(7), RegionId(5)],
        );
        let make = |r: u64, h: f32, k: f32| CornerOffer {
            corner: c.id,
            region: RegionId(r),
            height: h,
            gradient: Vec2::new(h * 0.01, -0.3),
            stiffness: k,
        };
        let offers = [make(2, 10.0, 1.0), make(7, 55.5, 2.5), make(5, 31.25, 0.7)];
        let forward = negotiate_corner(&c, &offers).unwrap();
        let backward = negotiate_corner(
            &c,
            &[offers[2].clone(), offers[0].clone(), offers[1].clone()],
        )
        .unwrap();
        assert_eq!(forward, backward);
        assert!(negotiate_corner(&c, &offers[..2]).is_err());
    }

    #[test]
    fn mismatched_offers_are_rejected() {
        let e = edge();
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        let stranger = offer(&e, 5, 30.0, 1.0);
        assert_eq!(
            negotiate(
                &e,
                &offer(&e, 4, 10.0, 1.0),
                &stranger,
                &s,
                &t,
                &Elevation::flat()
            ),
            Err(SeamError::WrongRegions)
        );
        let mut short = offer(&e, 9, 30.0, 1.0);
        short.natural.pop();
        assert!(matches!(
            negotiate(
                &e,
                &offer(&e, 4, 10.0, 1.0),
                &short,
                &s,
                &t,
                &Elevation::flat()
            ),
            Err(SeamError::SampleCount { .. })
        ));
    }

    #[test]
    fn crossing_samples_are_needed_only_for_wished_crossings() {
        let e = edge();
        let (s, t) = (corner(&e, true, 15.0), corner(&e, false, 25.0));
        let bare = |region, base| EdgeOffer {
            inner: Vec::new(),
            unfaded: Vec::new(),
            ..offer(&e, region, base, 1.0)
        };
        let (a, b) = (bare(4, 10.0), bare(9, 30.0));
        let full = negotiate(
            &e,
            &offer(&e, 4, 10.0, 1.0),
            &offer(&e, 9, 30.0, 1.0),
            &s,
            &t,
            &Elevation::flat(),
        );
        let without = negotiate(&e, &a, &b, &s, &t, &Elevation::flat());
        assert_eq!(without, full, "the same agreement without the samples");

        // A wish on either side needs both sides' samples.
        let wish = PortWish {
            along: e.length() / 2.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        };
        let wishing = EdgeOffer {
            roads: vec![wish],
            ..offer(&e, 9, 30.0, 1.0)
        };
        assert!(matches!(
            negotiate(&e, &a, &wishing, &s, &t, &Elevation::flat()),
            Err(SeamError::SampleCount { .. })
        ));
    }

    #[test]
    fn profile_interpolation_hits_samples() {
        let e = edge();
        let agreement = EdgeAgreement {
            edge: e.id,
            profile: offer(&e, 4, 10.0, 1.0).natural,
            ports: Vec::new(),
        };
        let profile = Profile::new(&e, &agreement);
        for (i, s) in agreement.profile.iter().enumerate() {
            let at = profile.sample(i as f64 * profile.step);
            assert!((at.height - s.height).abs() < 1e-4);
        }
    }
}
