//! A region's roads as a network: nodes where roads start, meet and end,
//! the road segments between them with their alignments and levels, and
//! the river crossings each road leaves for a later pass to span.

use glam::DVec2;
use procgena_core::ContentHasher;

/// Identifies a [`RoadNode`]. The same node gets the same id on every
/// build; a node on a region border has the same id in both regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoadNodeId(pub u64);

/// Identifies a [`RoadSegment`]. The same segment gets the same id on
/// every build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoadId(pub u64);

/// What a [`RoadNode`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RoadNodeKind {
    /// One end of the short straight piece where a road crosses a region
    /// border. The piece reaches as far into the neighbour as into this
    /// region, so one of its two ends lies in the neighbour.
    Border,
    /// Where the region's roads lead: the end of the road network.
    Hub,
    /// Where one road joins another.
    Junction,
    /// Where a road stops at a river bank. Its partner on the far bank is
    /// listed in a [`RiverCrossing`]; the gap between them is left for the
    /// caller to span.
    RiverBank,
}

/// A point of the road network: a segment's end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadNode {
    /// The node's id.
    pub id: RoadNodeId,
    /// What the node is.
    pub kind: RoadNodeKind,
    /// Position, in metres (x and z).
    pub position: DVec2,
    /// Height of the road surface there, in metres.
    pub level: f32,
    /// The road's direction there, as a unit vector (x and z): for a
    /// segment's end, the way the road runs on into the node.
    pub heading: DVec2,
    /// Width of the road there, in metres.
    pub width: f32,
}

/// One piece of a road's centre line: a straight or a circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignmentPiece {
    /// A straight from `from` to `to` (x and z, metres).
    Line {
        /// Start, in metres.
        from: DVec2,
        /// End, in metres.
        to: DVec2,
    },
    /// An arc round `centre` (x and z, metres) of `radius` metres, from
    /// the angle `start` (radians, measured from +x towards +z) turning by
    /// `sweep` radians: positive turns from +x towards +z.
    Arc {
        /// Centre, in metres.
        centre: DVec2,
        /// Radius, in metres. Positive.
        radius: f64,
        /// Angle of the arc's start round the centre, in radians.
        start: f64,
        /// How far the arc turns, in radians.
        sweep: f64,
    },
}

impl AlignmentPiece {
    /// Length along the piece, in metres.
    pub fn length(&self) -> f64 {
        match *self {
            Self::Line { from, to } => from.distance(to),
            Self::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    /// The point `s` metres along the piece, `s` from 0 to
    /// [`length`](Self::length).
    pub fn point_at(&self, s: f64) -> DVec2 {
        match *self {
            Self::Line { from, to } => {
                let length = from.distance(to);
                if s >= length {
                    to
                } else if s > 0.0 {
                    from.lerp(to, s / length)
                } else {
                    from
                }
            }
            Self::Arc {
                centre,
                radius,
                start,
                sweep,
            } => {
                let angle = start + sweep.signum() * (s / radius).clamp(0.0, sweep.abs());
                centre + radius * DVec2::from_angle(angle)
            }
        }
    }

    /// The direction of travel `s` metres along the piece, as a unit
    /// vector.
    pub fn heading_at(&self, s: f64) -> DVec2 {
        match *self {
            Self::Line { from, to } => (to - from).normalize_or_zero(),
            Self::Arc {
                radius,
                start,
                sweep,
                ..
            } => {
                let angle = start + sweep.signum() * (s / radius).clamp(0.0, sweep.abs());
                DVec2::from_angle(angle).perp() * sweep.signum()
            }
        }
    }

    /// The piece cut `s` metres along it: the part before and the part
    /// after. `s` must be strictly inside the piece.
    pub(crate) fn split(&self, s: f64) -> (Self, Self) {
        match *self {
            Self::Line { from, to } => {
                let at = self.point_at(s);
                (Self::Line { from, to: at }, Self::Line { from: at, to })
            }
            Self::Arc {
                centre,
                radius,
                start,
                sweep,
            } => {
                let turned = sweep.signum() * s / radius;
                (
                    Self::Arc {
                        centre,
                        radius,
                        start,
                        sweep: turned,
                    },
                    Self::Arc {
                        centre,
                        radius,
                        start: start + turned,
                        sweep: sweep - turned,
                    },
                )
            }
        }
    }
}

/// A road's centre line: straights and arcs, end to end.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Alignment {
    /// The pieces, in order along the road. Each starts where the one
    /// before ends.
    pub pieces: Vec<AlignmentPiece>,
}

impl Alignment {
    /// Length of the whole centre line, in metres.
    pub fn length(&self) -> f64 {
        self.pieces.iter().map(AlignmentPiece::length).sum()
    }

