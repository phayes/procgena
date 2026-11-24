//! One stem of a piece (the main stem, a stub, a side branch, a twig or a
//! root) and the functions that give its surface: the bark wall, the cut or
//! broken ends, and a hollow's inner wall.
//!
//! A stem is a [`Curve`] with a cross-section swept along it. A point on
//! the wall is named by `u`, from 0 at the base to 1 at the tip, and an
//! angle `theta` in radians around the curve. The surface is described by
//! functions rather than stored points, and every level of detail samples
//! the same functions, so coarse and fine meshes share one outline.
//!
//! "Nominal" radius below means the smooth radius from the taper and a
//! stump's flare, before bumps, bark cracks and peeling are added.
//!
//! An end's "middle plane" is a flat reference cross-section where the end
//! is placed. Depending on its kind, the end's surface sticks out past it
//! or dips in behind it, by up to the end's reach.

use std::f32::consts::TAU;

use glam::Vec3;
use procgena_core::Rng;
use procgena_core::hash::seed_for;

use crate::curve::{Curve, Frame, perpendicular};
use crate::options::{EndKind, SNAP_BASE, SNAP_SPIKE};
use crate::rng::{cell_border, fbm, smoothstep, value_noise};
use crate::seed_keys::{BARK_CRACKS, BARK_PATCHES, HOLLOW_WALL};

/// What a stem is, for size tags and for which detail levels keep it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Role {
    /// The piece's main stem: a log, a branch's main axis or a stump.
    Main,
    /// A short broken-off branch on another stem.
    Stub,
    /// A side branch.
    Child,
    /// A twig.
    Twig,
    /// A stump's surface root.
    Root,
}

/// How big a stem is: a trunk, a limb or a branch.
///
/// Each part of a [`crate::DeadwoodMesh`] carries one, and
/// [`tag`](Self::tag) turns it into a shared tag, so renderers and other
/// generators can treat thick and thin wood differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SizeClass {
    /// The main stem of a log or stump.
    Trunk,
    /// The main stem of a fallen branch.
    Limb,
    /// Side branches, stubs, twigs and roots.
    Branch,
}

/// One end of a stem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct End {
    pub kind: EndKind,
    /// How far the end's surface can lie above or below its middle plane,
    /// in metres. See [`EndKind::reach`].
    pub reach: f32,
    /// Nominal stem radius at this end, in metres.
    pub radius: f32,
    /// Direction that tilts, slants and steps face, as an angle in radians
    /// around the stem.
    pub phase: f32,
    /// Seed for the end's roughness and splinters.
    pub seed: u32,
    /// How much a break is worn down, 0 to 1.
    pub soften: f32,
    /// Splinters as `[x, y, width, height]` on the middle plane. `x`, `y`
    /// and `width` are in units of the radius; `height` is in metres. Only
    /// the first [`spike_count`](Self::spike_count) entries are used.
    pub spikes: [[f32; 4]; crate::options::MAX_SPLINTERS as usize],
    /// Number of entries in [`spikes`](Self::spikes) in use.
    pub spike_count: usize,
}

impl End {
    /// Prepares an end of `kind` on a stem of `radius`, with `seed` picking
    /// its random details and `soften` (0 to 1) wearing down a break.
    pub(crate) fn new(kind: EndKind, radius: f32, seed: u32, soften: f32) -> Self {
        let mut rng = Rng::new(seed);
        let phase = rng.range(0.0, TAU);
        let mut spikes = [[0.0; 4]; crate::options::MAX_SPLINTERS as usize];
        let mut spike_count = 0;
        if let EndKind::Snapped {
            jaggedness,
            splinters,
            ..
        } = kind
        {
            // Scatter splinters over the break, mostly away from the middle.
            let jag = jaggedness * 2.0 * radius;
            for spike in spikes.iter_mut().take(splinters as usize) {
                let (rho, angle) = (rng.range(0.25, 0.95).sqrt(), rng.range(0.0, TAU));
                *spike = [
                    rho * angle.cos(),
                    rho * angle.sin(),
                    rng.range(0.1, 0.25),
                    jag * SNAP_SPIKE * rng.range(0.4, 1.0) * (1.0 - 0.7 * soften),
                ];
            }
            spike_count = splinters as usize;
        }
        Self {
            kind,
            reach: kind.reach(radius),
            radius,
            phase,
            seed,
            soften,
            spikes,
            spike_count,
        }
    }

