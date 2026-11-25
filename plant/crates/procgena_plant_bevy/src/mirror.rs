//! Test-only CPU oracle for the GPU's placement and generation logic.
//!
//! This module mirrors the WESL shaders bit for bit where integers are
//! involved. It is compiled only for tests, so production code cannot
//! accidentally depend on it. Its WESL twins live in `shaders/common.wesl`
//! and `shaders/generate.wesl`.

use std::f32::consts::{PI, TAU};

use bevy::math::{IVec2, Vec2, Vec3};

use crate::PlantParams;
use crate::PlantSettings;
use crate::gpu_layout::LevelGpu;
use crate::render::view::MAX_BUCKETS;
use crate::scatters::level_entries;

use crate::shader_model::{
    CHILD_ROWS, CLUMP_PER_METRE, CLUMP_SIDE, ChunkLayout, DistanceLod, GRANDCHILD_ROWS, GUST_SIZE,
    PLANT_ROWS, TILE_POINTS, TUFT_ENTRIES_PER_TUFT, TUFT_SIDE, bare_noise, bare_threshold,
    best_candidate_tile, blue_noise_tile, clump_table, gust_noise, hash, hash2, lod_indices,
    quantised_tile, tile_position, toroidal_distance_squared, tuft_table, unit,
};

/// Vertical range an instance's root height can encode, in metres either side
/// of the view's instance origin.
pub(crate) const HEIGHT_RANGE: f32 = 256.0;

/// An instance's width encodes up to this many times its style's width, room
/// for the ±25% per-blade variation times the coverage widening, up to
/// `PlantSettings::MAX_WIDENING` (3.2×; 2.5× by default).
pub(crate) const WIDTH_RANGE: f32 = 4.0;

/// Reserved vertex-index stride for unbranched detail levels.
///
/// Bucket `b` at LOD `l` uses `base_vertex = (3b + l) * LOD_VERTEX_STRIDE`. The shader
/// decodes both from `vertex_index`, avoiding the optional GPU feature needed for
/// indirect `first_instance`.
pub(crate) const LOD_VERTEX_STRIDE: u32 = 64;
/// Direction of stem `k` within a `count`-stem crown. Stem 0 follows `crown`; other
/// stems are evenly spaced with seed-based jitter. The WESL twin is
/// `common::stem_facing`.
pub(crate) fn stem_facing(crown: f32, k: u32, count: u32, seed: u32) -> f32 {
    if k == 0 {
        return crown;
    }
    let jitter = 0.35 * (unit(hash(seed ^ 0x5e3)) - 0.5);
    (crown + (k as f32 + jitter) / count as f32).fract()
}

/// One candidate blade, before thinning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Candidate {
    /// Position inside the chunk, in metres from its minimum corner.
    pub local: Vec2,
    /// Thinning rank in `[0, 1)`: the blade is kept while its rank is below
    /// the local keep fraction. Lower ranks survive longer.
    pub rank: f32,
    /// Per-blade random seed.
    pub seed: u32,
}

impl ChunkLayout {
    /// Tile copies in one chunk.
    pub(crate) fn cells(&self) -> u32 {
        self.tiles_per_side * self.tiles_per_side
    }

    /// Candidates that must be evaluated so that every blade whose rank is
    /// below `keep` is among them. The WESL twin is in `cull`.
    pub(crate) fn candidates_needed(&self, keep: f32) -> u32 {
        let points = (keep.clamp(0.0, 1.0) * TILE_POINTS as f32).ceil() as u32;
        points * self.cells()
    }
}

/// Candidate `index` of World chunk `chunk` (its coordinates along X and Z)
/// in a field seeded with `field_seed`.
///
/// Candidates are numbered point-major: all tile copies' first point, then
/// all their second point, and so on. Rank grows with the index, so a chunk
/// that keeps only a fraction of its blades evaluates only a prefix of its
/// candidates. Each tile copy sits on a World tile cell, and its points come
/// from that cell and the seed alone, so any field with the same seed and
/// tile size places the same candidates there. The WESL twin is the start
/// of `evaluate`.
pub(crate) fn candidate(
    chunk: [i32; 2],
    field_seed: u32,
    layout: &ChunkLayout,
    index: u32,
) -> Candidate {
    let cells = layout.cells();
    let point = index / cells;
    let cell = index % cells;
    let cell_x = cell % layout.tiles_per_side;
    let cell_z = cell / layout.tiles_per_side;
    let cell_hash = hash2(
        hash2(
            world_tile(chunk[0], layout.tiles_per_side, cell_x),
            world_tile(chunk[1], layout.tiles_per_side, cell_z),
        ),
        field_seed,
    );
    // Each tile copy is shifted by its own random offset (wrapping), so the
    // repetition of one tile across the field is not visible.
    // Each tile copy is shifted by its own offset, in 1/65536ths of the
    // tile and wrapping.
    let tile = quantised_tile()[point as usize];
    let qx = ((tile & 0xffff) + (cell_hash & 0xffff)) & 0xffff;
    let qz = ((tile >> 16) + (cell_hash >> 16)) & 0xffff;
    Candidate {
        local: Vec2::new(
            tile_position(cell_x, qx, layout.tile_size),
            tile_position(cell_z, qz, layout.tile_size),
        ),
        rank: (point as f32 + 0.5) / TILE_POINTS as f32,
        seed: hash2(cell_hash, point),
    }
}

