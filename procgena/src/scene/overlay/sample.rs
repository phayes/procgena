//! What the segmentation overlay shows, sampled on the CPU over a grid of
//! texels, coloured, and described for the legend and the hover readout.
//!
//! Nothing here touches Bevy's renderer: the colours are RGBA8 texels in
//! sRGB, ready to put in an image.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::math::DVec2;
use procgena_terrain::{
    FeatureLevel, Labels, Landform, MEASURE_REACH, Measures, RegionId, SampleDetail, Terrain,
    WATER_HEIGHT_REACH, WorldError, WorldTerrain,
};

/// One way of colouring the land: an overlay mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// Each region in its own colour, with its outline.
    Regions,
    /// [`Measures::slope`].
    Slope,
    /// [`Measures::curvature`].
    Curvature,
    /// [`Measures::height_above_water`].
    HeightAboveWater,
    /// [`Measures::water_distance`].
    WaterDistance,
    /// [`Measures::road_distance`].
    RoadDistance,
    /// The [`Landform`] at each texel, with its [`Flags`] in the readout.
    Landform,
}

/// The values an overlay is coloured from. Overlays of one family share
/// their samples, so switching between them only recolours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// The region holding each texel.
    Regions,
    /// The [`Measures`] at each texel.
    Measures,
    /// The [`Labels`] at each texel.
    Labels,
}

impl Overlay {
    /// Every overlay, in the order the picker lists them.
    pub const ALL: [Self; 7] = [
        Self::Regions,
        Self::Landform,
        Self::Slope,
        Self::Curvature,
        Self::HeightAboveWater,
        Self::WaterDistance,
        Self::RoadDistance,
    ];