    /// Returns the end surface's height above its middle plane, along the
    /// outward direction, at fraction `rho` (0 at the middle, 1 at the rim)
    /// and angle `theta`. It always lies within `-reach..=reach`.
    pub(crate) fn height(&self, rho: f32, theta: f32) -> f32 {
        let r = self.radius;
        let (x, y) = (
            rho * (theta - self.phase).cos(),
            rho * (theta - self.phase).sin(),
        );
        match self.kind {
            EndKind::Saw { tilt } => tilt.to_radians().tan() * r * x,
            EndKind::Chainsaw { tilt, step } => {
                let half_step = step * r;
                let side = smoothstep(-0.06, 0.06, y) * 2.0 - 1.0;
                tilt.to_radians().tan() * r * x + half_step * side
            }
            EndKind::Axe { angle } => {
                let c = r / (0.5 * angle.to_radians()).tan();
                let facets = value_noise(Vec3::new(x * 3.0, y * 3.0, 0.5), self.seed);
                c * (0.5 - x.abs()) + 0.05 * c * facets
            }
            EndKind::Snapped {
                jaggedness,
                one_sided,
                ..
            } => {
                let jag = jaggedness * 2.0 * r * (1.0 - 0.5 * self.soften);
                let rough = fbm(Vec3::new(x * 2.5, y * 2.5, 0.37), self.seed, 3);
                let mut spike: f32 = 0.0;
                for s in &self.spikes[..self.spike_count] {
                    let d = ((x - s[0]).powi(2) + (y - s[1]).powi(2)).sqrt();
                    let w = (1.0 - d / s[2]).max(0.0);
                    spike = spike.max(s[3] * w * w);
                }
                SNAP_BASE * jag * rough + one_sided * jag * x + spike
            }
            EndKind::Taper => self.reach * (1.0 - rho * rho).max(0.0).sqrt(),
        }
    }

    /// Whether this end shows cut or broken wood, rather than a bark tip.
    pub(crate) fn shows_wood(&self) -> bool {
        !matches!(self.kind, EndKind::Taper)
    }
}

/// A stump's flared base.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Flare {
    /// Distance along the stem from its start to the ground, in metres.
    pub ground: f32,
    /// Height of the flare above the ground, in metres.
    pub height: f32,
    /// Extra radius at the ground, as a fraction of the radius.
    pub width: f32,
    /// Number of buttress ridges around the base.
    pub lobes: u32,
    /// How strongly the buttresses stand out, 0 to 1.
    pub lobe_depth: f32,
    /// Angle in radians that turns the buttresses around the stem.
    pub phase: f32,
}

impl Flare {
    /// Returns the radius multiplier at distance `s` along the stem and
    /// angle `theta`.
    fn factor(&self, s: f32, theta: f32) -> f32 {
        let x = ((s - self.ground) / self.height).clamp(0.0, 1.0);
        let lobes = if self.lobes > 0 {
            1.0 + self.lobe_depth * (self.lobes as f32 * theta + self.phase).cos()
        } else {
            1.0
        };
        1.0 + self.width * (1.0 - x).powi(2) * lobes
    }
}

/// A hollow core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hollow {
    /// Radius as a fraction of the nominal radius.
    pub ratio: f32,
    /// Whether the hollow opens at the base and tip ends.
    pub open: [bool; 2],
    /// Depth from an open end when only one end is open, as a fraction of
    /// the stem between the two ends' cuts.
    pub depth: f32,
    /// How uneven the inner wall is, 0 to 0.5.
    pub roughness: f32,
}

/// Bark settings for one stem, with decay already applied. See
/// [`crate::BarkOptions`] for what each value means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bark {
    /// Thickness as a fraction of the radius.
    pub thickness: f32,
    /// Fraction peeled off, 0 to 1.
    pub stripped: f32,
    /// Typical size of a peeled patch, in metres.
    pub patch_size: f32,
    /// Depth of the cracks between bark plates, 0 to 1.
    pub cracks: f32,
    /// Typical width of a bark plate, in metres.
    pub plate_size: f32,
}

