//! CPU model of the deterministic placement and static tables the GPU uses.
//!
//! Every function here has a WESL twin in `shaders/common.wesl` or
//! `shaders/generate.wesl`. The runtime CPU uses only this module: hashes,
//! the blue-noise tile, chunk layout, distance LOD, and the static clump,
//! tuft and gust tables. `mirror.rs` holds the test-only CPU oracle that
//! checks the same behaviour and the GPU readback test against it.

use std::sync::OnceLock;

use bevy::math::{IVec2, Vec2};

use crate::GrassSettings;

/// Side of the gust texture, in texels.
pub(crate) const GUST_SIZE: u32 = 256;

/// Points in the blue-noise tile.
pub(crate) const TILE_POINTS: u32 = 1024;

/// PCG hash (Jarzynski and Olano, 2020). The WESL twin is `hash`.
pub(crate) fn hash(x: u32) -> u32 {
    let state = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Hash of two values. The WESL twin is `hash2`.
pub(crate) fn hash2(a: u32, b: u32) -> u32 {
    hash(a ^ hash(b))
}

/// A hash mapped to `[0, 1)` with 24 bits of precision. The WESL twin is `unit`.
pub(crate) fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / 16_777_216.0
}

/// The blue-noise tile: [`TILE_POINTS`] points in the unit square, wrapping at
/// its edges.
///
/// The points are in *progressive* order: every prefix of the list is itself
/// evenly spread. Thinning keeps the blades whose index is below a threshold,
/// so the grass stays evenly spread at every density instead of clumping as
/// it thins. Generated once with Mitchell's best-candidate algorithm and a
/// fixed seed, so it is identical on every machine and every run.
pub(crate) fn blue_noise_tile() -> &'static [Vec2] {
    static TILE: OnceLock<Vec<Vec2>> = OnceLock::new();
    TILE.get_or_init(|| best_candidate_tile(TILE_POINTS as usize, 12, 0x6b8a_3a00))
}

pub(crate) fn best_candidate_tile(count: usize, candidates: u32, seed: u32) -> Vec<Vec2> {
    let mut points: Vec<Vec2> = Vec::with_capacity(count);
    let mut rng = seed;
    let mut next = || {
        rng = hash(rng);
        unit(rng)
    };
    while points.len() < count {
        let mut best = Vec2::ZERO;
        let mut best_distance = -1.0;
        for _ in 0..candidates {
            let candidate = Vec2::new(next(), next());
            let nearest = points
                .iter()
                .map(|p| toroidal_distance_squared(*p, candidate))
                .fold(f32::INFINITY, f32::min);
            if nearest > best_distance {
                best_distance = nearest;
                best = candidate;
            }
        }
        points.push(best);
    }
    points
}

/// Squared distance between two points of the unit square, wrapping at the edges.
pub(crate) fn toroidal_distance_squared(a: Vec2, b: Vec2) -> f32 {
    let d = (a - b).abs();
    let d = d.min(Vec2::ONE - d);
    d.length_squared()
}

/// How a chunk is covered by copies of the blue-noise tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChunkLayout {
    /// Tile copies along each side of the chunk.
    pub tiles_per_side: u32,
    /// Side of one tile copy, in metres.
    pub tile_size: f32,
    /// Blades per square metre when every candidate is kept.
    pub candidate_density: f32,
}

impl ChunkLayout {
    /// The coarsest layout with at least `density` candidates per square
    /// metre, never fewer than one tile per chunk. Rounding the tile count to
    /// the nearest instead silently capped some styles: 45 blades per m²
    /// drew 36. Spare candidates cost nothing, because a chunk only evaluates
    /// the prefix it can keep.
    pub(crate) fn for_density(chunk_size: f32, density: f32) -> Self {
        let ideal = chunk_size * (density.max(0.0) / TILE_POINTS as f32).sqrt();
        let tiles_per_side = (ideal.ceil() as u32).max(1);
        let tile_size = chunk_size / tiles_per_side as f32;
        Self {
            tiles_per_side,
            tile_size,
            candidate_density: TILE_POINTS as f32 / (tile_size * tile_size),
        }
    }
}