    /// The point `s` metres along the centre line, clamped to its ends.
    /// `None` if it has no pieces.
    pub fn point_at(&self, s: f64) -> Option<DVec2> {
        self.locate(s).map(|(piece, s)| piece.point_at(s))
    }

    /// The direction of travel `s` metres along the centre line, as a unit
    /// vector, clamped to its ends. `None` if it has no pieces.
    pub fn heading_at(&self, s: f64) -> Option<DVec2> {
        self.locate(s).map(|(piece, s)| piece.heading_at(s))
    }

    /// The piece holding the point `s` metres along, and how far into it.
    fn locate(&self, s: f64) -> Option<(&AlignmentPiece, f64)> {
        let mut rest = s.max(0.0);
        let last = self.pieces.len().checked_sub(1)?;
        for (i, piece) in self.pieces.iter().enumerate() {
            let length = piece.length();
            if rest <= length || i == last {
                return Some((piece, rest.min(length)));
            }
            rest -= length;
        }
        None
    }

    /// The part of the centre line from `from` to `to` metres along it.
    pub(crate) fn between(&self, from: f64, to: f64) -> Self {
        let mut pieces = Vec::new();
        let mut at = 0.0;
        for piece in &self.pieces {
            let length = piece.length();
            let (lo, hi) = ((from - at).max(0.0), (to - at).min(length));
            if hi - lo > 1e-9 {
                let mut part = *piece;
                if hi < length - 1e-9 {
                    part = part.split(hi).0;
                }
                if lo > 1e-9 {
                    part = part.split(lo).1;
                }
                pieces.push(part);
            }
            at += length;
        }
        Self { pieces }
    }
}

/// A point along a road segment, at a fixed spacing from its start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadStation {
    /// Distance along the segment from its start, in metres.
    pub along: f64,
    /// Position on the centre line, in metres (x and z).
    pub position: DVec2,
    /// Height of the road surface, in metres.
    pub level: f32,
    /// Width of the road, in metres.
    pub width: f32,
}

/// A stretch of road between two nodes, with no node in between.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadSegment {
    /// The segment's id.
    pub id: RoadId,
    /// Which road the segment is part of, by the border node the road
    /// starts from: the segments with the same `origin` run end to end
    /// from that border crossing to the hub, or to the road it joins. A
    /// crossing piece is a road of its own, starting from its first node.
    pub origin: RoadNodeId,
    /// The node at the segment's start.
    pub start: RoadNodeId,
    /// The node at the segment's end.
    pub end: RoadNodeId,
    /// The centre line, from `start` to `end`.
    pub alignment: Alignment,
    /// The road's level and width along it, from `start` to `end`. The
    /// first and last stations are at the two nodes.
    pub stations: Vec<RoadStation>,
}

/// A place where a road meets a river: the road stops at a node on each
/// bank, and the gap between them is left for the caller to span (with a
/// bridge, a ford or anything else). The ground and the river between the
/// two nodes are left as they are. A road that runs through the pond a
/// river ends in crosses that river there too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiverCrossing {
    /// The [`RiverBank`](RoadNodeKind::RiverBank) nodes on either side,
    /// in the order the road was planned.
    pub banks: [RoadNodeId; 2],
    /// The road that crosses, as in [`RoadSegment::origin`].
    pub origin: RoadNodeId,
    /// The river crossed, as an index into the terrain's rivers.
    pub river: usize,
    /// The river's water level where the road crosses it (or the pond's,
    /// when it crosses the river's pond), in metres.
    pub water_level: f32,
}

/// A region's roads: nodes, the segments between them, and the river
/// crossings the roads leave open. Nodes, segments and crossings are each
/// in id order (crossings by their first bank's id).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadNetwork {
    /// Every node.
    pub nodes: Vec<RoadNode>,
    /// Every segment.
    pub segments: Vec<RoadSegment>,
    /// Every river crossing.
    pub crossings: Vec<RiverCrossing>,
}

