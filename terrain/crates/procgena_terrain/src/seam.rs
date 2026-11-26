//! How two neighbouring terrains agree where rivers and roads cross their
//! shared border.
//!
//! The height both terrains meet along an edge needs no negotiation: each
//! works it out from the settings of the regions around it (see
//! [`TerrainGenerator::with_edge_targets`](crate::TerrainGenerator::with_edge_targets)).
//! Crossings do: each terrain makes an [`EdgeOffer`] for each border edge,
//! which depends only on the terrain making it, and [`negotiate`] turns the
//! two offers into agreed crossings. It is pure and gives the same result
//! whatever order the offers come in, so both regions agree.

use std::fmt;

use crate::border::{BorderProfile, ProfileShape, pin_shares};
use crate::elevation::Elevation;
use crate::options::RoadGrades;
use crate::ports::{
    CORNER_CLEARANCE, FEATURE_CLEARANCE, INNER_DEPTHS, Port, PortWish, Side, SideWishes,
    agree_ports, wishes_valid,
};
use crate::region::{EdgeId, RegionEdge, RegionId, point_at, segments};
#[cfg(doc)]
use crate::terrain::TerrainGenerator;

/// The spacing of an edge's offer samples, in metres (at most; edges are
/// split into equal steps).
pub const EDGE_STEP: f64 = 8.0;

/// Height and cross-slope at one point on a border edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSample {
    /// Ground height, in metres.
    pub height: f32,
    /// Rate of height change across the edge, along the edge's canonical
    /// normal (the edge direction turned a quarter turn anticlockwise).
    pub slope: f32,
}

/// What one terrain would like along one border edge: what river and road
/// crossings need to know about its side.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeOffer {
    /// The edge this offer is for.
    pub edge: EdgeId,
    /// The terrain's region.
    pub region: RegionId,
    /// The terrain's own height and cross-slope at each of the edge's
    /// [`sample_count`] evenly spaced samples, in canonical order. The
    /// cross-slopes may be 0, together with empty `inner` and `unfaded`,
    /// when neither offer on the edge has a river or road wish.
    pub natural: Vec<EdgeSample>,
    /// How strongly the terrain keeps its own shape. Positive.
    pub stiffness: f32,
    /// How far inside its region the terrain gives way to the agreed
    /// profile, in metres. Positive.
    pub margin: f32,
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
    /// placed only where the ground there, once it gives way to the agreed
    /// profile, stays close to it, and where a river's water would not sit
    /// above it.
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

/// The samples of an [`EdgeOffer`] that only crossings need, each at the
/// edge's [`sample_count`] samples.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CrossingSamples {
    pub(crate) inner: Vec<[f32; INNER_DEPTHS]>,
    pub(crate) unfaded: Vec<f32>,
    /// The terrain's own cross-slopes, for the offer's `natural`.
    pub(crate) slopes: Vec<f32>,
}

impl EdgeOffer {
    /// This offer with `samples` filled in.
    pub(crate) fn with_crossing(mut self, samples: CrossingSamples) -> Self {
        for (sample, slope) in self.natural.iter_mut().zip(samples.slopes) {
            sample.slope = slope;
        }
        self.inner = samples.inner;
        self.unfaded = samples.unfaded;
        self
    }
}

/// What both neighbours on a border edge agree: where rivers and roads
/// cross it.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeAgreement {
    /// The edge.
    pub edge: EdgeId,
    /// The border's height and the two sides' mean cross-slope at each of
    /// the edge's [`sample_count`] evenly spaced samples, in canonical
    /// order: what crossings are placed against. The cross-slopes are 0
    /// where neither side wishes for a crossing.
    pub profile: Vec<EdgeSample>,
    /// Agreed river and road crossings, in order along the edge.
    pub ports: Vec<Port>,
}

/// Offers that can't be negotiated together.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SeamError {
    /// An offer is for a different edge.
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
    /// A stiffness, margin or fade width isn't positive and finite.
    BadWeight,
    /// A river or road wish is off the edge, not finite, or too wide.
    BadWish,
    /// Both offers claim the same side of the edge.
    SameSide,
}

impl fmt::Display for SeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch => write!(f, "an offer is for a different edge"),
            Self::WrongRegions => write!(f, "the offers aren't from the two regions on the edge"),
            Self::SampleCount { expected, found } => {
                write!(
                    f,
                    "an offer has {found} samples, but the edge needs {expected}"
                )
            }
            Self::BadWeight => write!(
                f,
                "stiffness, margin and fade width must be positive and finite"
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

/// The number of offer samples for an edge `length` metres long: evenly
/// spaced, at most [`EDGE_STEP`] apart, including both ends.
pub fn sample_count(length: f64) -> usize {
    (length / EDGE_STEP).ceil().max(1.0) as usize + 1
}

/// The standard deviation of [`smooth`], in metres.
const PROFILE_SMOOTHING: f64 = 48.0;

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
            }
        })
        .collect()
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

