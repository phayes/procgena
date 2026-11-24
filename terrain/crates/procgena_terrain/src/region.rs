//! Regions: the area one terrain generator owns, and the borders it shares.

use std::fmt;

use glam::DVec2;

use crate::hash::mix64;

pub use procgena_core::RegionId;

/// Identifies a border edge. Both regions on an edge compute the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(pub u64);

/// Identifies a corner: a point where an edge ends. Every region touching
/// the point computes the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CornerId(pub u64);

/// The area a region covers, in world x and z (metres).
#[derive(Debug, Clone, PartialEq)]
pub enum Boundary {
    /// The whole plane.
    Unbounded,
    /// A simple polygon (no holes, not self-crossing), as its corner points
    /// in order. The first point is not repeated at the end.
    Polygon(Vec<DVec2>),
}

impl Boundary {
    /// Returns true if `p` is inside. Points exactly on the border may count
    /// as inside or outside.
    pub fn contains(&self, p: DVec2) -> bool {
        match self {
            Self::Unbounded => true,
            Self::Polygon(points) => {
                // Even-odd ray crossing along +x.
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let (a, b) = (points[i], points[j]);
                    if (a.y > p.y) != (b.y > p.y) {
                        let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                        if p.x < x {
                            inside = !inside;
                        }
                    }
                    j = i;
                }
                inside
            }
        }
    }

    /// Distance from `p` to the nearest side of the polygon, in metres. 0
    /// for an unbounded region.
    pub fn distance_to_border(&self, p: DVec2) -> f64 {
        match self {
            Self::Unbounded => 0.0,
            Self::Polygon(points) => {
                let mut closed = points.clone();
                closed.push(points[0]);
                segments(&closed)
                    .iter()
                    .map(|s| s.project(p).distance)
                    .fold(f64::INFINITY, f64::min)
            }
        }
    }
}

/// One run of a region's border shared with a single neighbour (or with
/// nothing, at the edge of the world).
///
/// The polyline is stored in a canonical direction, so both regions on the
/// edge hold identical points and ids.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionEdge {
    /// The edge's id, shared by both regions on it.
    pub id: EdgeId,
    /// The edge's points, in metres, in canonical order.
    pub polyline: Vec<DVec2>,
    /// The region on the other side, or `None` where nothing borders it yet.
    pub neighbor: Option<RegionId>,
    /// The corner at the first point of `polyline`.
    pub start: CornerId,
    /// The corner at the last point of `polyline`.
    pub end: CornerId,
}

/// A point where border edges end, with every region that touches it.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionCorner {
    /// The corner's id, shared by every region touching it.
    pub id: CornerId,
    /// The corner's position, in metres.
    pub position: DVec2,
    /// Every region touching the corner, sorted, without repeats.
    pub incident: Vec<RegionId>,
}

/// The area one terrain generator owns, with its borders and corners.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// The region's id.
    pub id: RegionId,
    /// The area the region covers.
    pub boundary: Boundary,
    /// The region's border, split into runs that each face one neighbour.
    pub edges: Vec<RegionEdge>,
    /// Every corner at the end of one of `edges`.
    pub corners: Vec<RegionCorner>,
}

/// A border edge or corner that could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RegionError {
    /// An edge needs at least two distinct points.
    TooFewPoints,
    /// An edge's points must be finite.
    NotFinite,
    /// An edge must not start and end at the same point.
    ClosedEdge,
    /// A region can't border itself.
    SelfNeighbor(RegionId),
}

impl fmt::Display for RegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewPoints => write!(f, "a border edge needs at least two distinct points"),
            Self::NotFinite => write!(f, "border edge points must be finite"),
            Self::ClosedEdge => write!(f, "a border edge must not start and end at the same point"),
            Self::SelfNeighbor(id) => write!(f, "region {} can't border itself", id.0),
        }
    }
}

impl std::error::Error for RegionError {}

/// The bits of a coordinate, with −0.0 counted as +0.0.
fn coordinate_bits(v: f64) -> u64 {
    if v == 0.0 { 0 } else { v.to_bits() }
}

/// Mixes a position into a hash.
fn mix_point(state: u64, p: DVec2) -> u64 {
    mix64(mix64(state, coordinate_bits(p.x)), coordinate_bits(p.y))
}

/// Returns the id of the corner at `position`. Equal positions (−0.0 and
/// +0.0 alike) give equal ids.
pub fn corner_id(position: DVec2) -> CornerId {
    CornerId(mix_point(0xc0_4e12, position))
}

/// Orders points by x, then by z.
fn point_less(a: DVec2, b: DVec2) -> bool {
    a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).is_lt()
}

impl RegionEdge {
    /// Builds the edge between `owner` and `neighbor` along `points`, in
    /// either direction.
    ///
    /// The points are put in canonical order and the ids computed, so both
    /// regions get equal edges from the same points. Repeated consecutive
    /// points are dropped.
    pub fn new(
        owner: RegionId,
        neighbor: Option<RegionId>,
        points: impl IntoIterator<Item = DVec2>,
    ) -> Result<Self, RegionError> {
        // Clean the points.
        let mut polyline: Vec<DVec2> = Vec::new();
        for p in points {
            if !p.is_finite() {
                return Err(RegionError::NotFinite);
            }
            if polyline.last() != Some(&p) {
                polyline.push(p);
            }
        }
        if neighbor == Some(owner) {
            return Err(RegionError::SelfNeighbor(owner));
        }
        let (first, last) = match (polyline.first(), polyline.last()) {
            (Some(&first), Some(&last)) if polyline.len() >= 2 => (first, last),
            _ => return Err(RegionError::TooFewPoints),
        };
        if first == last {
            return Err(RegionError::ClosedEdge);
        }

        // Canonical direction: from the smaller end point to the larger.
        if point_less(last, first) {
            polyline.reverse();
        }

        // The id covers both regions (in sorted order) and the end points.
        let (low, high) = match neighbor {
            Some(n) => (owner.min(n).0, owner.max(n).0),
            None => (owner.0, u64::MAX),
        };
        let start = polyline[0];
        let end = polyline[polyline.len() - 1];
        let state = mix64(mix64(0xed_9e, low), high);
        let id = EdgeId(mix_point(mix_point(state, start), end));
        Ok(Self {
            id,
            polyline,
            neighbor,
            start: corner_id(start),
            end: corner_id(end),
        })
    }