/// The World tile cell along one axis of tile `cell` in chunk `chunk`, with
/// `tiles` tiles along the chunk's side, as the bits of an `i32`. The WESL
/// twin is `world_tile`.
pub(crate) fn world_tile(chunk: i32, tiles: u32, cell: u32) -> u32 {
    chunk.wrapping_mul(tiles as i32).wrapping_add(cell as i32) as u32
}

/// The share of candidates to keep where the requested density is
/// `density × scale` blades per square metre (`scale` folds in the mask,
/// the distance falloff and the quality tier).
pub(crate) fn keep_fraction(density: f32, scale: f32, layout: &ChunkLayout) -> f32 {
    (density * scale / layout.candidate_density).clamp(0.0, 1.0)
}

impl DistanceLod {
    /// The High-tier table from the architecture, scaled to `radius`.
    pub(crate) fn for_radius(radius: f32) -> Self {
        Self::from_settings(&PlantSettings {
            radius,
            ..PlantSettings::HIGH
        })
    }

    /// Share of the full density drawn at `distance`. The architecture's
    /// table (100% to the start of LOD 1, 50% where LOD 2 starts, 15% at 90%
    /// of the radius, then fading to nothing at the radius so the edge never
    /// shows as a line), times `mid_range_density` from a dither band around
    /// `mid_range_start` on. The WESL twin is `density_falloff`.
    pub(crate) fn falloff(&self, distance: f32) -> f32 {
        let into_mid =
            ((distance - (self.mid_range_start - self.band * 0.5)) / self.band).clamp(0.0, 1.0);
        self.distance_thinning(distance) * lerp(1.0, self.mid_range_density, into_mid)
    }

    /// Fraction of branched plants retained at this distance: full density until the
    /// final fade, then zero at the radius. The WESL twin is `common::fade_falloff`.
    pub(crate) fn fade(&self, distance: f32) -> f32 {
        let edge = self.radius * self.fade_start;
        1.0 - ((distance - edge) / (self.radius - edge).max(1e-4)).clamp(0.0, 1.0)
    }

    /// The architecture's table alone. The WESL twin is `distance_thinning`.
    fn distance_thinning(&self, distance: f32) -> f32 {
        let [lod1, lod2] = self.lod_start;
        let [at_lod2, at_edge] = self.density_at;
        let edge = self.radius * self.fade_start;
        if distance <= lod1 {
            1.0
        } else if distance <= lod2 {
            lerp(1.0, at_lod2, (distance - lod1) / (lod2 - lod1))
        } else if distance <= edge {
            lerp(at_lod2, at_edge, (distance - lod2) / (edge - lod2))
        } else {
            lerp(
                at_edge,
                0.0,
                ((distance - edge) / (self.radius - edge).max(1e-4)).min(1.0),
            )
        }
    }

    /// The LOD of a blade at `distance` whose dither value is `dither` in
    /// `[0, 1)`. Inside a band around each boundary the blade switches when
    /// its own dither value says so, so the boundary dissolves into noise
    /// instead of a ring. The WESL twin is `select_lod`.
    pub(crate) fn lod(&self, distance: f32, dither: f32) -> u32 {
        self.lod_start
            .iter()
            .filter(|&&start| {
                let progress = (distance - (start - self.band * 0.5)) / self.band;
                dither < progress
            })
            .count() as u32
    }
}

/// The share of blades a view `offset` metres above the ground keeps with
/// `PlantCamera::radius_from_ground`, for a grass `radius`: the square of
/// `radius / (radius + offset)`, so the number of blades stays about the
/// same at any height. The WESL twin is `height_thinning`.
pub(crate) fn height_thinning(radius: f32, offset: f32) -> f32 {
    let share = radius / (radius + offset);
    share * share
}

