//! The one place feature lines are cut where they cross each other and
//! where they cross chunk edges, in whole millimetres and exact integer
//! arithmetic. Edge contracts and chunks both take their crossing vertices
//! from here, so neighbours agree on them.

use std::collections::BTreeSet;

use crate::feature::FeatureId;

use super::contract::{CHUNK_MM, ChunkEdgeKey, EdgeAxis};
use super::mesh::ConstraintKind;

/// A point across the ground, x and z in whole millimetres.
pub(crate) type Pt = (i64, i64);

/// One straight piece of a feature line, ends in whole millimetres, with
/// its smaller end first (by x, then z). Pieces sort by kind first, which
/// is also their precedence: where two pieces conflict and one must go,
/// the later one goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Piece {
    pub kind: ConstraintKind,
    pub feature: FeatureId,
    pub a: Pt,
    pub b: Pt,
    /// Two points, smaller first, on the straight line the piece lies on:
    /// its own ends, or the ends of the longer straight edge it was cut
    /// from. Where the piece crosses a chunk edge is measured on this line
    /// (see [`edge_crossing`](Self::edge_crossing)).
    pub line: (Pt, Pt),
}

impl Piece {
    /// The piece from `a` to `b`, either way round, or `None` if they are
    /// the same point.
    pub(crate) fn new(kind: ConstraintKind, feature: FeatureId, a: Pt, b: Pt) -> Option<Self> {
        Self::on_line(kind, feature, a, b, (a, b))
    }

    /// The piece from `a` to `b` (either way round) of the straight edge
    /// through the two points of `line`, or `None` if `a` and `b` are the
    /// same point. Pieces cut from one straight edge in different ways (for
    /// example where it starts depends on what water lies beside it) then
    /// cross chunk edges at the same points.
    pub(crate) fn on_line(
        kind: ConstraintKind,
        feature: FeatureId,
        a: Pt,
        b: Pt,
        line: (Pt, Pt),
    ) -> Option<Self> {
        (a != b).then(|| Self {
            kind,
            feature,
            a: a.min(b),
            b: a.max(b),
            line: (line.0.min(line.1), line.0.max(line.1)),
        })
    }

    /// The same kind, feature and line from `a` to `b`, or `None` if they
    /// are the same point.
    fn with_ends(self, a: Pt, b: Pt) -> Option<Self> {
        Self::on_line(self.kind, self.feature, a, b, self.line)
    }

    /// Where the piece meets chunk edge `edge` (see [`edge_crossing`]),
    /// with a crossing strictly between its ends measured on its
    /// [`line`](Self::line).
    pub(crate) fn edge_crossing(&self, edge: ChunkEdgeKey) -> Option<Pt> {
        edge_crossing(self.a, self.b, self.line, edge)
    }

    /// Whether the piece meets the box from `lo` to `hi` (edges included),
    /// judged by its own bounding box.
    pub(crate) fn near_box(&self, lo: Pt, hi: Pt) -> bool {
        let (min, max) = (
            (self.a.0.min(self.b.0), self.a.1.min(self.b.1)),
            (self.a.0.max(self.b.0), self.a.1.max(self.b.1)),
        );
        min.0 <= hi.0 && max.0 >= lo.0 && min.1 <= hi.1 && max.1 >= lo.1
    }
}

/// Twice the signed area of the triangle `a`, `b`, `c`: positive when `c`
/// is left of the line from `a` to `b`.
pub(crate) fn orient(a: Pt, b: Pt, c: Pt) -> i128 {
    let (bx, bz) = (i128::from(b.0 - a.0), i128::from(b.1 - a.1));
    let (cx, cz) = (i128::from(c.0 - a.0), i128::from(c.1 - a.1));
    bx * cz - bz * cx
}

/// `n / d` rounded to the nearest whole number, halves away from zero.
///
/// # Panics
///
/// If `d` is zero.
fn round_div(n: i128, d: i128) -> i128 {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    if n >= 0 {
        (2 * n + d) / (2 * d)
    } else {
        -((-2 * n + d) / (2 * d))
    }
}

