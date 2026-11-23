//! Cutting rivers, ponds and roads into the ground.

use std::collections::HashMap;

use glam::DVec2;

use crate::ports::BANK_REACH;

/// How steeply a river or pond bank rises, as rise over run.
const RIVER_BANK: f64 = 0.35;

/// How far a road may sit above or below the ground beneath it before its
/// banks start, in metres.
const ROAD_TOLERANCE: f64 = 0.25;

/// How steeply road cuttings and embankments start on gentle ground, as
/// rise over run (about 31°).
pub(crate) const ROAD_BANK: f64 = 0.6;

/// How many times steeper than the natural ground a road's banks start.
const BANK_FACTOR: f64 = 2.0;

/// The steepest a road's banks start, as rise over run (about 87°).
const MAX_BANK: f64 = 20.0;

/// The slope, as rise over run, from which a road cutting's face is a
/// bluff: 45°.
const BLUFF_SLOPE: f64 = 1.0;

/// The slope, as rise over run, from which a road cutting's face is a
/// cliff: 65° (tan 65°).
const CLIFF_SLOPE: f64 = 2.144_506_920_509_559_5;

/// Size of the spatial index's cells, in metres.
const INDEX_CELL: f64 = 64.0;

/// How softly river and pond cuts meet the ground, in metres of height. The
/// cut edge is rounded instead of creased, so the ground's slope stays
/// continuous.
const RIVER_BLEND: f64 = 0.5;

/// How softly road cuts and fills meet the ground, in metres of height.
/// Smaller than the road's tolerance band.
const ROAD_BLEND: f64 = 0.2;

/// A smooth minimum: `min(a, b)` with the corner rounded where the two are
/// within `k` of each other. Its slope is continuous. Inigo Quilez's
/// polynomial smooth minimum (MIT).
fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// A smooth maximum, the mirror of [`smooth_min`].
fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    -smooth_min(-a, -b, k)
}

/// What a [`FeatureLine`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureKind {
    /// A river. Its levels are the water surface, falling (or level) from
    /// the first point to the last.
    River,
    /// A road. Its levels are the road surface.
    Road,
}

/// A river or road as a line of points, each with a level.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureLine {
    /// What the line is.
    pub kind: FeatureKind,
    /// The line's points, in metres (x and z).
    pub points: Vec<DVec2>,
    /// The water or road surface height at each point, in metres.
    pub levels: Vec<f32>,
    /// Width, in metres.
    pub width: f32,
    /// For a river, depth of the bed below the water, in metres. 0 for roads.
    pub depth: f32,
}

/// A pond where a river ends: a round hollow holding water.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pond {
    /// Centre, in metres (x and z).
    pub centre: DVec2,
    /// Water surface height, in metres.
    pub level: f32,
    /// Radius of the water, in metres.
    pub radius: f64,
    /// Depth at the centre below the water, in metres.
    pub depth: f32,
}

/// What a road made of the ground at a point, as a set of tags; a point
/// can carry several, such as [`RoadTags::CUT`] and [`RoadTags::CLIFF`].
/// Empty where no road touches the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RoadTags(u8);

impl RoadTags {
    /// No tags.
    pub const NONE: Self = Self(0);
    /// The road's own surface.
    pub const ROAD: Self = Self(1);
    /// A bank cut down into the ground beside the road.
    pub const CUT: Self = Self(1 << 1);
    /// A bank built up beside the road, below 65°.
    pub const EMBANKMENT: Self = Self(1 << 2);
    /// A built-up bank of 65° or steeper: a wall holding up the road.
    pub const RETAINING_WALL: Self = Self(1 << 3);
    /// A bank between 45° and 65°.
    pub const BLUFF: Self = Self(1 << 4);
    /// A bank of 65° or steeper, up to vertical.
    pub const CLIFF: Self = Self(1 << 5);

    /// Whether every tag of `other` is in `self`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether there are no tags.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The tags' names, in the order the constants are listed:
    /// `"road"`, `"cut"`, `"embankment"`, `"retaining_wall"`, `"bluff"`,
    /// `"cliff"`.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Self::ROAD, "road"),
            (Self::CUT, "cut"),
            (Self::EMBANKMENT, "embankment"),
            (Self::RETAINING_WALL, "retaining_wall"),
            (Self::BLUFF, "bluff"),
            (Self::CLIFF, "cliff"),
        ]
        .into_iter()
        .filter(|&(tag, _)| self.contains(tag))
        .map(|(_, name)| name)
        .collect()
    }
}

