//! Labels of the ground at one point: what kind of land it is
//! ([`Landform`]) and the facts that hold there besides ([`Flags`]).

use crate::carve::{FeatureKind, RoadTags};
use crate::measure::Measures;
use crate::scale::WorldScale;

/// The sine of the slope from which ground is a [`Landform::Cliff`]: 55°.
pub const CLIFF_SLOPE: f32 = 0.819_152;

/// The sine of the slope from which ground is a [`Landform::Slope`] rather
/// than a [`Landform::Plain`]: 20°.
pub const STEEP_SLOPE: f32 = 0.342_020;

/// The step, in metres, at which the ground's shape is read for
/// [`Landform::Ridge`], [`Landform::Peak`], [`Landform::Valley`] and
/// [`Landform::Basin`] at scale 1: the ground's height is compared with
/// the heights this far away along x and z and the diagonals. A terrain at
/// another [`WorldScale`] uses this times its `horizontal` stretch.
pub const SHAPE_STEP: f64 = 12.0;

/// How sharply, per metre, the ground must bend across at least one
/// direction to be a ridge, peak, valley or basin at scale 1 (see
/// [`SHAPE_STEP`]). A terrain at another [`WorldScale`] uses this times
/// `vertical / horizontal²`, so the same landscape stretched gets the same
/// labels.
pub const SHAPE_CURVATURE: f32 = 0.012;

/// How a terrain at one [`WorldScale`] reads the ground's shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shape {
    /// The stencil's step, in metres.
    pub step: f64,
    /// The bend, per metre, from which ground has a shape.
    pub curvature: f32,
}

impl Shape {
    /// The shape reading for a landscape stretched by `scale`.
    pub fn at(scale: WorldScale) -> Self {
        let WorldScale {
            horizontal,
            vertical,
        } = scale;
        Self {
            step: SHAPE_STEP * horizontal,
            curvature: (f64::from(SHAPE_CURVATURE) * vertical / (horizontal * horizontal)) as f32,
        }
    }
}

/// How far from the water's edge, in metres, a bank or shore reaches
/// ([`Landform::Riverbank`], [`Landform::Shore`], [`Flags::RIVER`],
/// [`Flags::POND`]).
pub const BANK_WIDTH: f32 = 8.0;

/// How high above the nearby water, in metres, ground can be and still be
/// a bank or shore.
pub const BANK_HEIGHT: f32 = 2.0;

/// How high above the nearby water, in metres, ground can be and still be
/// [`Flags::WET`].
pub const WET_HEIGHT: f32 = 1.0;

/// What kind of land a point is: one label per point, chosen from what the
/// terrain made there first (water, roads), then from how steep it is and
/// how it bends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Landform {
    /// Gentle ground with no marked shape.
    Plain,
    /// Ground steeper than [`STEEP_SLOPE`] with no marked shape.
    Slope,
    /// Ground at least as steep as [`CLIFF_SLOPE`], or a road bank of 65°
    /// or more.
    Cliff,
    /// Ground that bends down on both sides across one direction: a ridge
    /// or a spur.
    Ridge,
    /// Ground that bends down in every direction: a hilltop or summit.
    Peak,
    /// Ground that bends up on both sides across one direction: a valley
    /// floor, gully or hollow line.
    Valley,
    /// Ground that bends up in every direction: a hollow or bowl.
    Basin,
    /// Ground under a river.
    Riverbed,
    /// Ground under a pond.
    Pondbed,
    /// A road's surface.
    Road,
    /// Low ground beside a river, within [`BANK_WIDTH`] of its edge and
    /// [`BANK_HEIGHT`] of its surface.
    Riverbank,
    /// Low ground beside a pond, within [`BANK_WIDTH`] of its edge and
    /// [`BANK_HEIGHT`] of its surface.
    Shore,
}

impl Landform {
    /// Every landform, in the order they are declared.
    pub const ALL: [Self; 12] = [
        Self::Plain,
        Self::Slope,
        Self::Cliff,
        Self::Ridge,
        Self::Peak,
        Self::Valley,
        Self::Basin,
        Self::Riverbed,
        Self::Pondbed,
        Self::Road,
        Self::Riverbank,
        Self::Shore,
    ];

