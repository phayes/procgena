//! Leaf, fern and shelf-fungus cards growing out of the main stem.
//!
//! Sprays are placed once, after the piece has settled, so they follow the
//! resting position. Each detail level then keeps a share of them and
//! meshes the kept cards as flat squares.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat3, Quat, Vec3};
use procgena_core::hash::{derive_seed, seed_for, unit_float};

use crate::colour::{linear_from_hex, rgba};
use crate::limb::Limb;
use crate::mesh::MeshBuffers;
use crate::options::{DeadwoodOptions, LeafOptions};
use crate::rng::{RngExt, stream};
use crate::seed_keys::{LEAF_KEEP, LEAF_SPRAYS};

/// Spots tried per spray before giving up on it.
const MAX_TRIES: u32 = 12;
/// How far a card's base sinks into the surface, as a fraction of its
/// size, so no gap shows between card and bark.
const SINK: f32 = 0.05;
/// Height above the ground or burial depth, as a fraction of the diameter,
/// below which no spray grows.
const CLEARANCE: f32 = 0.02;
/// Fraction of the visible stem that counts as "near an end" for
/// [`LeafOptions::ends`].
const END_REACH: f32 = 0.12;

/// One placed card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Card {
    /// Middle of the card's bottom edge, where it grows from, in metres.
    pub origin: Vec3,
    /// Turns the card's resting frame (tip along +Y, face along +Z, width
    /// along +X) into place.
    pub orientation: Quat,
    /// Side length, in metres.
    pub size: f32,
    /// Which spray the card belongs to, for thinning by detail level.
    pub spray: u32,
}

/// Places every card for `o.leaves` on the settled main stem `main`.
/// `burial` is the suggested burial depth in metres; no spray grows below
/// it. Returns no cards when leaves are off.
pub(crate) fn place(o: &DeadwoodOptions, main: &Limb, span: (f32, f32), burial: f32) -> Vec<Card> {
    let l = &o.leaves;
    let (u0, u1) = span;
    let count = (l.per_metre * (u1 - u0) * main.length()).round() as u32;
    let floor = burial + CLEARANCE * o.diameter;
    // Leaves draw from their own stream, apart from the stem streams in
    // `build`, so turning leaves on never changes the wood.
    let mut rng = stream(o.seed, LEAF_SPRAYS);
    let mut cards = Vec::new();

    for spray in 0..count {
        // Find a spot on the surface: draw a fixed number of values per try
        // so a rejected try never shifts the next spray's values.
        let mut spot = None;
        for _ in 0..MAX_TRIES {
            let near_end = rng.chance(l.ends);
            let at_tip = rng.chance(0.5);
            let reach = rng.next_f32().powi(2) * END_REACH;
            let anywhere = rng.range(u0, u1);
            let theta = rng.range(0.0, TAU);
            let roll = rng.next_f32();
            if spot.is_some() {
                continue;
            }
            let u = match (near_end, at_tip) {
                (false, _) => anywhere,
                (true, false) => u0 + reach * (u1 - u0),
                (true, true) => u1 - reach * (u1 - u0),
            };
            let (point, normal) = surface(main, u, theta);
            if point.y < floor {
                continue;
            }
            // Facing 0 points straight down, 1 straight up. With `top`
            // at 1 only up-facing spots are likely to be kept.
            let facing = 0.5 + 0.5 * normal.y;
            if roll <= 1.0 - l.top * (1.0 - facing * facing) {
                spot = Some((point, normal));
            }
        }

        // Draw the spray's own values whether or not it found a spot.
        let lean = l.lean.to_radians() * rng.next_f32();
        let lean_turn = rng.range(0.0, TAU);
        let first_turn = rng.range(0.0, TAU);
        let per_card: Vec<[f32; 3]> = (0..l.cards)
            .map(|_| [rng.jitter(0.3), rng.range(-0.3, 0.3), rng.range(-1.0, 1.0)])
            .collect();
        let Some((point, normal)) = spot else {
            continue;
        };

        // The spray's centre line: out of the surface, leaned at random,
        // then pulled toward up.
        let across = any_perpendicular(normal);
        let lean_dir = Quat::from_axis_angle(normal, lean_turn) * across;
        let leaned = (normal * lean.cos() + lean_dir * lean.sin()).normalize();
        let axis = leaned.lerp(Vec3::Y, l.up).normalize_or(Vec3::Y);

        // Fan the cards evenly around the centre line.
        let side = any_perpendicular(axis);
        for (k, [spread_jitter, turn_jitter, size_roll]) in per_card.into_iter().enumerate() {
            let step = TAU / l.cards as f32;
            let turn = first_turn + step * (k as f32 + turn_jitter);
            let outward = Quat::from_axis_angle(axis, turn) * side;
            let spread = (l.spread.to_radians() * spread_jitter).min(FRAC_PI_2);
            let tip = (axis * spread.cos() + outward * spread.sin()).normalize();
            let width = axis.cross(outward).normalize();
            let face = width.cross(tip).normalize();
            let orientation = flatten(
                Quat::from_mat3(&Mat3::from_cols(width, tip, face)),
                l.flatten,
            );
            let size = l.size * (1.0 + l.size_variance * size_roll);
            cards.push(Card {
                origin: point - normal * (SINK * size),
                orientation,
                size,
                spray,
            });
        }
    }
    cards
}

