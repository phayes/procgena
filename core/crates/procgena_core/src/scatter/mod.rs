//! What every Scatter gets from the level above it, and the shared rules
//! for standing copies on the ground.
//!
//! A Scatter spreads many copies of one kind of asset (trees, sticks,
//! stones) over a stretch of ground. Every Scatter implements [`Scatter`]
//! and takes a [`ScatterInput`]: the [`Ground`], an optional [`Mask`], the
//! [`ScatterArea`], a seed, the [`AddressPrefix`] its copies' addresses
//! start with, and the [`Footprints`] of the Scatters above it.
//!
//! Every Scatter places in the same order, with the integer rules in
//! [`lattice`]: candidates on a [`Lattice`] fixed to the World, kept by
//! [`rank`](lattice::rank) below the local density; then dropped where
//! their footprint overlaps one above them; then [`steeper_than`] from one
//! ground sample, and the copy's footing with [`place`] as its
//! [`Placement`] says; then spaced out with [`keep_spaced`]; then any
//! patches of the Scatter's own. What a Scatter leaves for the Scatters
//! below is its footprints, or a [`MaskLayer`] stamped from them.

mod cache;
mod footprint;
pub mod lattice;
mod mask;
mod placing;

pub use cache::ChunkCache;
pub use footprint::{Disc, Footprints};
pub use lattice::{
    ChunkGrid, ChunkId, DEFAULT_SPACING_ROUNDS, Fixed2, FixedRect, Lattice, Rank, Spaced,
    keep_spaced,
};
pub use mask::{FOOTPRINT_TEXEL_SHIFT, MaskLayer};
pub use placing::{MaskSource, Reach, Scatter, ScatterCopy, ScatterError, ScatterSlot};

use glam::{DVec2, Mat3, Quat, Vec2, Vec3};

use crate::address::AddressPrefix;
use crate::ground::{Ground, GroundSample};

/// A rectangle of World ground to scatter over, around a centre.
///
/// Scatters give copy positions relative to [`centre`](Self::centre), in
/// metres (`x` is X, `y` is Z), so they stay exact far from the World
/// origin. Use [`world`](Self::world) to turn one back into a World
/// position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScatterArea {
    /// The middle of the area, as a World position.
    pub centre: DVec2,
    /// Width along X and depth along Z, in metres.
    pub size: Vec2,
}

impl ScatterArea {
    /// An area `size` metres across (X, then Z), around World position
    /// `centre`.
    pub fn new(centre: DVec2, size: Vec2) -> Self {
        Self { centre, size }
    }

    /// Returns the World position of `local`, a position relative to the
    /// centre.
    pub fn world(&self, local: Vec2) -> DVec2 {
        self.centre + local.as_dvec2()
    }

    /// Whether `local`, a position relative to the centre, lies inside the
    /// area or on its edge.
    pub fn contains(&self, local: Vec2) -> bool {
        let half = self.size * 0.5;
        local.x.abs() <= half.x && local.y.abs() <= half.y
    }

    /// The area in fixed point: from its low corner (included) up to its
    /// high corner (left out), each rounded to the nearest 1/256 m. A
    /// Scatter placed over the area places the copies whose lattice cell's
    /// corner lies inside these bounds.
    pub fn bounds(&self) -> FixedRect {
        let half = self.size.as_dvec2() * 0.5;
        FixedRect::new(
            Fixed2::from_world(self.centre - half),
            Fixed2::from_world(self.centre + half),
        )
    }
}

/// How much of a Scatter's density is allowed at each World position, from
/// 0 (none) to 1 (all of it).
///
/// A mask only thins a Scatter out. It does not promise that nothing stands
/// in a spot: a copy near a masked-out patch can still reach into it.
///
/// Any closure `Fn(DVec2) -> f32` that is `Send + Sync` can be used as a
/// mask.
pub trait Mask: Send + Sync {
    /// Returns the share of the density allowed at World position `p`.
    /// Values outside 0 to 1 are clamped by [`ScatterInput::density`], and
    /// NaN counts as 0.
    fn density(&self, p: DVec2) -> f32;
}

impl<F: Fn(DVec2) -> f32 + Send + Sync> Mask for F {
    fn density(&self, p: DVec2) -> f32 {
        self(p)
    }
}

