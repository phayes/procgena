//! Turns [`DeadwoodOptions`] into the stems of a piece, in its generated
//! position before settling: logs and branches lie along +X from the
//! origin, and stumps stand up +Y with the ground at `y = 0`.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use crate::curve::Curve;
use crate::decay::Decay;
use crate::limb::{Bark, End, Flare, Hollow, Limb, Role, SizeClass};
use crate::options::{DeadwoodOptions, EndKind, MAX_END_SHARE, PieceKind};
use crate::rng::{Rng, sub_seed};

// Each feature draws its random numbers from its own stream. Changing how
// many numbers one feature draws then never changes another feature's
// shape for the same seed.
/// Random stream for the main stem.
const STREAM_MAIN: u32 = 1;
/// Random stream for branch stubs.
const STREAM_STUBS: u32 = 2;
/// Random stream for side branches and twigs.
const STREAM_CHILDREN: u32 = 3;
/// Random stream for a stump's roots.
const STREAM_ROOTS: u32 = 4;

/// Thinnest stem radius generated, in metres.
const MIN_RADIUS: f32 = 0.001;
/// The golden angle, about 137.5° in radians. Stepping around a parent by
/// this angle spreads side branches evenly, with no two lining up.
const GOLDEN_ANGLE: f32 = 2.399_963;

/// Builds every stem of a piece from validated options. The main stem is
/// first, and every stem comes after the stem it grows from.
pub(crate) fn build(o: &DeadwoodOptions, decay: &Decay) -> Vec<Limb> {
    let mut limbs = vec![main_limb(o, decay)];
    add_stubs(o, decay, &mut limbs);
    match o.kind {
        PieceKind::Branch => add_children(o, decay, &mut limbs),
        PieceKind::Stump => add_roots(o, decay, &mut limbs),
        PieceKind::Log => {}
    }
    limbs
}

/// Builds the main stem.
fn main_limb(o: &DeadwoodOptions, decay: &Decay) -> Limb {
    let mut rng = Rng::new(o.seed, STREAM_MAIN);
    let (r0, r1) = (o.base_radius(), o.tip_radius());
    let stump = o.kind == PieceKind::Stump;

    // Control points: lying along +X, or standing up +Y from below ground
    // for a stump.
    let depth = if stump {
        o.roots.depth * o.diameter
    } else {
        0.0
    };
    let total = o.length + depth;
    let (axis, across_a, across_b, start) = if stump {
        let lean_dir = rng.range(0.0, TAU);
        let lean = Quat::from_axis_angle(
            Vec3::new(lean_dir.cos(), 0.0, lean_dir.sin()),
            o.shape.lean.to_radians(),
        );
        // Start below ground so the stem crosses y = 0 at distance `depth`.
        (
            lean * Vec3::Y,
            lean * Vec3::X,
            lean * Vec3::Z,
            -(lean * Vec3::Y) * depth,
        )
    } else {
        (Vec3::X, Vec3::Y, Vec3::Z, Vec3::ZERO)
    };
    // Bend the whole stem, and kink it a little between control points. The
    // kinks stay below a tenth of the point spacing, so short stems such as
    // stumps never bend sharply.
    let bend_angle = rng.range(0.0, TAU);
    let bend_dir = across_a * bend_angle.cos() + across_b * bend_angle.sin();
    let wobble = (o.shape.wobble * o.diameter).min(0.1 * total / 5.0);
    let points: Vec<Vec3> = (0..6)
        .map(|k| {
            let t = k as f32 / 5.0;
            let mut p =
                start + axis * total * t + bend_dir * o.shape.bend * o.length * (PI * t).sin();
            if (1..5).contains(&k) {
                p += across_a * rng.range(-wobble, wobble) + across_b * rng.range(-wobble, wobble);
            }
            p
        })
        .collect();
    let samples = ((total / o.diameter) * 8.0).clamp(16.0, 512.0) as usize;
    let up = if stump { across_a } else { Vec3::Y };
    let curve = Curve::through(&points, samples, up);

    // Ends: a stump's base is a hidden flat cut below the ground.
    let base_kind = if stump {
        EndKind::Saw { tilt: 0.0 }
    } else {
        o.ends.base
    };
    let ends = [
        End::new(base_kind, r0, sub_seed(o.seed, 10), decay.soften),
        End::new(o.ends.tip, r1, sub_seed(o.seed, 11), decay.soften),
    ];

    let flare = stump.then(|| Flare {
        ground: depth,
        height: o.roots.flare_height * o.diameter,
        width: o.roots.flare,
        lobes: o.roots.lobes,
        lobe_depth: o.roots.lobe_depth,
        phase: rng.range(0.0, TAU),
    });
    // A hollow can only open through a cut or broken end, not a bark tip.
    let hollow = (o.hollow.enabled && o.kind != PieceKind::Branch).then(|| Hollow {
        ratio: o.hollow.radius,
        open: [
            o.hollow.open_base && !stump && ends[0].shows_wood(),
            o.hollow.open_tip && ends[1].shows_wood(),
        ],
        depth: o.hollow.depth,
        roughness: o.hollow.roughness,
    });

    Limb {
        role: Role::Main,
        size: if o.kind == PieceKind::Branch {
            SizeClass::Limb
        } else {
            SizeClass::Trunk
        },
        parent: None,
        curve,
        radius: [r0, r1],
        taper_from: depth,
        seed: sub_seed(o.seed, 0),
        ends,
        bark: bark(o, decay),
        lumps: o.shape.lumps,
        lump_scale: o.shape.lump_scale,
        flare,
        hollow,
        squash: 0.0,
        up: Vec3::Y,
    }
}