impl std::ops::BitOr for RoadTags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for RoadTags {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// What a [`PointOfInterest`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoiKind {
    /// Where the region's roads lead: the end of the road network.
    RoadEnd,
}

/// A place the caller may fill with content of their own, such as a
/// village at the end of a road. The ground there is left as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointOfInterest {
    /// What the place is.
    pub kind: PoiKind,
    /// Position, in metres (x and z).
    pub position: DVec2,
    /// The height of the place, in metres: for a road end, the road
    /// surface there.
    pub level: f32,
}

/// One entry in the spatial index.
#[derive(Debug, Clone, Copy)]
enum Item {
    River { line: u32, segment: u32 },
    Pond(u32),
    Road { line: u32, segment: u32 },
}

/// All features of one terrain, ready to carve the ground at any point.
#[derive(Debug, Clone, Default)]
pub(crate) struct Features {
    pub rivers: Vec<FeatureLine>,
    pub ponds: Vec<Pond>,
    pub roads: Vec<FeatureLine>,
    /// For each road, how steeply its banks start at each point, as rise
    /// over run.
    road_banks: Vec<Vec<f32>>,
    /// Places left for the caller to fill; they don't change the ground.
    pub points_of_interest: Vec<PointOfInterest>,
    index: HashMap<(i64, i64), Vec<Item>>,
}

/// The ground after carving, the water surface if there is water, and what
/// the roads made of the ground there.
pub(crate) struct Carved {
    pub height: f32,
    pub water: Option<f32>,
    pub tags: RoadTags,
}

/// The closest point on a feature line's segment: distance to it and the
/// level there.
fn segment_query(line: &FeatureLine, segment: usize, p: DVec2) -> (f64, f32) {
    let (d, t) = segment_position(line, segment, p);
    let (la, lb) = (line.levels[segment], line.levels[segment + 1]);
    (d, la + (lb - la) * t as f32)
}

/// The distance from `p` to a feature line's segment, and how far along
/// the segment (0 to 1) its closest point is.
fn segment_position(line: &FeatureLine, segment: usize, p: DVec2) -> (f64, f64) {
    let (a, b) = (line.points[segment], line.points[segment + 1]);
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.distance(a + ab * t), t)
}

/// The height a river or pond cuts the ground down to at distance `d` from
/// its centre line, or `None` beyond its banks.
///
/// Inside the water the bed is a parabola `depth` deep. Outside, the bank
/// rises from the water's edge and steepens until it no longer cuts
/// anything, [`BANK_REACH`] metres out.
fn channel(d: f64, half_width: f64, level: f32, depth: f32) -> Option<f32> {
    let level = f64::from(level);
    if d <= half_width {
        let r = d / half_width;
        return Some((level - f64::from(depth) * (1.0 - r * r)) as f32);
    }
    let x = d - half_width;
    (x < BANK_REACH).then(|| (level + RIVER_BANK * x / (1.0 - (x / BANK_REACH).powi(2))) as f32)
}

/// How far a road lets the ground differ from its level at distance `d`
/// from its centre line, or `None` beyond its cuttings and embankments.
/// `bank` is the slope of its banks, as rise over run: a flat face at that
/// slope, steepening only close to [`BANK_REACH`] so that every bank meets
/// the ground within it.
fn road_tolerance(d: f64, half_width: f64, bank: f64) -> Option<f64> {
    let x = d - half_width;
    if x <= 0.0 {
        return Some(ROAD_TOLERANCE);
    }
    (x < BANK_REACH).then(|| ROAD_TOLERANCE + bank * x / (1.0 - (x / BANK_REACH).powi(8)))
}

/// The slope, as rise over run, of a road's cutting or embankment bank at
/// distance `d` from its centre line, or `None` on the road itself or
/// beyond its banks. It is the slope of [`road_tolerance`] with the same
/// `bank`.
fn bank_slope(d: f64, half_width: f64, bank: f64) -> Option<f64> {
    let x = d - half_width;
    if x <= 0.0 || x >= BANK_REACH {
        return None;
    }
    let u8 = (x / BANK_REACH).powi(8);
    Some(bank * (1.0 + 7.0 * u8) / (1.0 - u8).powi(2))
}