/// Width multiplier that keeps coverage constant as density thins: blades
/// widen by the inverse square root of the density drop, at most `max` times
/// (2.5 by default). The WESL twin is `coverage_width`.
pub(crate) fn coverage_width(falloff: f32, max: f32) -> f32 {
    (1.0 / falloff.max(1.0 / (max * max)).sqrt()).min(max)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// One blade, unpacked. Every field is in the range the packed form can hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Blade {
    /// Root position relative to the view's instance origin, in metres. `x`
    /// and `z` are in `[-extent, extent]`, `y` in `[-HEIGHT_RANGE, HEIGHT_RANGE]`.
    pub position: Vec3,
    /// Yaw as a fraction of a full turn, `[0, 1)`.
    pub facing: f32,
    /// Height as a fraction of the style's tallest blade, `[0, 1]`.
    pub height: f32,
    /// Width as a fraction of `WIDTH_RANGE` times the style's width, `[0, 1]`.
    pub width: f32,
    /// Low bits of the blade's seed: colour variation and sway phase.
    pub seed: u8,
    /// Level of detail, 0–2.
    pub lod: u8,
    /// Index into the field table.
    pub field: u8,
    /// The blade's clump, 0–63: blades in the same 1 m clump share a tint.
    pub clump: u8,
    /// Tip offset from wind and interaction this frame, as a fraction of the
    /// blade's height in each horizontal axis, `[-1, 1]`.
    pub bend: Vec2,
    /// The same offset last frame, for motion vectors.
    pub previous_bend: Vec2,
}

fn quantize(value: f32, bits: u32) -> u32 {
    let max = ((1u64 << bits) - 1) as f32;
    (value.clamp(0.0, 1.0) * max).round() as u32
}

fn dequantize(value: u32, bits: u32) -> f32 {
    value as f32 / ((1u64 << bits) - 1) as f32
}

/// Steps of last frame's bend per unit of bend.
pub(crate) const BEND_DELTA_STEPS: f32 = 128.0;

/// One axis of a blade's bend in 16 bits: the bend in the low 12, and last
/// frame's bend as a 4-bit step from it. The WESL twin is `pack_bend`.
fn pack_bend(bend: f32, previous: f32) -> u32 {
    let now = quantize(bend * 0.5 + 0.5, 12);
    let decoded = dequantize(now, 12) * 2.0 - 1.0;
    let step = ((previous - decoded) * BEND_DELTA_STEPS)
        .round()
        .clamp(-8.0, 7.0);
    now | (((step as i32 + 8) as u32) << 12)
}

/// This frame's and last frame's bend from `pack_bend`'s 16 bits.
pub(crate) fn unpack_bend(bits: u32) -> (f32, f32) {
    let now = dequantize(bits & 0xfff, 12) * 2.0 - 1.0;
    let step = ((bits >> 12) & 0xf) as i32 - 8;
    (now, now + step as f32 / BEND_DELTA_STEPS)
}

/// Packs a blade into the 16-byte instance format. The WESL twins are
/// `pack` and `unpack`.
///
/// | Word | Bits |
/// | --- | --- |
/// | 0 | x (16), z (16), over ±`extent` around the view's instance origin |
/// | 1 | y (16) over ±`HEIGHT_RANGE`, facing (8), seed (8) |
/// | 2 | height (8), width (8), field (8), LOD (2), flags (6) |
/// | 3 | bend x and z (16 each: 12 for this frame, 4 for last frame's as a step from it) |
pub(crate) fn pack(blade: &Blade, extent: f32) -> [u32; 4] {
    let x = quantize(blade.position.x / (2.0 * extent) + 0.5, 16);
    let z = quantize(blade.position.z / (2.0 * extent) + 0.5, 16);
    let y = quantize(blade.position.y / (2.0 * HEIGHT_RANGE) + 0.5, 16);
    let facing = (blade.facing.rem_euclid(1.0) * 256.0) as u32 & 0xff;
    [
        x | (z << 16),
        y | (facing << 16) | (u32::from(blade.seed) << 24),
        quantize(blade.height, 8)
            | (quantize(blade.width, 8) << 8)
            | (u32::from(blade.field) << 16)
            | ((u32::from(blade.lod) & 3) << 24)
            | ((u32::from(blade.clump) & 63) << 26),
        pack_bend(blade.bend.x, blade.previous_bend.x)
            | (pack_bend(blade.bend.y, blade.previous_bend.y) << 16),
    ]
}