/// Distance-based thinning and level of detail for perspective views, from a
/// [`GrassSettings`]. The tier's own table is the architecture's: 100% to
/// the start of LOD 1, 50% where LOD 2 starts, 15% where the fade starts.
///
/// Orthographic views use none of it: every blade is the same size on screen,
/// so they keep full density and one fixed LOD per tier (T3 in the plan).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DistanceLod {
    /// Beyond this no grass is drawn, in metres.
    pub radius: f32,
    /// Where LOD 1 and LOD 2 begin, in metres.
    pub lod_start: [f32; 2],
    /// Width of the dithered band around each LOD boundary, in metres.
    pub band: f32,
    /// Where the last fade begins, as a share of `radius`.
    pub fade_start: f32,
    /// The share of the blades kept where LOD 2 begins, and where the fade
    /// begins.
    pub density_at: [f32; 2],
    /// Where the mid-range thinning begins, in metres.
    pub mid_range_start: f32,
    /// The share of blades kept from `mid_range_start` on, on top of the
    /// thinning; coverage widening makes the survivors wider. Thin blades
    /// waste most of the fragment work on the 2×2 pixel quads GPUs shade in,
    /// so half as many blades at 1.4× the width cost 16% less in the
    /// perspective benchmark and look the same.
    pub mid_range_density: f32,
}

impl DistanceLod {
    /// The table `settings` ask for, scaled to its radius.
    pub(crate) fn from_settings(settings: &GrassSettings) -> Self {
        Self {
            radius: settings.radius,
            lod_start: [
                settings.radius * settings.lod1_start,
                settings.radius * settings.lod2_start,
            ],
            band: settings.lod_blend,
            fade_start: settings.fade_start,
            density_at: [settings.lod2_density, settings.fade_density],
            mid_range_start: settings.radius * settings.mid_range_start,
            mid_range_density: settings.mid_range_density,
        }
    }
}

/// Clump table entries per metre along each axis.
pub(crate) const CLUMP_PER_METRE: i32 = 4;
/// Entries along each side of the clump table: it repeats every 64 m.
pub(crate) const CLUMP_SIDE: i32 = 256;

/// The clump of every 25 cm square of a 64 m tile, 0–63, one byte each,
/// four to a word. It is a Voronoi pattern with one jittered point per
/// square metre, wrapped so the tile repeats seamlessly. Blades in one clump
/// share a tint, which reads as natural patches rather than per-blade noise.
///
/// Built once and uploaded: searching the nine nearest points per blade on
/// the GPU made blade generation 40–130% slower; a table lookup costs one
/// load.
pub(crate) fn clump_table() -> &'static [u32] {
    static TABLE: OnceLock<Vec<u32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let metres = CLUMP_SIDE / CLUMP_PER_METRE;
        let mut words = vec![0u32; (CLUMP_SIDE * CLUMP_SIDE / 4) as usize];
        for z in 0..CLUMP_SIDE {
            for x in 0..CLUMP_SIDE {
                let point = (Vec2::new(x as f32, z as f32) + 0.5) / CLUMP_PER_METRE as f32;
                let cell = point.floor();
                let mut nearest = f32::MAX;
                let mut id = 0;
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let neighbour = cell + Vec2::new(dx as f32, dz as f32);
                        let wrapped = neighbour.as_ivec2().rem_euclid(IVec2::splat(metres));
                        let key = hash2(wrapped.x as u32, wrapped.y as u32);
                        let feature = neighbour + Vec2::new(unit(key), unit(hash(key)));
                        let distance = feature.distance_squared(point);
                        if distance < nearest {
                            nearest = distance;
                            id = key;
                        }
                    }
                }
                let index = (z * CLUMP_SIDE + x) as usize;
                words[index / 4] |= (hash(id ^ 0xc1) >> 26) << ((index % 4) * 8);
            }
        }
        words
    })
}
/// Tuft table entries per tuft spacing, along each axis.
pub(crate) const TUFT_ENTRIES_PER_TUFT: i32 = 4;
/// Entries along each side of the tuft table: it repeats every 64 tufts.
pub(crate) const TUFT_SIDE: i32 = 256;