/// How steeply a road's banks start, as rise over run, where the natural
/// ground around it has slope `ground`: twice as steep, so the bank always
/// meets the ground, and never gentler than [`ROAD_BANK`] or steeper than
/// [`MAX_BANK`].
pub(crate) fn bank_for(ground: f64) -> f64 {
    (BANK_FACTOR * ground).clamp(ROAD_BANK, MAX_BANK)
}

impl Features {
    /// Gathers features and builds the spatial index.
    ///
    /// `road_banks` gives, for each of `roads`, how steeply its banks start
    /// at each of its points (see [`bank_for`]).
    ///
    /// # Panics
    ///
    /// If `road_banks` doesn't have one value per road point.
    pub(crate) fn new(
        rivers: Vec<FeatureLine>,
        ponds: Vec<Pond>,
        roads: Vec<FeatureLine>,
        road_banks: Vec<Vec<f32>>,
        points_of_interest: Vec<PointOfInterest>,
    ) -> Self {
        assert!(
            road_banks.len() == roads.len()
                && road_banks
                    .iter()
                    .zip(&roads)
                    .all(|(b, r)| b.len() == r.points.len()),
            "one bank steepness per road point"
        );
        let mut features = Self {
            rivers,
            ponds,
            roads,
            road_banks,
            points_of_interest,
            index: HashMap::new(),
        };
        let mut entries = Vec::new();
        for (line, river) in features.rivers.iter().enumerate() {
            for segment in 0..river.points.len().saturating_sub(1) {
                let reach = f64::from(river.width) / 2.0 + BANK_REACH;
                let (a, b) = (river.points[segment], river.points[segment + 1]);
                entries.push((
                    a.min(b) - reach,
                    a.max(b) + reach,
                    Item::River {
                        line: line as u32,
                        segment: segment as u32,
                    },
                ));
            }
        }
        for (i, pond) in features.ponds.iter().enumerate() {
            let reach = DVec2::splat(pond.radius + BANK_REACH);
            entries.push((
                pond.centre - reach,
                pond.centre + reach,
                Item::Pond(i as u32),
            ));
        }
        for (line, road) in features.roads.iter().enumerate() {
            for segment in 0..road.points.len().saturating_sub(1) {
                let reach = f64::from(road.width) / 2.0 + BANK_REACH;
                let (a, b) = (road.points[segment], road.points[segment + 1]);
                entries.push((
                    a.min(b) - reach,
                    a.max(b) + reach,
                    Item::Road {
                        line: line as u32,
                        segment: segment as u32,
                    },
                ));
            }
        }
        for (lo, hi, item) in entries {
            let (lo, hi) = ((lo / INDEX_CELL).floor(), (hi / INDEX_CELL).floor());
            for z in lo.y as i64..=hi.y as i64 {
                for x in lo.x as i64..=hi.x as i64 {
                    features.index.entry((x, z)).or_default().push(item);
                }
            }
        }
        features
    }

    /// Segment `segment` of road `line` seen from `p`: the distance to it,
    /// the road's half width, and its level and bank steepness at the
    /// closest point.
    fn road_at(&self, line: u32, segment: u32, p: DVec2) -> (f64, f64, f64, f64) {
        let (road, banks) = (&self.roads[line as usize], &self.road_banks[line as usize]);
        let s = segment as usize;
        let (d, t) = segment_position(road, s, p);
        let lerp = |a: f32, b: f32| f64::from(a) + (f64::from(b) - f64::from(a)) * t;
        (
            d,
            f64::from(road.width) / 2.0,
            lerp(road.levels[s], road.levels[s + 1]),
            lerp(banks[s], banks[s + 1]),
        )
    }

