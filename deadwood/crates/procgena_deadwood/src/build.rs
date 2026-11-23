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

    // Ends: a stump's base is a hidden flat cut below the ground. Ends too
    // long for the stem are shrunk to fit.
    let base_kind = if stump {
        EndKind::Saw { tilt: 0.0 }
    } else {
        o.ends.base
    };
    let (base_kind, tip_kind) = fit_ends(base_kind, o.ends.tip, r0, r1, o.length);
    let ends = [
        End::new(base_kind, r0, sub_seed(o.seed, 10), decay.soften),
        End::new(tip_kind, r1, sub_seed(o.seed, 11), decay.soften),
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

/// Returns `base` and `tip`, shrunk if needed so that together they take up
/// at most [`MAX_END_SHARE`] of a stem `length` long, with radii `r0` at
/// the base and `r1` at the tip.
///
/// Both ends are shrunk by the same amount (see [`shrink`]), as little as
/// fits. If even the smallest versions are too long, which happens with an
/// axe cut or a natural tip on a very short stem, the longer end becomes a
/// flat cut, then the other.
fn fit_ends(base: EndKind, tip: EndKind, r0: f32, r1: f32, length: f32) -> (EndKind, EndKind) {
    let room = MAX_END_SHARE * length;
    let total = |b: EndKind, t: EndKind| b.span(r0) + t.span(r1);
    if total(base, tip) <= room {
        return (base, tip);
    }

    // The largest shrink factor that fits, by bisection: spans grow with it.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if total(shrink(base, mid), shrink(tip, mid)) <= room {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let (mut base, mut tip) = (shrink(base, lo), shrink(tip, lo));

    // Flat cuts in place of ends that can't shrink enough.
    let flat = EndKind::Saw { tilt: 0.0 };
    if total(base, tip) > room {
        if base.span(r0) >= tip.span(r1) {
            base = flat;
        } else {
            tip = flat;
        }
    }
    if total(base, tip) > room {
        (base, tip) = (flat, flat);
    }
    (base, tip)
}

/// Returns `end` with its tilt, step, jaggedness and slant scaled by `t`
/// (from 0 to 1) and an axe wedge opened toward its widest angle, so its
/// span grows with `t`. A natural tip is unchanged.
fn shrink(end: EndKind, t: f32) -> EndKind {
    match end {
        EndKind::Saw { tilt } => EndKind::Saw { tilt: tilt * t },
        EndKind::Chainsaw { tilt, step } => EndKind::Chainsaw {
            tilt: tilt * t,
            step: step * t,
        },
        EndKind::Axe { angle } => EndKind::Axe {
            angle: 150.0 - (150.0 - angle) * t,
        },
        EndKind::Snapped {
            jaggedness,
            splinters,
            one_sided,
        } => EndKind::Snapped {
            jaggedness: jaggedness * t,
            splinters,
            one_sided: one_sided * t,
        },
        EndKind::Taper => EndKind::Taper,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeadwoodPreset, Piece};

    /// The length both ends of `ends` take up, for radii `r0` and `r1`.
    fn spans(ends: (EndKind, EndKind), r0: f32, r1: f32) -> f32 {
        ends.0.span(r0) + ends.1.span(r1)
    }

    #[test]
    fn ends_that_fit_are_unchanged() {
        let base = EndKind::Saw { tilt: 10.0 };
        let tip = EndKind::Axe { angle: 60.0 };
        assert_eq!(fit_ends(base, tip, 0.1, 0.1, 3.0), (base, tip));
    }

    #[test]
    fn long_ends_shrink_just_enough_to_fit() {
        let base = EndKind::Chainsaw {
            tilt: 30.0,
            step: 0.3,
        };
        let tip = EndKind::Snapped {
            jaggedness: 1.0,
            splinters: 5,
            one_sided: 1.0,
        };
        let (r0, r1, length) = (0.2, 0.18, 0.8);
        assert!(spans((base, tip), r0, r1) > MAX_END_SHARE * length);

        let fitted = fit_ends(base, tip, r0, r1, length);
        let used = spans(fitted, r0, r1);
        assert!(used <= MAX_END_SHARE * length, "{fitted:?} uses {used}");
        assert!(
            used > 0.99 * MAX_END_SHARE * length,
            "{fitted:?} shrank too far"
        );
        // Same shapes, gentler; the splinter count is kept.
        assert!(matches!(fitted.0, EndKind::Chainsaw { tilt, .. } if tilt < 30.0));
        assert!(matches!(
            fitted.1,
            EndKind::Snapped { jaggedness, splinters: 5, .. } if jaggedness < 1.0
        ));
    }

    #[test]
    fn ends_that_cannot_shrink_enough_become_flat_cuts() {
        // A natural tip always reaches one radius; on a stem shorter than
        // that it can't fit, and nor can an axe cut.
        let flat = EndKind::Saw { tilt: 0.0 };
        // The longer end, the tip, goes flat; the axe cut, opened as wide as
        // it goes, then fits.
        let fitted = fit_ends(EndKind::Axe { angle: 40.0 }, EndKind::Taper, 0.1, 0.1, 0.1);
        assert_eq!(fitted, (EndKind::Axe { angle: 150.0 }, flat));
        // On a stem too short for even that, both go flat.
        let fitted = fit_ends(EndKind::Axe { angle: 40.0 }, EndKind::Taper, 0.1, 0.1, 0.02);
        assert_eq!(fitted, (flat, flat));
        // A natural tip too long for the stem goes flat on its own.
        let fitted = fit_ends(flat, EndKind::Taper, 0.1, 0.1, 0.2);
        assert_eq!(fitted, (flat, flat));
        // A thin natural tip fits once a blunter axe cut makes room.
        let fitted = fit_ends(EndKind::Axe { angle: 40.0 }, EndKind::Taper, 0.1, 0.02, 0.2);
        assert!(
            matches!(fitted.0, EndKind::Axe { angle } if angle > 40.0),
            "{fitted:?}"
        );
        assert_eq!(fitted.1, EndKind::Taper);
    }

    #[test]
    fn every_preset_builds_with_any_end_shape() {
        let shapes = [
            EndKind::Saw { tilt: 45.0 },
            EndKind::Chainsaw {
                tilt: 30.0,
                step: 0.3,
            },
            EndKind::Axe { angle: 30.0 },
            EndKind::Snapped {
                jaggedness: 1.0,
                splinters: 24,
                one_sided: 1.0,
            },
            EndKind::Taper,
        ];
        for preset in DeadwoodPreset::ALL {
            for base in shapes {
                for tip in shapes {
                    let mut o = preset.options();
                    o.ends = crate::EndsOptions { base, tip };
                    let piece = Piece::new(o);
                    assert!(piece.is_ok(), "{}: {base:?} {tip:?}", preset.name());
                }
            }
        }
    }
}
