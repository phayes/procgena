//! Clusters: several mushrooms of one species growing together.

use std::f32::consts::TAU;

use glam::{Affine3A, Quat, Vec3};
use procgena_mesh::{Level, Object, Part, SurfaceMesh};
use serde::{Deserialize, Serialize};

use crate::detail::MushroomLod;
use crate::mushroom::Mushroom;
use crate::options::{MushroomOptions, OptionsError, int_range, positive, within};
use crate::rng::Rng;

/// Largest allowed [`ClusterOptions::count_max`].
pub const MAX_CLUSTER: u32 = 32;
/// Largest allowed [`ClusterOptions::variants`].
pub const MAX_VARIANTS: u32 = 16;
/// Positions tried when sliding a member outward to find room for it.
const SLIDE_STEPS: u32 = 24;
/// Furthest a member slides from its starting point, in cap radii of the
/// first variant at the largest scale.
const SLIDE_RANGE: f32 = 5.0;

/// Random-number stream for the variants' differences.
const STREAM_VARIANTS: u32 = 11;
/// Random-number stream for member count, sizes and placement.
const STREAM_LAYOUT: u32 = 12;

/// How a cluster's members are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterLayout {
    /// A tuft: members spread around a shared centre and lean outward.
    #[default]
    Tuft,
    /// A row, as mushrooms grow along a crack or a fallen branch.
    Line,
}

/// Settings for a [`MushroomCluster`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterOptions {
    /// Number that picks the member count, sizes and positions. The same
    /// seed and settings always give the same cluster.
    pub seed: u32,
    /// Fewest members, at least 1.
    pub count_min: u32,
    /// Most members, from `count_min` through [`MAX_CLUSTER`]. A member
    /// that can't find room is left out, so a cluster can end up smaller.
    pub count_max: u32,
    /// How the members are arranged.
    pub layout: ClusterLayout,
    /// Smallest member size as a multiple of the base mushroom's size. Must
    /// be positive.
    pub scale_min: f32,
    /// Largest member size as a multiple of the base mushroom's size; at
    /// least `scale_min`.
    pub scale_max: f32,
    /// Most a member leans away from the cluster, in degrees, from 0
    /// through 60.
    pub lean: f32,
    /// How far each member's stem bends away from the cluster's centre, as
    /// a multiple of its height, from 0 through 1. Above 0, stems start
    /// close together and fan outward, replacing the base mushroom's
    /// [`StemOptions::lean`](crate::StemOptions::lean); combine it with
    /// [`StemOptions::upturn`](crate::StemOptions::upturn) so the caps turn
    /// back up. 0 keeps each stem's own lean, pointing in a random direction.
    pub sweep: f32,
    /// Smallest gap between members, as a fraction of the base cap radius,
    /// from 0 through 2.
    pub spacing: f32,
    /// How many different meshes the members are copies of, from 1 through
    /// [`MAX_VARIANTS`]. Fewer variants are cheaper to draw; more look less
    /// repetitive.
    pub variants: u32,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            count_min: 3,
            count_max: 6,
            layout: ClusterLayout::Tuft,
            scale_min: 0.4,
            scale_max: 1.0,
            lean: 25.0,
            sweep: 0.0,
            spacing: 0.05,
            variants: 3,
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
        int_range("cluster.count_min", self.count_min, 1, MAX_CLUSTER)?;
        int_range(
            "cluster.count_max",
            self.count_max,
            self.count_min,
            MAX_CLUSTER,
        )?;
        positive("cluster.scale_min", self.scale_min)?;
        within(
            "cluster.scale_max",
            self.scale_max,
            self.scale_min,
            f32::MAX,
        )?;
        within("cluster.lean", self.lean, 0.0, 60.0)?;
        within("cluster.sweep", self.sweep, 0.0, 1.0)?;
        within("cluster.spacing", self.spacing, 0.0, 2.0)?;
        int_range("cluster.variants", self.variants, 1, MAX_VARIANTS)
    }
}

/// One placed copy of a mushroom: which variant to draw, and where.
///
/// To draw it, apply [`transform`](Self::transform) to the variant's mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MushroomInstance {
    /// Which mushroom to draw: an index into the `variants()` list of the
    /// cluster or scatter that made this instance.
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
    /// Returns the transform that places the variant's mesh: scale, then
    /// rotate, then move.
    pub fn transform(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation,
            self.translation,
        )
    }
}