/// Returns the bark settings shared by every stem of a piece.
fn bark(o: &DeadwoodOptions, decay: &Decay) -> Bark {
    Bark {
        thickness: o.bark.thickness,
        stripped: decay.stripped,
        patch_size: o.bark.patch_size,
        cracks: o.bark.cracks,
        plate_size: o.bark.plate_size,
    }
}

/// A new side stem's starting point and shape, before it becomes a
/// [`Limb`].
struct Sprout {
    /// What the new stem is: a stub, side branch or twig.
    role: Role,
    /// Index of the stem it grows from.
    parent: usize,
    /// Where along the parent it grows, from 0 (base) to 1 (tip).
    u: f32,
    /// Angle around the parent where it grows, in radians.
    theta: f32,
    /// Angle from the parent's direction, in radians.
    angle: f32,
    /// Length in metres, not counting the part that starts inside the
    /// parent.
    length: f32,
    /// Radius at the base and tip, in metres.
    radius: [f32; 2],
    /// How deep inside the parent the stem starts, as a fraction of the
    /// parent's radius from its surface toward its centre line. Starting
    /// inside hides the joint.
    sink: f32,
    /// Sideways curve over the length, as a fraction of it.
    curl: f32,
    /// Downward curve over the length, as a fraction of it.
    droop: f32,
    /// How the far end finishes.
    tip: EndKind,
    /// Seed for the stem's random details.
    seed: u32,
}

/// Turns `s` into a stem growing from `limbs[s.parent]`.
fn sprout(o: &DeadwoodOptions, decay: &Decay, limbs: &[Limb], s: Sprout) -> Limb {
    let parent = &limbs[s.parent];
    let frame = parent.frame(s.u);
    let radial = frame.radial(s.theta);
    let parent_r = parent.nominal_radius(s.u, s.theta);
    let base = frame.position + radial * parent_r * (1.0 - s.sink);
    let dir = (frame.tangent * s.angle.cos() + radial * s.angle.sin()).normalize();

    // Curve gently sideways and downward along the length.
    let side = dir.cross(frame.tangent).normalize_or(frame.normal);

    // Add the stretch inside the parent, so `s.length` stays visible: the
    // base sits `parent_r * sink` below the surface, which a slanted stem
    // crosses over a longer distance.
    let buried = parent_r * s.sink / s.angle.sin().max(0.2);
    let length = s.length + buried;
    let points: Vec<Vec3> = (0..4)
        .map(|k| {
            let t = k as f32 / 3.0;
            base + dir * length * t + side * s.curl * length * t * t
                - Vec3::Y * s.droop * length * t * t
        })
        .collect();
    let samples = ((length / (2.0 * s.radius[0])) * 4.0).clamp(4.0, 128.0) as usize;
    let curve = Curve::through(&points, samples, radial);

    // A tip end that doesn't fit becomes a plain cut. The base is a flat
    // cut with no span, so only the tip needs room.
    let mut tip = s.tip;
    if tip.span(s.radius[1]) > MAX_END_SHARE * length {
        tip = EndKind::Saw { tilt: 0.0 };
    }
    Limb {
        role: s.role,
        size: SizeClass::Branch,
        parent: Some((s.parent, s.u)),
        curve,
        radius: s.radius,
        taper_from: 0.0,
        seed: s.seed,
        ends: [
            End::new(
                EndKind::Saw { tilt: 0.0 },
                s.radius[0],
                sub_seed(s.seed, 1),
                0.0,
            ),
            End::new(tip, s.radius[1], sub_seed(s.seed, 2), decay.soften),
        ],
        bark: bark(o, decay),
        lumps: o.shape.lumps * 0.5,
        lump_scale: o.shape.lump_scale,
        flare: None,
        hollow: None,
        squash: 0.0,
        up: Vec3::Y,
    }
}