    /// The name shown in the picker and the legend.
    pub fn label(self) -> &'static str {
        match self {
            Self::Regions => "Regions",
            Self::Slope => "Slope",
            Self::Curvature => "Curvature",
            Self::HeightAboveWater => "Height above water",
            Self::WaterDistance => "Water distance",
            Self::RoadDistance => "Road distance",
            Self::Landform => "Landform",
        }
    }

    /// The values this overlay is coloured from.
    pub fn family(self) -> Family {
        match self {
            Self::Regions => Family::Regions,
            Self::Landform => Family::Labels,
            Self::Slope
            | Self::Curvature
            | Self::HeightAboveWater
            | Self::WaterDistance
            | Self::RoadDistance => Family::Measures,
        }
    }

    /// The colour of one texel's `value`, as sRGB with alpha.
    ///
    /// # Panics
    ///
    /// If `value` is of another family.
    fn colour(self, value: &Value) -> [u8; 4] {
        let rgb = match (self, value) {
            (Self::Regions, Value::Region(id)) => region_colour(*id),
            (Self::Landform, Value::Labels(Some(labels))) => labels.landform.colour(),
            (_, Value::Measures(None) | Value::Labels(None)) => NONE,
            (_, Value::Measures(Some(m))) => match self {
                Self::Slope => ramp(&SEQUENTIAL, m.slope),
                Self::Curvature => ramp(&DIVERGING, 0.5 + 0.5 * m.curvature / CURVATURE_RANGE),
                Self::HeightAboveWater => match m.height_above_water {
                    None => NONE,
                    Some(h) if h < 0.0 => UNDER_WATER,
                    Some(h) => ramp(&SEQUENTIAL, h / WATER_HEIGHT_REACH),
                },
                Self::WaterDistance => ramp(&NEAR_FAR, m.water_distance / MEASURE_REACH),
                Self::RoadDistance => ramp(&NEAR_FAR, m.road_distance / MEASURE_REACH),
                Self::Regions | Self::Landform => {
                    panic!("{self:?} can't colour measures")
                }
            },
            _ => panic!("{self:?} can't colour {value:?}"),
        };
        [rgb[0], rgb[1], rgb[2], 255]
    }

    /// What one texel's `value` means, for the hover readout, such as
    /// "slope 0.42 (25°)".
    pub fn describe(self, value: &Value) -> String {
        match (self, value) {
            (Self::Regions, Value::Region(id)) => format!("region {}", id.0),
            (Self::Landform, Value::Labels(Some(labels))) => {
                let flags = labels.flags.names();
                if flags.is_empty() {
                    format!("landform {}", labels.landform.name())
                } else {
                    format!("landform {} ({})", labels.landform.name(), flags.join(", "))
                }
            }
            (_, Value::Measures(None) | Value::Labels(None)) => "no ground".to_string(),
            (_, Value::Measures(Some(m))) => match self {
                Self::Slope => {
                    format!("slope {:.2} ({:.0}°)", m.slope, m.slope.asin().to_degrees())
                }
                Self::Curvature => format!("curvature {:+.4} per m", m.curvature),
                Self::HeightAboveWater => match m.height_above_water {
                    None => format!("no water within {WATER_HEIGHT_REACH:.0} m"),
                    Some(h) => format!("{h:+.1} m above water"),
                },
                Self::WaterDistance => distance_text("water", m.water_distance),
                Self::RoadDistance => distance_text("road", m.road_distance),
                Self::Regions | Self::Landform => {
                    format!("{self:?} can't describe measures")
                }
            },
            _ => format!("{self:?} can't describe {value:?}"),
        }
    }

    /// The legend: what each colour means.
    pub fn legend(self) -> Legend {
        let ramp_stops = |stops: &[[u8; 3]]| stops.to_vec();
        match self {
            Self::Regions => Legend::Swatches(vec![
                (
                    region_colour(RegionId(1)),
                    "each region its own colour".into(),
                ),
                (OUTLINE, "region outline".into()),
            ]),
            Self::Slope => Legend::Ramp {
                stops: ramp_stops(&SEQUENTIAL),
                low: "flat".into(),
                high: "wall (sine 1)".into(),
                extra: vec![],
            },
            Self::Curvature => Legend::Ramp {
                stops: ramp_stops(&DIVERGING),
                low: format!("concave −{CURVATURE_RANGE} per m"),
                high: format!("convex +{CURVATURE_RANGE} per m"),
                extra: vec![],
            },
            Self::HeightAboveWater => Legend::Ramp {
                stops: ramp_stops(&SEQUENTIAL),
                low: "0 m".into(),
                high: format!("{WATER_HEIGHT_REACH:.0} m"),
                extra: vec![
                    (UNDER_WATER, "under water".into()),
                    (NONE, format!("no water within {WATER_HEIGHT_REACH:.0} m")),
                ],
            },
            Self::WaterDistance | Self::RoadDistance => Legend::Ramp {
                stops: ramp_stops(&NEAR_FAR),
                low: "0 m".into(),
                high: format!("{MEASURE_REACH:.0} m or more"),
                extra: vec![],
            },
            Self::Landform => Legend::Swatches(
                Landform::ALL
                    .iter()
                    .map(|l| (l.colour(), l.name().to_string()))
                    .collect(),
            ),
        }
    }
}

impl Family {
    /// What sampling one texel of this family costs, in words, for the
    /// legend.
    pub fn cost(self) -> &'static str {
        match self {
            Self::Regions => "a region lookup per texel",
            Self::Measures => "a height, gradient and four more heights per texel",
            Self::Labels => "the measures and eight more heights for the shape per texel",
        }
    }

    /// How many texels a side of an area gets: regions finer, to draw
    /// their outlines.
    pub fn texels(self) -> u32 {
        match self {
            Self::Regions => 65,
            Self::Measures | Self::Labels => 33,
        }
    }
}

/// What a legend shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Legend {
    /// One swatch per colour, with what it means.
    Swatches(Vec<([u8; 3], String)>),
    /// A colour ramp from `low` to `high`, with `extra` swatches for
    /// values off the ramp.
    Ramp {
        /// The ramp's colours, evenly spaced from low to high.
        stops: Vec<[u8; 3]>,
        /// What the low end means.
        low: String,
        /// What the high end means.
        high: String,
        /// Colours off the ramp, with what they mean.
        extra: Vec<([u8; 3], String)>,
    },
}

/// One texel's sampled value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// The region holding it.
    Region(RegionId),
    /// The measures, `None` where there is no ground.
    Measures(Option<Measures>),
    /// The labels, `None` where the ground has none.
    Labels(Option<Labels>),
}