/// Agrees on where rivers and roads cross a shared edge, from the two
/// neighbours' offers and the edge's height profile `border`: either
/// side's [`TerrainGenerator::border`], or `None` for an edge between two
/// terrains of the same relief that needs no border, where both offers'
/// heights are the border's.
///
/// The result is the same, bit for bit, if `a` and `b` are swapped. Its
/// `profile` holds the border's height at each offer sample, with the
/// stiffness-weighted mean of the two sides' cross-slopes. `elevation` is
/// the world elevation both terrains stand on (see
/// [`TerrainGenerator::with_elevation`]).
pub fn negotiate(
    edge: &RegionEdge,
    a: &EdgeOffer,
    b: &EdgeOffer,
    border: Option<&BorderProfile>,
    elevation: &Elevation,
) -> Result<EdgeAgreement, SeamError> {
    // Check the inputs. The edge id covers both regions, so rebuilding it
    // from the offering regions confirms they are the two on this edge.
    if a.edge != edge.id || b.edge != edge.id {
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

    // The border's height at each sample, with the sides' mean
    // cross-slope.
    let segments = segments(&edge.polyline);
    let step = length / (count - 1) as f64;
    let total = a.stiffness + b.stiffness;
    let profile: Vec<EdgeSample> = (0..count)
        .map(|i| {
            let along = i as f64 * step;
            let (sa, sb) = (a.natural[i], b.natural[i]);
            let height = match border.map(|b| &b.shape) {
                None => f64::from(sa.height),
                Some(ProfileShape::Shared(knots)) => {
                    let (point, _) = point_at(&segments, along);
                    knots.at(along).0 + f64::from(elevation.height(point))
                }
                Some(ProfileShape::Relative { offsets, ramp }) => {
                    let (shares, _) = pin_shares(along, length, *ramp);
                    f64::from(sa.height) + offsets[0] * shares[0] + offsets[1] * shares[1]
                }
            };
            EdgeSample {
                height: height as f32,
                slope: (sa.slope * a.stiffness + sb.slope * b.stiffness) / total,
            }
        })
        .collect();

    // Crossings.
    fn side(o: &EdgeOffer) -> SideWishes<'_> {
        SideWishes {
            region: o.region,
            side: o.side,
            rivers: &o.rivers,
            roads: &o.roads,
            natural: &o.natural,
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
    let ports = agree_ports(&side(a), &side(b), &profile, step, &usable);
    Ok(EdgeAgreement {
        edge: edge.id,
        profile,
        ports,
    })
}

#[cfg(test)]
mod tests {
    use glam::DVec2;

    use super::*;
    use crate::border::Knots;

    fn edge() -> RegionEdge {
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
                })
                .collect(),
            stiffness,
            margin: 50.0,
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
        /// Sets the inland heights to the offer's own profile, so crossings
        /// fit everywhere.
        fn with_fitting_inner(mut self) -> Self {
            self.inner = self
                .natural
                .iter()
                .map(|s| [s.height; INNER_DEPTHS])
                .collect();
            self
        }
    }

    /// A border following `offer`'s heights, as a different relief on
    /// either side would give, pinned to `corners`.
    fn border_along(offer: &EdgeOffer, corners: [f32; 2]) -> BorderProfile {
        let length = edge().length();
        let step = length / (offer.natural.len() - 1) as f64;
        let natural = offer.natural.clone();
        BorderProfile {
            shape: ProfileShape::Shared(Knots::sample(length, corners.map(f64::from), move |s| {
                let i = ((s / step).round() as usize).min(natural.len() - 1);
                f64::from(natural[i].height)
            })),
            corners,
        }
    }

    #[test]
    fn negotiation_is_symmetric() {
        let e = edge();
        let (a, b) = (offer(&e, 4, 10.0, 1.0), offer(&e, 9, 30.0, 3.0));
        let border = border_along(&a, [15.0, 25.0]);
        assert_eq!(
            negotiate(&e, &a, &b, Some(&border), &Elevation::flat()),
            negotiate(&e, &b, &a, Some(&border), &Elevation::flat())
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
        // Strips wide enough that the ground under each crossing piece is
        // already close to the agreement, so crossings can fit.
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
        let border = border_along(&b, [15.0, 25.0]);
        let forward = negotiate(&e, &a, &b, Some(&border), &Elevation::flat()).unwrap();
        assert_eq!(
            Ok(forward.clone()),
            negotiate(&e, &b, &a, Some(&border), &Elevation::flat())
        );
        assert!(!forward.ports.is_empty());
    }

    #[test]
    fn bad_wishes_and_same_sides_are_rejected() {
        let e = edge();
        let (a, mut b) = (offer(&e, 4, 10.0, 1.0), offer(&e, 9, 30.0, 3.0));
        let border = border_along(&a, [15.0, 25.0]);
        b.roads = vec![PortWish {
            along: -1.0,
            level: 0.0,
            width: 6.0,
            ground: f32::INFINITY,
        }];
        assert_eq!(
            negotiate(&e, &a, &b, Some(&border), &Elevation::flat()),
            Err(SeamError::BadWish)
        );
        b.roads.clear();
        b.side = Side::Left;
        assert_eq!(
            negotiate(&e, &a, &b, Some(&border), &Elevation::flat()),
            Err(SeamError::SameSide)
        );
    }

    #[test]
    fn mismatched_offers_are_rejected() {
        let e = edge();
        let border = border_along(&offer(&e, 4, 10.0, 1.0), [15.0, 25.0]);
        let stranger = offer(&e, 5, 30.0, 1.0);
        assert_eq!(
            negotiate(
                &e,
                &offer(&e, 4, 10.0, 1.0),
                &stranger,
                Some(&border),
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
                Some(&border),
                &Elevation::flat()
            ),
            Err(SeamError::SampleCount { .. })
        ));
    }

    #[test]
    fn crossing_samples_are_needed_only_for_wished_crossings() {
        let e = edge();
        let border = border_along(&offer(&e, 4, 10.0, 1.0), [15.0, 25.0]);
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
            Some(&border),
            &Elevation::flat(),
        );
        let without = negotiate(&e, &a, &b, Some(&border), &Elevation::flat());
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
            negotiate(&e, &a, &wishing, Some(&border), &Elevation::flat()),
            Err(SeamError::SampleCount { .. })
        ));
    }
}