/// What every Scatter gets from the level above it: where the ground is,
/// where copies may stand, which stretch of ground to fill, a seed, how its
/// copies are addressed, and what it must keep clear of.
///
/// The same input and options always give the same copies.
#[derive(Clone, Copy)]
pub struct ScatterInput<'a> {
    /// The ground copies stand on.
    pub ground: &'a dyn Ground,
    /// Thins the Scatter in places; `None` allows full density everywhere.
    pub mask: Option<&'a dyn Mask>,
    /// The stretch of ground to fill: the Scatter places the copies whose
    /// lattice cell's corner lies inside it (see [`ScatterArea::bounds`]).
    pub area: ScatterArea,
    /// Picks where copies stand and how each one varies. Different seeds
    /// give different copies over the same ground.
    pub seed: u32,
    /// What every copy's address starts with: the region, Ecotope and
    /// Scatter that placed it. [`AddressPrefix::ALONE`] for a Scatter built
    /// on its own.
    pub prefix: AddressPrefix,
    /// Footprints no copy may overlap, such as the trunks and boulders of
    /// the Scatters above this one. `None` keeps nothing clear.
    pub exclude: Option<&'a Footprints>,
}

impl<'a> ScatterInput<'a> {
    /// An input over `area` of `ground`, with no mask, no exclusions and the
    /// address prefix of a Scatter built on its own.
    pub fn new(ground: &'a dyn Ground, area: ScatterArea, seed: u32) -> Self {
        Self {
            ground,
            mask: None,
            area,
            seed,
            prefix: AddressPrefix::ALONE,
            exclude: None,
        }
    }

    /// The same input, thinned by `mask`.
    #[must_use]
    pub fn with_mask(self, mask: &'a dyn Mask) -> Self {
        Self {
            mask: Some(mask),
            ..self
        }
    }

    /// The same input, with copy addresses starting with `prefix`.
    #[must_use]
    pub fn with_prefix(self, prefix: AddressPrefix) -> Self {
        Self { prefix, ..self }
    }

    /// The same input, keeping every copy off `footprints`.
    #[must_use]
    pub fn with_exclusion(self, footprints: &'a Footprints) -> Self {
        Self {
            exclude: Some(footprints),
            ..self
        }
    }

    /// The same input over another area.
    #[must_use]
    pub fn over(self, area: ScatterArea) -> Self {
        Self { area, ..self }
    }

    /// The share of the density allowed at World position `p`, in 8 bits
    /// (0 to 255): 255 everywhere without a mask, 0 where the mask gives
    /// NaN. Scatters decide which candidates to keep from this value.
    pub fn local_density(&self, p: Fixed2) -> u8 {
        match self.mask {
            None => 255,
            Some(mask) => lattice::quantise(mask.density(p.to_world())),
        }
    }

    /// Whether any of `footprint` overlaps the footprints this input keeps
    /// clear.
    pub fn excluded(&self, footprint: &[Disc]) -> bool {
        self.exclude.is_some_and(|f| f.overlaps_any(footprint))
    }

    /// Returns the ground's height and slope at World position `p`.
    pub fn sample_at(&self, p: Fixed2) -> GroundSample {
        self.ground.sample(p.to_world())
    }

    /// Returns the share of the density allowed at `local` (relative to the
    /// area's centre), from 0 to 1. It is 1 everywhere without a mask, and 0
    /// where the mask gives NaN.
    pub fn density(&self, local: Vec2) -> f32 {
        let Some(mask) = self.mask else {
            return 1.0;
        };
        let d = mask.density(self.area.world(local));
        if d.is_nan() { 0.0 } else { d.clamp(0.0, 1.0) }
    }

    /// Returns the ground's height and slope at `local` (relative to the
    /// area's centre).
    pub fn sample(&self, local: Vec2) -> GroundSample {
        self.ground.sample(self.area.world(local))
    }
}

/// Whether the ground sample `s` is too steep for a slope limit of
/// `max_degrees`.
///
/// A limit of 90 degrees or more never rejects a measured slope. A NaN
/// gradient, where the ground can't tell its slope, always counts as too
/// steep.
pub fn steeper_than(s: &GroundSample, max_degrees: f32) -> bool {
    let rise = s.gradient.length_squared();
    if rise.is_nan() {
        return true;
    }
    if max_degrees >= 90.0 {
        return false;
    }
    let limit = max_degrees.to_radians().tan();
    rise > limit * limit
}