/// One stem of a piece.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Limb {
    /// What the stem is: main stem, stub, side branch, twig or root.
    pub role: Role,
    /// Size class that tags the stem's mesh part.
    pub size: SizeClass,
    /// Index of the stem this one grows from, and where along it (`u`).
    /// `None` for the main stem.
    pub parent: Option<(usize, f32)>,
    /// The centre line.
    pub curve: Curve,
    /// Nominal radius at the start and end of the tapering stretch, in
    /// metres.
    pub radius: [f32; 2],
    /// Distance from the start, in metres, where tapering begins (the
    /// ground, for a stump).
    pub taper_from: f32,
    /// Seed for the stem's surface noise.
    pub seed: u32,
    /// Base and tip ends.
    pub ends: [End; 2],
    /// Bark settings.
    pub bark: Bark,
    /// Bump height as a fraction of the radius.
    pub lumps: f32,
    /// Roughly how many bumps fit around the stem.
    pub lump_scale: f32,
    /// A stump's flared base, if any.
    pub flare: Option<Flare>,
    /// A hollow core, if any.
    pub hollow: Option<Hollow>,
    /// Flattening toward `up`, 0 to 0.6. It is applied after settling.
    pub squash: f32,
    /// The world's up direction, used for flattening.
    pub up: Vec3,
}

impl Limb {
    /// Length of the centre line, in metres.
    pub(crate) fn length(&self) -> f32 {
        self.curve.length()
    }

    /// Nominal radius at the base, in metres.
    pub(crate) fn base_radius(&self) -> f32 {
        self.radius[0]
    }

    /// Returns the smooth radius at `u` and `theta`, before bumps, bark
    /// cracks and peeling: the taper and a stump's flare.
    pub(crate) fn nominal_radius(&self, u: f32, theta: f32) -> f32 {
        let length = self.length();
        let s = u * length;
        let span = (length - self.taper_from).max(1e-6);
        let t = ((s - self.taper_from) / span).clamp(0.0, 1.0);
        let r = self.radius[0] + (self.radius[1] - self.radius[0]) * t;
        match &self.flare {
            Some(f) => r * f.factor(s, theta),
            None => r,
        }
    }

    /// Returns a point on an imaginary cylinder around the stem, in metres,
    /// for sampling noise that wraps seamlessly around it.
    fn wrap(&self, u: f32, theta: f32) -> Vec3 {
        let r = self.radius[0];
        Vec3::new(theta.cos() * r, theta.sin() * r, u * self.length())
    }

    /// Returns how bare the bark is at `u`, `theta`: 0 where bark remains, 1
    /// where it has peeled off.
    pub(crate) fn bare(&self, u: f32, theta: f32) -> f32 {
        let stripped = self.bark.stripped;
        if stripped <= 0.0 {
            return 0.0;
        }
        if stripped >= 1.0 {
            return 1.0;
        }
        let n = fbm(
            self.wrap(u, theta) / self.bark.patch_size,
            seed_for(self.seed, BARK_PATCHES),
            3,
        );
        let threshold = 0.55 - 1.1 * stripped;
        smoothstep(threshold - 0.05, threshold + 0.05, n)
    }

    /// Returns the outer surface's distance from the centre line at `u`,
    /// `theta`, in metres, with bumps, cracks between bark plates and
    /// peeled patches.
    pub(crate) fn outer_radius(&self, u: f32, theta: f32) -> f32 {
        let nominal = self.nominal_radius(u, theta);
        let wrap = self.wrap(u, theta);
        let k = self.lump_scale / (TAU * self.radius[0]);
        let bumps = 1.0 + self.lumps * fbm(wrap * k, self.seed, 2);
        let bark = self.bark.thickness * nominal;
        let bare = self.bare(u, theta);

        // Cracks between plates, in elongated cells along the grain.
        let plate = self.bark.plate_size;
        let cell = Vec3::new(wrap.x / plate, wrap.y / plate, wrap.z / (3.0 * plate));
        let crack = 1.0
            - smoothstep(
                0.0,
                0.12,
                cell_border(cell, seed_for(self.seed, BARK_CRACKS)),
            );
        let crack_depth = self.bark.cracks * bark * crack * (1.0 - bare);

        nominal * bumps - bark * bare - crack_depth
    }

    /// Returns the frame at `u`.
    pub(crate) fn frame(&self, u: f32) -> Frame {
        self.curve.at(u)
    }

    /// Returns the offset from the centre line in direction `theta` at
    /// distance `radius`, flattened toward the ground once settled.
    pub(crate) fn offset(&self, frame: &Frame, theta: f32, radius: f32) -> Vec3 {
        let d = frame.radial(theta) * radius;
        if self.squash <= 0.0 {
            return d;
        }
        let up = perpendicular(self.up, frame.tangent);
        let along = d.dot(up);
        let across = d - up * along;
        across * (1.0 + 0.5 * self.squash) + up * along * (1.0 - self.squash)
    }