/// Where the tufts are: one centre per tuft spacing (a square of side 1 in
/// tuft units), jittered, as a Voronoi pattern like the clump table's. Each
/// entry covers a quarter of a tuft spacing and holds its nearest centre,
/// relative to the entry's own centre, in tuft spacings: x and z as signed
/// bytes over ±2 in the low two bytes, and a random byte per tuft in the
/// third. Wraps every 64 tufts.
///
/// A table for the same reason as the clump table: searching nine centres
/// per blade is what made generation 40–130% slower there.
pub(crate) fn tuft_table() -> &'static [u32] {
    static TABLE: OnceLock<Vec<u32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let tufts = TUFT_SIDE / TUFT_ENTRIES_PER_TUFT;
        let byte = |offset: f32| ((offset / 4.0 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u32;
        (0..TUFT_SIDE * TUFT_SIDE)
            .map(|index| {
                let entry = Vec2::new((index % TUFT_SIDE) as f32, (index / TUFT_SIDE) as f32);
                let point = (entry + 0.5) / TUFT_ENTRIES_PER_TUFT as f32;
                let cell = point.floor();
                let mut nearest = (f32::MAX, Vec2::ZERO, 0);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let neighbour = cell + Vec2::new(dx as f32, dz as f32);
                        let wrapped = neighbour.as_ivec2().rem_euclid(IVec2::splat(tufts));
                        let key = hash2(wrapped.x as u32, wrapped.y as u32) ^ 0x7f4a_7c15;
                        let centre = neighbour + Vec2::new(unit(key), unit(hash(key)));
                        let distance = centre.distance_squared(point);
                        if distance < nearest.0 {
                            nearest = (distance, centre - point, key);
                        }
                    }
                }
                let (_, offset, key) = nearest;
                byte(offset.x) | (byte(offset.y) << 8) | ((hash(key ^ 0x3c) >> 24) << 16)
            })
            .collect()
    })
}

/// One bilinear sample of the gust noise (an R8 texture, wrapping), as the
/// GPU's sampler takes it.
pub(crate) fn sample_noise(noise: &[u8], uv: Vec2) -> f32 {
    let size = (noise.len() as f32).sqrt() as i32;
    let p = uv * size as f32 - 0.5;
    let base = p.floor();
    let t = p - base;
    let at = |x: i32, y: i32| {
        let x = (base.x as i32 + x).rem_euclid(size);
        let y = (base.y as i32 + y).rem_euclid(size);
        f32::from(noise[(y * size + x) as usize]) / 255.0
    };
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * t.x;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * t.x;
    top + (bottom - top) * t.y
}
/// The gust noise at [`GUST_SIZE`], made once: the texture the GPU samples,
/// for the CPU to sample the same way.
pub(crate) fn gust_noise_table() -> &'static [u8] {
    static NOISE: OnceLock<Vec<u8>> = OnceLock::new();
    NOISE.get_or_init(|| gust_noise(GUST_SIZE))
}

/// The noise bare patches are cut from, at world point `at`, for patches
/// about `size` metres across: two samples of the gust noise, the second
/// turned and scaled, so the pattern does not visibly repeat. The WESL twin
/// is `bare_noise`.
#[expect(clippy::manual_midpoint, reason = "the same arithmetic as the GPU's")]
pub(crate) fn bare_noise(noise: &[u8], at: Vec2, size: f32) -> f32 {
    let uv = at / (size * 8.0);
    let turned = Vec2::new(uv.x * 0.8 - uv.y * 0.6, uv.x * 0.6 + uv.y * 0.8) * 1.37 + 0.5;
    0.5 * (sample_noise(noise, uv) + sample_noise(noise, turned))
}

/// The value of `bare_noise` below which `share` of the ground lies.
#[expect(clippy::manual_midpoint, reason = "the same arithmetic as the GPU's")]
pub(crate) fn bare_threshold(share: f32) -> f32 {
    static SORTED: OnceLock<Vec<f32>> = OnceLock::new();
    let sorted = SORTED.get_or_init(|| {
        let noise = gust_noise(GUST_SIZE);
        let mut values: Vec<f32> = (0..32_768u32)
            .map(|i| {
                let uv = Vec2::new(unit(hash(i ^ 0xba5e)), unit(hash(i ^ 0xbead))) * 16.0;
                let turned =
                    Vec2::new(uv.x * 0.8 - uv.y * 0.6, uv.x * 0.6 + uv.y * 0.8) * 1.37 + 0.5;
                0.5 * (sample_noise(&noise, uv) + sample_noise(&noise, turned))
            })
            .collect();
        values.sort_by(f32::total_cmp);
        values
    });
    let index = (share.clamp(0.0, 1.0) * (sorted.len() - 1) as f32).round() as usize;
    sorted[index]
}

/// Index list for one level of detail, with vertex indices local to the level.
///
/// Rows run root to tip; row `r` is vertices `2r` (left) and `2r + 1` (right),
/// and the tip is vertex `2 * rows`.
pub(crate) fn lod_indices(rows: u32) -> Vec<u32> {
    let tip = 2 * rows;
    let mut indices = Vec::with_capacity((2 * rows as usize - 1) * 3);
    for r in 0..rows - 1 {
        let (l, rt) = (2 * r, 2 * r + 1);
        indices.extend_from_slice(&[l, rt, l + 2, rt, rt + 2, l + 2]);
    }
    indices.extend_from_slice(&[2 * (rows - 1), 2 * (rows - 1) + 1, tip]);
    indices
}