/// How a piece stands on uneven ground: how far it tilts, what it rests on,
/// how deep it sinks and how much bend under it is too much.
///
/// It says how copies are put down, not what they look like, so it is not
/// part of any options hash or specimen id. Assets declare their usual
/// placement, and a Scatter may override it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Placement {
    /// How far the piece tilts towards the ground's slope, from 0 (it stays
    /// upright, like a tree or a mushroom) to 1 (it lies along the ground,
    /// like a log). Values between tilt it partway: 0.5 tilts it by half the
    /// slope's angle.
    pub conform: f32,
    /// What the piece rests on.
    pub rest: Rest,
    /// How deep the piece sinks below where it rests, as a share of its
    /// [`size`](Footprint::size). 0 or more.
    pub embed: f32,
    /// How far the ground under the piece may lie from the plane fitted to
    /// it, as a share of its [`size`](Footprint::size), before the spot is
    /// rejected (a log bridging a gully, a boulder on a ridge). `None`
    /// never rejects.
    pub max_bend: Option<f32>,
}

impl Default for Placement {
    /// Upright, resting on the lowest ground, not sunk, any bend allowed.
    fn default() -> Self {
        Self {
            conform: 0.0,
            rest: Rest::Lowest,
            embed: 0.0,
            max_bend: None,
        }
    }
}

/// What a placed piece rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rest {
    /// The piece's lowest contact meets the lowest ground under its
    /// footprint, so nothing floats; on a slope its uphill side sinks in.
    #[default]
    Lowest,
    /// The piece sits on the plane fitted to the ground under it, lifted
    /// just enough that no contact is below the ground.
    Plane,
}

/// Where a piece touches flat ground, and how big it is, so [`place`] can
/// put it on uneven ground.
///
/// Points are in the piece's own space, scaled as it will be drawn, with Y
/// up and flat ground at `y = 0`, measured from the point that stands over
/// the spot being placed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Footprint {
    /// Points on the piece's underside that touch, or nearly touch, flat
    /// ground. A piece with none stands on its origin.
    pub contacts: Vec<Vec3>,
    /// More points (X and Z) where the ground is sampled to find its tilt,
    /// without resting on them, such as points either side of a thin stick
    /// so it still finds the slope across it.
    pub extra: Vec<Vec2>,
    /// The piece's size in metres, which [`Placement::embed`] and
    /// [`Placement::max_bend`] are shares of, such as its width.
    pub size: f32,
}

/// A piece put on the ground by [`place`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Turns the piece from its own space: the requested yaw, then the tilt.
    pub rotation: Quat,
    /// The height in metres at which to put the piece's origin, after
    /// [`rotation`](Self::rotation).
    pub height: f32,
    /// The upward unit normal of the plane fitted to the ground under the
    /// piece, whatever the tilt.
    pub normal: Vec3,
}

/// Puts a piece with `footprint`, turned by `yaw` radians about Y, on
/// `ground` at World position `at`, as `placement` says.
///
/// The ground is sampled under every contact, every extra point and the
/// piece's origin, and a plane is fitted to the contacts and extra points
/// (to the origin alone when there are none). The piece tilts by
/// [`Placement::conform`] of that plane's slope, rests as
/// [`Placement::rest`] says, then sinks by [`Placement::embed`]. Put the
/// piece's origin at `at` and [`Placed::height`], and turn it by
/// [`Placed::rotation`].
///
/// Returns `None` where any sample has no ground (NaN), or where one lies
/// further from the plane than [`Placement::max_bend`] allows. Check the
/// slope with [`steeper_than`] first: it is cheaper than placing.
pub fn place(
    ground: &dyn Ground,
    at: DVec2,
    yaw: f32,
    footprint: &Footprint,
    placement: &Placement,
) -> Option<Placed> {
    let turn = Quat::from_rotation_y(yaw);
    let height = |p: Vec3| ground.height(at + DVec2::new(f64::from(p.x), f64::from(p.z)));

    // Sample the ground under the turned contacts, extra points and origin.
    let turned = footprint.contacts.iter().map(|&c| turn * c).chain(
        footprint
            .extra
            .iter()
            .map(|e| turn * Vec3::new(e.x, 0.0, e.y)),
    );
    let mut samples: Vec<(Vec3, f32)> = Vec::new();
    for p in turned.chain(std::iter::once(Vec3::ZERO)) {
        let h = height(p);
        if !h.is_finite() {
            return None;
        }
        samples.push((p, h));
    }

    // Fit the plane, and reject ground that bends too far from it.
    let fitted = if samples.len() > 1 {
        &samples[..samples.len() - 1]
    } else {
        &samples[..]
    };
    let [a, b, c] = fit_plane(fitted);
    let normal = Vec3::new(-a, 1.0, -b).normalize();
    if let Some(max_bend) = placement.max_bend {
        let limit = max_bend * footprint.size;
        let bent = samples
            .iter()
            .any(|&(p, h)| (h - (a * p.x + b * p.z + c)).abs() > limit);
        if bent {
            return None;
        }
    }

    // Tilt partway from upright towards the plane.
    let tilt = Quat::IDENTITY.slerp(
        Quat::from_rotation_arc(Vec3::Y, normal),
        placement.conform.clamp(0.0, 1.0),
    );
    let rotation = tilt * turn;

    // Rest the tilted piece, then sink it.
    let origin_ground = samples[samples.len() - 1].1;
    let rested = if footprint.contacts.is_empty() {
        origin_ground
    } else {
        let mut lowest_ground = f32::INFINITY;
        let mut lowest_contact = f32::INFINITY;
        let mut lift = f32::NEG_INFINITY;
        for &contact in &footprint.contacts {
            let p = rotation * contact;
            let h = height(p);
            if !h.is_finite() {
                return None;
            }
            lowest_ground = lowest_ground.min(h);
            lowest_contact = lowest_contact.min(p.y);
            lift = lift.max(h - p.y);
        }
        match placement.rest {
            Rest::Lowest => lowest_ground.min(origin_ground) - lowest_contact,
            Rest::Plane => lift,
        }
    };
    Some(Placed {
        rotation,
        height: rested - placement.embed * footprint.size,
        normal,
    })
}