/// Unpacks the 16-byte instance format.
pub(crate) fn unpack(words: [u32; 4], extent: f32) -> Blade {
    let bend_x = unpack_bend(words[3] & 0xffff);
    let bend_z = unpack_bend(words[3] >> 16);
    Blade {
        position: Vec3::new(
            (dequantize(words[0] & 0xffff, 16) - 0.5) * 2.0 * extent,
            (dequantize(words[1] & 0xffff, 16) - 0.5) * 2.0 * HEIGHT_RANGE,
            (dequantize(words[0] >> 16, 16) - 0.5) * 2.0 * extent,
        ),
        facing: ((words[1] >> 16) & 0xff) as f32 / 256.0,
        height: dequantize(words[2] & 0xff, 8),
        width: dequantize((words[2] >> 8) & 0xff, 8),
        seed: (words[1] >> 24) as u8,
        lod: ((words[2] >> 24) & 3) as u8,
        field: ((words[2] >> 16) & 0xff) as u8,
        clump: (words[2] >> 26) as u8,
        bend: Vec2::new(bend_x.0, bend_z.0),
        previous_bend: Vec2::new(bend_x.1, bend_z.1),
    }
}
/// The clump a root at `point` (world XZ) belongs to, 0–63. The WESL twin
/// is `clump_of`.
pub(crate) fn clump_of(point: Vec2) -> u8 {
    let entry = (point * CLUMP_PER_METRE as f32).floor().as_ivec2() & (CLUMP_SIDE - 1);
    let index = (entry.y * CLUMP_SIDE + entry.x) as usize;
    ((clump_table()[index / 4] >> ((index % 4) * 8)) & 63) as u8
}

/// The nearest tuft centre to `q` (in tuft spacings) and the tuft's random
/// value in `[0, 1]`. The WESL twin is `tuft_at`.
pub(crate) fn tuft_at(q: Vec2) -> (Vec2, f32) {
    let entry = (q * TUFT_ENTRIES_PER_TUFT as f32).floor();
    let wrapped = entry.as_ivec2() & (TUFT_SIDE - 1);
    let word = tuft_table()[(wrapped.y * TUFT_SIDE + wrapped.x) as usize];
    let offset = (Vec2::new((word & 0xff) as f32, ((word >> 8) & 0xff) as f32) / 255.0 - 0.5) * 4.0;
    let centre = (entry + 0.5) / TUFT_ENTRIES_PER_TUFT as f32 + offset;
    (centre, ((word >> 16) & 0xff) as f32 / 255.0)
}
/// Chooses a plant index from ordered (chance, clustering) pairs.
///
/// Each chance applies to roots not selected by earlier plants. The last
/// plant with a positive density has chance 1 and fills the gaps. Clustering uses a
/// shuffled clump value to group nearby roots, with per-root randomness
/// within each clump. Returns 0 if no entry accepts the root.
/// The WESL twin is `pick_plant`.
pub(crate) fn pick_plant(seed: u32, clump: u32, plants: &[(f32, f32)]) -> usize {
    for (index, (chance, clustering)) in plants.iter().enumerate() {
        let k = index as u32 + 1;
        let salt = k.wrapping_mul(0x9e37_79b9);
        let scattered = unit(hash(seed ^ salt));
        let mixer = hash(salt ^ 0x5bd1_e995);
        let shuffled = (clump
            .wrapping_mul((mixer & 0x3e) | 1)
            .wrapping_add(mixer >> 8))
            & 63;
        let patched = (shuffled as f32 + unit(hash(seed ^ salt ^ 0x68e3_1da4))) / 64.0;
        let clustered = unit(hash(seed ^ salt ^ 0x1b87_3593)) < *clustering;
        if (if clustered { patched } else { scattered }) < *chance {
            return index;
        }
    }
    0
}

/// Tests whether this root uses the plant's accent appearance.
/// Uses a separate random choice from plant selection, with probability `share`.
/// The WESL twin is `is_accent`.
pub(crate) fn is_accent(seed: u32, share: f32) -> bool {
    unit(hash(seed ^ 0x2c1b_3c6d)) < share
}