    /// Carves ground of height `h` at `p`: rivers and ponds cut it down,
    /// then roads cut or fill it to their level.
    pub(crate) fn apply(&self, p: DVec2, h: f32) -> Carved {
        let key = (
            (p.x / INDEX_CELL).floor() as i64,
            (p.y / INDEX_CELL).floor() as i64,
        );
        let Some(items) = self.index.get(&key) else {
            return Carved {
                height: h,
                water: None,
                tags: RoadTags::NONE,
            };
        };

        // Rivers and ponds: the lowest cut wins; note the water surface
        // wherever the point is within the water's width.
        let mut height = h;
        let mut water: Option<f32> = None;
        let mut lowest: Option<f32> = None;
        for item in items {
            let (d, half_width, level, depth) = match *item {
                Item::River { line, segment } => {
                    let river = &self.rivers[line as usize];
                    let (d, level) = segment_query(river, segment as usize, p);
                    (d, f64::from(river.width) / 2.0, level, river.depth)
                }
                Item::Pond(i) => {
                    let pond = &self.ponds[i as usize];
                    (p.distance(pond.centre), pond.radius, pond.level, pond.depth)
                }
                Item::Road { .. } => continue,
            };
            if let Some(cut) = channel(d, half_width, level, depth) {
                lowest = Some(lowest.map_or(cut, |l: f32| l.min(cut)));
                if d <= half_width {
                    water = Some(water.map_or(level, |w| w.max(level)));
                }
            }
        }
        if let Some(cut) = lowest {
            height = smooth_min(f64::from(height), f64::from(cut), RIVER_BLEND) as f32;
        }

        // Roads: hold the ground within reach of the road's level.
        let roads: Vec<(f64, f64, f64, f64)> = items
            .iter()
            .filter_map(|item| match *item {
                Item::Road { line, segment } => Some(self.road_at(line, segment, p)),
                _ => None,
            })
            .collect();
        let uncut = f64::from(height);
        for &(d, half_width, level, bank) in &roads {
            if let Some(e) = road_tolerance(d, half_width, bank) {
                let held = smooth_min(f64::from(height), level + e, ROAD_BLEND);
                height = smooth_max(held, level - e, ROAD_BLEND) as f32;
            }
        }

        // Tags: the road surface, and banks that are the ground here,
        // cut down or built up, with how steep they are.
        let mut tags = RoadTags::NONE;
        for &(d, half_width, level, bank) in &roads {
            if d <= half_width {
                tags |= RoadTags::ROAD;
                continue;
            }
            let (Some(e), Some(slope)) = (
                road_tolerance(d, half_width, bank),
                bank_slope(d, half_width, bank),
            ) else {
                continue;
            };
            let on = |edge: f64| (f64::from(height) - edge).abs() <= ROAD_BLEND;
            if uncut > level + e && on(level + e) {
                tags |= RoadTags::CUT;
            } else if uncut < level - e && on(level - e) {
                tags |= if slope >= CLIFF_SLOPE {
                    RoadTags::RETAINING_WALL
                } else {
                    RoadTags::EMBANKMENT
                };
            } else {
                continue;
            }
            if slope >= CLIFF_SLOPE {
                tags |= RoadTags::CLIFF;
            } else if slope >= BLUFF_SLOPE {
                tags |= RoadTags::BLUFF;
            }
        }
        Carved {
            height,
            water: water.filter(|&w| height < w),
            tags,
        }
    }
}