/// Main-stem rows at near and mid detail. Far detail is [`FAR_PARTS`] triangles. The
/// WESL twin is `plant::plant_rows`.
pub(crate) const PLANT_ROWS: [u32; 2] = [8, 4];

/// First-level branch rows at near and mid detail. Entry `[0]` is for terminal
/// leaves; `[1]` is for twigs carrying another level. The WESL twin is
/// `plant::child_rows`.
pub(crate) const CHILD_ROWS: [[u32; 2]; 2] = [[4, 1], [2, 1]];

/// Leaf rows on twigs at near and mid detail. The WESL twin is
/// `plant::grandchild_rows`.
pub(crate) const GRANDCHILD_ROWS: [u32; 2] = [3, 1];

/// Leaf rows used for near-detail shadows.
///
/// Each leaf without further branches becomes one triangle. Twigs retain their
/// usual rows. The WESL twin is `plant::SHADOW_LEAF_ROWS`.
pub(crate) const SHADOW_LEAF_ROWS: u32 = 1;

/// Vertex-index space reserved for one branched stem. The 1024-vertex limit keeps draw
/// decoding unambiguous; larger classes are skipped. The WESL twin is
/// `common::BRANCHED_VERTEX_STRIDE`.
pub(crate) const BRANCHED_VERTEX_STRIDE: u32 = 1024;

/// Rendered children per parent, rounding opposite arrangements up to complete pairs.
/// The shader reads this result from the field table rather than recalculating it.
pub(crate) fn drawn_children(count: u32, opposite: bool) -> u32 {
    if opposite {
        count.next_multiple_of(2)
    } else {
        count
    }
}

/// Maximum branches retained on each parent at mid detail.
///
/// Applies at both branch levels. The WESL twin is `plant::MID_MAX_CHILDREN`.
pub(crate) const MID_MAX_CHILDREN: u32 = 8;

/// Number of branches retained on each parent at mid detail.
///
/// Keeps roughly half, up to [`MID_MAX_CHILDREN`]. Even counts remain even
/// so paired branches can stay together. The WESL twin is `plant::mid_children`.
pub(crate) fn mid_children(children: u32) -> u32 {
    let half = 2 * (children / 4) + (children % 4).min(2);
    half.min(MID_MAX_CHILDREN)
}

/// Selects an original branch for position `k` in the reduced mid-detail set.
///
/// Selections are spread along the parent, keeping opposite branches paired.
/// Used to check the shader's selection. The WESL twin is `plant::mid_child`.
#[cfg(test)]
pub(crate) fn mid_child(k: u32, children: u32, opposite: bool) -> u32 {
    let kept = mid_children(children);
    if opposite {
        let (points, kept_points) = ((children / 2).max(1), (kept / 2).max(1));
        2 * ((2 * (k / 2) + 1) * points / (2 * kept_points)) + k % 2
    } else {
        (2 * k + 1) * children / (2 * kept)
    }
}

/// Single-triangle parts a branched stem keeps at far detail.
///
/// They come from the stem's dominant level (see `far_level`). When that level is
/// the second, the stem keeps 2 twigs with 3 leaves on each. The WESL twin is
/// `plant::FAR_PARTS`.
pub(crate) const FAR_PARTS: u32 = 6;

/// The level whose parts a stem keeps at far detail: 0 for the stalk, 1 for the
/// branches on it, 2 for leaves on those branches.
///
/// Picks the level with the largest total area. `length` and `width` are the stalk's,
/// in metres; `children`, `lengths` and `widths` are each branch level's drawn count,
/// length relative to its parent and width relative to its own length. Used to check
/// the shader. The WESL twin is `plant::far_level`.
#[cfg(test)]
pub(crate) fn far_level(
    length: f32,
    width: f32,
    children: [u32; 2],
    lengths: [f32; 2],
    widths: [f32; 2],
) -> u32 {
    let first = lengths[0] * length;
    let second = lengths[1] * first;
    let areas = [
        length * width,
        children[0] as f32 * first * first * widths[0],
        (children[0] * children[1]) as f32 * second * second * widths[1],
    ];
    let mut level = 0;
    if areas[1] > areas[level] {
        level = 1;
    }
    if areas[2] > areas[level] {
        level = 2;
    }
    level as u32
}

/// Children a parent keeps at far detail when it may keep up to `kept`.
///
/// Never more than [`mid_children`] keeps, so far parts are a subset of mid ones.
/// Opposite arrangements keep whole pairs. The WESL twin is `plant::far_kept`.
#[cfg(test)]
pub(crate) fn far_kept(children: u32, opposite: bool, kept: u32) -> u32 {
    let step = if opposite { 2 } else { 1 };
    let mid_points = mid_children(children) / step;
    (kept / step).min(mid_points) * step
}

