//! Rests a piece on flat ground in three steps. First it turns the piece,
//! without bending it, onto a stable resting side. Then it flattens and
//! sags the stems in that position. Last it moves the piece so it sits on
//! the ground at `y = 0`.
//!
//! The centre of mass is the piece's balance point. A piece is stable when
//! that point is as low as it can get, the way a dropped log rolls until it
//! can't fall any further. The search, which follows `procgena_stone`'s
//! placement, tries many "down" directions around the piece. Starting from
//! the generated position, it moves to neighbouring directions that lower
//! the centre of mass until none does.

use std::f32::consts::{PI, TAU};

use glam::{Mat3, Vec3};

use crate::decay::Decay;
use crate::limb::Limb;
use crate::options::{DeadwoodOptions, PieceKind};
use crate::rng::smoothstep;

/// Number of directions sampled around the piece to find a resting side.
const DIRECTION_COUNT: u32 = 512;
/// Maximum number of moves toward a lower, more stable resting position.
const MAX_DESCENT_STEPS: u32 = 64;
/// Minimum dot product for directions close enough to compare as neighbours.
const NEIGHBOUR_COS: f32 = 0.95;
/// Number of searches with smaller tilts to improve the resting position.
const REFINE_ROUNDS: u32 = 3;
/// Number of nearby directions tested per search, including the current one.
const REFINE_CANDIDATES: u32 = 48;
/// Initial range of nearby tilts, roughly in radians (about 9°).
const REFINE_SPREAD: f32 = 0.157;
/// The golden angle as a fraction of a full turn. Stepping around a circle
/// by this much spreads candidate directions evenly, with no two lining up.
const GOLDEN_FRACTION: f32 = 0.381_966_02;
/// Directions sampled around a stem when measuring its surface.
const ANALYSIS_SIDES: u32 = 16;

/// Where a piece came to rest on the ground.
///
/// A piece's meshes are already in their resting position and need no
/// further transform. This value describes how the piece got there, for
/// placing anything else that was built in the piece's generated position.
///
/// The turn and move are `rotation` then `translation`: a point `p` of the
/// generated piece moves to `rotation * p + translation`. Sagging and
/// flattening then bend the piece a little further, which this transform
/// does not include, so it is close but not exact for a sagged piece.
///
/// Stumps stand as built, so their placement is [`Placement::IDENTITY`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Rotation onto the resting side.
    pub rotation: Mat3,
    /// Movement after the rotation, in metres, including the lift to the
    /// ground.
    pub translation: Vec3,
    /// Suggested burial depth in metres, from
    /// [`crate::SettleOptions::burial`]. The piece is not lowered by it.
    /// Move it down by this much to sink it into the ground, as old,
    /// rotting wood does.
    pub burial: f32,
}

impl Placement {
    /// No movement and no burial.
    pub const IDENTITY: Self = Self {
        rotation: Mat3::IDENTITY,
        translation: Vec3::ZERO,
        burial: 0.0,
    };
}

/// Rests `limbs` on the ground, bending them in place, and returns the
/// turn and move used. Stumps are left standing as built.
pub(crate) fn settle(o: &DeadwoodOptions, decay: &Decay, limbs: &mut [Limb]) -> Placement {
    if o.kind == PieceKind::Stump {
        return Placement::IDENTITY;
    }

    // Turn onto a resting side without bending, using the round, unsagged
    // shape.
    let points = surface_points(limbs, |_| true);
    let com = centre_of_mass(limbs);
    let (rotation, mut translation) = if o.settle.settle {
        let offsets: Vec<Vec3> = points.iter().map(|&p| p - com).collect();
        let (down, height) = settle_direction(&offsets);
        let rotation = rotation_to_down(down);
        (rotation, Vec3::new(0.0, height, 0.0) - rotation * com)
    } else {
        let lowest = points.iter().fold(f32::INFINITY, |m, p| m.min(p.y));
        (Mat3::IDENTITY, Vec3::new(0.0, -lowest, 0.0))
    };
    for limb in limbs.iter_mut() {
        limb.curve.transform(rotation, translation);
    }

    // Flatten toward the ground, then sag between the supports.
    for limb in limbs.iter_mut() {
        limb.squash = decay.ovality;
        limb.up = Vec3::Y;
    }
    if decay.sag > 0.0 {
        sag(limbs, decay.sag);
    }

    // Put the main stem on the ground. With full sag the side stems may
    // end up below ground (buried); stiff wood keeps them all above it.
    let main_low = lowest(&surface_points(&limbs[..1], |_| true));
    let all_low = lowest(&surface_points(limbs, |_| true));
    let lift = main_low + (all_low - main_low) * (1.0 - decay.sag);
    for limb in limbs.iter_mut() {
        limb.curve
            .transform(Mat3::IDENTITY, Vec3::new(0.0, -lift, 0.0));
    }
    translation.y -= lift;

    Placement {
        rotation,
        translation,
        burial: decay.burial * o.diameter,
    }
}