/// Smooths a line by Chaikin corner cutting, `iterations` times, keeping its
/// end points. Levels are blended with the same weights, so a line whose
/// levels never rise keeps that property.
pub(crate) fn chaikin(
    points: &[DVec2],
    levels: &[f32],
    iterations: usize,
) -> (Vec<DVec2>, Vec<f32>) {
    let (mut points, mut levels) = (points.to_vec(), levels.to_vec());
    for _ in 0..iterations {
        if points.len() < 3 {
            break;
        }
        let mut p = vec![points[0]];
        let mut l = vec![levels[0]];
        for i in 0..points.len() - 1 {
            let (a, b) = (points[i], points[i + 1]);
            let (la, lb) = (levels[i], levels[i + 1]);
            if i > 0 {
                p.push(a.lerp(b, 0.25));
                l.push(la + (lb - la) * 0.25);
            }
            if i < points.len() - 2 {
                p.push(a.lerp(b, 0.75));
                l.push(la + (lb - la) * 0.75);
            }
        }
        p.push(points[points.len() - 1]);
        l.push(levels[levels.len() - 1]);
        (points, levels) = (p, l);
    }
    (points, levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn river() -> FeatureLine {
        FeatureLine {
            kind: FeatureKind::River,
            points: vec![DVec2::ZERO, DVec2::new(100.0, 0.0)],
            levels: vec![10.0, 9.0],
            width: 6.0,
            depth: 1.5,
        }
    }

    #[test]
    fn rivers_cut_a_bed_and_hold_water() {
        let features = Features::new(vec![river()], Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let middle = features.apply(DVec2::new(50.0, 0.0), 20.0);
        assert!((middle.height - 8.0).abs() < 1e-4);
        assert_eq!(middle.water, Some(9.5));
        // Far away, nothing changes.
        let far = features.apply(DVec2::new(50.0, 200.0), 20.0);
        assert_eq!(far.height, 20.0);
        assert_eq!(far.water, None);
    }

    #[test]
    fn banks_meet_the_ground_without_a_step() {
        let features = Features::new(vec![river()], Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut last = features.apply(DVec2::new(50.0, 0.0), 20.0).height;
        for k in 1..2000 {
            let h = features
                .apply(DVec2::new(50.0, f64::from(k) * 0.05), 20.0)
                .height;
            assert!((h - last).abs() < 0.2, "step at {}", f64::from(k) * 0.05);
            last = h;
        }
        assert_eq!(last, 20.0);
    }

    #[test]
    fn roads_cut_and_fill_to_their_level() {
        let road = FeatureLine {
            kind: FeatureKind::Road,
            depth: 0.0,
            ..river()
        };
        let features =
            Features::new(Vec::new(), Vec::new(), vec![road], vec![vec![0.6; 2]], Vec::new());
        let cut = features.apply(DVec2::new(50.0, 0.0), 30.0).height;
        let fill = features.apply(DVec2::new(50.0, 0.0), 0.0).height;
        assert!((cut - 9.75).abs() < 1e-4);
        assert!((fill - 9.25).abs() < 1e-4);
    }

    #[test]
    fn road_banks_are_tagged_by_kind_and_steepness() {
        let road = FeatureLine {
            kind: FeatureKind::Road,
            depth: 0.0,
            ..river()
        };
        // Three copies of the road far apart, with banks of 0.6 (31°), 1.5
        // (56°) and 3 (72°).
        let roads: Vec<FeatureLine> = [0.0, 1000.0, 2000.0]
            .iter()
            .map(|&z| FeatureLine {
                points: road.points.iter().map(|p| *p + DVec2::new(0.0, z)).collect(),
                ..road.clone()
            })
            .collect();
        let banks = vec![vec![0.6; 2], vec![1.5; 2], vec![3.0; 2]];
        let features = Features::new(Vec::new(), Vec::new(), roads, banks, Vec::new());
        // 5 m out from a road's edge (3 m from its centre), on high ground
        // (cut) or low ground (fill).
        let tags = |z: f64, ground: f32| features.apply(DVec2::new(50.0, z + 8.0), ground).tags;
        let (cut, fill) = (1000.0, -1000.0);
        assert_eq!(tags(0.0, cut), RoadTags::CUT);
        assert_eq!(tags(1000.0, cut), RoadTags::CUT | RoadTags::BLUFF);
        assert_eq!(tags(2000.0, cut), RoadTags::CUT | RoadTags::CLIFF);
        assert_eq!(tags(0.0, fill), RoadTags::EMBANKMENT);
        assert_eq!(tags(1000.0, fill), RoadTags::EMBANKMENT | RoadTags::BLUFF);
        assert_eq!(tags(2000.0, fill), RoadTags::RETAINING_WALL | RoadTags::CLIFF);
        // The road itself, and ground the bank doesn't reach.
        assert_eq!(features.apply(DVec2::new(50.0, 1.0), cut).tags, RoadTags::ROAD);
        assert_eq!(tags(2000.0, 9.5), RoadTags::NONE);
        assert_eq!(
            (RoadTags::RETAINING_WALL | RoadTags::CLIFF).names(),
            ["retaining_wall", "cliff"]
        );
    }

    #[test]
    fn steep_banks_are_flat_faces() {
        // A bank keeps its slope, within 5%, out to half its reach.
        for bank in [0.6, 2.0, 20.0] {
            let near = bank_slope(3.0 + 1.0, 3.0, bank).unwrap();
            let far = bank_slope(3.0 + 0.5 * BANK_REACH, 3.0, bank).unwrap();
            assert!((far / near - 1.0).abs() < 0.05, "bank {bank}: {near} then {far}");
        }
    }

    #[test]
    fn smoothing_keeps_ends_and_falling_levels() {
        let points = [
            DVec2::ZERO,
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(20.0, 10.0),
        ];
        let levels = [5.0, 4.0, 4.0, 1.0];
        let (p, l) = chaikin(&points, &levels, 2);
        assert_eq!(p[0], points[0]);
        assert_eq!(p[p.len() - 1], points[3]);
        assert_eq!(l[0], 5.0);
        assert_eq!(l[l.len() - 1], 1.0);
        assert!(l.windows(2).all(|w| w[1] <= w[0]));
    }
}