    /// Returns the point on the outer surface at `u`, `theta`.
    pub(crate) fn point(&self, u: f32, theta: f32) -> Vec3 {
        let frame = self.frame(u);
        frame.position + self.offset(&frame, theta, self.outer_radius(u, theta))
    }

    /// Returns the hollow's inner radius at `u`, `theta`, kept inside the
    /// outer surface.
    pub(crate) fn inner_radius(&self, u: f32, theta: f32) -> f32 {
        let Some(h) = &self.hollow else { return 0.0 };
        let wrap = self.wrap(u, theta);
        let n = fbm(
            wrap * (2.0 / self.radius[0]),
            seed_for(self.seed, HOLLOW_WALL),
            2,
        );
        let r = h.ratio * self.nominal_radius(u, theta) * (1.0 + h.roughness * n);
        r.min(0.9 * self.outer_radius(u, theta))
    }

    /// Returns the point on the hollow's inner wall at `u`, `theta`.
    pub(crate) fn inner_point(&self, u: f32, theta: f32) -> Vec3 {
        let frame = self.frame(u);
        frame.position + self.offset(&frame, theta, self.inner_radius(u, theta))
    }

    /// Returns `u` at the middle plane of end `e` (0 base, 1 tip).
    pub(crate) fn end_plane(&self, e: usize) -> f32 {
        let reach = self.ends[e].reach / self.length();
        if e == 0 { reach } else { 1.0 - reach }
    }

    /// Returns the outward direction (+1 or -1 along the curve) of end `e`.
    pub(crate) fn end_sign(e: usize) -> f32 {
        if e == 0 { -1.0 } else { 1.0 }
    }

    /// Returns `u` where the wall's column at `theta` meets end `e`.
    pub(crate) fn wall_end(&self, e: usize, theta: f32) -> f32 {
        let h = self.ends[e].height(1.0, theta) / self.length();
        self.end_plane(e) + Self::end_sign(e) * h
    }

    /// Returns `u` at fraction `t` (0 base, 1 tip) along the wall's column
    /// at `theta`, between the two ends.
    pub(crate) fn wall_u(&self, t: f32, theta: f32) -> f32 {
        // The ends return the exact end values, so rims built from
        // `wall_end` match the wall's first and last rings bit for bit.
        if t <= 0.0 {
            return self.wall_end(0, theta);
        }
        if t >= 1.0 {
            return self.wall_end(1, theta);
        }
        let (a, b) = (self.wall_end(0, theta), self.wall_end(1, theta));
        a + (b - a) * t
    }

    /// Returns the point on end `e`'s surface at fraction `rho` (0 middle, 1
    /// rim) and angle `theta`. At `rho = 1` it meets the wall, up to
    /// rounding; mesh builders reuse the wall's own rim points there.
    pub(crate) fn end_point(&self, e: usize, rho: f32, theta: f32) -> Vec3 {
        let plane = self.frame(self.end_plane(e));
        let out = plane.tangent * Self::end_sign(e);
        let rim = self.point(self.wall_end(e, theta), theta) - plane.position;
        let along = rim.dot(out);
        let across = rim - out * along;

        // On a curved stem the rim's height differs a little from the end
        // shape's; blend that difference in toward the rim, so rings near
        // the rim never fold over it.
        let end = &self.ends[e];
        let mismatch = along - end.height(1.0, theta);
        plane.position + across * rho + out * (end.height(rho, theta) + mismatch * rho)
    }

    /// Returns `u` where the hollow's inner wall meets end `e`'s surface at
    /// `theta`, with the end's ring there at fraction `rho`.
    pub(crate) fn inner_end(&self, e: usize, rho: f32, theta: f32) -> f32 {
        let h = self.ends[e].height(rho, theta) / self.length();
        self.end_plane(e) + Self::end_sign(e) * h
    }

    /// Returns the fraction of the rim radius where the hollow meets end
    /// `e` at `theta`.
    pub(crate) fn hollow_rho(&self, e: usize, theta: f32) -> f32 {
        let u = self.end_plane(e);
        let inner = self.inner_radius(u, theta);
        let outer = self.outer_radius(u, theta).max(1e-6);
        (inner / outer)
            .clamp(0.05, 0.92)
            .min(self.bark_rho(e, theta) - 0.02)
    }