/// Fits a plane `h = a·x + b·z + c` to `samples` (points and their ground
/// heights) by least squares, returning `[a, b, c]`. The tiny diagonal term
/// keeps a single point or a straight line of points solvable, as a level
/// plane across it.
fn fit_plane(samples: &[(Vec3, f32)]) -> [f32; 3] {
    let mut m = Mat3::ZERO;
    let mut rhs = Vec3::ZERO;
    for &(p, h) in samples {
        let row = Vec3::new(p.x, p.z, 1.0);
        m += Mat3::from_cols(row * row.x, row * row.y, row * row.z);
        rhs += row * h;
    }
    m += Mat3::from_diagonal(Vec3::new(1e-6, 1e-6, 0.0));
    (m.inverse() * rhs).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FlatGround;

    fn sample(gradient: Vec2) -> GroundSample {
        GroundSample {
            height: 0.0,
            gradient,
        }
    }

    #[test]
    fn the_area_measures_from_its_centre() {
        let area = ScatterArea::new(DVec2::new(1.0e6, -2.0e6), Vec2::new(10.0, 4.0));
        assert_eq!(
            area.world(Vec2::new(1.5, -2.0)),
            DVec2::new(1.0e6 + 1.5, -2.0e6 - 2.0)
        );
        assert!(area.contains(Vec2::new(5.0, -2.0)));
        assert!(!area.contains(Vec2::new(5.01, 0.0)));
        assert!(!area.contains(Vec2::new(0.0, 2.01)));
    }

    #[test]
    fn the_mask_is_read_at_world_positions_and_clamped() {
        let area = ScatterArea::new(DVec2::new(100.0, 0.0), Vec2::splat(10.0));
        let input = ScatterInput::new(&FlatGround, area, 3);
        assert_eq!(input.density(Vec2::ZERO), 1.0);

        // Full density east of x = 100 in World, none west of it.
        let east = |p: DVec2| if p.x >= 100.0 { 2.0 } else { -1.0 };
        let masked = input.with_mask(&east);
        assert_eq!(masked.density(Vec2::new(1.0, 0.0)), 1.0);
        assert_eq!(masked.density(Vec2::new(-1.0, 0.0)), 0.0);

        let broken = |_: DVec2| f32::NAN;
        assert_eq!(input.with_mask(&broken).density(Vec2::ZERO), 0.0);
    }

    #[test]
    fn the_ground_is_sampled_at_world_positions() {
        let area = ScatterArea::new(DVec2::new(50.0, 20.0), Vec2::splat(10.0));
        let ramp = |p: DVec2| (p.x * 0.5) as f32;
        let input = ScatterInput::new(&ramp, area, 0);
        let s = input.sample(Vec2::new(2.0, 0.0));
        assert!((s.height - 26.0).abs() < 1e-5);
        assert!((s.gradient.x - 0.5).abs() < 1e-4);
    }

    #[test]
    fn slope_limits_compare_against_the_steepest_slope() {
        // A 1 in 1 rise is 45 degrees.
        let s = sample(Vec2::new(0.6, 0.8));
        assert!(steeper_than(&s, 44.0));
        assert!(!steeper_than(&s, 46.0));
        assert!(!steeper_than(&sample(Vec2::ZERO), 0.0));
    }

    #[test]
    fn ninety_degrees_is_no_limit_but_nan_still_rejects() {
        let cliff = sample(Vec2::new(1.0e6, 0.0));
        assert!(!steeper_than(&cliff, 90.0));
        assert!(!steeper_than(&cliff, 120.0));
        let unknown = sample(Vec2::new(f32::NAN, 0.0));
        assert!(steeper_than(&unknown, 90.0));
        assert!(steeper_than(&unknown, 30.0));
    }

    /// A ramp rising 0.5 m per metre along X: about 26.6 degrees.
    fn ramp(p: DVec2) -> f32 {
        (0.5 * p.x) as f32
    }

    /// A box 2 m long along X and 1 m wide, on its four corners, 2 m in size.
    fn slab() -> Footprint {
        Footprint {
            contacts: vec![
                Vec3::new(-1.0, 0.0, -0.5),
                Vec3::new(1.0, 0.0, -0.5),
                Vec3::new(-1.0, 0.0, 0.5),
                Vec3::new(1.0, 0.0, 0.5),
            ],
            extra: Vec::new(),
            size: 2.0,
        }
    }

    #[test]
    fn a_piece_on_flat_ground_stays_level_and_keeps_its_yaw() {
        let lying = placement(1.0, Rest::Plane);
        let p = place(&FlatGround, DVec2::new(1.0e6, 3.0), 0.7, &slab(), &lying).unwrap();
        assert!((p.normal - Vec3::Y).length() < 1e-5);
        assert!(p.height.abs() < 1e-5);
        assert!((p.rotation * Vec3::X - Quat::from_rotation_y(0.7) * Vec3::X).length() < 1e-5);
    }

    #[test]
    fn only_the_contacts_and_extra_points_set_the_tilt() {
        // A piece whose contacts lie 1 m and 2 m to one side of its origin,
        // on curved ground. The plane through the two contacts rises 3 m
        // per metre; the ground under the origin must not pull it.
        let side = Footprint {
            contacts: vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
            extra: Vec::new(),
            size: 2.0,
        };
        let curve = |p: DVec2| (p.x * p.x) as f32;
        let p = place(
            &curve,
            DVec2::ZERO,
            0.0,
            &side,
            &placement(1.0, Rest::Plane),
        )
        .unwrap();
        assert!((p.normal - Vec3::new(-3.0, 1.0, 0.0).normalize()).length() < 1e-3);
    }

    #[test]
    fn extra_points_find_the_slope_across_a_thin_piece() {
        // A stick along X on ground that rises along Z.
        let stick = Footprint {
            contacts: vec![Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            extra: vec![Vec2::new(0.0, -0.2), Vec2::new(0.0, 0.2)],
            size: 2.0,
        };
        let ramp = |p: DVec2| (0.5 * p.y) as f32;
        let p = place(
            &ramp,
            DVec2::ZERO,
            0.0,
            &stick,
            &placement(1.0, Rest::Plane),
        )
        .unwrap();
        assert!((p.normal - Vec3::new(0.0, 1.0, -0.5).normalize()).length() < 1e-3);
    }

    fn placement(conform: f32, rest: Rest) -> Placement {
        Placement {
            conform,
            rest,
            ..Placement::default()
        }
    }

    /// The angle in degrees between the piece's up and World up.
    fn tilt_degrees(placed: &Placed) -> f32 {
        (placed.rotation * Vec3::Y)
            .angle_between(Vec3::Y)
            .to_degrees()
    }

    #[test]
    fn an_upright_piece_stays_vertical_and_rests_on_the_low_side() {
        let p = place(
            &ramp,
            DVec2::ZERO,
            0.0,
            &slab(),
            &placement(0.0, Rest::Lowest),
        )
        .unwrap();
        assert!(tilt_degrees(&p) < 1e-3);
        // The downhill corners at x = -1 sit on the ground at -0.5.
        assert!((p.height + 0.5).abs() < 1e-5, "{}", p.height);
        assert!((p.normal - Vec3::new(-0.5, 1.0, 0.0).normalize()).length() < 1e-4);
    }

    #[test]
    fn a_fully_conforming_piece_follows_the_plane() {
        let p = place(
            &ramp,
            DVec2::new(3.0, 1.0),
            0.4,
            &slab(),
            &placement(1.0, Rest::Plane),
        )
        .unwrap();
        let slope = 0.5f32.atan().to_degrees();
        assert!((tilt_degrees(&p) - slope).abs() < 1e-3);
        // Every corner lies on the ramp.
        for c in slab().contacts {
            let q = p.rotation * c + Vec3::Y * p.height;
            let ground = ramp(DVec2::new(3.0 + f64::from(q.x), 1.0 + f64::from(q.z)));
            assert!((q.y - ground).abs() < 1e-3, "{} vs {ground}", q.y);
        }
    }

    #[test]
    fn a_partway_piece_tilts_by_its_share_of_the_slope() {
        let p = place(
            &ramp,
            DVec2::ZERO,
            0.0,
            &slab(),
            &placement(0.3, Rest::Lowest),
        )
        .unwrap();
        let slope = 0.5f32.atan().to_degrees();
        assert!(
            (tilt_degrees(&p) - 0.3 * slope).abs() < 1e-3,
            "{}",
            tilt_degrees(&p)
        );
        // It still leans downhill, about the slope's own axis.
        assert!((p.rotation * Vec3::Y).x < 0.0);
    }

    #[test]
    fn plane_rest_sits_higher_than_lowest_rest_for_an_upright_piece() {
        let lowest = place(
            &ramp,
            DVec2::ZERO,
            0.0,
            &slab(),
            &placement(0.0, Rest::Lowest),
        );
        let plane = place(
            &ramp,
            DVec2::ZERO,
            0.0,
            &slab(),
            &placement(0.0, Rest::Plane),
        );
        // Lowest: the downhill corners touch (-0.5). Plane: lifted until the
        // uphill corners are out of the ground (+0.5).
        assert!((lowest.unwrap().height + 0.5).abs() < 1e-5);
        assert!((plane.unwrap().height - 0.5).abs() < 1e-5);
    }

    #[test]
    fn embed_sinks_the_piece_by_a_share_of_its_size() {
        let flat = Placement {
            embed: 0.25,
            ..placement(1.0, Rest::Plane)
        };
        let p = place(&FlatGround, DVec2::ZERO, 0.0, &slab(), &flat).unwrap();
        assert!((p.height + 0.5).abs() < 1e-5, "{}", p.height);
    }

    #[test]
    fn too_much_bend_is_rejected() {
        // A V-shaped gully 0.5 m deep under the middle of the slab.
        let gully = |p: DVec2| (p.x.abs() * 0.5) as f32;
        let strict = Placement {
            max_bend: Some(0.1),
            ..placement(1.0, Rest::Plane)
        };
        assert!(place(&gully, DVec2::ZERO, 0.0, &slab(), &strict).is_none());
        let loose = Placement {
            max_bend: Some(0.3),
            ..strict
        };
        assert!(place(&gully, DVec2::ZERO, 0.0, &slab(), &loose).is_some());
        let any = Placement {
            max_bend: None,
            ..strict
        };
        assert!(place(&gully, DVec2::ZERO, 0.0, &slab(), &any).is_some());
    }

    #[test]
    fn missing_ground_is_rejected() {
        let edge = |p: DVec2| if p.x > 0.5 { f32::NAN } else { 0.0 };
        let p = place(&edge, DVec2::ZERO, 0.0, &slab(), &Placement::default());
        assert!(p.is_none());
        let hole = |p: DVec2| if p.length() < 0.1 { f32::NAN } else { 0.0 };
        let p = place(&hole, DVec2::ZERO, 0.0, &slab(), &Placement::default());
        assert!(p.is_none(), "the origin's ground counts too");
    }

    #[test]
    fn a_piece_without_contacts_stands_on_its_origin() {
        let pin = Footprint {
            size: 1.0,
            ..Footprint::default()
        };
        let p = place(
            &ramp,
            DVec2::new(2.0, 0.0),
            0.0,
            &pin,
            &Placement::default(),
        )
        .unwrap();
        assert!((p.height - 1.0).abs() < 1e-5);
        assert!(tilt_degrees(&p) < 1e-3);
    }
}