/// The point `a + (b - a) * t_num / t_den`, rounded to whole millimetres.
pub(crate) fn point_along(a: Pt, b: Pt, t_num: i128, t_den: i128) -> Pt {
    let step = |from: i64, to: i64| {
        from + i64::try_from(round_div(i128::from(to - from) * t_num, t_den))
            .expect("a point between two millimetre points fits")
    };
    (step(a.0, b.0), step(a.1, b.1))
}

/// Where segments `p` and `q` cross, rounded to whole millimetres, if they
/// cross properly: each passes strictly from one side of the other to the
/// other side. The answer does not depend on which is given first.
pub(crate) fn crossing(p: (Pt, Pt), q: (Pt, Pt)) -> Option<Pt> {
    let norm = |(a, b): (Pt, Pt)| (a.min(b), a.max(b));
    let (p, q) = (norm(p), norm(q));
    let (p, q) = if p <= q { (p, q) } else { (q, p) };
    let (d1, d2) = (orient(q.0, q.1, p.0), orient(q.0, q.1, p.1));
    let (d3, d4) = (orient(p.0, p.1, q.0), orient(p.0, p.1, q.1));
    let strict = |x: i128, y: i128| (x > 0 && y < 0) || (x < 0 && y > 0);
    if !strict(d1, d2) || !strict(d3, d4) {
        return None;
    }
    // Along p by d1 / (d1 - d2).
    Some(point_along(p.0, p.1, d1, d1 - d2))
}

/// Whether `c` lies on segment `a`–`b`, strictly between its ends.
fn strictly_on(a: Pt, b: Pt, c: Pt) -> bool {
    orient(a, b, c) == 0 && c != a && c != b && {
        let dot = i128::from(c.0 - a.0) * i128::from(b.0 - a.0)
            + i128::from(c.1 - a.1) * i128::from(b.1 - a.1);
        let len = i128::from(b.0 - a.0).pow(2) + i128::from(b.1 - a.1).pow(2);
        dot > 0 && dot < len
    }
}

/// Where segment `a`–`b` meets chunk edge `edge`, in whole millimetres, or
/// `None` if it doesn't, or runs along the edge's line (where the edge
/// itself is the constraint).
///
/// An end of the segment on the edge is that end; otherwise the crossing is
/// rounded along the edge, halves away from zero. Crossings rounded beyond
/// the edge's corners belong to no edge. The answer depends only on the
/// segment, its carrier and the edge, so the chunks either side of the
/// edge agree.
///
/// The segment lies on the straight line through `carrier`'s two points
/// (pass `(a, b)` for the segment's own line). Whether it meets the edge,
/// and an end on the edge, are judged by its own ends; a crossing strictly
/// between them is measured on `carrier`, so segments cut from one carrier
/// in different ways cross the edge at the same point.
fn edge_crossing(a: Pt, b: Pt, carrier: (Pt, Pt), edge: ChunkEdgeKey) -> Option<Pt> {
    let (a, b) = (a.min(b), a.max(b));
    // Work with `u` across the edge's line and `v` along it: the edge is
    // `u = line`, from `v = lo` for one chunk's width.
    let (x0, z0) = (edge.start().0.0, edge.start().1.0);
    let (line, lo) = match edge.axis {
        EdgeAxis::X => (z0, x0),
        EdgeAxis::Z => (x0, z0),
    };
    let split = |p: Pt| match edge.axis {
        EdgeAxis::X => (p.1, p.0),
        EdgeAxis::Z => (p.0, p.1),
    };
    let join = |u: i64, v: i64| match edge.axis {
        EdgeAxis::X => (v, u),
        EdgeAxis::Z => (u, v),
    };
    let ((ua, va), (ub, vb)) = (split(a), split(b));
    let (sa, sb) = ((ua - line).signum(), (ub - line).signum());
    let v = if sa == 0 && sb == 0 {
        return None;
    } else if sa == 0 {
        va
    } else if sb == 0 {
        vb
    } else if sa == sb {
        return None;
    } else {
        // On the carrier (the segment itself if the carrier runs along the
        // edge), from its smaller end, whichever way it was given.
        let (c, d) = (carrier.0.min(carrier.1), carrier.0.max(carrier.1));
        let ((uc, vc), (ud, vd)) = (split(c), split(d));
        let ((u0, v0), (u1, v1)) = if uc == ud {
            ((ua, va), (ub, vb))
        } else {
            ((uc, vc), (ud, vd))
        };
        let num = i128::from(v1 - v0) * i128::from(line - u0);
        v0 + i64::try_from(round_div(num, i128::from(u1 - u0))).expect("fits")
    };
    (lo..=lo + CHUNK_MM).contains(&v).then(|| join(line, v))
}