/// Several mushrooms of one species, placed around the origin so they don't
/// overlap.
///
/// Each member is a copy of one of a few [`variants`](Self::variants), so a
/// renderer can mesh each variant once and draw every member from it. Use
/// [`object`](Self::object) if you need everything in a single mesh.
#[derive(Debug, Clone)]
pub struct MushroomCluster {
    variants: Vec<Mushroom>,
    members: Vec<MushroomInstance>,
}

impl MushroomCluster {
    /// Grows a cluster of mushrooms like `base`.
    ///
    /// The variants have different seeds from `base`, and all but the first
    /// have slightly different stem and cap heights. Each member's lean
    /// direction is chosen by the cluster (randomly, or away from the centre
    /// with [`ClusterOptions::sweep`]), so [`StemOptions::lean_direction`] in
    /// `base` is not used.
    ///
    /// # Errors
    ///
    /// Returns an error if a setting in `base` or `options` is out of range.
    ///
    /// [`StemOptions::lean_direction`]: crate::StemOptions::lean_direction
    pub fn new(base: &MushroomOptions, options: &ClusterOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let variants = make_variants(base, options.variants, options.seed, options.sweep)?;
        let members = arrange(&variants, options, Affine3A::IDENTITY, options.seed);
        Ok(Self { variants, members })
    }

    /// Returns the different mushrooms the members are copies of.
    pub fn variants(&self) -> &[Mushroom] {
        &self.variants
    }

    /// Returns the placed members, largest first.
    pub fn members(&self) -> &[MushroomInstance] {
        &self.members
    }

    /// Combines every member into one [`Object`], with one detail level for
    /// each level of `lod`. Parts of the same kind are merged, so the cluster
    /// has one cap part, one stem part, and so on.
    ///
    /// # Errors
    ///
    /// Returns an error if `lod` is invalid; see [`MushroomLod::validate`].
    pub fn object(&self, lod: &MushroomLod) -> Result<Object, OptionsError> {
        bake(&self.variants, &self.members, lod)
    }
}