/// The three Bézier control points of a blade of length `height`, leaning
/// `lean` (a fraction of the height, horizontal) along +Z, bent by `bend`
/// (a horizontal tip offset as a fraction of the height) and drooping by
/// `droop` (0 to 1), with its tip lowered onto the ground by `arch` (0 to 1).
/// The WESL twin is `blade_curve`.
///
/// The tip is pushed horizontally, then pulled back down so the curve keeps
/// its length: wind bends a blade, never stretches it. The construction is
/// Jahrmann and Wimmer's (2017), with the length correction applied twice so
/// that strong bends stay within 1% of the rest length. A drooping blade
/// reaches further, its tip sags and its control point rises: it stands up
/// from the root and arches over. An arching blade blends towards a parabola
/// from root to ground, rounder the less it droops, facing along its lean; the
/// bend sways only its top, so its tip stays put. With `arch` 0 the curve is
/// exactly the drooping blade's.
pub(crate) fn blade_curve(height: f32, lean: f32, bend: Vec2, droop: f32, arch: f32) -> [Vec3; 3] {
    let wanted = (Vec2::new(0.0, lean) + bend) * height;
    // An upright blade has no side to droop toward.
    let (droop, arch) = if wanted.length_squared() > 1e-12 {
        (droop, arch)
    } else {
        (0.0, 0.0)
    };
    let horizontal = (wanted.length() + droop * 0.7 * height).min(height * 0.95);
    let offset = wanted.normalize_or_zero() * horizontal;
    let p0 = Vec3::ZERO;
    let mut p2 = Vec3::new(
        offset.x,
        (height * height - horizontal * horizontal).max(0.0).sqrt() * (1.0 - 0.7 * droop),
        offset.y,
    );
    // The control point sits above the root, lower the further the tip leans,
    // which is what makes the blade arch instead of hinging at the root.
    let ratio = horizontal / height;
    let mut p1 = Vec3::new(0.0, height * (1.0 - ratio).max(0.05 * ratio.max(1.0)), 0.0);
    p1.y += (height * 0.9 - p1.y) * (droop * 2.0).min(1.0);
    if arch > 0.0 {
        // Blend towards an arch whose tip rests on the ground. Its top is 0.25 of
        // its span for an undrooped stem, leaving the root at 45°, and flattens to
        // 0.12 as droop rises. The span is sized with the length estimate below,
        // so that the correction leaves a full arch's tip on the ground. Both ends
        // are rooted, so the arch faces along the lean alone and the bend only
        // sways its top, by half as much as it would move a free tip.
        let leaning = Vec2::new(0.0, lean);
        let along = if leaning.length_squared() > 1e-12 {
            leaning.normalize()
        } else {
            wanted.normalize()
        };
        let rise = 0.25 - 0.13 * droop;
        let span = height * 3.0 / (2.0 + (1.0 + 16.0 * rise * rise).sqrt());
        let tip = Vec3::new(along.x * span, 0.0, along.y * span);
        let sway = bend * height * 0.5;
        let top = Vec3::new(
            along.x * span * 0.5 + sway.x,
            2.0 * rise * span,
            along.y * span * 0.5 + sway.y,
        );
        p1 += (top - p1) * arch;
        p2 += (tip - p2) * arch;
    }
    for _ in 0..2 {
        let chord = p2.distance(p0);
        let polygon = p1.distance(p0) + p2.distance(p1);
        let length = (2.0 * chord + polygon) / 3.0;
        let scale = height / length;
        p1 = p0 + (p1 - p0) * scale;
        p2 = p1 + (p2 - p1) * scale;
    }
    [p0, p1, p2]
}

/// A point on a quadratic Bézier curve.
pub(crate) fn bezier(points: &[Vec3; 3], t: f32) -> Vec3 {
    let u = 1.0 - t;
    points[0] * (u * u) + points[1] * (2.0 * u * t) + points[2] * (t * t)
}

/// One stem's `(droop, arch)` for [`blade_curve`], from a main segment's droop
/// setting (0 to 2), the stem's random number and its height as a fraction of
/// the tallest. The WESL twin is `common::stem_droop`.
pub(crate) fn stem_droop(setting: f32, random: f32, height: f32) -> (f32, f32) {
    (
        setting.min(1.0) * random * (0.5 + 0.5 * height),
        (setting - 1.0).clamp(0.0, 1.0),
    )
}

/// Child length relative to the longest child at `widest`, for `u` from the
/// base (0) to the tip (1) of the parent's leafy part. The WESL twin is
/// `plant::length_envelope`.
pub(crate) fn length_envelope(u: f32, widest: f32) -> f32 {
    let w = widest.clamp(0.01, 0.99);
    let rise = u / w;
    let fall = (1.0 - u) / (1.0 - w);
    (if u < w { rise } else { fall }).clamp(0.0, 1.0).powf(0.6)
}

/// Strip width at `t` from base (0) to tip (1), relative to the segment's
/// width, for an outline `profile`. The WESL twin is `common::profile_width`.
pub(crate) fn profile_width(t: f32, profile: f32) -> f32 {
    let pointed = 1.0 - t.powf(1.4);
    let oval = (4.0 * t * (1.0 - t)).max(0.0).sqrt();
    // WGSL's `mix`, in its own order of operations.
    pointed * (1.0 - profile) + oval * profile
}