/// The original child shown as far part `k`, or `None` when the parent keeps fewer
/// than `k + 1`.
///
/// Keeps the mid-detail children nearest `widest`, the point along the parent where
/// children are longest, so the largest survive. `kept` is the most it may keep.
/// The WESL twin is `plant::far_child`.
#[cfg(test)]
pub(crate) fn far_child(
    k: u32,
    children: u32,
    opposite: bool,
    widest: f32,
    kept: u32,
) -> Option<u32> {
    let step = if opposite { 2 } else { 1 };
    let mid_points = mid_children(children) / step;
    let points = far_kept(children, opposite, kept) / step;
    let point = k / step;
    if point >= points {
        return None;
    }
    let centre = widest * mid_points as f32 - 0.5 - (points - 1) as f32 * 0.5;
    let start = ((centre + 0.5).floor().max(0.0) as u32).min(mid_points - points);
    Some(mid_child(
        step * (start + point) + k % step,
        children,
        opposite,
    ))
}

/// Branch counts shared by plants that can use the same mesh.
///
/// Counts are the rendered children per parent at each level, after opposite-pair
/// rounding. Size, colour and other shape settings do not require a separate class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct ShapeClass {
    pub children: [u32; 2],
}

impl ShapeClass {
    /// Branch counts and geometry rows for the main view and prepass.
    ///
    /// Near detail keeps every branch and mid detail uses [`mid_children`]. Far detail
    /// is [`FAR_PARTS`] single triangles, which the shader fills from the stem's
    /// dominant level.
    fn layout(self, lod: usize) -> StemLayout {
        let children = match lod {
            0 => self.children,
            1 => self.children.map(mid_children),
            _ => {
                return StemLayout {
                    children: [FAR_PARTS, 0],
                    rows: [0, 1, 0],
                };
            }
        };
        StemLayout {
            children,
            rows: [
                PLANT_ROWS[lod],
                CHILD_ROWS[usize::from(self.children[1] > 0)][lod],
                GRANDCHILD_ROWS[lod],
            ],
        }
    }

    /// Near-detail shadow geometry, retaining every branch with simpler leaves.
    ///
    /// Leaves without further branches use [`SHADOW_LEAF_ROWS`]; stems and twigs
    /// keep their usual rows.
    fn shadow_layout(self) -> StemLayout {
        let mut layout = self.layout(0);
        if self.children[1] > 0 {
            layout.rows[2] = SHADOW_LEAF_ROWS;
        } else {
            layout.rows[1] = SHADOW_LEAF_ROWS;
        }
        layout
    }

    /// Total vertices for one stem: main segment, first-level branches and second-level
    /// leaves.
    pub(crate) fn vertices(self, lod: usize) -> u32 {
        self.layout(lod).vertices()
    }

    /// Number of vertices needed for one stem's near-detail shadow geometry.
    #[cfg(test)]
    pub(crate) fn shadow_vertices(self) -> u32 {
        self.shadow_layout().vertices()
    }

    /// Checks that near-detail geometry fits the reserved vertex-index space.
    pub(crate) fn fits(self) -> bool {
        self.vertices(0) <= BRANCHED_VERTEX_STRIDE
    }

    /// Builds stem-local indices in the order used by
    /// [`vertices`](ShapeClass::vertices): main segment, first-level branches, then
    /// second-level leaves.
    pub(crate) fn indices(self, lod: usize) -> Vec<u32> {
        self.layout(lod).indices()
    }

    /// Builds triangle indices for one stem's near-detail shadow geometry.
    ///
    /// Indices refer to vertices within this stem: main segment, first-level
    /// branches, then second-level leaves.
    pub(crate) fn shadow_indices(self) -> Vec<u32> {
        self.shadow_layout().indices()
    }
}

/// Branch counts and geometry rows needed to build one stem.
///
/// `children` counts branches per parent at each level. `rows` gives the row
/// counts for the main stem, first-level branches and second-level leaves.
#[derive(Debug, Clone, Copy)]
struct StemLayout {
    children: [u32; 2],
    rows: [u32; 3],
}

impl StemLayout {
    /// Total vertex count for the stem and all its branches.
    fn vertices(self) -> u32 {
        let [first, second] = self.children;
        let strip = |rows: u32| if rows == 0 { 0 } else { 2 * rows + 1 };
        strip(self.rows[0]) + first * strip(self.rows[1]) + first * second * strip(self.rows[2])
    }

