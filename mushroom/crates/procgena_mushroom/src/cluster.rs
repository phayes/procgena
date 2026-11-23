//! Clusters: several mushrooms of one species growing together, in a tuft,
//! a row or a fairy ring.

use std::f32::consts::TAU;

use glam::{Affine3A, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::mushroom::MushroomBody;
use crate::options::{MushroomOptions, OptionsError, int_range, within};
use crate::rng::Rng;

/// Largest allowed [`ClusterOptions::stems`].
pub const MAX_STEMS: u32 = 64;
/// Largest allowed [`ClusterOptions::variants`].
pub const MAX_VARIANTS: u32 = 16;
/// Positions tried when sliding a member outward to find room for it.
const SLIDE_STEPS: u32 = 24;
/// Furthest a member slides from its starting point, in cap radii of the
/// first variant at full size.
const SLIDE_RANGE: f32 = 5.0;

/// Random-number stream for the variants' differences.
const STREAM_VARIANTS: u32 = 11;
/// Random-number stream for member sizes and placement.
const STREAM_LAYOUT: u32 = 12;

/// How the mushrooms of a cluster are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterLayout {
    /// A tuft: members spread around a shared centre and lean outward.
    #[default]
    Tuft,
    /// A row, as mushrooms grow along a crack or a fallen branch.
    Line,
    /// A fairy ring: members spaced around a wide circle, as mushrooms grow
    /// at the edge of a fungus spreading underground. See
    /// [`FairyRingOptions`].
    FairyRing,
}

/// Settings for a [`ClusterLayout::FairyRing`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FairyRingOptions {
    /// Radius of the circle as a multiple of the cap diameter, from 1
    /// through 200. Real fairy rings are often several metres across.
    pub radius: f32,
    /// How far members stray from the circle, inward or outward, as a
    /// fraction of its radius, from 0 (exactly on it) through 0.5.
    pub width: f32,
    /// How much of the circle carries mushrooms, from 0.05 (a short arc)
    /// through 1 (the whole ring).
    pub arc: f32,
}

impl Default for FairyRingOptions {
    fn default() -> Self {
        Self {
            radius: 12.0,
            width: 0.08,
            arc: 1.0,
        }
    }
}

/// How many mushrooms grow together, and how they are arranged. Part of
/// [`MushroomOptions::cluster`].
///
/// With one stem, the mushroom grows alone, exactly as the rest of its
/// options describe. With more, each member is a slightly different copy:
/// the [`crate::Mushroom`] is then the whole group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterOptions {
    /// How many mushrooms grow together, from 1 (a single mushroom) up to
    /// [`MAX_STEMS`]. A member that can't find room is left out, so a
    /// cluster can end up smaller.
    pub stems: u32,
    /// How the members are arranged.
    pub layout: ClusterLayout,
    /// How much smaller than the full size members can be, from 0 (all the
    /// same size) through 0.9 (from a tenth of the size up to the full
    /// size).
    pub size_variation: f32,
    /// Most a member leans away from the cluster, in degrees, from 0
    /// through 60.
    pub lean: f32,
    /// How far each member's stem bends away from the cluster's centre, as
    /// a multiple of its height, from 0 through 1. Above 0, stems start
    /// close together and fan outward, replacing
    /// [`StemOptions::lean`](crate::StemOptions::lean); combine it with
    /// [`StemOptions::upturn`](crate::StemOptions::upturn) so the caps turn
    /// back up. 0 keeps each stem's own lean, pointing in a random
    /// direction.
    pub sweep: f32,
    /// Smallest gap between members, as a fraction of the cap radius, from
    /// 0 through 2.
    pub spacing: f32,
    /// How many different meshes the members are copies of, from 1 through
    /// [`MAX_VARIANTS`]. Fewer variants are cheaper to draw; more look less
    /// repetitive. A cluster never has more variants than stems.
    pub variants: u32,
    /// The circle used by [`ClusterLayout::FairyRing`].
    pub fairy_ring: FairyRingOptions,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            stems: 1,
            layout: ClusterLayout::Tuft,
            size_variation: 0.6,
            lean: 25.0,
            sweep: 0.0,
            spacing: 0.05,
            variants: 3,
            fairy_ring: FairyRingOptions::default(),
        }
    }
}