/// Returns the outer surface point of `limb` at `u`, `theta` and its
/// outward unit normal, found from nearby surface points.
fn surface(limb: &Limb, u: f32, theta: f32) -> (Vec3, Vec3) {
    let point = limb.point(u, theta);
    // Step about a centimetre along the stem and a small angle around it.
    let du = 0.01 / limb.length().max(0.01);
    let dt = 0.01;
    let along = limb.point((u + du).min(1.0), theta) - limb.point((u - du).max(0.0), theta);
    let around = limb.point(u, theta + dt) - limb.point(u, theta - dt);
    let outward = point - limb.frame(u).position;
    let normal = along
        .cross(around)
        .normalize_or(outward.normalize_or(Vec3::Y));
    let normal = if normal.dot(outward) < 0.0 {
        -normal
    } else {
        normal
    };
    (point, normal)
}

/// Returns a unit vector perpendicular to the unit vector `v`.
fn any_perpendicular(v: Vec3) -> Vec3 {
    let helper = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
    v.cross(helper).normalize()
}

/// Turns a card `orientation` (tip along local +Y, face along local +Z)
/// the fraction `amount` (0–1) of the way toward lying level with its face
/// up. The level card points in the horizontal direction its tip already
/// leans; an upright card tips over backwards instead.
fn flatten(orientation: Quat, amount: f32) -> Quat {
    if amount <= 0.0 {
        return orientation;
    }
    let level = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let tip = orientation * Vec3::Y;
    let forward = level(tip)
        .try_normalize()
        .unwrap_or_else(|| -level(orientation * Vec3::Z).normalize());
    let flat = Quat::from_mat3(&Mat3::from_cols(forward.cross(Vec3::Y), forward, Vec3::Y));
    orientation.slerp(flat, amount.min(1.0))
}

/// Returns true if `spray` is among the share `share` (0 to 1) of sprays a
/// detail level keeps. A spray kept at one share is kept at every larger
/// share. `keep_seed` is the piece's seed for [`LEAF_KEEP`].
fn keeps(keep_seed: u32, spray: u32, share: f32) -> bool {
    unit_float(derive_seed(keep_seed, spray)) < share
}

/// Meshes the cards of the sprays kept at `share`, coloured by
/// `leaves.tint`. Returns `None` when no card is kept.
///
/// Each card is a square with its bottom edge's middle at the card's
/// origin; [`LeafOptions::crossed`] adds a second square at right angles,
/// turned around the card's centre line. UVs run 0 to 1 across each
/// square, with `v` 0 at its base. Unused wood values are zero.
pub(crate) fn mesh_cards(
    cards: &[Card],
    leaves: &LeafOptions,
    seed: u32,
    share: f32,
) -> Option<MeshBuffers> {
    let tint = rgba(linear_from_hex(&leaves.tint).unwrap_or([1.0; 3]));
    let turns: &[f32] = if leaves.crossed {
        &[0.0, FRAC_PI_2]
    } else {
        &[0.0]
    };
    let mut out = MeshBuffers::default();
    let keep_seed = seed_for(seed, LEAF_KEEP);
    for card in cards.iter().filter(|c| keeps(keep_seed, c.spray, share)) {
        for &turn in turns {
            let rotation = card.orientation * Quat::from_rotation_y(turn);
            let half = card.size / 2.0;
            let corners = [
                Vec3::new(-half, card.size, 0.0),
                Vec3::new(-half, 0.0, 0.0),
                Vec3::new(half, 0.0, 0.0),
                Vec3::new(half, card.size, 0.0),
            ]
            .map(|c| rotation * c + card.origin);
            let uvs = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
            let normal = rotation * Vec3::Z;
            let width = rotation * Vec3::X;
            let tip = rotation * Vec3::Y;
            // Choose the bitangent sign so `normal × tangent` runs toward
            // the tip, the way `v` increases.
            let w = if normal.cross(width).dot(tip) < 0.0 {
                -1.0
            } else {
                1.0
            };

            let i = out.positions.len() as u32;
            for (corner, uv) in corners.into_iter().zip(uvs) {
                out.positions.push(corner.into());
                out.normals.push(normal.into());
                out.uvs.push(uv);
                out.tangents.push(width.extend(w).into());
                out.colors.push(tint);
                out.wood.push([0.0; 4]);
            }
            out.indices
                .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
        }
    }
    (!out.indices.is_empty()).then_some(out)
}