    /// The landform's name in lower case with underscores, such as
    /// `"plain"` or `"riverbed"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Slope => "slope",
            Self::Cliff => "cliff",
            Self::Ridge => "ridge",
            Self::Peak => "peak",
            Self::Valley => "valley",
            Self::Basin => "basin",
            Self::Riverbed => "riverbed",
            Self::Pondbed => "pondbed",
            Self::Road => "road",
            Self::Riverbank => "riverbank",
            Self::Shore => "shore",
        }
    }

    /// A colour to draw the landform with on a map, as sRGB bytes. Each
    /// landform has its own.
    pub fn colour(self) -> [u8; 3] {
        match self {
            Self::Plain => [150, 190, 100],
            Self::Slope => [190, 160, 90],
            Self::Cliff => [100, 90, 90],
            Self::Ridge => [235, 125, 55],
            Self::Peak => [215, 35, 35],
            Self::Valley => [40, 150, 150],
            Self::Basin => [125, 80, 175],
            Self::Riverbed => [30, 80, 210],
            Self::Pondbed => [20, 35, 130],
            Self::Road => [245, 220, 60],
            Self::Riverbank => [120, 205, 235],
            Self::Shore => [215, 200, 150],
        }
    }
}

/// Facts about a point that its [`Landform`] doesn't say, as a set: a
/// riverbank can also be wet, a cliff can be a road's bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(u8);

impl Flags {
    /// No flags.
    pub const NONE: Self = Self(0);
    /// Under a river's or pond's surface.
    pub const UNDERWATER: Self = Self(1);
    /// In a river, or within [`BANK_WIDTH`] of one when it is the nearest
    /// water.
    pub const RIVER: Self = Self(1 << 1);
    /// In a pond, or within [`BANK_WIDTH`] of one when it is the nearest
    /// water.
    pub const POND: Self = Self(1 << 2);
    /// On a road's surface.
    pub const ROAD: Self = Self(1 << 3);
    /// On a bank a road cut or built up beside it (see [`RoadTags`]).
    pub const ROAD_BANK: Self = Self(1 << 4);
    /// Above water, but less than [`WET_HEIGHT`] above the nearby water's
    /// surface.
    pub const WET: Self = Self(1 << 5);

    /// Every flag with its name, in the order the constants are listed.
    const NAMED: [(Self, &'static str); 6] = [
        (Self::UNDERWATER, "underwater"),
        (Self::RIVER, "river"),
        (Self::POND, "pond"),
        (Self::ROAD, "road"),
        (Self::ROAD_BANK, "road_bank"),
        (Self::WET, "wet"),
    ];

    /// Whether every flag of `other` is in `self`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any flag of `other` is in `self`.
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether there are no flags.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The flags' names, in the order the constants are listed:
    /// `"underwater"`, `"river"`, `"pond"`, `"road"`, `"road_bank"`,
    /// `"wet"`.
    pub fn names(self) -> Vec<&'static str> {
        Self::NAMED
            .into_iter()
            .filter(|&(flag, _)| self.contains(flag))
            .map(|(_, name)| name)
            .collect()
    }
}

impl std::ops::BitOr for Flags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Flags {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// The labels of one point (see [`Terrain::annotate`](crate::Terrain::annotate)).
///
/// On a border between two terrains, the labels that come from water and
/// roads, and the flags, are the same on both sides. Those that come from
/// slope and shape can differ in a thin strip along the border, because
/// each terrain reads its own ground past it: the two terrains meet in
/// height and slope on the border line, not beyond it. The strip widens
/// with the world's horizontal scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Labels {
    /// What kind of land the point is.
    pub landform: Landform,
    /// What else holds there.
    pub flags: Flags,
}

/// Everything the labels of a point are chosen from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Site {
    /// The water surface, if water covers the point.
    pub water: Option<f32>,
    /// What the roads made of the ground there.
    pub tags: RoadTags,
    /// The point's measures.
    pub measures: Measures,
    /// What the nearest water within reach is.
    pub water_kind: Option<FeatureKind>,
    /// The ground's bend across its most convex and most concave
    /// directions at the shape step, per metre, convex positive (see
    /// [`principal_bends`]).
    pub bends: [f32; 2],
}