/// One curved strip of a branched plant at rest: no wind, no trampling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MirrorSegment {
    pub points: [Vec3; 3],
    /// Length scale before bending, in metres.
    pub length: f32,
    /// Width scale before the outline profile, in metres.
    pub width: f32,
    /// Direction across the strip at its base.
    pub across: Vec3,
    /// Total base-to-tip rotation, in radians.
    pub twist: f32,
    pub profile: f32,
    pub fold: f32,
}

/// Position and orientation along a segment.
struct SegmentFrame {
    point: Vec3,
    tangent: Vec3,
    across: Vec3,
    normal: Vec3,
}

/// The WESL twin is `plant::bezier_tangent`.
fn bezier_tangent(points: &[Vec3; 3], t: f32) -> Vec3 {
    let d = (points[1] - points[0]) * (2.0 * (1.0 - t)) + (points[2] - points[1]) * (2.0 * t);
    (d + Vec3::new(0.0, 1e-5, 0.0)).normalize()
}

/// Rotates `v` around the unit-length `axis` by `angle` radians. The WESL twin
/// is `plant::turn_about`.
fn turn_about(v: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    if angle == 0.0 {
        return v;
    }
    let c = angle.cos();
    let s = angle.sin();
    v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
}

/// The WESL twin is `plant::frame_at`.
fn frame_at(s: &MirrorSegment, t: f32) -> SegmentFrame {
    let point = bezier(&s.points, t);
    let tangent = bezier_tangent(&s.points, t);
    let twisted = turn_about(s.across, tangent, s.twist * t);
    let flat = twisted - tangent * twisted.dot(tangent);
    let across = if flat.length_squared() > 1e-10 {
        flat.normalize()
    } else {
        Vec3::X
    };
    let normal = across.cross(tangent).normalize();
    SegmentFrame {
        point,
        tangent,
        across,
        normal,
    }
}

/// The WESL literal `1.5707963` in `plant::child_segment`, which rounds to one
/// ulp below `FRAC_PI_2`. Its bits are pinned so both sides agree exactly.
const WESL_HALF_PI: f32 = f32::from_bits(0x3fc9_0fda);

/// The random values one branched stem draws, which shape it at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RestingStem {
    /// Height as a fraction of the plant's tallest stem, 0 to 1.
    pub height: f32,
    /// Width as a fraction of the plant's width, 0.75 to 1.25.
    pub width: f32,
    /// The stem's droop random number, 0 to 1.
    pub droop_random: f32,
    /// The stem's twist random number, 0 to 1.
    pub twist_random: f32,
    /// The stem's seed, 0 to 1, which varies its branches' twist.
    pub key: f32,
}

/// A branched plant's main segment at rest, facing +Z from a root at the
/// origin. The WESL twin is `plant::own_segment`, with no wind bend.
pub(crate) fn resting_stem(plant: &PlantParams, stem: &RestingStem) -> MirrorSegment {
    let height = stem.height * plant.height_range().1;
    let setting = plant.segment.droop.clamp(0.0, 2.0);
    let (droop, arch) = stem_droop(setting, stem.droop_random, stem.height);
    MirrorSegment {
        points: blade_curve(height, plant.lean, Vec2::ZERO, droop, arch),
        length: height,
        width: stem.width * plant.width,
        across: Vec3::X,
        twist: plant.segment.twist.max(0.0) * TAU * (stem.twist_random * 2.0 - 1.0),
        profile: plant.segment.profile.clamp(0.0, 1.0),
        fold: plant.segment.fold.clamp(0.0, 1.0),
    }
}

/// Child `index` of `count` on `parent`, at rest. The WESL twin is
/// `plant::child_segment`, with no sway.
pub(crate) fn resting_child(
    parent: &MirrorSegment,
    level: &LevelGpu,
    index: u32,
    count: u32,
    key: f32,
) -> MirrorSegment {
    // Attachment point and side, as the shader numbers them.
    let opposite = level.attach.y < 0.5;
    let points = if opposite { (count / 2).max(1) } else { count };
    let at_point = if opposite { index / 2 } else { index };
    let side = if index % 2 == 1 { -1.0 } else { 1.0 };
    let u = (at_point as f32 + 0.5) / points as f32;
    let start = level.attach.z;
    let f = frame_at(parent, start + (1.0 - start) * u);
    let length = level.size.x * parent.length * length_envelope(u, level.attach.w);

    // Direction: rolled round the parent, swept towards its tip, tilted by its fold.
    let spread = level.root_color.w;
    let roll = at_point as f32 * spread * PI;
    let aside = turn_about(f.across * side, f.tangent, roll);
    let face = turn_about(f.normal, f.tangent, roll);
    let angle = level.size.z * WESL_HALF_PI;
    let mut out_dir = (aside * angle.cos() + f.tangent * angle.sin()).normalize();
    out_dir = (out_dir - face * parent.fold * 0.6).normalize();
    let droop = level.form.y;
    let end_dir = (out_dir + Vec3::new(0.0, -1.2 * droop, 0.0)).normalize();

    // Width direction: along the parent, or level for spiralling leaves.
    let along = f.tangent - out_dir * f.tangent.dot(out_dir);
    let mut across = if along.length_squared() > 1e-6 {
        along.normalize()
    } else {
        face
    };
    let level_across = out_dir.cross(Vec3::Y);
    if spread > 0.0 && level_across.length_squared() > 1e-6 {
        across = level_across.normalize();
    }
    let turn = key * 7.31 + index as f32 * 0.618;
    let p1 = f.point + out_dir * (0.5 * length);
    MirrorSegment {
        points: [f.point, p1, p1 + end_dir * (0.5 * length)],
        length,
        width: level.size.y * length,
        across,
        twist: level.form.z * TAU * ((turn - turn.floor()) * 2.0 - 1.0),
        profile: level.form.x,
        fold: level.form.w,
    }
}

