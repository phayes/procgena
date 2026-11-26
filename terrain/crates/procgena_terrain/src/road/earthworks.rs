//! Road earthworks: each station's cross-section and where its slopes meet
//! the natural ground, cut and fill volumes, junction footprints, and the
//! breaklines a ground mesh must keep.

use glam::{DVec2, DVec3};

use crate::carve::FeatureLine;

use super::network::{RoadId, RoadNetwork, RoadNodeId, RoadNodeKind};
use super::section::{CROWN, Grad, RoadView, SectionShape, WALL};

/// How far apart, in metres, the daylight search samples the ground
/// outward from a shoulder's edge before narrowing down by halves.
const DAYLIGHT_STEP: f64 = 1.0;

/// How many times the daylight search halves the step it found the
/// daylight in: 1 m down to about 0.25 mm.
const DAYLIGHT_HALVINGS: usize = 12;

/// How close, in metres, the ground at a shoulder's edge may be to the
/// edge's level for that side to count as neither cut nor fill.
const LEVEL_TOLERANCE: f64 = 0.01;

/// How far a point may come closer to a road's centre line than the
/// offset it was placed at before it counts as folded back over the road,
/// in metres.
const FOLD_TOLERANCE: f64 = 1e-6;

/// What a road does to the ground on one side of a station.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Earthwork {
    /// The ground is above the road: a cutting rises from the shoulder to
    /// meet it.
    Cut,
    /// The ground is below the road: an embankment falls from the shoulder
    /// to meet it.
    Fill,
    /// The ground meets the shoulder's edge.
    Level,
}

/// One side of a road's cross-section at a station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideSection {
    /// What the road does to the ground on this side.
    pub earthwork: Earthwork,
    /// How far from the centre line the road's cutting or embankment meets
    /// the natural ground (the daylight point), in metres; the shoulder's
    /// edge for [`Earthwork::Level`].
    pub daylight: f64,
    /// The ground's height at the daylight point, in metres.
    pub daylight_level: f32,
    /// Whether the slope only met the ground through a retaining wall at
    /// the end of the road's reach.
    pub wall: bool,
    /// The area across this side, in square metres, where the road cuts
    /// into the ground, from the centre line out to the daylight point.
    pub cut_area: f64,
    /// The area across this side, in square metres, where the road builds
    /// up the ground.
    pub fill_area: f64,
}

/// A road's cross-section at one station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationSection {
    /// Where the station is on the centre line (x and z, metres).
    pub position: DVec2,
    /// The unit direction across the road towards side 0, the left looking
    /// along the segment.
    pub across: DVec2,
    /// The road's level at the crown, in metres.
    pub level: f32,
    /// Half the carriageway's width, in metres.
    pub half_width: f32,
    /// The road's shape across.
    pub shape: SectionShape,
    /// The left (0) and right (1) sides.
    pub sides: [SideSection; 2],
}

impl StationSection {
    /// The road surface's height `d` metres from the centre line, out to
    /// the shoulder's edge.
    pub fn surface(&self, d: f64) -> f64 {
        view(self.level, self.half_width, self.shape, d).band_surface()
    }

    /// The point `d` metres from the centre line towards `side`.
    pub fn offset(&self, side: usize, d: f64) -> DVec2 {
        let sign = if side == 0 { 1.0 } else { -1.0 };
        self.position + self.across * (sign * d)
    }
}

/// The earthworks of one road segment.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentEarthworks {
    /// The segment, as in [`RoadNetwork::segments`].
    pub segment: RoadId,
    /// The cross-section at each of the segment's stations, in order.
    pub stations: Vec<StationSection>,
    /// The volume cut out of the ground, in cubic metres.
    pub cut_volume: f64,
    /// The volume of ground built up, in cubic metres.
    pub fill_volume: f64,
    /// How many station sides end in a retaining wall.
    pub walls: usize,
}