/// Returns the `u` range of the main stem's visible part: above the ground
/// and flare for a stump, clear of the ends for a log.
fn visible_span(o: &DeadwoodOptions, main: &Limb) -> (f32, f32) {
    let length = main.length();
    let start = if o.kind == PieceKind::Stump {
        (main.taper_from + o.roots.flare_height * o.diameter * 0.7) / length
    } else {
        main.end_plane(0) + 0.05
    };
    (
        start.min(0.9),
        (main.end_plane(1) - 0.05).max(start.min(0.9) + 0.01),
    )
}

/// Adds broken-off branch stubs to the main stem.
fn add_stubs(o: &DeadwoodOptions, decay: &Decay, limbs: &mut Vec<Limb>) {
    let mut rng = Rng::new(o.seed, STREAM_STUBS);
    let (u0, u1) = visible_span(o, &limbs[0]);
    let visible = (u1 - u0) * limbs[0].length();
    let count = (o.stubs.per_metre * visible).round() as u32;
    let sink = limbs[0]
        .hollow
        .map_or(0.7, |h| (1.0 - h.ratio - 0.08).clamp(0.1, 0.7));
    for i in 0..count {
        // Draw every value, so keeping a stub or not never shifts the others.
        let u = rng.range(u0, u1);
        let theta = rng.range(0.0, TAU);
        let angle = o.stubs.angle.to_radians() * rng.jitter(0.2);
        let ratio = o.stubs.radius * rng.jitter(0.25);
        let length = o.stubs.length * o.diameter * rng.jitter(0.4);
        let snapped = rng.chance(0.65);
        let keep = rng.chance(decay.stub_keep);
        if !keep {
            continue;
        }
        let r = (limbs[0].nominal_radius(u, theta) * ratio).max(MIN_RADIUS);
        let tip = if snapped {
            EndKind::Snapped {
                jaggedness: 0.3,
                splinters: 2,
                one_sided: 0.5,
            }
        } else {
            EndKind::Saw { tilt: 20.0 }
        };
        let limb = sprout(
            o,
            decay,
            limbs,
            Sprout {
                role: Role::Stub,
                parent: 0,
                u,
                theta,
                angle,
                length,
                radius: [r, r * 0.85],
                sink,
                curl: 0.0,
                droop: 0.0,
                tip,
                seed: sub_seed(o.seed, 100 + i),
            },
        );
        limbs.push(limb);
    }
}

/// Adds side branches and twigs to a fallen branch.
fn add_children(o: &DeadwoodOptions, decay: &Decay, limbs: &mut Vec<Limb>) {
    let b = &o.branching;
    let mut rng = Rng::new(o.seed, STREAM_CHILDREN);
    let mut next_seed = 1000;

    // Grow each level of side branches on the one before.
    let mut parents: Vec<usize> = vec![0];
    for level in 0..b.levels {
        let mut grown = Vec::new();
        for &p in &parents {
            let count = (b.children as f32 * 0.7f32.powi(level as i32)).round() as u32;
            let phase = rng.range(0.0, TAU);
            for k in 0..count {
                let u = rng.range(0.15, 0.95);
                let theta = phase + k as f32 * GOLDEN_ANGLE + rng.range(-0.3, 0.3);
                let angle = b.angle.to_radians() * rng.jitter(0.25);
                let ratio = b.radius * rng.jitter(0.2);
                let len_jitter = rng.jitter(0.3);
                let curl = rng.range(-0.08, 0.08);
                let broken = rng.chance(0.15 + 0.15 * o.decay);
                let keep = rng.chance(decay.child_keep);
                if !keep {
                    continue;
                }
                let parent = &limbs[p];
                let pr = parent.nominal_radius(u, theta);
                let r = (pr * ratio).min(0.9 * pr).max(MIN_RADIUS);
                let length = parent.length() * b.length * len_jitter * (1.15 - 0.5 * u);
                let tip = if broken {
                    EndKind::Snapped {
                        jaggedness: 0.4,
                        splinters: 2,
                        one_sided: 0.6,
                    }
                } else {
                    EndKind::Taper
                };
                next_seed += 1;
                let limb = sprout(
                    o,
                    decay,
                    limbs,
                    Sprout {
                        role: Role::Child,
                        parent: p,
                        u,
                        theta,
                        angle,
                        length,
                        radius: [r, (r * 0.35).max(MIN_RADIUS)],
                        sink: 0.5,
                        curl,
                        droop: 0.03,
                        tip,
                        seed: sub_seed(o.seed, next_seed),
                    },
                );
                grown.push(limbs.len());
                limbs.push(limb);
            }
        }
        if grown.is_empty() {
            break;
        }
        parents = grown;
    }

    // Twigs on the thinnest branches.
    for &p in &parents {
        for _ in 0..b.twigs {
            let u = rng.range(0.2, 0.98);
            let theta = rng.range(0.0, TAU);
            let angle = rng.range(25.0f32, 70.0).to_radians();
            let len_jitter = rng.jitter(0.4);
            let curl = rng.range(-0.15, 0.15);
            let keep = rng.chance(decay.twig_keep);
            if !keep {
                continue;
            }
            let parent = &limbs[p];
            let r = (parent.nominal_radius(u, theta) * 0.45).max(MIN_RADIUS);
            let length = parent.length() * b.twig_length * len_jitter;
            next_seed += 1;
            let limb = sprout(
                o,
                decay,
                limbs,
                Sprout {
                    role: Role::Twig,
                    parent: p,
                    u,
                    theta,
                    angle,
                    length,
                    radius: [r, (r * 0.3).max(MIN_RADIUS)],
                    sink: 0.5,
                    curl,
                    droop: 0.0,
                    tip: EndKind::Taper,
                    seed: sub_seed(o.seed, next_seed),
                },
            );
            limbs.push(limb);
        }
    }
}