/// Every vertex of a segment's strip with `rows` rows, at nearest detail and
/// with no minimum on-screen width. The WESL twins are `plant::strip_at` and
/// `plant::strip_vertex`.
fn strip_vertices(s: &MirrorSegment, rows: u32, out: &mut Vec<Vec3>) {
    for local in 0..=2 * rows {
        let is_tip = local >= 2 * rows;
        let row = if is_tip { rows } else { local / 2 };
        let t = row as f32 / rows as f32;
        let side = if is_tip {
            0.0
        } else {
            (local % 2) as f32 - 0.5
        };
        let f = frame_at(s, t);
        let width = s.width * profile_width(t, s.profile);
        out.push(f.point + f.across * side * width * (1.0 - 0.4 * s.fold));
    }
}

/// Every vertex of one branched stem at rest and nearest detail, relative to
/// its root. The WESL twin is `plant::plant_vertex`.
pub(crate) fn resting_vertices(plant: &PlantParams, stem: &RestingStem) -> Vec<Vec3> {
    let levels = level_entries(plant);
    let first = levels[0].attach.x as u32;
    let second = levels[1].attach.x as u32;
    let own = resting_stem(plant, stem);
    let mut out = Vec::new();
    strip_vertices(&own, PLANT_ROWS[0], &mut out);
    let child_rows = CHILD_ROWS[usize::from(second > 0)][0];
    let leaf_key = stem.key + 0.37;
    let leaf_key = leaf_key - leaf_key.floor();
    for index in 0..first {
        let child = resting_child(&own, &levels[0], index, first, stem.key);
        strip_vertices(&child, child_rows, &mut out);
        for leaf in 0..second {
            let leaf = resting_child(&child, &levels[1], leaf, second, leaf_key);
            strip_vertices(&leaf, GRANDCHILD_ROWS[0], &mut out);
        }
    }
    out
}

/// The highest point of a branched plant at rest, in metres above its root.
///
/// Takes the tallest, widest stem and every seed a stem can carry, over a
/// sweep of its droop and twist random numbers. Panics if `plant` has no
/// branches; an unbranched plant is drawn by `blade.wesl`, not modelled here.
pub(crate) fn branched_top(plant: &PlantParams) -> f32 {
    assert!(
        plant.levels[0].is_on(),
        "branched_top needs a branched plant"
    );
    // A random number only matters when the setting it scales is non-zero.
    let sweep = |on: bool, steps: u32| -> Vec<f32> {
        if on {
            (0..=steps).map(|k| k as f32 / steps as f32).collect()
        } else {
            vec![0.0]
        }
    };
    let droops = sweep(plant.segment.droop > 0.0, 8);
    let twists = sweep(plant.segment.twist > 0.0, 4);
    let mut top = f32::MIN;
    // An instance's seed is 8 bits, so every key is covered.
    for seed in 0..=255u32 {
        for &droop_random in &droops {
            for &twist_random in &twists {
                let stem = RestingStem {
                    height: 1.0,
                    width: 1.25,
                    droop_random,
                    twist_random,
                    key: seed as f32 / 255.0,
                };
                for vertex in resting_vertices(plant, &stem) {
                    top = top.max(vertex.y);
                }
            }
        }
    }
    top
}

/// Cells along a trample window's side. Twin of `trample::TEXELS`.
const TRAMPLE_TEXELS: i32 = 256;