/// A finest-level chunk's square, in whole millimetres.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Square {
    pub lo: Pt,
    pub hi: Pt,
}

impl Square {
    /// Whether `p` is inside, not on the border.
    pub(crate) fn strictly_inside(&self, p: Pt) -> bool {
        p.0 > self.lo.0 && p.0 < self.hi.0 && p.1 > self.lo.1 && p.1 < self.hi.1
    }

    /// Whether twice the midpoint of a segment, `twice`, is strictly
    /// inside.
    fn strictly_inside_twice(&self, twice: (i128, i128)) -> bool {
        let (lo, hi) = (
            (i128::from(self.lo.0) * 2, i128::from(self.lo.1) * 2),
            (i128::from(self.hi.0) * 2, i128::from(self.hi.1) * 2),
        );
        twice.0 > lo.0 && twice.0 < hi.0 && twice.1 > lo.1 && twice.1 < hi.1
    }

    /// The nearest point strictly inside to `p`.
    fn clamp_inside(&self, p: Pt) -> Pt {
        (
            p.0.clamp(self.lo.0 + 1, self.hi.0 - 1),
            p.1.clamp(self.lo.1 + 1, self.hi.1 - 1),
        )
    }

    /// Whether `p` and `q` both lie on one side of the square.
    fn same_side(&self, p: Pt, q: Pt) -> bool {
        (p.0 == q.0 && (p.0 == self.lo.0 || p.0 == self.hi.0))
            || (p.1 == q.1 && (p.1 == self.lo.1 || p.1 == self.hi.1))
    }
}

/// A chunk's feature lines, cut where they cross, ready to be added to its
/// triangulation.
#[derive(Debug, Default)]
pub(crate) struct Resolved {
    /// Pieces that cross nothing and touch the border only at vertices of
    /// its edge contracts, sorted.
    pub pieces: Vec<Piece>,
    /// Pieces left out because they crossed another piece that takes
    /// precedence and couldn't be cut, sorted.
    pub dropped: Vec<Piece>,
}

/// How many rounds of cutting [`resolve_chunk`] tries before it leaves
/// out pieces that still cross.
const MAX_ROUNDS: usize = 16;