    /// The edge's length along its polyline, in metres.
    pub fn length(&self) -> f64 {
        self.polyline.windows(2).map(|w| w[0].distance(w[1])).sum()
    }
}

impl RegionCorner {
    /// Builds the corner at `position`, touched by `incident` regions in any
    /// order.
    pub fn new(position: DVec2, incident: impl IntoIterator<Item = RegionId>) -> Self {
        let mut incident: Vec<RegionId> = incident.into_iter().collect();
        incident.sort_unstable();
        incident.dedup();
        Self {
            id: corner_id(position),
            position,
            incident,
        }
    }
}

/// One straight piece of an edge, ready for distance queries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Segment {
    /// Start point.
    pub start: DVec2,
    /// Unit direction, in the edge's canonical direction.
    pub direction: DVec2,
    /// Unit normal: `direction` turned a quarter turn anticlockwise.
    pub normal: DVec2,
    /// Length in metres.
    pub length: f64,
    /// Distance along the whole edge where this segment starts.
    pub offset: f64,
}

/// Where a point lies relative to a [`Segment`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Projection {
    /// Distance along the whole edge of the closest point on the segment.
    pub along: f64,
    /// Distance to the closest point on the segment.
    pub distance: f64,
    /// Offset from the closest point along the segment's normal (positive on
    /// the normal's side).
    pub across: f64,
}

impl Segment {
    /// Finds the closest point on the segment to `p`.
    pub(crate) fn project(&self, p: DVec2) -> Projection {
        let rel = p - self.start;
        let t = rel.dot(self.direction).clamp(0.0, self.length);
        let closest = self.start + self.direction * t;
        Projection {
            along: self.offset + t,
            distance: p.distance(closest),
            across: (p - closest).dot(self.normal),
        }
    }
}

/// The straight pieces of `polyline`, skipping zero-length ones.
pub(crate) fn segments(polyline: &[DVec2]) -> Vec<Segment> {
    let mut offset = 0.0;
    let mut out = Vec::with_capacity(polyline.len().saturating_sub(1));
    for w in polyline.windows(2) {
        let delta = w[1] - w[0];
        let length = delta.length();
        if length == 0.0 {
            continue;
        }
        let direction = delta / length;
        out.push(Segment {
            start: w[0],
            direction,
            normal: direction.perp(),
            length,
            offset,
        });
        offset += length;
    }
    out
}

/// The point at distance `along` on `segments` and the normal there. At a
/// joint between segments, the later segment's normal is used.
pub(crate) fn point_at(segments: &[Segment], along: f64) -> (DVec2, DVec2) {
    let segment = segments
        .iter()
        .rev()
        .find(|s| s.offset <= along)
        .unwrap_or(&segments[0]);
    let t = (along - segment.offset).clamp(0.0, segment.length);
    (segment.start + segment.direction * t, segment.normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_build_the_same_edge() {
        let points = [
            DVec2::new(5.0, 0.0),
            DVec2::new(2.0, 3.0),
            DVec2::new(-0.0, 1.0),
        ];
        let a = RegionEdge::new(RegionId(1), Some(RegionId(2)), points).unwrap();
        let b = RegionEdge::new(RegionId(2), Some(RegionId(1)), points.into_iter().rev()).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.polyline, b.polyline);
        assert_eq!(a.start, corner_id(DVec2::new(0.0, 1.0)));
        assert_eq!(a.polyline[0].x, 0.0);
    }

    #[test]
    fn bad_edges_are_rejected() {
        let p = DVec2::ONE;
        assert_eq!(
            RegionEdge::new(RegionId(1), None, [p, p]),
            Err(RegionError::TooFewPoints)
        );
        assert_eq!(
            RegionEdge::new(RegionId(1), None, [p, DVec2::ZERO, p]),
            Err(RegionError::ClosedEdge)
        );
        assert_eq!(
            RegionEdge::new(RegionId(1), Some(RegionId(1)), [p, DVec2::ZERO]),
            Err(RegionError::SelfNeighbor(RegionId(1)))
        );
    }

    #[test]
    fn polygon_contains_inside_points() {
        let square = Boundary::Polygon(vec![
            DVec2::ZERO,
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(0.0, 10.0),
        ]);
        assert!(square.contains(DVec2::splat(5.0)));
        assert!(!square.contains(DVec2::new(15.0, 5.0)));
        assert!(Boundary::Unbounded.contains(DVec2::splat(1e9)));
    }

    #[test]
    fn projection_reports_side_and_distance() {
        let s = segments(&[DVec2::ZERO, DVec2::new(10.0, 0.0)]);
        let pr = s[0].project(DVec2::new(4.0, 3.0));
        assert_eq!(pr.along, 4.0);
        assert_eq!(pr.distance, 3.0);
        assert_eq!(pr.across, 3.0);
        let (q, n) = point_at(&s, 2.5);
        assert_eq!(q, DVec2::new(2.5, 0.0));
        assert_eq!(n, DVec2::new(0.0, 1.0));
    }
}