    /// Triangle indices referring to vertices within this stem.
    ///
    /// Ordered by main segment, first-level branches, then second-level leaves.
    fn indices(self) -> Vec<u32> {
        let [first, second] = self.children;
        let mut out = Vec::new();
        let mut base = 0;
        let mut strip = |rows: u32, base: &mut u32| {
            // A level with no rows is not drawn and takes no vertices.
            if rows > 0 {
                out.extend(lod_indices(rows).into_iter().map(|i| i + *base));
                *base += 2 * rows + 1;
            }
        };
        strip(self.rows[0], &mut base);
        for _ in 0..first {
            strip(self.rows[1], &mut base);
        }
        for _ in 0..first * second {
            strip(self.rows[2], &mut base);
        }
        out
    }
}

/// Tileable gust noise: three octaves of value noise on lattices that divide
/// the texture, so it wraps without a seam. Values span the full 0–255 range.
pub(crate) fn gust_noise(size: u32) -> Vec<u8> {
    const OCTAVES: [(u32, f32); 3] = [(4, 0.6), (8, 0.28), (16, 0.12)];
    let lattice = |period: u32, octave: u32, x: u32, y: u32| {
        unit(hash2(hash2(x % period, y % period), octave))
    };
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let mut values = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut value = 0.0;
            for (octave, (period, weight)) in OCTAVES.iter().enumerate() {
                let fx = x as f32 / size as f32 * *period as f32;
                let fy = y as f32 / size as f32 * *period as f32;
                let (ix, iy) = (fx.floor() as u32, fy.floor() as u32);
                let (tx, ty) = (smooth(fx.fract()), smooth(fy.fract()));
                let o = octave as u32;
                let top =
                    lattice(*period, o, ix, iy) * (1.0 - tx) + lattice(*period, o, ix + 1, iy) * tx;
                let bottom = lattice(*period, o, ix, iy + 1) * (1.0 - tx)
                    + lattice(*period, o, ix + 1, iy + 1) * tx;
                value += (top * (1.0 - ty) + bottom * ty) * weight;
            }
            values.push(value);
        }
    }
    let (low, high) = values
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let span = (high - low).max(1e-6);
    values
        .iter()
        .map(|v| ((v - low) / span * 255.0).round() as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn gust_noise_wraps_without_a_seam() {
        let size = 64;
        let noise = gust_noise(size);
        let at = |x: u32, y: u32| i32::from(noise[(y * size + x) as usize]);
        let inner = (0..size - 1)
            .flat_map(|y| (0..size - 1).map(move |x| (x, y)))
            .map(|(x, y)| {
                (at(x, y) - at(x + 1, y))
                    .abs()
                    .max((at(x, y) - at(x, y + 1)).abs())
            })
            .max()
            .unwrap();
        for i in 0..size {
            assert!((at(size - 1, i) - at(0, i)).abs() <= inner);
            assert!((at(i, size - 1) - at(i, 0)).abs() <= inner);
        }
    }

    #[test]
    fn a_shape_class_indexes_every_vertex_it_counts_and_no_more() {
        let classes = [
            ShapeClass { children: [28, 0] },
            ShapeClass { children: [7, 0] },
            ShapeClass { children: [6, 4] },
            ShapeClass { children: [1, 1] },
        ];
        for class in classes {
            for lod in 0..3 {
                let indices = class.indices(lod);
                let vertices = class.vertices(lod);
                assert_eq!(indices.len() % 3, 0, "{class:?} LOD {lod}");
                // The mesh must reference every vertex without exceeding its allocated
                // range.
                let used: std::collections::HashSet<u32> = indices.iter().copied().collect();
                assert_eq!(used.len() as u32, vertices, "{class:?} LOD {lod}");
                assert_eq!(used.iter().max(), Some(&(vertices - 1)));
            }
        }
        // Fourteen leaflet pairs need 8 rows for the stalk and 4 per leaflet.
        let fern = ShapeClass { children: [28, 0] };
        assert_eq!(fern.vertices(0), 17 + 28 * 9);
        assert!(fern.fits());
        // Far detail is six single triangles.
        assert_eq!(fern.vertices(2), 6 * 3);
        assert!(!ShapeClass { children: [32, 8] }.fits());

        // Eight twigs with eight leaves each use 8 main-stem rows, 2 per twig and 3 per
        // leaf. Mid detail keeps four twigs, each with four single-triangle leaves.
        let bush = ShapeClass { children: [8, 8] };
        assert_eq!(bush.vertices(0), 17 + 8 * 5 + 64 * 7);
        assert_eq!(bush.vertices(1), 9 + 4 * 3 + 16 * 3);
        assert_eq!(bush.vertices(2), 6 * 3);
        assert!(bush.fits());
    }

    #[test]
    fn far_detail_keeps_the_level_with_the_most_area() {
        // (stalk length and width, drawn children, relative lengths and widths) for
        // presets at a typical height, and the level they keep.
        let presets = [
            ("fern", 0.725, 0.007, [28, 0], [0.3, 0.0], [0.22, 0.0], 1),
            ("nettle", 0.8, 0.006, [16, 0], [0.2, 0.0], [0.55, 0.0], 1),
            ("salal", 0.65, 0.005, [5, 7], [0.4, 0.38], [0.03, 0.6], 2),
            (
                "huckleberry",
                0.825,
                0.006,
                [8, 8],
                [0.45, 0.14],
                [0.03, 0.55],
                2,
            ),
            ("bramble", 2.3, 0.02, [8, 8], [0.07, 1.1], [0.09, 0.6], 2),
            ("broom", 0.475, 0.02, [6, 3], [2.2, 0.35], [0.012, 0.025], 1),
            ("bare stalk", 0.5, 0.01, [0, 0], [0.0, 0.0], [0.0, 0.0], 0),
        ];
        for (name, length, width, children, lengths, widths, level) in presets {
            assert_eq!(
                far_level(length, width, children, lengths, widths),
                level,
                "{name}"
            );
        }
        // A thick stalk with a few tiny leaves keeps the stalk.
        assert_eq!(far_level(1.0, 0.1, [4, 0], [0.1, 0.0], [0.2, 0.0]), 0);
    }

    proptest! {
        #[test]
        fn far_parts_are_mid_parts_nearest_the_widest_point(
            children in 1u32..=32,
            opposite in any::<bool>(),
            widest in 0.0f32..=1.0,
            kept in 1u32..=8,
        ) {
            let children = drawn_children(children, opposite);
            let kept = if opposite { kept.next_multiple_of(2) } else { kept };
            let mid: Vec<u32> = (0..mid_children(children))
                .map(|k| mid_child(k, children, opposite))
                .collect();
            let count = far_kept(children, opposite, kept);
            let far: Vec<u32> = (0..kept).filter_map(|k| far_child(k, children, opposite, widest, kept)).collect();
            prop_assert_eq!(far.len() as u32, count);
            prop_assert!(count >= 1 && count <= kept, "{count}");
            // Far parts are distinct mid parts, kept in pairs when opposite.
            prop_assert!(far.windows(2).all(|w| w[0] < w[1]), "{far:?}");
            prop_assert!(far.iter().all(|i| mid.contains(i)), "{far:?} in {mid:?}");
            if opposite {
                for pair in far.chunks(2) {
                    prop_assert_eq!(pair[0] / 2, pair[1] / 2);
                }
            }
            // They are a run of mid parts, so none left out is nearer the widest point
            // than the farthest kept one, give or take one attachment point.
            let step = if opposite { 2 } else { 1 };
            let points = mid.len() as f32 / step as f32;
            let at = |i: u32| {
                let j = mid.iter().position(|&m| m == i).expect("a mid part") / step;
                ((j as f32 + 0.5) / points - widest).abs()
            };
            let farthest = far.iter().map(|&i| at(i)).fold(0.0, f32::max);
            let nearest_dropped = mid
                .iter()
                .filter(|i| !far.contains(i))
                .map(|&i| at(i))
                .fold(f32::MAX, f32::min);
            prop_assert!(nearest_dropped + 1.0 / points >= farthest, "{far:?} of {mid:?}");
        }
    }

    #[test]
    fn shadow_stems_keep_every_child_with_single_triangle_leaves() {
        let classes = [
            ShapeClass { children: [28, 0] },
            ShapeClass { children: [7, 0] },
            ShapeClass { children: [6, 4] },
            ShapeClass { children: [8, 8] },
            ShapeClass { children: [1, 1] },
        ];
        for class in classes {
            let indices = class.shadow_indices();
            let vertices = class.shadow_vertices();
            assert_eq!(indices.len() % 3, 0, "{class:?}");
            let used: std::collections::HashSet<u32> = indices.iter().copied().collect();
            assert_eq!(used.len() as u32, vertices, "{class:?}");
            assert_eq!(used.iter().max(), Some(&(vertices - 1)), "{class:?}");
            assert!(vertices < class.vertices(0), "{class:?} must get cheaper");
        }
        // The fern's shadow retains its full stalk and all 28 leaflets,
        // using three vertices per leaflet.
        assert_eq!(
            ShapeClass { children: [28, 0] }.shadow_vertices(),
            17 + 28 * 3
        );
        // The bush's shadow retains its stalk and twigs, simplifying only the leaves.
        assert_eq!(
            ShapeClass { children: [8, 8] }.shadow_vertices(),
            17 + 8 * 5 + 64 * 3
        );
    }

    #[test]
    fn mid_detail_keeps_about_half_and_at_most_eight_children() {
        let expected = [
            (0, 0),
            (1, 1),
            (2, 2),
            (5, 3),
            (7, 4),
            (8, 4),
            (14, 8),
            (28, 8),
        ];
        for (children, kept) in expected {
            assert_eq!(mid_children(children), kept, "{children} children");
        }
        // At mid detail, a fern retains four leaflet pairs. Huckleberry retains
        // four twigs with four leaves each; mature bracken retains six with six each.
        assert_eq!(ShapeClass { children: [28, 0] }.vertices(1), 9 + 8 * 3);
        assert_eq!(
            ShapeClass { children: [8, 8] }.vertices(1),
            9 + 4 * 3 + 16 * 3
        );
        assert_eq!(
            ShapeClass { children: [10, 12] }.vertices(1),
            9 + 6 * 3 + 36 * 3
        );
    }

    proptest! {
        #[test]
        fn mid_detail_children_are_distinct_spread_and_paired(
            children in 1u32..=32,
            opposite in any::<bool>(),
        ) {
            // Opposite branches grow in pairs, so start with an even count.
            let children = drawn_children(children, opposite);
            let kept: Vec<u32> = (0..mid_children(children))
                .map(|k| mid_child(k, children, opposite))
                .collect();
            prop_assert!(kept.windows(2).all(|w| w[0] < w[1]), "{kept:?}");
            prop_assert!(kept.iter().all(|&i| i < children), "{kept:?}");
            if opposite {
                // Retaining a branch must also retain its partner.
                for pair in kept.chunks(2) {
                    prop_assert_eq!(pair.len(), 2);
                    prop_assert_eq!(pair[0] / 2, pair[1] / 2);
                }
            }
            // Check spacing between retained attachment points: the largest gap
            // must be no more than twice the smallest.
            let step = if opposite { 2 } else { 1 };
            let points: Vec<u32> = kept.iter().step_by(step).map(|i| i / step as u32).collect();
            let gaps: Vec<u32> = points.windows(2).map(|w| w[1] - w[0]).collect();
            if let (Some(&low), Some(&high)) = (gaps.iter().min(), gaps.iter().max()) {
                prop_assert!(high <= 2 * low, "{points:?}");
            }
        }
    }

    #[test]
    fn opposite_children_round_up_to_pairs() {
        assert_eq!(drawn_children(13, true), 14, "opposite pairs round up");
        assert_eq!(drawn_children(13, false), 13);
    }

    #[test]
    fn gust_noise_uses_the_full_range() {
        let noise = gust_noise(64);
        assert_eq!(noise.iter().min(), Some(&0));
        assert_eq!(noise.iter().max(), Some(&255));
    }

    proptest! {
        #[test]
        fn unit_is_in_the_unit_interval(h in any::<u32>()) {
            let value = unit(h);
            prop_assert!((0.0..1.0).contains(&value), "{h} -> {value}");
        }

        #[test]
        fn toroidal_distance_is_symmetric(
            ax in -1e6f32..1e6,
            ay in -1e6f32..1e6,
            bx in -1e6f32..1e6,
            by in -1e6f32..1e6,
        ) {
            let a = Vec2::new(ax, ay);
            let b = Vec2::new(bx, by);
            prop_assert_eq!(
                toroidal_distance_squared(a, b),
                toroidal_distance_squared(b, a)
            );
        }

        #[test]
        fn toroidal_distance_is_never_negative(
            ax in -1e6f32..1e6,
            ay in -1e6f32..1e6,
            bx in -1e6f32..1e6,
            by in -1e6f32..1e6,
        ) {
            let a = Vec2::new(ax, ay);
            let b = Vec2::new(bx, by);
            prop_assert!(toroidal_distance_squared(a, b) >= 0.0);
        }

        #[test]
        fn chunk_layout_tile_size_is_positive(
            chunk_size in 0.001f32..10_000.0,
            density in 0.0f32..100_000.0,
        ) {
            let layout = ChunkLayout::for_density(chunk_size, density);
            prop_assert!(layout.tile_size > 0.0);
            prop_assert!(layout.tiles_per_side >= 1);
            prop_assert!(layout.candidate_density > 0.0);
        }
    }
}