/// Cuts `pieces` to the chunk `square` and at every crossing between them.
///
/// A piece crossing the border is cut there by [`Piece::edge_crossing`], and kept
/// only if that crossing is one of `border` (the vertices of the chunk's
/// edge contracts) or a corner. Pieces `drop` says to leave out (such as a
/// water's edge inside other water) are left out after cutting. Where two
/// pieces cross, both are cut at the crossing (rounded to a millimetre
/// strictly inside the chunk), until nothing crosses; a piece's end lying
/// on another piece cuts that piece there; overlapping pieces keep one
/// copy. A piece left out for reaching the border away from `border` is
/// listed as dropped unless `hidden_at` says the contract left that
/// crossing out on purpose. The result depends only on these inputs.
pub(crate) fn resolve_chunk(
    pieces: &[Piece],
    square: Square,
    border: &BTreeSet<Pt>,
    drop: impl Fn(&Piece) -> bool,
    hidden_at: impl Fn(&Piece, Pt) -> bool,
) -> Resolved {
    let edges = [
        ChunkEdgeKey::from_corner(square.lo, EdgeAxis::X),
        ChunkEdgeKey::from_corner((square.hi.0, square.lo.1), EdgeAxis::Z),
        ChunkEdgeKey::from_corner((square.lo.0, square.hi.1), EdgeAxis::X),
        ChunkEdgeKey::from_corner(square.lo, EdgeAxis::Z),
    ];

    // Each piece cut to the square: between consecutive points where it
    // starts, ends or crosses the border, the stretches inside.
    let mut cut: Vec<Piece> = Vec::new();
    for piece in pieces {
        let mut events: Vec<Pt> = [piece.a, piece.b]
            .into_iter()
            .filter(|&p| square.strictly_inside(p))
            .chain(edges.iter().filter_map(|&e| piece.edge_crossing(e)))
            .collect();
        sort_along(&mut events, piece.a, piece.b);
        events.dedup();
        for w in events.windows(2) {
            let twice = (
                i128::from(w[0].0) + i128::from(w[1].0),
                i128::from(w[0].1) + i128::from(w[1].1),
            );
            if square.strictly_inside_twice(twice) && !square.same_side(w[0], w[1]) {
                cut.extend(piece.with_ends(w[0], w[1]));
            }
        }
    }
    let (cut, mut dropped) = untangle(cut, square);

    // Leave out what `drop` asks, and pieces reaching the border away from
    // the contracts' vertices. Those are expected only where the contract
    // left out a crossing for being inside water (`hidden_at`); any other is
    // recorded.
    let on_border = |p: Pt| !square.strictly_inside(p);
    let mut kept = Vec::with_capacity(cut.len());
    for piece in cut {
        if drop(&piece) {
            continue;
        }
        let stray = [piece.a, piece.b]
            .into_iter()
            .find(|&p| on_border(p) && !border.contains(&p));
        match stray {
            None => kept.push(piece),
            Some(p) if hidden_at(&piece, p) => {}
            Some(_) => dropped.push(piece),
        }
    }
    dropped.sort();
    Resolved {
        pieces: kept,
        dropped,
    }
}

/// Cuts `pieces` (inside chunk `square`, ends included) at every crossing
/// between them, until nothing crosses: both pieces at a crossing (rounded
/// to a millimetre strictly inside the square), a piece at another's end
/// lying on it, and overlapping pieces into one copy. Returns the pieces,
/// sorted, and those left out because they still crossed a piece that
/// takes precedence after [`MAX_ROUNDS`] rounds.
pub(crate) fn untangle(mut cut: Vec<Piece>, square: Square) -> (Vec<Piece>, Vec<Piece>) {
    cut.sort();
    cut.dedup();
    let mut dropped = Vec::new();
    for round in 0..=MAX_ROUNDS {
        let last = round == MAX_ROUNDS;
        let mut splits: Vec<Vec<Pt>> = vec![Vec::new(); cut.len()];
        let mut gone = vec![false; cut.len()];
        for i in 0..cut.len() {
            for j in i + 1..cut.len() {
                if gone[i] || gone[j] {
                    continue;
                }
                let (p, q) = (cut[i], cut[j]);
                if (p.a, p.b) == (q.a, q.b) {
                    gone[j] = true;
                    continue;
                }
                // An end of one on the other (this also cuts overlaps).
                for (k, s, t) in [(i, p, q), (j, q, p)] {
                    for end in [t.a, t.b] {
                        if strictly_on(s.a, s.b, end) {
                            splits[k].push(end);
                        }
                    }
                }
                if let Some(x) = crossing((p.a, p.b), (q.a, q.b)) {
                    if last {
                        gone[j] = true;
                        dropped.push(q);
                        continue;
                    }
                    let x = square.clamp_inside(x);
                    for (k, s) in [(i, p), (j, q)] {
                        if x != s.a && x != s.b {
                            splits[k].push(x);
                        }
                    }
                }
            }
        }
        let changed = gone.iter().any(|&g| g) || splits.iter().any(|s| !s.is_empty());
        let mut next = Vec::with_capacity(cut.len());
        for (k, piece) in cut.iter().enumerate() {
            if gone[k] {
                continue;
            }
            let mut points = std::mem::take(&mut splits[k]);
            points.push(piece.a);
            points.push(piece.b);
            sort_along(&mut points, piece.a, piece.b);
            points.dedup();
            next.extend(
                points
                    .windows(2)
                    .filter_map(|w| piece.with_ends(w[0], w[1])),
            );
        }
        next.sort();
        next.dedup();
        cut = next;
        if !changed {
            break;
        }
    }
    (cut, dropped)
}