/// Returns the lowest height among `points`.
fn lowest(points: &[Vec3]) -> f32 {
    points.iter().fold(f32::INFINITY, |m, p| m.min(p.y))
}

/// Samples the outer surfaces of the stems that `keep` accepts, including
/// cut and broken ends, at a fixed density that doesn't depend on mesh
/// detail.
pub(crate) fn surface_points(limbs: &[Limb], keep: impl Fn(&Limb) -> bool) -> Vec<Vec3> {
    let mut points = Vec::new();
    for limb in limbs.iter().filter(|l| keep(l)) {
        let rings = (limb.length() / (2.0 * limb.base_radius()) * 4.0).clamp(8.0, 160.0) as u32;
        for j in 0..ANALYSIS_SIDES {
            let theta = j as f32 / ANALYSIS_SIDES as f32 * TAU;
            for i in 0..=rings {
                points.push(limb.point(limb.wall_u(i as f32 / rings as f32, theta), theta));
            }
            for e in 0..2 {
                for rho in [0.0, 0.5] {
                    points.push(limb.end_point(e, rho, theta));
                }
            }
        }
    }
    points
}

/// Estimates the centre of mass (balance point) of all stems, assuming the
/// wood is equally dense throughout. It weighs each sample along a centre
/// line by its cross-section area, minus any hollow.
pub(crate) fn centre_of_mass(limbs: &[Limb]) -> Vec3 {
    let (mut sum, mut mass) = (Vec3::ZERO, 0.0);
    for limb in limbs {
        let frames = limb.curve.frames();
        let n = frames.len();
        let ds = limb.length() / (n - 1) as f32;
        for (i, f) in frames.iter().enumerate() {
            let u = i as f32 / (n - 1) as f32;
            let r = limb.nominal_radius(u, 0.0);
            let hollow = limb.hollow.map_or(0.0, |h| h.ratio * h.ratio);
            let m = PI * r * r * (1.0 - hollow) * ds;
            sum += f.position * m;
            mass += m;
        }
    }
    if mass > 0.0 { sum / mass } else { Vec3::ZERO }
}

/// Bends each stem down toward the ground between its supports. `amount`
/// from 0 (stiff) to 1 (lies on the ground) sets how far.
fn sag(limbs: &mut [Limb], amount: f32) {
    let mut drops: Vec<Vec<f32>> = Vec::with_capacity(limbs.len());
    for limb in limbs.iter_mut() {
        // Height of the lowest surface point at each sample, smoothed.
        let n = limb.curve.sample_count();
        let gaps: Vec<f32> = (0..n)
            .map(|i| {
                let u = i as f32 / (n - 1) as f32;
                (0..ANALYSIS_SIDES)
                    .map(|j| limb.point(u, j as f32 / ANALYSIS_SIDES as f32 * TAU).y)
                    .fold(f32::INFINITY, f32::min)
                    .max(0.0)
            })
            .collect();
        let smooth = smoothed(&gaps, (n / 16).max(1));

        // Side stems start where their parent moved to, then follow their
        // own gap to the ground.
        let attach = limb.parent.map(|(p, u)| sample(&drops[p], u));
        let drop: Vec<f32> = (0..n)
            .map(|i| {
                let own = amount * smooth[i];
                match attach {
                    Some(a) => {
                        let u = i as f32 / (n - 1) as f32;
                        a + (own - a) * smoothstep(0.0, 0.5, u)
                    }
                    None => own,
                }
            })
            .collect();
        let offsets: Vec<Vec3> = drop.iter().map(|&d| Vec3::new(0.0, -d, 0.0)).collect();
        limb.curve.displace(&offsets);
        drops.push(drop);
    }
}

/// Returns `values` averaged over a window of `radius` samples each side.
fn smoothed(values: &[f32], radius: usize) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(radius), (i + radius + 1).min(values.len()));
            values[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}