impl SegmentEarthworks {
    /// The ground the segment changes, as a closed outline (x and z,
    /// metres): out along the left daylight points, back along the right.
    /// Points folded back over the road on the inside of tight bends are
    /// left out.
    pub fn footprint(&self) -> Vec<DVec2> {
        let centre: Vec<DVec2> = self.stations.iter().map(|s| s.position).collect();
        let side = |side: usize| -> Vec<DVec2> {
            self.stations
                .iter()
                .map(|s| s.offset(side, s.sides[side].daylight))
                .zip(self.stations.iter().map(|s| s.sides[side].daylight))
                .filter(|&(q, d)| !folded(&centre, q, d))
                .map(|(q, _)| q)
                .collect()
        };
        let mut outline = side(0);
        outline.extend(side(1).into_iter().rev());

        // Loops cut out, the closing edge included.
        let Some(&first) = outline.first() else {
            return outline;
        };
        outline.push(first);
        let flat = |q: DVec2| DVec3::new(q.x, 0.0, q.y);
        let mut outline: Vec<DVec2> = untangle(outline.into_iter().map(flat).collect())
            .into_iter()
            .map(|p| DVec2::new(p.x, p.z))
            .collect();
        outline.pop();
        outline
    }
}

/// `points` (x, height, z) with every loop cut out: where an edge crosses
/// an earlier edge, the points between are dropped and the line goes
/// through the crossing instead, so in x and z it never crosses itself.
fn untangle(points: Vec<DVec3>) -> Vec<DVec3> {
    let xz = |p: DVec3| DVec2::new(p.x, p.z);
    let mut out: Vec<DVec3> = Vec::with_capacity(points.len());
    for p in points {
        if let Some(&last) = out.last() {
            // The first earlier edge, not the one just before, that the new
            // edge crosses.
            let (a, b) = (xz(last), xz(p));
            let hit = (0..out.len().saturating_sub(2))
                .find_map(|j| crossing(xz(out[j]), xz(out[j + 1]), a, b).map(|t| (j, t)));
            if let Some((j, t)) = hit {
                let through = out[j].lerp(out[j + 1], t);
                out.truncate(j + 1);
                out.push(through);
            }
        }
        out.push(p);
    }
    out
}

/// Where the segment from `a` to `b` properly crosses the one from `c` to
/// `d`, as the share of the way from `a` to `b`; `None` if they don't
/// cross, only touch, or are parallel to within rounding.
fn crossing(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<f64> {
    let (r, s) = (b - a, d - c);
    let denominator = r.perp_dot(s);
    if denominator.abs() <= 1e-12 * r.length() * s.length() {
        return None;
    }
    let t = (c - a).perp_dot(s) / denominator;
    let u = (c - a).perp_dot(r) / denominator;
    (t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0).then_some(t)
}

/// What a breakline marks: a line along which the designed ground changes
/// slope, which a ground mesh should keep as an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BreaklineKind {
    /// The road's crown.
    Centre,
    /// The edge of the carriageway, where the shoulder starts.
    CarriagewayEdge,
    /// The outer edge of the shoulder, where a cutting or embankment
    /// starts.
    ShoulderEdge,
    /// Where the slope steepens into a retaining wall.
    WallStart,
    /// The top of a cutting, where it meets the natural ground.
    Crest,
    /// The foot of an embankment, where it meets the natural ground.
    Toe,
}

/// A line along a road segment the ground mesh should keep as edges.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadBreakline {
    /// What the line marks.
    pub kind: BreaklineKind,
    /// The segment it follows.
    pub segment: RoadId,
    /// The side it is on, 0 (left) or 1 (right); `None` for the centre.
    pub side: Option<usize>,
    /// Its points in order along the segment, as x, height and z in
    /// metres. At least two.
    pub points: Vec<DVec3>,
}

/// Where roads meet: the outline of the shared road surface, out to the
/// shoulders.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionFootprint {
    /// The [`Junction`](RoadNodeKind::Junction) node.
    pub node: RoadNodeId,
    /// The outline (x and z, metres): convex, counter-clockwise seen from
    /// above with z pointing down the page, with no repeated points.
    pub outline: Vec<DVec2>,
}