impl ClusterOptions {
    /// Checks every setting against the range given in its documentation.
    ///
    /// # Errors
    ///
    /// Returns the first setting found out of range.
    pub fn validate(&self) -> Result<(), OptionsError> {
        int_range("cluster.stems", self.stems, 1, MAX_STEMS)?;
        within("cluster.size_variation", self.size_variation, 0.0, 0.9)?;
        within("cluster.lean", self.lean, 0.0, 60.0)?;
        within("cluster.sweep", self.sweep, 0.0, 1.0)?;
        within("cluster.spacing", self.spacing, 0.0, 2.0)?;
        int_range("cluster.variants", self.variants, 1, MAX_VARIANTS)?;
        let f = &self.fairy_ring;
        within("cluster.fairy_ring.radius", f.radius, 1.0, 200.0)?;
        within("cluster.fairy_ring.width", f.width, 0.0, 0.5)?;
        within("cluster.fairy_ring.arc", f.arc, 0.05, 1.0)
    }
}

/// One placed copy of a mushroom: which body to draw, and where.
///
/// To draw it, apply [`transform`](Self::transform) to the body's mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MushroomInstance {
    /// Which mushroom to draw: an index into the `bodies()` list of the
    /// mushroom or scatter that made this instance.
    pub variant: u32,
    /// Position of the stem base, in metres.
    pub translation: Vec3,
    /// Rotation applied to the mushroom, which grows along +Y before
    /// rotating.
    pub rotation: Quat,
    /// Size multiplier, the same along every axis.
    pub scale: f32,
}

impl MushroomInstance {
    /// Returns the transform that places the body's mesh: scale, then
    /// rotate, then move.
    pub fn transform(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation,
            self.translation,
        )
    }
}

/// Makes `count` variations of `base`, each a single mushroom with its own
/// seed; all but the first also vary stem and cap height a little. `salt`
/// changes which variations are made, so different groups of the same
/// species differ.
///
/// With `cluster.sweep` above 0, every variant's stem leans by about
/// `sweep` times its height toward +X, so [`arrange`] can turn it to face
/// away from the cluster's centre. Otherwise each keeps `base`'s lean, in a
/// random direction.
pub(crate) fn make_variants(
    base: &MushroomOptions,
    count: u32,
    salt: u32,
) -> Result<Vec<MushroomBody>, OptionsError> {
    base.validate()?;
    let sweep = base.cluster.sweep;
    let mut rng = Rng::new(base.seed ^ salt, STREAM_VARIANTS);
    (0..count)
        .map(|i| {
            let mut o = base.clone();
            o.cluster = ClusterOptions::default();
            o.seed = base.seed.wrapping_add(i.wrapping_mul(0x9e37_79b9) ^ salt);
            if i > 0 {
                o.stem.height *= rng.jitter(0.2);
                o.cap.height *= rng.jitter(0.1);
            }
            o.stem.lean_direction = rng.range(0.0, 360.0);

            // Sweeping stems all lean toward +X, by slightly different
            // amounts.
            if sweep > 0.0 {
                let amount = if i > 0 { rng.jitter(0.2) } else { 1.0 };
                o.stem.lean = (sweep * amount).min(1.0);
                o.stem.lean_direction = 0.0;
            }
            MushroomBody::new(o)
        })
        .collect()
}

/// A rough stand-in for a mushroom's volume, used to keep members apart:
/// a capsule for the stem and a sphere for the cap.
#[derive(Debug, Clone, Copy)]
struct Proxy {
    stem: [Vec3; 2],
    stem_radius: f32,
    cap: Vec3,
    cap_radius: f32,
}

impl Proxy {
    /// Returns the stand-in for mushroom `m` placed by `at`, whose sizes
    /// are multiplied by `scale`. `scale` should match the scale in `at`.
    fn new(m: &MushroomBody, at: Affine3A, scale: f32) -> Self {
        let cap_mid = (m.cap_centre() + m.apex()) * 0.5;
        Self {
            stem: [
                at.transform_point3(Vec3::ZERO),
                at.transform_point3(m.cap_centre()),
            ],
            stem_radius: m.base_radius() * scale,
            cap: at.transform_point3(cap_mid),
            cap_radius: m.cap_radius() * scale,
        }
    }

    /// Returns true if the two stand-ins come closer than `gap` metres. The
    /// stem test checks nine points along this stem, so it is approximate.
    fn hits(&self, other: &Proxy, gap: f32) -> bool {
        let caps = self.cap.distance(other.cap) < self.cap_radius + other.cap_radius + gap;
        let cap_stem = |a: &Proxy, b: &Proxy| {
            segment_distance(a.cap, b.stem) < a.cap_radius + b.stem_radius + gap
        };
        let stems = (0..=8).any(|k| {
            let p = self.stem[0].lerp(self.stem[1], k as f32 / 8.0);
            segment_distance(p, other.stem) < self.stem_radius + other.stem_radius + gap
        });
        caps || cap_stem(self, other) || cap_stem(other, self) || stems
    }
}