/// Ground the overlay can sample: its regions, measures and labels.
pub trait OverlayGround: Sync {
    /// The region holding `p` (x and z, metres).
    fn region(&self, p: DVec2) -> RegionId;

    /// The ground's height at `p` from the ground read at feature level
    /// `level`, NaN where there is no ground.
    fn height(&self, p: DVec2, level: FeatureLevel) -> f32;

    /// The measures at `p` from the ground read at feature level `level`,
    /// `None` where there is no ground.
    fn measures(&self, p: DVec2, level: FeatureLevel) -> Option<Measures>;

    /// The labels at `p` from the ground read at feature level `level`,
    /// `None` where the ground has none.
    fn labels(&self, p: DVec2, level: FeatureLevel) -> Option<Labels>;
}

/// The terrain of the region of `world` holding `p`, at the tier the
/// world reads at `level`.
fn world_terrain(
    world: &WorldTerrain,
    p: DVec2,
    level: FeatureLevel,
) -> Result<Arc<Terrain>, WorldError> {
    world.terrain_tier(world.region_at(p), world.tier_levels().tier(level))
}

impl OverlayGround for WorldTerrain {
    fn region(&self, p: DVec2) -> RegionId {
        self.region_at(p)
    }

    /// The height of the region holding `p`, at the tier read at `level`;
    /// NaN where the region can't be built.
    fn height(&self, p: DVec2, level: FeatureLevel) -> f32 {
        world_terrain(self, p, level).map_or(f32::NAN, |terrain| terrain.height(p))
    }

    /// The measures of the region holding `p`, at the tier read at
    /// `level`; `None` where the region can't be built.
    fn measures(&self, p: DVec2, level: FeatureLevel) -> Option<Measures> {
        let terrain = world_terrain(self, p, level).ok()?;
        terrain.query(p, SampleDetail::Measures).measures
    }

    /// The labels of the region holding `p`, at the tier read at `level`;
    /// `None` where the region can't be built.
    fn labels(&self, p: DVec2, level: FeatureLevel) -> Option<Labels> {
        let terrain = world_terrain(self, p, level).ok()?;
        Some(terrain.annotate(p))
    }
}

impl OverlayGround for Terrain {
    fn region(&self, _: DVec2) -> RegionId {
        self.region().id
    }

    /// The terrain's own height, whatever level is asked for.
    fn height(&self, p: DVec2, _: FeatureLevel) -> f32 {
        Terrain::height(self, p)
    }

    /// The terrain's own measures, whatever level is asked for.
    fn measures(&self, p: DVec2, _: FeatureLevel) -> Option<Measures> {
        self.query(p, SampleDetail::Measures).measures
    }

    /// The terrain's own labels, whatever level is asked for.
    fn labels(&self, p: DVec2, _: FeatureLevel) -> Option<Labels> {
        Some(self.annotate(p))
    }
}

/// One family's values over a square area: `size` texels along each side,
/// the outer ones on its edges, row by row along x.
#[derive(Debug, Clone)]
pub struct AreaValues {
    /// The family sampled.
    pub family: Family,
    /// Texels along each side.
    pub size: u32,
    /// The values, row by row along x; regions have a ring of one texel
    /// beyond the area on every side, so outlines meet across areas.
    values: Vec<Value>,
    /// How long sampling took.
    pub cost: Duration,
}

impl AreaValues {
    /// Samples `family` over the square from `min` with side `side`, from
    /// `ground` read at feature level `level`.
    pub fn sample(
        family: Family,
        ground: &(impl OverlayGround + ?Sized),
        min: DVec2,
        side: f64,
        level: FeatureLevel,
    ) -> Self {
        let start = Instant::now();
        let size = family.texels();
        let step = side / f64::from(size - 1);
        let ring = i64::from(family == Family::Regions);
        let n = i64::from(size);
        let mut values = Vec::with_capacity(((n + 2 * ring) * (n + 2 * ring)) as usize);
        for j in -ring..n + ring {
            for i in -ring..n + ring {
                let p = min + DVec2::new(i as f64, j as f64) * step;
                values.push(sample_point(family, ground, p, level));
            }
        }
        Self {
            family,
            size,
            values,
            cost: start.elapsed(),
        }
    }