/// Sorts `points` by how far along the direction from `a` to `b` they
/// are, then by position.
fn sort_along(points: &mut [Pt], a: Pt, b: Pt) {
    let (dx, dz) = (i128::from(b.0 - a.0), i128::from(b.1 - a.1));
    points.sort_by_key(|&p| (i128::from(p.0 - a.0) * dx + i128::from(p.1 - a.1) * dz, p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(round_div(5, 2), 3);
        assert_eq!(round_div(-5, 2), -3);
        assert_eq!(round_div(4, 3), 1);
        assert_eq!(round_div(-4, -3), 1);
        assert_eq!(round_div(7, -2), -4);
    }

    #[test]
    fn crossings_ignore_the_order_given() {
        let p = ((0, 0), (1000, 333));
        let q = ((0, 300), (700, -10));
        let x = crossing(p, q).unwrap();
        assert_eq!(crossing(q, p), Some(x));
        assert_eq!(crossing((p.1, p.0), (q.1, q.0)), Some(x));
        // Touching at an end is not a crossing.
        assert_eq!(crossing(((0, 0), (10, 0)), ((10, 0), (10, 10))), None);
    }

    #[test]
    fn edge_crossings_use_ends_on_the_edge() {
        let edge = ChunkEdgeKey {
            x: 1,
            z: 0,
            axis: EdgeAxis::Z,
        };
        let own = |a: Pt, b: Pt| edge_crossing(a, b, (a, b), edge);
        // Across x = 64 000 mm.
        assert_eq!(
            own((63_000, 10_000), (65_000, 10_001)),
            Some((64_000, 10_001))
        );
        assert_eq!(own((64_000, 5), (65_000, 10_001)), Some((64_000, 5)));
        // Along the edge: the edge is the constraint.
        assert_eq!(own((64_000, 5), (64_000, 10)), None);
        // Beyond the corners.
        assert_eq!(own((63_000, -10), (65_000, -10)), None);
    }

    #[test]
    fn pieces_of_one_straight_edge_cross_a_chunk_edge_together() {
        // Two pieces of the edge from (0, 0) to (100 m, 0.4 m), starting at
        // different rounded points of it, cross x = 64 m where the whole
        // edge does, wherever they start.
        let edge = ChunkEdgeKey {
            x: 1,
            z: 0,
            axis: EdgeAxis::Z,
        };
        let line = ((0, 0), (100_000, 400));
        let kind = ConstraintKind::RiverEdge;
        let feature = FeatureId {
            kind: crate::carve::FeatureKind::River,
            key: 1,
        };
        let whole = Piece::new(kind, feature, line.0, line.1).unwrap();
        let at = whole.edge_crossing(edge).unwrap();
        assert_eq!(at, (64_000, 256));
        for start in [(40_001, 160), (63_999, 256), (10_000, 39)] {
            let piece = Piece::on_line(kind, feature, start, (90_000, 360), line).unwrap();
            assert_eq!(piece.edge_crossing(edge), Some(at), "from {start:?}");
        }
    }
}
