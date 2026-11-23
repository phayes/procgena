//! A mushroom's shape worked out from its options: functions that give any
//! point on the stem, its closed bottom, the cap and the annulus, plus the
//! placed warts. Every detail level samples these same functions, so all
//! levels share one shape.
//!
//! Positions are in metres in the mushroom's own coordinates: Y up, with
//! the stem base at the origin.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};

use crate::options::MushroomOptions;
use crate::profile::Profile;
use crate::rng::{Rng, fbm, smoothstep};

// Separate random-number streams, one per use, so that changing how many
// values one feature draws never changes another feature.
/// Stream for the rim waves, lopsidedness and tilt.
const STREAM_CAP: u32 = 1;
/// Stream for wart placement.
const STREAM_WARTS: u32 = 2;
/// Stream that picks the surface-noise pattern.
const STREAM_NOISE: u32 = 3;

/// Radius of the disc that closes the cap's underside, relative to the stem
/// radius under the cap. Below 1 keeps the disc hidden inside the stem.
const NECK_INSET: f32 = 0.9;
/// How far the stem reaches up into the cap, as a fraction of the cap's
/// height at its centre, so the joint never shows a gap.
const STEM_EMBED: f32 = 0.5;
/// The most the stem reaches into the cap, in cap radii, so tall caps are
/// not pulled down onto the ground.
const MAX_EMBED: f32 = 0.08;
/// Largest stretch of each side of the cap when
/// [`crate::CapOptions::asymmetry`] is 1.
const MAX_ASYMMETRY: f32 = 0.3;
/// Random spots tried per requested wart before giving up on fitting more.
const WART_ATTEMPTS: u32 = 30;
/// Number of noise layers in surface bumps; each adds finer detail.
const NOISE_OCTAVES: u32 = 3;

/// A wart placed on top of the cap, for painting into vertex colours.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wart {
    /// Radius in metres.
    pub radius: f32,
    /// Centre of the wart, on the cap surface.
    pub centre: Vec3,
}

/// The annulus's shape, in metres.
#[derive(Debug, Clone)]
pub(crate) struct AnnulusShape {
    /// Where on the stem the annulus hangs, from 0 at the base to 1 at the
    /// top.
    pub at: f32,
    /// Side-view outline from the skirt's lower edge up to the stem, as
    /// (distance from the stem's centre line, height below the attachment).
    pub profile: Profile,
    /// Radius where the annulus meets the stem.
    pub inner: f32,
    /// How torn the lower edge is, from 0 through 1.
    pub raggedness: f32,
}

/// Everything needed to compute points on the mushroom's surfaces.
#[derive(Debug, Clone)]
pub(crate) struct Shape {
    /// Cap radius in metres.
    pub radius: f32,
    /// Stem height in metres, measured along its height before leaning.
    pub stem_height: f32,
    neck_radius: f32,
    base_radius: f32,
    bulb: f32,
    bulb_height: f32,
    /// How far the closed bottom reaches below the ground, in metres, or
    /// `None` when the stem is open at the ground.
    pub bottom_depth: Option<f32>,
    collar: f32,
    lean: Vec3,
    curve: f32,
    upturn: f32,
    stem_roughness: f32,
    /// Top outline from rim to top centre, for a cap of radius 1.
    pub top: Profile,
    /// Underside outline from the centre of its closing disc out to the
    /// rim, for a cap of radius 1.
    pub underside: Profile,
    lobes: f32,
    lobe_depth: f32,
    lobe_phase: f32,
    asymmetry: [f32; 4],
    cap_roughness: f32,
    roughness_frequency: f32,
    /// Where the cap's outline origin (its centre at height 0) sits.
    pub cap_origin: Vec3,
    /// Turns the cap's own +Y axis into the direction it faces.
    pub cap_rotation: Quat,
    noise_seed: u32,
    /// The annulus, if the mushroom has one.
    pub annulus: Option<AnnulusShape>,
    /// Warts on the cap.
    pub warts: Vec<Wart>,
}