/// The labels of a point, with shapes read as `shape` says.
pub(crate) fn classify(site: &Site, shape: Shape) -> Labels {
    let m = &site.measures;
    let above_water = m.height_above_water;
    let near_water = site.water.is_some() || m.water_distance <= BANK_WIDTH;

    // The facts that hold whatever the landform.
    let mut flags = Flags::NONE;
    if site.water.is_some() {
        flags |= Flags::UNDERWATER;
    }
    if near_water {
        flags |= match site.water_kind {
            Some(FeatureKind::Pond) => Flags::POND,
            Some(_) => Flags::RIVER,
            None => Flags::NONE,
        };
    }
    if site.tags.contains(RoadTags::ROAD) {
        flags |= Flags::ROAD;
    }
    let road_bank = [
        RoadTags::CUT,
        RoadTags::EMBANKMENT,
        RoadTags::RETAINING_WALL,
    ];
    if road_bank.iter().any(|&tag| site.tags.contains(tag)) {
        flags |= Flags::ROAD_BANK;
    }
    if site.water.is_none() && above_water.is_some_and(|h| h < WET_HEIGHT) {
        flags |= Flags::WET;
    }

    // The landform: what the terrain made first, then steepness, then
    // shape.
    let pond = site.water_kind == Some(FeatureKind::Pond);
    let landform = if site.water.is_some() {
        if pond {
            Landform::Pondbed
        } else {
            Landform::Riverbed
        }
    } else if flags.contains(Flags::ROAD) {
        Landform::Road
    } else if m.slope >= CLIFF_SLOPE
        || site.tags.contains(RoadTags::CLIFF)
        || site.tags.contains(RoadTags::RETAINING_WALL)
    {
        Landform::Cliff
    } else if near_water && above_water.is_some_and(|h| h <= BANK_HEIGHT) {
        if pond {
            Landform::Shore
        } else {
            Landform::Riverbank
        }
    } else if let Some(landform) = shape_of(site.bends, shape.curvature) {
        landform
    } else if m.slope >= STEEP_SLOPE {
        Landform::Slope
    } else {
        Landform::Plain
    };
    Labels { landform, flags }
}

/// The ridge, peak, valley or basin that ground bending by `bends` (most
/// convex first, see [`principal_bends`]) makes, if it bends by at least
/// `t` per metre in some direction.
fn shape_of([most, least]: [f32; 2], t: f32) -> Option<Landform> {
    if least >= t {
        Some(Landform::Peak)
    } else if most <= -t {
        Some(Landform::Basin)
    } else if most >= t || least <= -t {
        // Bent one way across and less along, or a saddle: the stronger
        // bend wins.
        Some(if most >= -least {
            Landform::Ridge
        } else {
            Landform::Valley
        })
    } else {
        None
    }
}

/// How ground of height `centre` bends at `step` metres, from the heights
/// `around` it that far away, in the order `+x, -x, +z, -z, (+x +z),
/// (+x -z), (-x +z), (-x -z)` (see [`shape_points`]): the bend across its
/// most convex direction, then across its most concave, per metre, convex
/// positive. They are the eigenvalues of the height's second derivatives,
/// negated, so a ridge falling by `k x² / 2` across it bends by `k` across
/// and 0 along.
pub(crate) fn principal_bends(centre: f32, around: [f32; 8], step: f64) -> [f32; 2] {
    let s2 = (step * step) as f32;
    let [px, nx, pz, nz, pp, pn, np, nn] = around;
    let xx = (px + nx - 2.0 * centre) / s2;
    let zz = (pz + nz - 2.0 * centre) / s2;
    let xz = (pp - pn - np + nn) / (4.0 * s2);
    // Eigenvalues of [[xx, xz], [xz, zz]].
    let mean = 0.5 * (xx + zz);
    let spread = (0.25 * (xx - zz) * (xx - zz) + xz * xz).sqrt();
    [-(mean - spread), -(mean + spread)]
}