    /// Returns the fraction of the rim radius where bark gives way to wood
    /// on end `e` at `theta`: nearly 1 where the bark has peeled off. It
    /// stays just below 1, so the bark band never shrinks to nothing and
    /// its two rings never land on the same points.
    pub(crate) fn bark_rho(&self, e: usize, theta: f32) -> f32 {
        let u = self.wall_end(e, theta);
        let outer = self.outer_radius(u, theta).max(1e-6);
        let bark =
            self.bark.thickness * self.nominal_radius(u, theta) * (1.0 - self.bare(u, theta));
        (1.0 - bark / outer).clamp(0.5, 0.995)
    }

    /// Returns the `u` range of the hollow's inner wall: from where it ends
    /// deep inside (or the base opening) to where it ends (or the tip
    /// opening). `None` when the stem has no visible hollow.
    pub(crate) fn hollow_span(&self) -> Option<(HollowEnd, HollowEnd)> {
        let h = self.hollow.as_ref()?;

        // A blind hollow stays between the two ends' cuts, with room left
        // for its rounded bottom, so it never breaks through the far end.
        let length = self.length();
        let (first, last) = (
            self.ends[0].reach * 2.0 / length,
            1.0 - self.ends[1].reach * 2.0 / length,
        );
        let bottom = 0.8 * h.ratio * self.radius[0].max(self.radius[1]) / length;
        let room = (last - first - bottom).max(0.01);
        match h.open {
            [true, true] => Some((HollowEnd::Open(0), HollowEnd::Open(1))),
            [true, false] => Some((
                HollowEnd::Open(0),
                HollowEnd::Closed(first + h.depth * room),
            )),
            [false, true] => Some((HollowEnd::Closed(last - h.depth * room), HollowEnd::Open(1))),
            [false, false] => None,
        }
    }
}

/// One end of a hollow's inner wall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum HollowEnd {
    /// The hollow opens through end `e`.
    Open(usize),
    /// The hollow stops at this `u`, closed by a rounded bottom.
    Closed(f32),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(kind: EndKind) -> Limb {
        let curve = Curve::through(&[Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)], 32, Vec3::Y);
        let end = End::new(kind, 0.15, 7, 0.0);
        Limb {
            role: Role::Main,
            size: SizeClass::Trunk,
            parent: None,
            curve,
            radius: [0.15, 0.15],
            taper_from: 0.0,
            seed: 3,
            ends: [End::new(EndKind::Saw { tilt: 0.0 }, 0.15, 8, 0.0), end],
            bark: Bark {
                thickness: 0.1,
                stripped: 0.0,
                patch_size: 0.2,
                cracks: 0.0,
                plate_size: 0.05,
            },
            lumps: 0.0,
            lump_scale: 3.0,
            flare: None,
            hollow: None,
            squash: 0.0,
            up: Vec3::Y,
        }
    }

    #[test]
    fn end_heights_stay_within_reach() {
        let kinds = [
            EndKind::Saw { tilt: 30.0 },
            EndKind::Chainsaw {
                tilt: 20.0,
                step: 0.3,
            },
            EndKind::Axe { angle: 40.0 },
            EndKind::Snapped {
                jaggedness: 1.0,
                splinters: 12,
                one_sided: 1.0,
            },
            EndKind::Taper,
        ];
        for kind in kinds {
            let end = End::new(kind, 0.2, 11, 0.0);
            for i in 0..=20 {
                for j in 0..64 {
                    let h = end.height(i as f32 / 20.0, j as f32 / 64.0 * TAU);
                    assert!(h.abs() <= end.reach + 1e-5, "{kind:?}: {h} > {}", end.reach);
                }
            }
        }
    }

    #[test]
    fn end_rim_meets_the_wall() {
        let limb = straight(EndKind::Snapped {
            jaggedness: 0.5,
            splinters: 4,
            one_sided: 0.5,
        });
        for j in 0..16 {
            let theta = j as f32 / 16.0 * TAU;
            let wall = limb.point(limb.wall_end(1, theta), theta);
            let cap = limb.end_point(1, 1.0, theta);
            assert!(wall.distance(cap) < 1e-4);
        }
    }

    #[test]
    fn peeled_bark_sinks_the_surface() {
        let mut limb = straight(EndKind::Taper);
        let full = limb.outer_radius(0.5, 1.0);
        limb.bark.stripped = 1.0;
        let bare = limb.outer_radius(0.5, 1.0);
        assert!((full - bare - 0.1 * 0.15).abs() < 1e-5);
    }
}