/// Makes `count` variations of `base`, each with its own seed; all but the
/// first also vary stem and cap height a little. `seed` picks the
/// variations.
///
/// With `sweep` above 0, every variant's stem leans by about `sweep` times
/// its height toward +X, so [`arrange`] can turn it to face away from the
/// cluster's centre. Otherwise each keeps `base`'s lean, in a random
/// direction.
pub(crate) fn make_variants(
    base: &MushroomOptions,
    count: u32,
    seed: u32,
    sweep: f32,
) -> Result<Vec<Mushroom>, OptionsError> {
    base.validate()?;
    let mut rng = Rng::new(seed, STREAM_VARIANTS);
    (0..count)
        .map(|i| {
            let mut o = base.clone();
            o.seed = base.seed.wrapping_add(i.wrapping_mul(0x9e37_79b9) ^ seed);
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
            Mushroom::new(o)
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
    fn new(m: &Mushroom, at: Affine3A, scale: f32) -> Self {
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

/// Places a cluster's members around `site` and returns them, with
/// transforms that already include `site`. `seed` picks the count, sizes and
/// positions, and `variants` must not be empty.
///
/// Each member starts near the centre (or at its place in a row) and slides
/// outward until its stand-in clears the members already placed; one that
/// never finds room is left out.
pub(crate) fn arrange(
    variants: &[Mushroom],
    o: &ClusterOptions,
    site: Affine3A,
    seed: u32,
) -> Vec<MushroomInstance> {
    let mut rng = Rng::new(seed, STREAM_LAYOUT);
    let count = rng.int(o.count_min, o.count_max);
    let reach = variants[0].cap_radius() * o.scale_max;
    let gap = variants[0].cap_radius() * o.spacing;

    // Place the largest first, so big mushrooms take the middle and small
    // ones gather round them.
    let mut scales: Vec<f32> = (0..count)
        .map(|_| rng.range(o.scale_min, o.scale_max))
        .collect();
    scales.sort_by(|a, b| b.total_cmp(a));

    let mut placed: Vec<(MushroomInstance, Proxy)> = Vec::new();
    let start_angle = rng.range(0.0, TAU);
    for (k, &scale) in scales.iter().enumerate() {
        let variant = rng.int(0, variants.len() as u32 - 1);
        let mushroom = &variants[variant as usize];

        // Where the member starts, and which way it slides and leans. In a
        // tuft, the first (largest) member stays nearly upright.
        let (start, direction) = match o.layout {
            ClusterLayout::Tuft => {
                let angle = start_angle + k as f32 / count as f32 * TAU + rng.range(-0.3, 0.3);
                (Vec3::ZERO, Vec3::new(angle.cos(), 0.0, angle.sin()))
            }
            ClusterLayout::Line => {
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                let along = (k as f32 - (count as f32 - 1.0) * 0.5) * reach * 0.9;
                (Vec3::X * along, Vec3::Z * side)
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
            let offset =
                start + direction * (reach * SLIDE_RANGE * step as f32 / SLIDE_STEPS as f32);
            let local =
                Affine3A::from_scale_rotation_translation(Vec3::splat(scale), rotation, offset);
            let proxy = Proxy::new(mushroom, site * local, scale);
            if placed.iter().all(|(_, p)| !p.hits(&proxy, gap)) {
                chosen = Some((offset, proxy));
                break;
            }
        }
        // Leave out a member that never finds room, then fold the site's
        // placement into the member's own.
        let Some((offset, proxy)) = chosen else {
            continue;
        };
        let (_, site_rotation, site_translation) = site.to_scale_rotation_translation();
        let site_scale = site.matrix3.x_axis.length();
        placed.push((
            MushroomInstance {
                variant,
                translation: site_translation + site_rotation * (offset * site_scale),
                rotation: site_rotation * rotation,
                scale: scale * site_scale,
            },
            proxy,
        ));
    }
    placed.into_iter().map(|(m, _)| m).collect()
}

/// Combines every instance of `variants` into one object with a level per
/// level of `lod`, merging parts with the same tags across instances.
pub(crate) fn bake(
    variants: &[Mushroom],
    instances: &[MushroomInstance],
    lod: &MushroomLod,
) -> Result<Object, OptionsError> {
    let meshes: Vec<_> = variants
        .iter()
        .map(|v| v.lod_meshes(lod))
        .collect::<Result<_, _>>()?;
    let levels = lod
        .levels
        .iter()
        .enumerate()
        .map(|(l, level)| {
            // Move each instance's parts into place, then add them to the
            // merged part with the same tags.
            let mut merged: Vec<Part> = Vec::new();
            for instance in instances {
                let transform = instance.transform();
                for part in meshes[instance.variant as usize][l].1.object_parts() {
                    let moved = transform_part(&part, transform);
                    match merged.iter_mut().find(|m| m.tags == moved.tags) {
                        Some(target) => append_part(target, &moved),
                        None => merged.push(moved),
                    }
                }
            }
            Level {
                start: level.distance,
                parts: merged,
            }
        })
        .collect();
    Ok(Object { levels })
}

/// Returns a copy of `part` moved by `transform`, which must scale equally
/// along every axis so the normals stay correct.
fn transform_part(part: &Part, transform: Affine3A) -> Part {
    let mesh = SurfaceMesh {
        positions: part
            .mesh
            .positions
            .iter()
            .map(|&p| transform.transform_point3(p.into()).into())
            .collect(),
        normals: part
            .mesh
            .normals
            .iter()
            .map(|&n| {
                transform
                    .transform_vector3(n.into())
                    .normalize_or_zero()
                    .into()
            })
            .collect(),
        indices: part.mesh.indices.clone(),
    };
    let mut out = Part::new(part.tags.clone(), mesh);
    out.closed = part.closed;
    out.shells = part.shells.clone();
    out
}

/// Appends `extra`'s triangles to `target`. The result is closed only if
/// both were, and its list of closed pieces is kept only if both had one.
fn append_part(target: &mut Part, extra: &Part) {
    let base_vertex = target.mesh.positions.len() as u32;
    let base_triangle = target.triangle_count() as u32;
    let both_known = !target.shells.is_empty() && !extra.shells.is_empty();
    let mesh = std::sync::Arc::make_mut(&mut target.mesh);
    mesh.positions.extend_from_slice(&extra.mesh.positions);
    mesh.normals.extend_from_slice(&extra.mesh.normals);
    mesh.indices
        .extend(extra.mesh.indices.iter().map(|i| i + base_vertex));
    target.closed &= extra.closed;
    if both_known {
        target.shells.extend(
            extra
                .shells
                .iter()
                .map(|s| s.start + base_triangle..s.end + base_triangle),
        );
    } else {
        target.shells.clear();
    }
}
