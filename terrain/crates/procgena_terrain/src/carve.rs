//! Cutting rivers, ponds and roads into the ground.

use std::collections::HashMap;

use glam::DVec2;

use crate::ports::BANK_REACH;

/// How steeply a river or pond bank rises, as rise over run.
const RIVER_BANK: f64 = 0.35;

/// How far a road may sit above or below the ground beneath it before its
/// banks start, in metres.
const ROAD_TOLERANCE: f64 = 0.25;

/// How steeply road cuttings and embankments slope, as rise over run.
const ROAD_BANK: f64 = 0.6;

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
    index: HashMap<(i64, i64), Vec<Item>>,
}

/// The ground after carving, and the water surface if there is water.
pub(crate) struct Carved {
    pub height: f32,
    pub water: Option<f32>,
}

/// The closest point on a feature line's segment: distance to it and the
/// level there.
fn segment_query(line: &FeatureLine, segment: usize, p: DVec2) -> (f64, f32) {
    let (a, b) = (line.points[segment], line.points[segment + 1]);
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (la, lb) = (line.levels[segment], line.levels[segment + 1]);
    (p.distance(a + ab * t), la + (lb - la) * t as f32)
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
fn road_tolerance(d: f64, half_width: f64) -> Option<f64> {
    let x = d - half_width;
    if x <= 0.0 {
        return Some(ROAD_TOLERANCE);
    }
    (x < BANK_REACH).then(|| ROAD_TOLERANCE + ROAD_BANK * x / (1.0 - (x / BANK_REACH).powi(2)))
}

impl Features {
    /// Gathers features and builds the spatial index.
    pub(crate) fn new(rivers: Vec<FeatureLine>, ponds: Vec<Pond>, roads: Vec<FeatureLine>) -> Self {
        let mut features = Self {
            rivers,
            ponds,
            roads,
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
        for item in items {
            if let Item::Road { line, segment } = *item {
                let road = &self.roads[line as usize];
                let (d, level) = segment_query(road, segment as usize, p);
                if let Some(e) = road_tolerance(d, f64::from(road.width) / 2.0) {
                    let level = f64::from(level);
                    let held = smooth_min(f64::from(height), level + e, ROAD_BLEND);
                    height = smooth_max(held, level - e, ROAD_BLEND) as f32;
                }
            }
        }
        Carved {
            height,
            water: water.filter(|&w| height < w),
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
        let features = Features::new(vec![river()], Vec::new(), Vec::new());
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
        let features = Features::new(vec![river()], Vec::new(), Vec::new());
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
        let features = Features::new(Vec::new(), Vec::new(), vec![road]);
        let cut = features.apply(DVec2::new(50.0, 0.0), 30.0).height;
        let fill = features.apply(DVec2::new(50.0, 0.0), 0.0).height;
        assert!((cut - 9.75).abs() < 1e-4);
        assert!((fill - 9.25).abs() < 1e-4);
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
