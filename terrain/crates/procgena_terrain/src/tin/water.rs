//! Rivers and ponds as a canonical mesh sees them: the lines it must keep
//! (each river's lowest line and water's edges, each pond's shore), and
//! how far any point is from water.
//!
//! The ground cut by a river is deepest along its points and reaches its
//! water's edge half its width from the line; a pond's edge is a circle.
//! Water from several segments, rivers and ponds overlaps, and only the
//! outline of the whole is an edge of the ground. Every line here is made
//! from a whole feature, never from the part of it a chunk can see, so
//! every chunk and edge contract gets the same pieces.

use glam::DVec2;

use crate::chunk::{ChunkFeature, ChunkFeatureShape};
use crate::feature::FeatureId;
use crate::{FeatureKind, FeatureLine};

use super::mesh::ConstraintKind;
use super::numeric::{MM_PER_M, Mm};
use super::resolve::{Piece, Pt};
use super::tolerance::TinTolerance;

/// A river segment's water: every point within `half_width` of the
/// segment from `a` to `b`.
#[derive(Debug, Clone, Copy)]
struct Capsule {
    a: DVec2,
    b: DVec2,
    half_width: f64,
}

impl Capsule {
    /// How far `p` is from the water's edge: negative inside the water.
    fn gap(&self, p: DVec2) -> f64 {
        let ab = self.b - self.a;
        let t = ((p - self.a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        p.distance(self.a + ab * t) - self.half_width
    }
}

/// A pond's water: every point within `radius` of `centre`.
#[derive(Debug, Clone, Copy)]
struct Disc {
    centre: DVec2,
    radius: f64,
}

/// The rivers and ponds near one box, with the lines a mesh must keep.
#[derive(Debug, Default)]
pub(crate) struct Water {
    capsules: Vec<Capsule>,
    discs: Vec<Disc>,
    /// The lines' pieces meeting the box, sorted, each once.
    pieces: Vec<Piece>,
    /// How far inside other water a water's edge must be to be left out,
    /// in metres.
    inset: f64,
}

impl Water {
    /// The rivers and ponds among `features` that reach the box from `lo`
    /// to `hi` (metres), with the pieces of their lines that meet it, made
    /// to `tolerance`'s chord. Roads are left out.
    pub(crate) fn new(
        features: &[ChunkFeature],
        tolerance: &TinTolerance,
        lo: DVec2,
        hi: DVec2,
    ) -> Self {
        // Pieces' chord midpoints sit up to `chord` inside their own water,
        // so only edges further inside count as inside.
        let inset = 2.0 * tolerance.chord;
        let reach = tolerance.water_band + inset + 2.0;
        let mut water = Self {
            inset,
            ..Self::default()
        };
        let rivers: Vec<(FeatureId, FeatureLine)> = features
            .iter()
            .filter_map(|f| match &f.shape {
                ChunkFeatureShape::Line(line) if f.id.kind == FeatureKind::River => {
                    Some((f.id, cleaned(line)))
                }
                _ => None,
            })
            .filter(|(_, line)| line.points.len() >= 2)
            .collect();

        // Rivers: capsules near the box, and the pieces of their lines. A
        // river carries on from the first other river (by id) ending where
        // it starts, and into the first starting where it ends.
        for (i, (id, line)) in rivers.iter().enumerate() {
            let others = || {
                rivers
                    .iter()
                    .enumerate()
                    .filter(move |&(j, _)| j != i)
                    .map(|(_, (_, other))| other)
            };
            let before = others().find(|other| other.points.last() == line.points.first());
            let after = others().find(|other| other.points.first() == line.points.last());
            let river = River::new(line, before, after);
            for k in 0..river.segments() {
                let capsule = river.capsule(k);
                if capsule_near(&capsule, lo, hi, reach) {
                    water.capsules.push(capsule);
                }
                if capsule_near(&capsule, lo, hi, tolerance.chord + 0.01) {
                    river.pieces(k, *id, tolerance.chord, &mut water.pieces);
                }
            }
        }

        // Ponds: discs near the box, and their shores.
        for feature in features {
            let ChunkFeatureShape::Pond(pond) = &feature.shape else {
                continue;
            };
            let disc = Disc {
                centre: pond.centre,
                radius: pond.radius,
            };
            let near = |margin: f64| {
                let m = DVec2::splat(disc.radius + margin);
                disc.centre.cmpge(lo - m).all() && disc.centre.cmple(hi + m).all()
            };
            if near(reach) {
                water.discs.push(disc);
            }
            if near(tolerance.chord + 0.01) {
                let n = steps(std::f64::consts::TAU, disc.radius, tolerance.chord).max(8);
                let at = |j: usize| {
                    let angle = std::f64::consts::TAU * j as f64 / n as f64;
                    mm(disc.centre + DVec2::new(angle.cos(), angle.sin()) * disc.radius)
                };
                for j in 0..n {
                    water.pieces.extend(Piece::new(
                        ConstraintKind::PondShore,
                        feature.id,
                        at(j),
                        at(j + 1),
                    ));
                }
            }
        }

        let (lo_mm, hi_mm) = (mm(lo), mm(hi));
        water.pieces.retain(|p| p.near_box(lo_mm, hi_mm));
        water.pieces.sort();
        water.pieces.dedup();
        water
    }

    /// The lines' pieces meeting the box, sorted.
    pub(crate) fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// How far `p` (metres) is from the nearest water's edge: negative in
    /// the water, infinite with no water near the box.
    pub(crate) fn gap(&self, p: DVec2) -> f64 {
        let rivers = self.capsules.iter().map(|c| c.gap(p));
        let ponds = self.discs.iter().map(|d| p.distance(d.centre) - d.radius);
        rivers.chain(ponds).fold(f64::INFINITY, f64::min)
    }

    /// Whether `p` (millimetres) lies well inside water, further in than
    /// the pieces of curved edges can stray.
    pub(crate) fn inside(&self, p: Pt) -> bool {
        self.gap(DVec2::new(Mm(p.0).to_m(), Mm(p.1).to_m())) < -self.inset
    }

    /// Whether `piece` is a water's edge whose middle lies well inside
    /// water: not part of the outline of all the water together.
    pub(crate) fn hidden(&self, piece: &Piece) -> bool {
        let edge = matches!(
            piece.kind,
            ConstraintKind::RiverEdge | ConstraintKind::PondShore
        );
        let middle = DVec2::new(
            (piece.a.0 + piece.b.0) as f64,
            (piece.a.1 + piece.b.1) as f64,
        ) / (2.0 * MM_PER_M);
        edge && self.gap(middle) < -self.inset
    }

    /// Whether `piece` is a water's edge and `at` (millimetres) lies well
    /// inside water: a point where the piece is not part of the outline of
    /// all the water together, so no edge contract keeps a crossing there.
    pub(crate) fn hidden_at(&self, piece: &Piece, at: Pt) -> bool {
        matches!(
            piece.kind,
            ConstraintKind::RiverEdge | ConstraintKind::PondShore
        ) && self.inside(at)
    }
}

/// A river's line with consecutive repeated points removed.
fn cleaned(line: &FeatureLine) -> FeatureLine {
    let mut out = FeatureLine {
        points: Vec::with_capacity(line.points.len()),
        levels: Vec::with_capacity(line.points.len()),
        widths: Vec::with_capacity(line.points.len()),
        ..line.clone()
    };
    for (k, &p) in line.points.iter().enumerate() {
        if out.points.last() != Some(&p) {
            out.points.push(p);
            out.levels.push(line.levels[k]);
            out.widths.push(line.widths[k]);
        }
    }
    out
}

/// Whether `capsule`'s water, widened by `margin` metres, meets the box
/// from `lo` to `hi`.
fn capsule_near(capsule: &Capsule, lo: DVec2, hi: DVec2, margin: f64) -> bool {
    let m = DVec2::splat(capsule.half_width + margin);
    let (min, max) = (capsule.a.min(capsule.b) - m, capsule.a.max(capsule.b) + m);
    min.cmple(hi).all() && max.cmpge(lo).all()
}

/// `p` (metres) in whole millimetres.
fn mm(p: DVec2) -> Pt {
    let to = |v: f64| Mm::from_m(v).expect("feature lines lie within the world").0;
    (to(p.x), to(p.y))
}

/// How many equal steps an arc of `angle` radians and `radius` metres
/// needs so that no step strays more than `chord` metres from it.
fn steps(angle: f64, radius: f64, chord: f64) -> usize {
    let step = if chord >= radius {
        std::f64::consts::FRAC_PI_2
    } else {
        2.0 * (1.0 - chord / radius).acos()
    };
    ((angle.abs() / step).ceil() as usize).max(1)
}

/// The left of direction `d`: `d` turned a quarter anticlockwise in (x, z).
fn left(d: DVec2) -> DVec2 {
    DVec2::new(-d.y, d.x)
}

/// One river's line, with the river it carries on from and into, if any.
struct River<'a> {
    line: &'a FeatureLine,
    /// The last segment of the river this one carries on from, as its two
    /// points and half width.
    before: Option<(DVec2, DVec2, f64)>,
    /// The first segment of the river this one carries on into.
    after: Option<(DVec2, DVec2, f64)>,
}

/// Half the wider of a river's widths at the two ends of segment `k`, in
/// metres: how far its water reaches from the segment.
fn half_width(line: &FeatureLine, k: usize) -> f64 {
    0.5 * f64::from(line.widths[k].max(line.widths[k + 1]))
}

impl<'a> River<'a> {
    fn new(
        line: &'a FeatureLine,
        before: Option<&FeatureLine>,
        after: Option<&FeatureLine>,
    ) -> Self {
        let segment = |l: &FeatureLine, k: usize| (l.points[k], l.points[k + 1], half_width(l, k));
        Self {
            line,
            before: before.map(|l| segment(l, l.points.len() - 2)),
            after: after.map(|l| segment(l, 0)),
        }
    }