/// Adds a stump's surface roots, one on each buttress where possible.
fn add_roots(o: &DeadwoodOptions, decay: &Decay, limbs: &mut Vec<Limb>) {
    let r = &o.roots;
    let mut rng = Rng::new(o.seed, STREAM_ROOTS);
    let main = &limbs[0];
    let flare = main.flare.expect("stumps have a flare");
    let u = (flare.ground + 0.15 * flare.height) / main.length();
    let frame = main.frame(u);
    let pitch = r.pitch.to_radians();
    // Start roots inside the solid wood, clear of any hollow.
    let inset = main.hollow.map_or(0.45, |h| (h.ratio + 0.1).min(0.9));

    let mut roots = Vec::new();
    for i in 0..r.count {
        // Put each root on a buttress: where cos(lobes·θ + phase) is 1.
        let slot = (i as f32 + rng.range(-0.15, 0.15)) / r.count as f32;
        let mut theta = slot * TAU;
        if flare.lobes > 0 {
            let lobe = (slot * flare.lobes as f32).round();
            theta = (lobe * TAU - flare.phase) / flare.lobes as f32;
        }
        let length = r.length * o.diameter * rng.jitter(0.3);
        let radius = (r.radius * o.base_radius() * rng.jitter(0.25)).max(MIN_RADIUS);
        let wiggle = rng.range(-0.1, 0.1);

        // Run outward and down, curving into the soil.
        let radial = frame.radial(theta);
        let flat = (radial - Vec3::Y * radial.y).normalize_or(radial);
        let dir = (flat * pitch.cos() - Vec3::Y * pitch.sin()).normalize();
        let base = frame.position + radial * main.nominal_radius(u, theta) * inset;
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        let points: Vec<Vec3> = (0..5)
            .map(|k| {
                let t = k as f32 / 4.0;
                base + dir * length * t - Vec3::Y * 0.12 * length * t * t
                    + side * wiggle * length * t
            })
            .collect();
        let samples = ((length / (2.0 * radius)) * 4.0).clamp(6.0, 128.0) as usize;
        let seed = sub_seed(o.seed, 5000 + i);
        roots.push(Limb {
            role: Role::Root,
            size: SizeClass::Branch,
            parent: Some((0, u)),
            curve: Curve::through(&points, samples, Vec3::Y),
            radius: [radius, (radius * 0.25).max(MIN_RADIUS)],
            taper_from: 0.0,
            seed,
            ends: [
                End::new(EndKind::Saw { tilt: 0.0 }, radius, sub_seed(seed, 1), 0.0),
                End::new(
                    EndKind::Taper,
                    (radius * 0.25).max(MIN_RADIUS),
                    sub_seed(seed, 2),
                    0.0,
                ),
            ],
            bark: bark(o, decay),
            lumps: o.shape.lumps,
            lump_scale: o.shape.lump_scale,
            flare: None,
            hollow: None,
            squash: 0.0,
            up: Vec3::Y,
        });
    }
    limbs.extend(roots);
}