impl RoadNetwork {
    /// The node `id`, if the network has it.
    pub fn node(&self, id: RoadNodeId) -> Option<&RoadNode> {
        self.nodes
            .binary_search_by_key(&id, |n| n.id)
            .ok()
            .map(|i| &self.nodes[i])
    }

    /// The segment `id`, if the network has it.
    pub fn segment(&self, id: RoadId) -> Option<&RoadSegment> {
        self.segments
            .binary_search_by_key(&id, |s| s.id)
            .ok()
            .map(|i| &self.segments[i])
    }

    /// A hash of the whole network, positions and levels to the
    /// millimetre: equal networks hash equal on every build on the same
    /// platform.
    pub fn content_hash(&self) -> u64 {
        let mut h = ContentHasher::new();
        let mm = |v: f64| (v * 1000.0).round() as i64;
        let point = |h: &mut ContentHasher, p: DVec2| {
            h.write_i64(mm(p.x));
            h.write_i64(mm(p.y));
        };
        h.write_usize(self.nodes.len());
        for node in &self.nodes {
            h.write_u64(node.id.0);
            h.write_u32(node.kind as u32);
            point(&mut h, node.position);
            h.write_i64(mm(f64::from(node.level)));
            h.write_i64(mm(f64::from(node.width)));
        }
        h.write_usize(self.segments.len());
        for segment in &self.segments {
            h.write_u64(segment.id.0);
            h.write_u64(segment.origin.0);
            h.write_u64(segment.start.0);
            h.write_u64(segment.end.0);
            h.write_usize(segment.alignment.pieces.len());
            for piece in &segment.alignment.pieces {
                match *piece {
                    AlignmentPiece::Line { from, to } => {
                        h.write_u32(0);
                        point(&mut h, from);
                        point(&mut h, to);
                    }
                    AlignmentPiece::Arc { centre, radius, .. } => {
                        h.write_u32(1);
                        point(&mut h, centre);
                        h.write_i64(mm(radius));
                        point(&mut h, piece.point_at(0.0));
                        point(&mut h, piece.point_at(piece.length()));
                    }
                }
            }
            h.write_usize(segment.stations.len());
            for station in &segment.stations {
                h.write_i64(mm(station.along));
                point(&mut h, station.position);
                h.write_i64(mm(f64::from(station.level)));
                h.write_i64(mm(f64::from(station.width)));
            }
        }
        h.write_usize(self.crossings.len());
        for crossing in &self.crossings {
            h.write_u64(crossing.banks[0].0);
            h.write_u64(crossing.banks[1].0);
            h.write_u64(crossing.origin.0);
            h.write_usize(crossing.river);
            h.write_i64(mm(f64::from(crossing.water_level)));
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arc_turns_the_way_its_sweep_says() {
        // A quarter turn from (1, 0) to (0, 1) round the origin.
        let arc = AlignmentPiece::Arc {
            centre: DVec2::ZERO,
            radius: 1.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        let end = arc.point_at(arc.length());
        assert!(end.distance(DVec2::new(0.0, 1.0)) < 1e-12);
        assert!(arc.heading_at(0.0).distance(DVec2::new(0.0, 1.0)) < 1e-12);
        let back = AlignmentPiece::Arc {
            centre: DVec2::ZERO,
            radius: 1.0,
            start: 0.0,
            sweep: -std::f64::consts::FRAC_PI_2,
        };
        assert!(back.heading_at(0.0).distance(DVec2::new(0.0, -1.0)) < 1e-12);
    }

    #[test]
    fn between_keeps_the_part_asked_for() {
        let alignment = Alignment {
            pieces: vec![
                AlignmentPiece::Line {
                    from: DVec2::ZERO,
                    to: DVec2::new(10.0, 0.0),
                },
                AlignmentPiece::Arc {
                    centre: DVec2::new(10.0, 5.0),
                    radius: 5.0,
                    start: -std::f64::consts::FRAC_PI_2,
                    sweep: std::f64::consts::PI,
                },
            ],
        };
        let total = alignment.length();
        let part = alignment.between(4.0, total - 2.0);
        assert!((part.length() - (total - 6.0)).abs() < 1e-9);
        let start = part.point_at(0.0).unwrap();
        assert!(start.distance(DVec2::new(4.0, 0.0)) < 1e-9);
        let end = part.point_at(part.length()).unwrap();
        assert!(end.distance(alignment.point_at(total - 2.0).unwrap()) < 1e-9);
    }
}