impl Shape {
    /// Works out the shape for `o`, which must already be validated.
    pub(crate) fn new(o: &MushroomOptions) -> Self {
        let radius = o.cap_radius();
        let mut rng = Rng::new(o.seed, STREAM_CAP);

        // Stem sizes in metres.
        let s = &o.stem;
        let stem_height = s.height * o.size;
        let neck_radius = s.radius * o.size;
        let direction = s.lean_direction.to_radians();
        let lean = Vec3::new(direction.cos(), 0.0, direction.sin()) * s.lean * stem_height;

        // Cap outlines for a cap of radius 1, with heights scaled.
        let (anchors, sharp) = o.cap.family.top_anchors();
        let top_points: Vec<Vec2> = anchors
            .iter()
            .map(|&[r, y]| Vec2::new(r, y * o.cap.height))
            .collect();
        let top = Profile::new(&top_points, sharp);
        let stem_top = (neck_radius * (1.0 + s.collar)) / radius;
        let underside = underside_profile(o, top_points[0], stem_top);

        // Where the rim waves start, how lopsided each side is, and which
        // way the cap tilts all come from the seed.
        let lobe_phase = rng.range(0.0, TAU);
        let asymmetry = [0; 4].map(|_| rng.range(-1.0, 1.0) * o.cap.asymmetry * MAX_ASYMMETRY);
        let tilt_direction = rng.range(0.0, TAU);

        let mut shape = Self {
            radius,
            stem_height,
            neck_radius,
            base_radius: neck_radius * s.base,
            bulb: neck_radius * o.bulb.size,
            bulb_height: o.bulb.height,
            bottom_depth: None,
            collar: neck_radius * s.collar,
            lean,
            curve: s.curve,
            upturn: s.upturn,
            stem_roughness: s.roughness,
            top,
            underside,
            lobes: o.cap.lobes as f32,
            lobe_depth: o.cap.lobe_depth,
            lobe_phase,
            asymmetry,
            cap_roughness: o.cap.roughness,
            roughness_frequency: o.cap.roughness_scale * 0.5,
            cap_origin: Vec3::ZERO,
            cap_rotation: Quat::IDENTITY,
            noise_seed: Rng::new(o.seed, STREAM_NOISE).int(0, u32::MAX - 1),
            annulus: None,
            warts: Vec::new(),
        };
        shape.bottom_depth = o.bulb.closed.then(|| o.bulb.depth * shape.stem_radius(0.0));

        // Sit the cap on top of the stem, facing along the stem's direction
        // there plus its tilt, and lowered so the stem reaches into it.
        let axis = shape.stem_tangent(1.0);
        let tilt_axis = Vec3::new(tilt_direction.cos(), 0.0, tilt_direction.sin());
        shape.cap_rotation = Quat::from_rotation_arc(Vec3::Y, axis)
            * Quat::from_axis_angle(tilt_axis, o.cap.tilt.to_radians());
        let apex = shape.top.at(shape.top.length()).y.max(0.0);
        let embed = (apex * STEM_EMBED).min(MAX_EMBED);
        shape.cap_origin = shape.stem_centre(1.0) - axis * (embed * radius);

        shape.annulus = o.annulus.enabled.then(|| annulus_shape(o, &shape));
        shape.warts = place_warts(o, &shape);
        shape
    }

    // ----------------------------------------------------------------------
    // Stem

    /// Returns the point on the stem's centre line at `s`, from 0 (base) to
    /// 1 (top).
    pub(crate) fn stem_centre(&self, s: f32) -> Vec3 {
        Vec3::Y * (self.stem_height * s) + self.lean * self.sweep(s).0
    }

    /// Returns the unit direction the stem runs in at `s`.
    pub(crate) fn stem_tangent(&self, s: f32) -> Vec3 {
        (Vec3::Y * self.stem_height + self.lean * self.sweep(s).1).normalize()
    }

    /// Returns how far along its lean the stem has moved sideways at `s`,
    /// from 0 at the base to 1 at the top, and how fast that changes.
    ///
    /// Without upturn this is `s^curve`, which keeps bending to the top. With
    /// full upturn it is an S-shaped curve that is flat at both ends, so the
    /// stem rises upright, sweeps sideways, and ends upright again.
    fn sweep(&self, s: f32) -> (f32, f32) {
        let c = self.curve;

        // Bending all the way to the top.
        let bend = s.powf(c);
        let bend_slope = c * s.powf(c - 1.0);

        // The S-curve a / (a + b), flat at both ends when `c` is above 1.
        let (a, b) = (s.powf(c), (1.0 - s).powf(c));
        let (da, db) = (c * s.powf(c - 1.0), -c * (1.0 - s).powf(c - 1.0));
        let total = a + b;
        let s_curve = a / total;
        let s_slope = (da * b - a * db) / (total * total);

        let u = self.upturn;
        (
            bend + (s_curve - bend) * u,
            bend_slope + (s_slope - bend_slope) * u,
        )
    }