/// A region's road earthworks (see [`Terrain::road_earthworks`](crate::Terrain::road_earthworks)).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadEarthworks {
    /// Each segment's earthworks, in the order of the roads carved.
    pub segments: Vec<SegmentEarthworks>,
    /// One footprint per junction, in node id order.
    pub junctions: Vec<JunctionFootprint>,
    /// The lines along the roads a ground mesh should keep. Inside junction
    /// footprints and where an offset line would fold back over its road,
    /// they are broken off.
    pub breaklines: Vec<RoadBreakline>,
    /// The total volume cut, in cubic metres.
    pub cut_volume: f64,
    /// The total volume built up, in cubic metres.
    pub fill_volume: f64,
    /// How many station sides end in a retaining wall.
    pub walls: usize,
}

/// One road segment as carved: its line, shape across at each point, and
/// id.
pub(crate) struct CarvedLine<'a> {
    pub line: &'a FeatureLine,
    pub sections: &'a [SectionShape],
    pub segment: RoadId,
}

/// The earthworks of `lines`, the segments of `network` as carved, on the
/// natural ground `ground` (the ground before roads are cut in).
pub(crate) fn earthworks(
    network: &RoadNetwork,
    lines: &[CarvedLine<'_>],
    ground: &(dyn Fn(DVec2) -> f64 + Sync),
) -> RoadEarthworks {
    // Each segment's sections and volumes.
    let segments: Vec<SegmentEarthworks> = lines.iter().map(|l| segment(l, ground)).collect();
    let junctions = junctions(network, &segments);

    // Breaklines, broken off inside junctions.
    let mut breaklines = Vec::new();
    for s in &segments {
        breaklines.extend(segment_breaklines(s, &junctions));
    }
    RoadEarthworks {
        cut_volume: segments.iter().map(|s| s.cut_volume).sum(),
        fill_volume: segments.iter().map(|s| s.fill_volume).sum(),
        walls: segments.iter().map(|s| s.walls).sum(),
        segments,
        junctions,
        breaklines,
    }
}

/// The earthworks of one carved segment.
fn segment(carved: &CarvedLine<'_>, ground: &(dyn Fn(DVec2) -> f64 + Sync)) -> SegmentEarthworks {
    let line = carved.line;
    let n = line.points.len();
    let stations: Vec<StationSection> = (0..n)
        .map(|i| {
            // Across the road: square to the line through the neighbours.
            let (prev, next) = (
                line.points[i.saturating_sub(1)],
                line.points[(i + 1).min(n - 1)],
            );
            let across = (next - prev).normalize_or_zero().perp();
            let (level, half_width, shape) =
                (line.levels[i], 0.5 * line.widths[i], carved.sections[i]);
            let sides = [0, 1].map(|side| {
                let sign = if side == 0 { 1.0 } else { -1.0 };
                let at = |d: f64| ground(line.points[i] + across * (sign * d));
                side_section(level, half_width, shape, &at)
            });
            StationSection {
                position: line.points[i],
                across,
                level,
                half_width,
                shape,
                sides,
            }
        })
        .collect();

    // Volumes by the mean of neighbouring areas.
    let (mut cut_volume, mut fill_volume) = (0.0, 0.0);
    for pair in stations.windows(2) {
        let length = pair[0].position.distance(pair[1].position);
        let area = |s: &StationSection, cut: bool| -> f64 {
            s.sides
                .iter()
                .map(|side| if cut { side.cut_area } else { side.fill_area })
                .sum()
        };
        cut_volume += 0.5 * (area(&pair[0], true) + area(&pair[1], true)) * length;
        fill_volume += 0.5 * (area(&pair[0], false) + area(&pair[1], false)) * length;
    }
    let walls = stations
        .iter()
        .flat_map(|s| s.sides.iter())
        .filter(|s| s.wall)
        .count();
    SegmentEarthworks {
        segment: carved.segment,
        stations,
        cut_volume,
        fill_volume,
        walls,
    }
}

/// The road's view at distance `d` from its centre line, with every value
/// fixed.
fn view(level: f32, half_width: f32, shape: SectionShape, d: f64) -> RoadView {
    let c = |v: f32| Grad::constant(f64::from(v));
    RoadView {
        distance: Grad::constant(d),
        half_width: c(half_width),
        level: c(level),
        shoulder: c(shape.shoulder),
        crossfall: c(shape.crossfall),
        cut: c(shape.cut),
        fill: c(shape.fill),
    }
}

impl RoadView {
    /// The band's surface on the road (its low and high are equal there).
    fn band_surface(&self) -> f64 {
        self.band().map_or(f64::NAN, |b| b.low.v)
    }
}

/// One side of a station's section: where its slope meets `ground` (the
/// natural ground's height at a distance from the centre line), with the
/// areas cut and filled. The search walks out from the shoulder's edge in
/// [`DAYLIGHT_STEP`]s, then halves the step it crossed in
/// [`DAYLIGHT_HALVINGS`] times. If it reaches the end of the road's reach
/// without meeting the ground, which the retaining wall's rise to vertical
/// rules out but rounding might not, the daylight is put at the reach.
fn side_section(
    level: f32,
    half_width: f32,
    shape: SectionShape,
    ground: &dyn Fn(f64) -> f64,
) -> SideSection {
    let edge = f64::from(half_width) + f64::from(shape.shoulder);
    let at = |d: f64| view(level, half_width, shape, d);
    let reach = edge + at(edge).reach().v;

    // On the carriageway and shoulder: the areas between the surface and
    // the ground, sampled at the crown, its rounding, and every half metre.
    let (mut cut_area, mut fill_area) = (0.0, 0.0);
    let mut add = |d0: f64, d1: f64, g0: f64, g1: f64| {
        // Trapezoids of the ground's height above the design, split where
        // the two cross.
        let width = d1 - d0;
        if g0 * g1 >= 0.0 {
            let mean = 0.5 * (g0 + g1);
            if mean > 0.0 {
                cut_area += mean * width;
            } else {
                fill_area -= mean * width;
            }
        } else {
            let t = g0 / (g0 - g1);
            let (a, b) = (0.5 * g0 * t * width, 0.5 * g1 * (1.0 - t) * width);
            for part in [a, b] {
                if part > 0.0 {
                    cut_area += part;
                } else {
                    fill_area -= part;
                }
            }
        }
    };
    let mut d_prev = 0.0;
    let mut g_prev = ground(0.0) - at(0.0).band_surface();
    let mut d = CROWN;
    while d_prev < edge {
        let d_next = d.min(edge);
        let g = ground(d_next) - at(d_next).band_surface();
        add(d_prev, d_next, g_prev, g);
        (d_prev, g_prev) = (d_next, g);
        d = d_next + 0.5;
    }

    // Which way the side goes, from the ground at the shoulder's edge.
    let earthwork = if g_prev > LEVEL_TOLERANCE {
        Earthwork::Cut
    } else if g_prev < -LEVEL_TOLERANCE {
        Earthwork::Fill
    } else {
        return SideSection {
            earthwork: Earthwork::Level,
            daylight: edge,
            daylight_level: ground(edge) as f32,
            wall: false,
            cut_area,
            fill_area,
        };
    };

    // The ground's height above the slope (cut) or below it (fill), which
    // turns non-positive where the slope meets the ground.
    let beyond = |d: f64| -> f64 {
        let band = at(d).band().expect("within reach");
        match earthwork {
            Earthwork::Cut => ground(d) - band.high.v,
            _ => band.low.v - ground(d),
        }
    };

    // Out in steps until the ground is met, adding the areas on the way.
    let last = reach - 1e-6;
    let (mut d0, mut b0) = (edge, beyond(edge).max(0.0));
    let mut met = None;
    while d0 < last {
        let d1 = (d0 + DAYLIGHT_STEP).min(last);
        let b1 = beyond(d1);
        if b1 <= 0.0 {
            met = Some((d0, d1, b0));
            break;
        }
        let (c0, c1) = match earthwork {
            Earthwork::Cut => (b0, b1),
            _ => (-b0, -b1),
        };
        add(d0, d1, c0, c1);
        (d0, b0) = (d1, b1);
    }
    let (daylight, wall) = match met {
        Some((mut lo, mut hi, b_lo)) => {
            for _ in 0..DAYLIGHT_HALVINGS {
                let mid = 0.5 * (lo + hi);
                if beyond(mid) <= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            // The last sliver, as a triangle.
            let sliver = 0.5 * b_lo * (hi - d0);
            match earthwork {
                Earthwork::Cut => cut_area += sliver,
                _ => fill_area += sliver,
            }
            (hi, hi > reach - WALL)
        }
        None => (reach, true),
    };
    SideSection {
        earthwork,
        daylight,
        daylight_level: ground(daylight) as f32,
        wall,
        cut_area,
        fill_area,
    }
}

/// Whether `q`, placed `offset` metres from a road's centre line `centre`,
/// is nearer to some other part of the line than that: folded back over
/// the road on the inside of a tight bend.
fn folded(centre: &[DVec2], q: DVec2, offset: f64) -> bool {
    centre.windows(2).any(|w| {
        let ab = w[1] - w[0];
        let t = ((q - w[0]).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
        q.distance(w[0] + ab * t) < offset - FOLD_TOLERANCE
    })
}

/// One footprint per junction node of `network`: the convex hull of the
/// shoulder edges of every segment meeting there, over the stations within
/// twice the formation's width of the node.
fn junctions(network: &RoadNetwork, segments: &[SegmentEarthworks]) -> Vec<JunctionFootprint> {
    let mut out = Vec::new();
    for node in network
        .nodes
        .iter()
        .filter(|n| n.kind == RoadNodeKind::Junction)
    {
        let mut points = Vec::new();
        for s in segments {
            let Some(meeting) = network.segment(s.segment) else {
                continue;
            };
            if meeting.start != node.id && meeting.end != node.id {
                continue;
            }
            for station in &s.stations {
                let edge = f64::from(station.half_width) + f64::from(station.shape.shoulder);
                if station.position.distance(node.position) <= 2.0 * edge {
                    points.extend([0, 1].map(|side| station.offset(side, edge)));
                }
            }
        }
        let outline = convex_hull(points);
        if outline.len() >= 3 {
            out.push(JunctionFootprint {
                node: node.id,
                outline,
            });
        }
    }
    out
}

/// The convex hull of `points`, counter-clockwise (in x and z), with no
/// repeated or collinear points (Andrew's monotone chain).
pub(crate) fn convex_hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Whether `p` is inside the convex, counter-clockwise outline `outline`.
fn inside(outline: &[DVec2], p: DVec2) -> bool {
    outline.len() >= 3
        && (0..outline.len()).all(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            (b - a).perp_dot(p - a) > 0.0
        })
}

/// The breaklines of one segment: its centre, and on each side the
/// carriageway and shoulder edges, the start of any retaining wall, and
/// the crest or toe where its slopes meet the ground. A line is broken
/// where a point falls inside a junction footprint or folds back over the
/// road, and where the daylight changes between crest and toe.
fn segment_breaklines(
    segment: &SegmentEarthworks,
    junctions: &[JunctionFootprint],
) -> Vec<RoadBreakline> {
    let centre: Vec<DVec2> = segment.stations.iter().map(|s| s.position).collect();
    let mut out = Vec::new();
    // Runs of points, broken by `None`.
    let mut runs = |kind: BreaklineKind, side: Option<usize>, points: Vec<Option<DVec3>>| {
        let mut run: Vec<DVec3> = Vec::new();
        for p in points.into_iter().chain([None]) {
            match p {
                Some(p) => run.push(p),
                None => {
                    let points = untangle(std::mem::take(&mut run));
                    if points.len() >= 2 {
                        out.push(RoadBreakline {
                            kind,
                            segment: segment.segment,
                            side,
                            points,
                        });
                    }
                    run.clear();
                }
            }
        }
    };
    let keep = |q: DVec2, offset: f64| {
        !folded(&centre, q, offset) && !junctions.iter().any(|j| inside(&j.outline, q))
    };
    let point = |q: DVec2, height: f64| DVec3::new(q.x, height, q.y);

    // The crown.
    let crown = segment
        .stations
        .iter()
        .map(|s| keep(s.position, 0.0).then(|| point(s.position, f64::from(s.level))))
        .collect();
    runs(BreaklineKind::Centre, None, crown);

    for side in 0..2 {
        // Edges at fixed offsets, on the road surface.
        for (kind, offset) in [
            (BreaklineKind::CarriagewayEdge, 0.0f32),
            (BreaklineKind::ShoulderEdge, 1.0),
        ] {
            let points = segment
                .stations
                .iter()
                .map(|s| {
                    let d = f64::from(s.half_width) + f64::from(offset * s.shape.shoulder);
                    let q = s.offset(side, d);
                    keep(q, d).then(|| point(q, s.surface(d)))
                })
                .collect();
            runs(kind, Some(side), points);
        }

        // Where walls start, and the daylight as crests or toes.
        let wall_start = segment
            .stations
            .iter()
            .map(|s| {
                let section = s.sides[side];
                if !section.wall {
                    return None;
                }
                let edge = f64::from(s.half_width) + f64::from(s.shape.shoulder);
                let v = view(s.level, s.half_width, s.shape, 0.0);
                let d = edge + v.reach().v - WALL;
                let band = view(s.level, s.half_width, s.shape, d).band()?;
                let height = match section.earthwork {
                    Earthwork::Cut => band.high.v,
                    _ => band.low.v,
                };
                let q = s.offset(side, d);
                keep(q, d).then(|| point(q, height))
            })
            .collect();
        runs(BreaklineKind::WallStart, Some(side), wall_start);
        for (kind, earthwork) in [
            (BreaklineKind::Crest, Earthwork::Cut),
            (BreaklineKind::Toe, Earthwork::Fill),
        ] {
            let points = segment
                .stations
                .iter()
                .map(|s| {
                    let section = s.sides[side];
                    if section.earthwork != earthwork {
                        return None;
                    }
                    let q = s.offset(side, section.daylight);
                    keep(q, section.daylight).then(|| point(q, f64::from(section.daylight_level)))
                })
                .collect();
            runs(kind, Some(side), points);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carve::FeatureKind;

    /// A straight road along x, level 10, 6 m wide with the default shape,
    /// over `ground`.
    fn straight(ground: &(dyn Fn(DVec2) -> f64 + Sync)) -> SegmentEarthworks {
        let line = FeatureLine {
            kind: FeatureKind::Road,
            points: (0..=10)
                .map(|i| DVec2::new(10.0 * f64::from(i), 0.0))
                .collect(),
            levels: vec![10.0; 11],
            widths: vec![6.0; 11],
            depth: 0.0,
        };
        let sections = vec![SectionShape::PIECE; 11];
        segment(
            &CarvedLine {
                line: &line,
                sections: &sections,
                segment: RoadId(1),
            },
            ground,
        )
    }

    #[test]
    fn a_road_on_a_side_hill_cuts_one_side_and_fills_the_other() {
        // Ground rising 0.2 to the left (+z), level with the crown there.
        let earthworks = straight(&|p: DVec2| 10.0 + 0.2 * p.y);
        let s = &earthworks.stations[5];
        assert!(s.across.y > 0.99, "side 0 is +z: {}", s.across);
        let [left, right] = s.sides;
        assert_eq!(left.earthwork, Earthwork::Cut);
        assert_eq!(right.earthwork, Earthwork::Fill);
        // The 1:1 cutting from the shoulder's edge (4 m, level `edge`) meets
        // ground rising at 0.2: edge + (d - 4) = 10 + 0.2 d.
        let edge = 10.0 - 0.025 * (4.0 - CROWN / 2.0);
        let expected = (10.0 - edge + 4.0) / 0.8;
        assert!((left.daylight - expected).abs() < 1e-3, "{}", left.daylight);
        // The embankment at 1:1.5 meets ground falling at 0.2 the other way.
        let expected = (edge - 10.0 + 4.0 / 1.5) / (1.0 / 1.5 - 0.2);
        assert!(
            (right.daylight - expected).abs() < 1e-3,
            "{}",
            right.daylight
        );
        assert!(left.cut_area > 0.0 && right.fill_area > 0.0);
        assert!(!left.wall && !right.wall);
        assert!(earthworks.cut_volume > 0.0 && earthworks.fill_volume > 0.0);
    }

    #[test]
    fn flat_ground_at_the_shoulder_level_needs_no_slopes() {
        let earthworks = straight(&|_| 10.0 - 0.025 * 3.75);
        for side in earthworks.stations[3].sides {
            assert_eq!(side.earthwork, Earthwork::Level);
            assert_eq!(side.daylight, 4.0);
        }
    }

    #[test]
    fn a_slope_that_never_meets_the_ground_ends_in_a_wall() {
        // A cliff: ground 200 m up on the left, 200 m down on the right.
        let earthworks = straight(&|p: DVec2| 10.0 + 200.0 * p.y.signum());
        for side in earthworks.stations[5].sides {
            assert!(side.wall, "{side:?}");
            assert!(side.daylight < 3.0 + crate::ports::BANK_REACH);
        }
        assert_eq!(earthworks.walls, 22);
    }

    #[test]
    fn a_tight_bend_has_no_folded_footprint() {
        // A hairpin of radius 3 m about the origin, on a side hill.
        let points: Vec<DVec2> = (0..=36)
            .map(|i| {
                let a = std::f64::consts::PI * f64::from(i) / 36.0;
                DVec2::new(3.0 * a.cos(), 3.0 * a.sin())
            })
            .collect();
        let n = points.len();
        let line = FeatureLine {
            kind: FeatureKind::Road,
            points,
            levels: vec![10.0; n],
            widths: vec![6.0; n],
            depth: 0.0,
        };
        let sections = vec![SectionShape::PIECE; n];
        let earthworks = segment(
            &CarvedLine {
                line: &line,
                sections: &sections,
                segment: RoadId(1),
            },
            &|p: DVec2| 12.0 + 0.1 * p.x,
        );
        let footprint = earthworks.footprint();
        assert!(footprint.len() >= 3);
        assert!(simple(&footprint), "{footprint:?}");
    }

    #[test]
    fn the_hull_is_convex_and_counter_clockwise() {
        let hull = convex_hull(vec![
            DVec2::ZERO,
            DVec2::new(2.0, 0.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(0.0, 2.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(2.0, 2.0),
        ]);
        assert_eq!(
            hull,
            vec![
                DVec2::ZERO,
                DVec2::new(2.0, 0.0),
                DVec2::new(2.0, 2.0),
                DVec2::new(0.0, 2.0)
            ]
        );
        assert!(inside(&hull, DVec2::new(1.0, 1.0)));
        assert!(!inside(&hull, DVec2::new(3.0, 1.0)));
    }

    /// Whether the closed outline `outline` never crosses itself.
    pub(crate) fn simple(outline: &[DVec2]) -> bool {
        let n = outline.len();
        let cross = |a: DVec2, b: DVec2, c: DVec2, d: DVec2| {
            let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
            side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
        };
        (0..n).all(|i| {
            (i + 2..n).all(|j| {
                (i == 0 && j == n - 1)
                    || !cross(
                        outline[i],
                        outline[(i + 1) % n],
                        outline[j],
                        outline[(j + 1) % n],
                    )
            })
        })
    }
}