    /// The value at texel `(i, j)`, which may be one texel outside the
    /// area for regions.
    fn at(&self, i: i64, j: i64) -> &Value {
        let ring = i64::from(self.family == Family::Regions);
        let row = i64::from(self.size) + 2 * ring;
        &self.values[((j + ring) * row + i + ring) as usize]
    }

    /// The texels coloured by `overlay`, as sRGB with alpha, row by row
    /// along x.
    ///
    /// # Panics
    ///
    /// If `overlay` is of another family.
    pub fn colours(&self, overlay: Overlay) -> Vec<[u8; 4]> {
        assert_eq!(
            overlay.family(),
            self.family,
            "{overlay:?} is of another family"
        );
        let n = i64::from(self.size);
        let mut out = Vec::with_capacity((n * n) as usize);
        for j in 0..n {
            for i in 0..n {
                let value = self.at(i, j);
                let edge = overlay == Overlay::Regions
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|&(di, dj)| self.at(i + di, j + dj) != value);
                out.push(if edge {
                    [OUTLINE[0], OUTLINE[1], OUTLINE[2], 255]
                } else {
                    overlay.colour(value)
                });
            }
        }
        out
    }
}

/// `family`'s value at `p`.
pub fn sample_point(
    family: Family,
    ground: &(impl OverlayGround + ?Sized),
    p: DVec2,
    level: FeatureLevel,
) -> Value {
    match family {
        Family::Regions => Value::Region(ground.region(p)),
        Family::Measures => Value::Measures(ground.measures(p, level)),
        Family::Labels => Value::Labels(ground.labels(p, level)),
    }
}

/// The distance readout, such as "road 12 m away".
fn distance_text(what: &str, distance: f32) -> String {
    if distance <= 0.0 {
        format!("on the {what}")
    } else if distance >= MEASURE_REACH {
        format!("{what} {MEASURE_REACH:.0} m or more away")
    } else {
        format!("{what} {distance:.0} m away")
    }
}

/// The ± curvature, per metre, at the ends of the curvature ramp.
pub const CURVATURE_RANGE: f32 = 0.02;

/// Where there is nothing to show.
const NONE: [u8; 3] = [110, 110, 110];
/// Ground under water.
const UNDER_WATER: [u8; 3] = [25, 70, 190];
/// Region outlines.
const OUTLINE: [u8; 3] = [15, 15, 15];

/// Low to high: dark purple through teal to yellow.
const SEQUENTIAL: [[u8; 3]; 5] = [
    [68, 1, 84],
    [59, 82, 139],
    [33, 145, 140],
    [94, 201, 98],
    [253, 231, 37],
];
/// Near to far: bright to dark, so what is near stands out.
const NEAR_FAR: [[u8; 3]; 5] = [
    [253, 231, 37],
    [94, 201, 98],
    [33, 145, 140],
    [59, 82, 139],
    [68, 1, 84],
];
/// Negative through zero (white) to positive: blue to red.
const DIVERGING: [[u8; 3]; 5] = [
    [33, 102, 172],
    [146, 197, 222],
    [247, 247, 247],
    [244, 165, 130],
    [178, 24, 43],
];

/// The colour `t` (clamped to 0..1) along `stops`, mixed in sRGB.
fn ramp(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let x = t * (stops.len() - 1) as f32;
    let k = (x.floor() as usize).min(stops.len() - 2);
    let f = x - k as f32;
    std::array::from_fn(|c| {
        let (a, b) = (f32::from(stops[k][c]), f32::from(stops[k + 1][c]));
        (a + (b - a) * f).round() as u8
    })
}

/// A colour for the `index`th thing of a kind: hues stepped by the golden
/// angle, so neighbours differ.
fn categorical(index: u64) -> [u8; 3] {
    let hue = (index as f64 * 0.618_033_988_75).fract() as f32;
    hsl(hue, 0.65, 0.55)
}

/// The colour of `region`.
fn region_colour(region: RegionId) -> [u8; 3] {
    // Mix the id so neighbouring ids, which are often neighbouring
    // regions, get unrelated hues.
    categorical(region.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40)
}