    /// Returns the rotation that turns +Y into the stem's direction at `s`,
    /// for laying out circles around the stem.
    pub(crate) fn stem_frame(&self, s: f32) -> Quat {
        Quat::from_rotation_arc(Vec3::Y, self.stem_tangent(s))
    }

    /// Returns the stem radius at `s` in metres, before surface bumps:
    /// tapering from base to top, plus any bulb and collar.
    pub(crate) fn stem_radius(&self, s: f32) -> f32 {
        let taper =
            self.base_radius + (self.neck_radius - self.base_radius) * smoothstep(0.0, 1.0, s);
        let swell = 1.0 - smoothstep(0.0, self.bulb_height, s);
        taper + self.bulb * swell * swell + self.collar * smoothstep(0.75, 1.0, s)
    }

    /// Returns a point at height `s` (0 to 1) and angle `theta` (radians)
    /// around the stem. `scale` moves it between the centre line (0) and
    /// the bumpy surface (1), which is used to fill the stem's end discs.
    pub(crate) fn stem_point(&self, s: f32, theta: f32, scale: f32) -> Vec3 {
        let around = Vec3::new(theta.cos(), 0.0, theta.sin());
        let mut r = self.stem_radius(s) * scale;
        if self.stem_roughness > 0.0 && scale > 0.0 {
            // Read the noise on a cylinder so the bumps join up all the way
            // around.
            let p = Vec3::new(
                around.x * 2.0,
                s * self.stem_height / self.neck_radius * 0.5,
                around.z * 2.0,
            );
            r *= 1.0 + self.stem_roughness * fbm(p, self.noise_seed ^ 0x57e3, NOISE_OCTAVES);
        }
        self.stem_centre(s) + self.stem_frame(s) * (around * r)
    }

    /// Returns a point on the closed bottom at `t` and angle `theta`
    /// (radians) around the stem: `t = 0` is the lowest point, below the
    /// ground, and `t = 1` the stem's edge at the ground, where the bottom
    /// meets the stem's side exactly. The bottom is a rounded dome shaped
    /// like the lower half of an ellipsoid. Returns the ground point when
    /// the stem has no closed bottom.
    pub(crate) fn bottom_point(&self, t: f32, theta: f32) -> Vec3 {
        let Some(depth) = self.bottom_depth.filter(|_| t < 1.0) else {
            return self.stem_point(0.0, theta, 1.0);
        };
        let (out, down) = (t * TAU * 0.25).sin_cos();
        self.stem_point(0.0, theta, out) - self.stem_tangent(0.0) * (down * depth)
    }

    // ----------------------------------------------------------------------
    // Cap

    /// Returns the point on the cap for outline point `p` (for a cap of
    /// radius 1) spun to angle `theta` (radians) around the cap's axis, with
    /// rim waves, bumps, lopsidedness and tilt applied.
    pub(crate) fn cap_point(&self, p: Vec2, theta: f32) -> Vec3 {
        let (sin, cos) = theta.sin_cos();

        // Rim waves, strongest at the rim and fading toward the centre.
        let mut r = p.x;
        if self.lobe_depth > 0.0 && self.lobes > 0.0 {
            r *= 1.0 + self.lobe_depth * p.x * p.x * (self.lobes * theta + self.lobe_phase).sin();
        }
        let mut local = Vec3::new(r * cos, p.y, r * sin);

        // Surface bumps, read at the position before stretching so they
        // don't depend on lopsidedness.
        if self.cap_roughness > 0.0 {
            let n = fbm(
                local * self.roughness_frequency * 2.0,
                self.noise_seed,
                NOISE_OCTAVES,
            );
            local.y += n * self.cap_roughness;
            local.x *= 1.0 + n * self.cap_roughness;
            local.z *= 1.0 + n * self.cap_roughness;
        }

        // Lopsidedness: each of the four sides stretches by its own amount.
        // Points on the axis stay put.
        let [xp, xn, zp, zn] = self.asymmetry;
        local.x *= 1.0 + if local.x > 0.0 { xp } else { xn };
        local.z *= 1.0 + if local.z > 0.0 { zp } else { zn };

        self.cap_origin + self.cap_rotation * (local * self.radius)
    }

    /// Returns the unit direction the cap faces (its axis).
    pub(crate) fn cap_axis(&self) -> Vec3 {
        self.cap_rotation * Vec3::Y
    }