/// Returns the distance from point `p` to the nearest point of the line
/// segment between `s[0]` and `s[1]`.
fn segment_distance(p: Vec3, s: [Vec3; 2]) -> f32 {
    let d = s[1] - s[0];
    let t = if d.length_squared() > 0.0 {
        ((p - s[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(s[0] + d * t)
}

/// Places the members of a cluster of `variants` with settings `o` and
/// returns them, relative to the cluster's centre on the ground. `seed`
/// picks the sizes and positions, and `variants` must not be empty.
///
/// Each member starts near the centre, at its place in a row, or on the
/// fairy ring, and slides outward until its stand-in clears the members
/// already placed; one that never finds room is left out.
pub(crate) fn arrange(
    variants: &[MushroomBody],
    o: &ClusterOptions,
    seed: u32,
) -> Vec<MushroomInstance> {
    let mut rng = Rng::new(seed, STREAM_LAYOUT);
    let count = o.stems;
    let reach = variants[0].cap_radius();
    let gap = variants[0].cap_radius() * o.spacing;
    let ring_radius = o.fairy_ring.radius * reach * 2.0;

    // Place the largest first, so big mushrooms take the middle and small
    // ones gather round them.
    let mut scales: Vec<f32> = (0..count)
        .map(|_| 1.0 - rng.range(0.0, o.size_variation))
        .collect();
    scales.sort_by(|a, b| b.total_cmp(a));

    let mut placed: Vec<(MushroomInstance, Proxy)> = Vec::new();
    let start_angle = rng.range(0.0, TAU);
    for (k, &scale) in scales.iter().enumerate() {
        let variant = rng.int(0, variants.len() as u32 - 1);
        let mushroom = &variants[variant as usize];

        // Where the member starts, which way it slides and leans, and how
        // far it may slide. In a tuft, the first (largest) member stays
        // nearly upright.
        let (start, direction, slide) = match o.layout {
            ClusterLayout::Tuft => {
                let angle = start_angle + k as f32 / count as f32 * TAU + rng.range(-0.3, 0.3);
                let out = Vec3::new(angle.cos(), 0.0, angle.sin());
                (Vec3::ZERO, out, SLIDE_RANGE)
            }
            ClusterLayout::Line => {
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                let along = (k as f32 - (count as f32 - 1.0) * 0.5) * reach * 0.9;
                (Vec3::X * along, Vec3::Z * side, SLIDE_RANGE)
            }
            ClusterLayout::FairyRing => {
                // Evenly around the arc, a little uneven, and a little in or
                // out from the circle.
                let step = o.fairy_ring.arc * TAU / count as f32;
                let angle = start_angle + (k as f32 + 0.5 + rng.range(-0.3, 0.3)) * step;
                let out = Vec3::new(angle.cos(), 0.0, angle.sin());
                let radius = ring_radius * (1.0 + rng.range(-1.0, 1.0) * o.fairy_ring.width);
                (out * radius, out, SLIDE_RANGE * 0.5)
            }
        };
        let lean_amount = if k == 0 && o.layout == ClusterLayout::Tuft {
            0.2
        } else {
            rng.range(0.4, 1.0)
        };
        // Turn the member about its stem. Sweeping variants bend toward +X,
        // so turn +X to point outward (give or take a little); otherwise
        // turn at random.
        let turn = rng.range(0.0, TAU);
        let turn = if o.sweep > 0.0 {
            -direction.z.atan2(direction.x) + (turn / TAU - 0.5) * 0.6
        } else {
            turn
        };
        let lean_axis = Vec3::Y.cross(direction).normalize_or(Vec3::X);
        let rotation = Quat::from_axis_angle(lean_axis, (o.lean * lean_amount).to_radians())
            * Quat::from_rotation_y(turn);

        // Slide outward step by step until the member clears every member
        // already placed.
        let mut chosen = None;
        for step in 0..=SLIDE_STEPS {
            let offset = start + direction * (reach * slide * step as f32 / SLIDE_STEPS as f32);
            let at =
                Affine3A::from_scale_rotation_translation(Vec3::splat(scale), rotation, offset);
            let proxy = Proxy::new(mushroom, at, scale);
            if placed.iter().all(|(_, p)| !p.hits(&proxy, gap)) {
                chosen = Some((offset, proxy));
                break;
            }
        }
        // Leave out a member that never finds room.
        let Some((offset, proxy)) = chosen else {
            continue;
        };
        placed.push((
            MushroomInstance {
                variant,
                translation: offset,
                rotation,
                scale,
            },
            proxy,
        ));
    }
    placed.into_iter().map(|(m, _)| m).collect()
}