    fn segments(&self) -> usize {
        self.line.points.len() - 1
    }

    /// Segment `k`'s ends and half width; `-1` and `segments()` are the
    /// rivers before and after, if any.
    fn segment(&self, k: isize) -> Option<(DVec2, DVec2, f64)> {
        let n = self.segments() as isize;
        if k < 0 {
            self.before
        } else if k >= n {
            self.after
        } else {
            let k = k as usize;
            Some((
                self.line.points[k],
                self.line.points[k + 1],
                half_width(self.line, k),
            ))
        }
    }

    fn capsule(&self, k: usize) -> Capsule {
        let (a, b, half_width) = self.segment(k as isize).expect("a segment of the river");
        Capsule { a, b, half_width }
    }

    /// Adds segment `k`'s pieces to `out`: its stretch of the lowest line,
    /// its two water's edges, the round edge where the river turns away
    /// from each side at its start, and the round ends of the water where
    /// the river starts or ends.
    fn pieces(&self, k: usize, id: FeatureId, chord: f64, out: &mut Vec<Piece>) {
        let (a, b, hw) = self.segment(k as isize).expect("a segment of the river");
        out.extend(Piece::new(ConstraintKind::RiverThalweg, id, mm(a), mm(b)));
        let n = left((b - a).normalize());
        let prev = self.segment(k as isize - 1);
        let next = self.segment(k as isize + 1);
        for side in [1.0, -1.0] {
            let start = joint(prev, (a, b, hw), side).unwrap_or(a + n * (side * hw));
            let end = next
                .and_then(|next| joint(Some((a, b, hw)), next, side))
                .unwrap_or(b + n * (side * hw));
            out.extend(Piece::new(
                ConstraintKind::RiverEdge,
                id,
                mm(start),
                mm(end),
            ));
            // Turning away from this side at the start: round it off.
            if let Some((pa, pb, phw)) = prev {
                let pn = left((pb - pa).normalize());
                if side * (pb - pa).perp_dot(b - a) < 0.0 {
                    arc(a, pn * (side * phw), n * (side * hw), chord, id, out);
                }
            }
        }
        // Round ends where the water starts or ends.
        if prev.is_none() {
            arc(a, n * hw, -n * hw, chord, id, out);
        }
        if next.is_none() {
            arc(b, -n * hw, n * hw, chord, id, out);
        }
    }
}

/// On side `side` (1 left, -1 right) of the bend from segment `prev` into
/// segment `cur` (each its ends and half width), where `cur`'s water's
/// edge starts: where the two edges meet, if the river turns towards this
/// side and they meet within both segments. `None` otherwise: the edges
/// then start and end at the segments' ends (rounded off by an arc where
/// the river turns away from this side), and the resolver cuts any overlap.
fn joint(prev: Option<(DVec2, DVec2, f64)>, cur: (DVec2, DVec2, f64), side: f64) -> Option<DVec2> {
    let (pa, pb, phw) = prev?;
    let (a, b, hw) = cur;
    if pb != a {
        return None;
    }
    let (d0, d1) = ((pb - pa).normalize(), (b - a).normalize());
    let turn = d0.perp_dot(d1);
    if side * turn <= 0.0 {
        return None;
    }
    // The two edges: p0 + s d0 and p1 + t d1.
    let (p0, p1) = (pa + left(d0) * (side * phw), a + left(d1) * (side * hw));
    let s = (p1 - p0).perp_dot(d1) / turn;
    let t = (p1 - p0).perp_dot(d0) / turn;
    let (len0, len1) = ((pb - pa).length(), (b - a).length());
    (s >= 0.0 && s <= len0 && t >= 0.0 && t <= len1).then(|| p0 + d0 * s)
}

/// Adds the arc round `centre` from `centre + from` to `centre + to`
/// (turning the shorter way, or anticlockwise through a half turn) as
/// river edge pieces to `out`, each straying at most `chord` metres.
fn arc(centre: DVec2, from: DVec2, to: DVec2, chord: f64, id: FeatureId, out: &mut Vec<Piece>) {
    let angle = if from.perp_dot(to) == 0.0 && from.dot(to) < 0.0 {
        std::f64::consts::PI
    } else {
        from.perp_dot(to).atan2(from.dot(to))
    };
    let (r0, r1) = (from.length(), to.length());
    let n = steps(angle, r0.max(r1), chord);
    let unit = from / r0;
    let point = |j: usize| {
        if j == 0 {
            return centre + from;
        }
        if j == n {
            return centre + to;
        }
        let f = j as f64 / n as f64;
        let (sin, cos) = (angle * f).sin_cos();
        let dir = DVec2::new(unit.x * cos - unit.y * sin, unit.x * sin + unit.y * cos);
        centre + dir * (r0 + (r1 - r0) * f)
    };
    for j in 0..n {
        out.extend(Piece::new(
            ConstraintKind::RiverEdge,
            id,
            mm(point(j)),
            mm(point(j + 1)),
        ));
    }
}