/// Returns `values` (evenly spaced from u = 0 to 1) blended at `u`.
fn sample(values: &[f32], u: f32) -> f32 {
    let last = values.len() - 1;
    let x = u.clamp(0.0, 1.0) * last as f32;
    let i = (x.floor() as usize).min(last.saturating_sub(1));
    let t = x - i as f32;
    values[i] + (values[(i + 1).min(last)] - values[i]) * t
}

/// Returns how high the centre of mass sits when the piece rests on the
/// side facing `down`. `offsets` are surface points measured from the
/// centre of mass; the height is the furthest any of them reaches in the
/// `down` direction.
fn support_height(offsets: &[Vec3], down: Vec3) -> f32 {
    offsets
        .iter()
        .fold(f32::NEG_INFINITY, |h, &p| h.max(p.dot(down)))
}

/// Returns candidate direction `i` of [`DIRECTION_COUNT`], spread evenly
/// over a sphere from up (`i = 0`) to down (the last).
fn sphere_direction(i: u32) -> Vec3 {
    let y = 1.0 - (2 * i + 1) as f32 / DIRECTION_COUNT as f32;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let angle = (i as f32 * GOLDEN_FRACTION).fract() * TAU;
    Vec3::new(r * angle.cos(), y, r * angle.sin())
}

/// Returns nearby candidate `j` around `down`, within `spread`.
fn refine_direction(down: Vec3, spread: f32, j: u32) -> Vec3 {
    if j == 0 {
        return down;
    }
    let (u, v) = tangent_basis(down);
    let radius = spread * (j as f32 / (REFINE_CANDIDATES - 1) as f32).sqrt();
    let angle = (j as f32 * GOLDEN_FRACTION).fract() * TAU;
    (down + radius * (angle.cos() * u + angle.sin() * v)).normalize()
}

/// Returns two unit vectors at right angles to `d` and to each other.
fn tangent_basis(d: Vec3) -> (Vec3, Vec3) {
    let helper = if d.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = helper.cross(d).normalize();
    (u, d.cross(u))
}

/// Finds a stable resting direction, starting from straight down, and
/// returns it with the centre of mass's height above the ground.
fn settle_direction(offsets: &[Vec3]) -> (Vec3, f32) {
    // Compare balance-point heights for the candidate resting sides.
    let heights: Vec<f32> = (0..DIRECTION_COUNT)
        .map(|i| support_height(offsets, sphere_direction(i)))
        .collect();

    // Start from the generated position and move to lower neighbouring
    // orientations.
    let mut current = DIRECTION_COUNT - 1;
    for _ in 0..MAX_DESCENT_STEPS {
        let here = sphere_direction(current);
        let mut best = current;
        for i in 0..DIRECTION_COUNT {
            if sphere_direction(i).dot(here) >= NEIGHBOUR_COS
                && heights[i as usize] < heights[best as usize]
            {
                best = i;
            }
        }
        if best == current {
            break;
        }
        current = best;
    }

    // Try progressively smaller tilts around the selected orientation.
    let mut down = sphere_direction(current);
    let mut height = heights[current as usize];
    let mut spread = REFINE_SPREAD;
    for _ in 0..REFINE_ROUNDS {
        for j in 1..REFINE_CANDIDATES {
            let d = refine_direction(down, spread, j);
            let h = support_height(offsets, d);
            if h < height {
                (down, height) = (d, h);
            }
        }
        spread *= 0.5;
    }
    (down, height)
}

/// Returns the rotation that turns `down` to point straight down (−Y).
fn rotation_to_down(down: Vec3) -> Mat3 {
    if down.y > 0.0 {
        let half_turn = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
        return smallest_rotation_to_down(half_turn * down) * half_turn;
    }
    smallest_rotation_to_down(down)
}

/// Returns the smallest rotation turning `down` (with `down.y <= 0`) to −Y.
fn smallest_rotation_to_down(down: Vec3) -> Mat3 {
    let c = -down.y;
    let v = down.cross(Vec3::NEG_Y);
    let k = Mat3::from_cols(
        Vec3::new(0.0, v.z, -v.y),
        Vec3::new(-v.z, 0.0, v.x),
        Vec3::new(v.y, -v.x, 0.0),
    );
    Mat3::IDENTITY + k + k * k * (1.0 / (1.0 + c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_turns_down_to_minus_y() {
        for i in 0..DIRECTION_COUNT {
            let d = sphere_direction(i);
            let r = rotation_to_down(d);
            assert!((r * d - Vec3::NEG_Y).length() < 1e-3, "{d}");
        }
    }
}