/// An sRGB colour from hue, saturation and lightness, each 0 to 1.
fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h6 = h * 6.0;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use procgena_terrain::{
        Elevation, MIN_EDGE, RegionGraphOptions, TerrainPreset, WorldTerrainOptions,
    };

    use super::*;

    /// A world of small rolling-hill regions, so an area crosses borders.
    fn world() -> WorldTerrain {
        let options = WorldTerrainOptions {
            regions: RegionGraphOptions {
                size: 2000.0,
                min_edge: MIN_EDGE,
                ..RegionGraphOptions::default()
            },
            ..WorldTerrainOptions::default()
        };
        let source =
            |_: RegionId, _: DVec2, _: &Elevation| TerrainPreset::rolling_hills().terrain.clone();
        WorldTerrain::new(options, Arc::new(source)).unwrap()
    }

    #[test]
    fn every_overlay_is_listed_once_with_its_own_label() {
        let mut ids: Vec<&str> = Overlay::ALL.iter().map(|o| o.label()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Overlay::ALL.len());
    }

    #[test]
    fn regions_are_outlined_where_they_meet() {
        let world = world();
        let values = AreaValues::sample(
            Family::Regions,
            &world,
            DVec2::splat(-2000.0),
            4000.0,
            FeatureLevel::COARSEST,
        );
        let colours = values.colours(Overlay::Regions);
        let outline = [OUTLINE[0], OUTLINE[1], OUTLINE[2], 255];
        let edges = colours.iter().filter(|&&c| c == outline).count();
        assert!(edges > 0, "regions of 2 km meet in a 4 km square");
        assert!(edges < colours.len() / 4, "outlines are thin");
    }

    #[test]
    fn readout_and_colours_follow_the_measures() {
        let world = world();
        let p = DVec2::new(37.0, -12.0);
        let value = sample_point(Family::Measures, &world, p, FeatureLevel::FINEST);
        let measures = world
            .terrain_at(p)
            .unwrap()
            .query(p, SampleDetail::Measures)
            .measures
            .unwrap();
        assert_eq!(value, Value::Measures(Some(measures)));
        let slope = Overlay::Slope.describe(&value);
        assert!(
            slope.starts_with(&format!("slope {:.2}", measures.slope)),
            "{slope}"
        );
        assert_eq!(
            Overlay::Slope.colour(&value)[..3],
            ramp(&SEQUENTIAL, measures.slope)
        );
        let region = sample_point(Family::Regions, &world, p, FeatureLevel::FINEST);
        assert_eq!(
            Overlay::Regions.describe(&region),
            format!("region {}", world.region_at(p).0)
        );
    }

    #[test]
    fn landforms_are_the_terrains_own_labels() {
        let world = world();
        let p = DVec2::new(37.0, -12.0);
        let value = sample_point(Family::Labels, &world, p, FeatureLevel::COARSEST);
        let labels = world
            .terrain_tier(world.region_at(p), procgena_terrain::FeatureTier::Rough)
            .unwrap()
            .annotate(p);
        assert_eq!(value, Value::Labels(Some(labels)));
        assert!(
            Overlay::Landform
                .describe(&value)
                .starts_with(&format!("landform {}", labels.landform.name()))
        );
        assert_eq!(
            Overlay::Landform.colour(&value)[..3],
            labels.landform.colour()
        );
        let Legend::Swatches(swatches) = Overlay::Landform.legend() else {
            panic!("landforms are listed one by one");
        };
        assert_eq!(swatches.len(), Landform::ALL.len());
    }

    #[test]
    fn ramps_reach_both_ends_and_clamp() {
        assert_eq!(ramp(&SEQUENTIAL, 0.0), SEQUENTIAL[0]);
        assert_eq!(ramp(&SEQUENTIAL, 1.0), SEQUENTIAL[4]);
        assert_eq!(ramp(&SEQUENTIAL, 7.0), SEQUENTIAL[4]);
        assert_eq!(ramp(&SEQUENTIAL, f32::NAN), SEQUENTIAL[0]);
        assert_eq!(ramp(&DIVERGING, 0.5), DIVERGING[2]);
    }
}