/// A push packed for `atomicMax`: strength in the top 10 bits, so the larger
/// word is always the stronger push, and the direction as two 11-bit
/// components below. The WESL twin is `trample_pack`.
pub(crate) fn trample_pack(push: Vec2) -> u32 {
    let strength = push.length().clamp(0.0, 1.0);
    let direction = if strength > 1e-6 {
        push / push.length()
    } else {
        Vec2::X
    };
    let d = ((direction * 0.5 + 0.5) * 2047.0).round().as_uvec2();
    (((strength * 1023.0).round() as u32) << 22) | (d.x << 11) | d.y
}

/// The WESL twin is `trample_unpack`.
pub(crate) fn trample_unpack(word: u32) -> Vec2 {
    let strength = (word >> 22) as f32 / 1023.0;
    let direction =
        Vec2::new(((word >> 11) & 0x7ff) as f32, (word & 0x7ff) as f32) / 2047.0 * 2.0 - 1.0;
    direction * strength
}

/// Index of a world cell's slot in the window buffer. The WESL twin is
/// `trample_slot`.
pub(crate) fn trample_slot(cell: IVec2) -> u32 {
    let wrapped = cell & IVec2::splat(TRAMPLE_TEXELS - 1);
    (wrapped.y * TRAMPLE_TEXELS + wrapped.x) as u32
}

/// The world cell a slot holds while the window's lowest cell is `origin`.
/// The WESL twin is `trample_cell`.
pub(crate) fn trample_cell(slot: IVec2, origin: IVec2) -> IVec2 {
    origin + ((slot - origin) & IVec2::splat(TRAMPLE_TEXELS - 1))
}

/// The WESL twin is `in_trample_window`.
pub(crate) fn in_trample_window(cell: IVec2, origin: IVec2) -> bool {
    cell.cmpge(origin).all() && cell.cmplt(origin + TRAMPLE_TEXELS).all()
}

/// Extra stem slots requested per branched bucket to allow new plants to appear.
///
/// This allowance may shrink when the total budget is insufficient. The WESL twin
/// is `generate::MIN_SLACK`.
pub(crate) const MIN_SLACK: u32 = 256;

/// Precision used to scale plant allocations with integer arithmetic.
///
/// Fractional shares use this many bits, keeping CPU and GPU results identical.
/// The WESL twin is `generate::SHARE_BITS`.
pub(crate) const SHARE_BITS: u32 = 12;

/// Calculates a branched bucket's requested stem slots from last frame's demand.
///
/// Adds 25% and `MIN_SLACK` for growth, capped at the entire plant budget. Demand
/// includes stems that did not fit. The WESL twin is `generate::plant_want`.
pub(crate) fn plant_want(demand: u32, budget: u32) -> u32 {
    if demand >= budget {
        return budget;
    }
    (demand + demand / 4 + MIN_SLACK).min(budget)
}

/// Divides instance storage between buckets using last frame's demand.
///
/// Blade allocations stay fixed. Plant buckets receive their requested slots plus
/// an equal share of any remainder, or proportionally reduced requests if the
/// budget is insufficient. Returns capacities and starting offsets. The WESL twin
/// is `generate::plan_buckets`.
#[expect(
    clippy::manual_checked_ops,
    reason = "explicit zero checks match the WESL twin, which has no checked division"
)]
pub(crate) fn plan_buckets(
    counts: &[u32; MAX_BUCKETS],
    blade_capacity: u32,
    plant_capacity: u32,
    bucket_count: u32,
) -> ([u32; MAX_BUCKETS], [u32; MAX_BUCKETS]) {
    let buckets = bucket_count as usize;
    let mut capacity = [0; MAX_BUCKETS];

    // Request room for last frame's demand plus growth, including previously dropped stems.
    let mut wanted = 0;
    for b in 3..buckets {
        capacity[b] = plant_want(counts[b], plant_capacity);
        wanted += capacity[b];
    }

    // Share spare slots equally, or reduce every request by the same proportion.
    let branched = bucket_count - 3;
    if branched > 0 {
        if wanted <= plant_capacity {
            let extra = (plant_capacity - wanted) / branched;
            for slots in &mut capacity[3..buckets] {
                *slots += extra;
            }
        } else {
            let share = (plant_capacity << SHARE_BITS) / wanted;
            for slots in &mut capacity[3..buckets] {
                *slots = (*slots * share) >> SHARE_BITS;
            }
        }
    }

    // Keep blade allocations fixed and place all buckets consecutively in the buffer.
    capacity[..3].copy_from_slice(&[blade_capacity, blade_capacity / 2, blade_capacity / 2]);
    let mut base = [0; MAX_BUCKETS];
    let mut next = 0;
    for b in 0..MAX_BUCKETS {
        base[b] = next;
        next += capacity[b];
    }
    (capacity, base)
}

mod tests;