    /// Returns the position of the cap's top centre.
    pub(crate) fn cap_apex(&self) -> Vec3 {
        self.cap_point(self.top.at(self.top.length()), 0.0)
    }

    // ----------------------------------------------------------------------
    // Annulus

    /// Returns the point on `annulus` at distance `s` along its outline
    /// (from the lower edge up to the stem) and angle `theta` around the
    /// stem.
    pub(crate) fn annulus_point(&self, annulus: &AnnulusShape, s: f32, theta: f32) -> Vec3 {
        let around = Vec3::new(theta.cos(), 0.0, theta.sin());
        let p = annulus.profile.at(s);

        // A ragged edge: shorten the skirt by an amount that varies smoothly
        // around the stem.
        let rag = 1.0
            - annulus.raggedness
                * 0.5
                * (fbm(around * 3.0, self.noise_seed ^ 0x2196, NOISE_OCTAVES) + 1.0);
        let r = annulus.inner + (p.x - annulus.inner) * rag;
        let y = p.y * rag;
        self.stem_centre(annulus.at) + self.stem_frame(annulus.at) * (around * r + Vec3::Y * y)
    }
}

/// Builds the underside outline for a cap of radius 1, from the centre of
/// its closing disc out to the bottom of the rim. `rim_top` is the first
/// point of the top outline and `stem_top` the stem radius under the cap.
fn underside_profile(o: &MushroomOptions, rim_top: Vec2, stem_top: f32) -> Profile {
    let u = &o.underside;
    let neck = stem_top * NECK_INSET;
    let rim = Vec2::new(rim_top.x, rim_top.y - u.rim_thickness);
    let drop = -u.decurrent;

    // The gill surface meets the stem just outside it; a flat disc inside
    // the stem then closes the cap. A cap whose rim is narrower than the
    // stem skips straight from the disc to the rim.
    let attach = Vec2::new(stem_top * 1.02, drop);
    let middle = (attach + rim) * 0.5 - Vec2::Y * u.sag;
    let mut anchors = vec![Vec2::new(0.0, drop), Vec2::new(neck, drop)];
    if attach.x < rim.x {
        anchors.push(attach);
        anchors.push(middle);
    }
    anchors.push(rim);
    Profile::new(&anchors, &[1, 2])
}

/// Works out the annulus's shape from the options and the stem it hangs on.
fn annulus_shape(o: &MushroomOptions, shape: &Shape) -> AnnulusShape {
    let r = &o.annulus;
    let inner = shape.stem_radius(r.position) * 0.97;
    let outer = shape.stem_radius(r.position) * r.flare;
    let length = r.length * o.size;
    let anchors = [
        Vec2::new(outer, -length),
        Vec2::new(inner + (outer - inner) * 0.75, -length * 0.45),
        Vec2::new(inner, 0.0),
    ];
    AnnulusShape {
        at: r.position,
        profile: Profile::new(&anchors, &[]),
        inner,
        raggedness: r.raggedness,
    }
}

/// Places up to `o.warts.count` warts on top of the cap at random, keeping
/// them apart. Returns fewer when no more fit.
fn place_warts(o: &MushroomOptions, shape: &Shape) -> Vec<Wart> {
    let w = &o.warts;
    let mut warts: Vec<Wart> = Vec::new();
    if w.count == 0 {
        return warts;
    }
    let mut rng = Rng::new(o.seed, STREAM_WARTS);
    let length = shape.top.length();
    let start = length * (1.0 - w.coverage);
    let max_r = (0..=16)
        .map(|i| shape.top.at(start + (length - start) * i as f32 / 16.0).x)
        .fold(0.0f32, f32::max)
        .max(1e-3);

    for _ in 0..w.count * WART_ATTEMPTS {
        if warts.len() as u32 >= w.count {
            break;
        }
        // Pick a random spot. Spots near the centre cover less area, so
        // they are skipped more often, which spreads warts evenly.
        let s = rng.range(start, length);
        let theta = rng.range(0.0, TAU);
        let size = shape.radius * w.size * rng.jitter(w.size_jitter * 0.99);
        if rng.next_f32() * max_r > shape.top.at(s).x {
            continue;
        }

        // Skip spots too close to a wart already placed.
        let centre = shape.cap_point(shape.top.at(s), theta);
        if warts
            .iter()
            .any(|o| o.centre.distance(centre) < (o.radius + size) * 1.1)
        {
            continue;
        }
        warts.push(Wart {
            radius: size,
            centre,
        });
    }
    warts
}