/// The points `step` metres around `p` at which [`principal_bends`] reads
/// heights, in its order.
pub(crate) fn shape_points(p: glam::DVec2, step: f64) -> [glam::DVec2; 8] {
    let s = step;
    [
        (s, 0.0),
        (-s, 0.0),
        (0.0, s),
        (0.0, -s),
        (s, s),
        (s, -s),
        (-s, s),
        (-s, -s),
    ]
    .map(|(x, z)| p + glam::DVec2::new(x, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::MEASURE_REACH;
    use glam::DVec2;

    /// The bends of `height` at the origin, read at `step`.
    fn bends_at(height: impl Fn(f64, f64) -> f64, step: f64) -> [f32; 2] {
        let around = shape_points(DVec2::ZERO, step).map(|p| height(p.x, p.y) as f32);
        principal_bends(height(0.0, 0.0) as f32, around, step)
    }

    /// The bends of `height` at the origin at scale 1.
    fn bends_of(height: impl Fn(f64, f64) -> f64) -> [f32; 2] {
        bends_at(height, SHAPE_STEP)
    }

    /// A dry site far from water and roads with `slope` and `bends`.
    fn dry(slope: f32, bends: [f32; 2]) -> Site {
        Site {
            water: None,
            tags: RoadTags::NONE,
            measures: Measures {
                slope,
                curvature: 0.0,
                height_above_water: None,
                water_distance: MEASURE_REACH,
                road_distance: MEASURE_REACH,
            },
            water_kind: None,
            bends,
        }
    }

    /// The landform at `scale` of a dry site on the ground `height`, as
    /// steep as it is at the origin.
    fn landform_at(height: impl Fn(f64, f64) -> f64, scale: WorldScale) -> Landform {
        let d = 0.01;
        let gx = (height(d, 0.0) - height(-d, 0.0)) / (2.0 * d);
        let gz = (height(0.0, d) - height(0.0, -d)) / (2.0 * d);
        let slope = crate::measure::slope_of(glam::Vec2::new(gx as f32, gz as f32));
        let shape = Shape::at(scale);
        classify(&dry(slope, bends_at(&height, shape.step)), shape).landform
    }

    /// The landform at scale 1 of a dry site on the ground `height`.
    fn landform_of(height: impl Fn(f64, f64) -> f64) -> Landform {
        landform_at(height, WorldScale::default())
    }

    /// The labels at scale 1 of `site`.
    fn classify1(site: &Site) -> Labels {
        classify(site, Shape::at(WorldScale::default()))
    }

    #[test]
    fn stretched_ground_keeps_its_labels() {
        // A dome that is a peak at scale 1, and a gentle bump that is
        // plain, each stretched by (h, v): the same labels at that scale.
        let k = 1.5 * f64::from(SHAPE_CURVATURE);
        for (h, v) in [(2.0, 2.0), (0.5, 0.5), (3.0, 1.5)] {
            let scale = WorldScale {
                horizontal: h,
                vertical: v,
            };
            let dome = |x: f64, z: f64| -v * k * ((x / h).powi(2) + (z / h).powi(2)) / 2.0;
            let bump = |x: f64, z: f64| -v * k / 3.0 * ((x / h).powi(2) + (z / h).powi(2)) / 2.0;
            assert_eq!(landform_at(dome, scale), Landform::Peak, "{h} × {v}");
            assert_eq!(landform_at(bump, scale), Landform::Plain, "{h} × {v}");
        }
        // Stretching lengths alone flattens the bend: twice as wide reads
        // as plain at scale 1.
        assert_eq!(
            landform_of(|x, z| -k * (x * x + z * z) / 8.0),
            Landform::Plain
        );
    }

    #[test]
    fn bends_are_the_curvature_across_and_along() {
        let k = 0.01;
        // A ridge along z, turned 30°: bent across, flat along.
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let [most, least] = bends_of(|x, z| -k * (c * x + s * z).powi(2) / 2.0);
        assert!((most - k as f32).abs() < 1e-5, "{most}");
        assert!(least.abs() < 1e-5, "{least}");
        // A saddle: up along x, down along z.
        let [most, least] = bends_of(|x, z| k * (z * z - x * x) / 2.0);
        assert!((most - k as f32).abs() < 1e-5 && (least + k as f32).abs() < 1e-5);
        // Planes don't bend.
        let [most, least] = bends_of(|x, z| 5.0 + 0.3 * x - 0.1 * z);
        assert!(most.abs() < 1e-8 && least.abs() < 1e-8, "{most} {least}");
    }

    #[test]
    fn shapes_and_slopes_give_their_landforms() {
        let k = 1.5 * f64::from(SHAPE_CURVATURE);
        assert_eq!(landform_of(|x, z| 0.05 * x + 0.02 * z), Landform::Plain);
        assert_eq!(landform_of(|x, _| 0.5 * x), Landform::Slope);
        assert_eq!(landform_of(|x, _| 2.0 * x), Landform::Cliff);
        assert_eq!(landform_of(|x, _| -k * x * x / 2.0), Landform::Ridge);
        assert_eq!(
            landform_of(|x, z| -k * (x * x + z * z) / 2.0),
            Landform::Peak
        );
        assert_eq!(landform_of(|_, z| k * z * z / 2.0), Landform::Valley);
        assert_eq!(
            landform_of(|x, z| k * (x * x + z * z) / 2.0),
            Landform::Basin
        );
        // A spur on a hillside is still a ridge; a slight bend isn't.
        assert_eq!(
            landform_of(|x, z| 0.4 * z - k * x * x / 2.0),
            Landform::Ridge
        );
        assert_eq!(landform_of(|x, _| -k / 3.0 * x * x / 2.0), Landform::Plain);
        // A cliff beats its shape.
        assert_eq!(
            landform_of(|x, _| 2.0 * x - k * x * x / 2.0),
            Landform::Cliff
        );
    }

    #[test]
    fn water_and_roads_come_first() {
        let flat = dry(0.0, [0.0, 0.0]);
        let with = |water_distance, height_above_water, kind| Site {
            measures: Measures {
                water_distance,
                height_above_water,
                ..flat.measures
            },
            water_kind: kind,
            ..flat
        };
        let river = Some(FeatureKind::River);
        let pond = Some(FeatureKind::Pond);

        // In the water: its bed, under water.
        let bed = Site {
            water: Some(3.0),
            ..with(0.0, Some(-1.0), river)
        };
        let labels = classify1(&bed);
        assert_eq!(labels.landform, Landform::Riverbed);
        assert_eq!(labels.flags, Flags::UNDERWATER | Flags::RIVER);
        let pond_bed = Site {
            water_kind: pond,
            ..bed
        };
        assert_eq!(classify1(&pond_bed).landform, Landform::Pondbed);

        // Beside it and low: its bank, wet near the water.
        let bank = classify1(&with(3.0, Some(0.5), river));
        assert_eq!(bank.landform, Landform::Riverbank);
        assert_eq!(bank.flags, Flags::RIVER | Flags::WET);
        assert_eq!(
            classify1(&with(3.0, Some(1.5), pond)).landform,
            Landform::Shore
        );
        // High above it or far off: not a bank.
        assert_eq!(
            classify1(&with(3.0, Some(5.0), river)).landform,
            Landform::Plain
        );
        let far = classify1(&with(12.0, Some(0.5), river));
        assert_eq!((far.landform, far.flags), (Landform::Plain, Flags::WET));

        // Roads, and their steep banks.
        let road = Site {
            tags: RoadTags::ROAD,
            ..flat
        };
        assert_eq!(classify1(&road).landform, Landform::Road);
        let wall = Site {
            tags: RoadTags::RETAINING_WALL | RoadTags::CLIFF,
            ..flat
        };
        let labels = classify1(&wall);
        assert_eq!(labels.landform, Landform::Cliff);
        assert_eq!(labels.flags, Flags::ROAD_BANK);
    }

    #[test]
    fn every_landform_has_its_own_name_and_colour() {
        let names: std::collections::BTreeSet<_> = Landform::ALL.map(Landform::name).into();
        let colours: std::collections::BTreeSet<_> = Landform::ALL.map(Landform::colour).into();
        assert_eq!(names.len(), Landform::ALL.len());
        assert_eq!(colours.len(), Landform::ALL.len());
        let flags = Flags::UNDERWATER | Flags::WET;
        assert_eq!(flags.names(), ["underwater", "wet"]);
        assert!(flags.intersects(Flags::WET | Flags::ROAD));
        assert!(!flags.contains(Flags::WET | Flags::ROAD));
    }
}
