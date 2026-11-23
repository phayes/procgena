//! Lays out a batch of requests: each request's slices of the global
//! arrays, the arena regions those arrays live in, and the packed `Request`
//! and `Batch` structs of `shaders/batch.wesl`.

use glam::{Mat3A, Vec3, Vec3A};

use super::GpuSurfaceInput;
use super::scan::{ScanLevel, plan_scan, sums_words};
use super::sort::{SortKey, SortPlan, hist_words, plan_sort};
use crate::blob::ATTEMPTS_PER_CLUMP;
use crate::field::{exposure_ray_dirs, field_words};
use crate::options::{ClumpOptions, Placement};

/// Byte sizes of `Request` and `Batch` in `batch.wesl`.
pub(crate) const REQUEST_SIZE: usize = 416;
pub(crate) const BATCH_SIZE: usize = 544;

/// Words of the counters buffer per request.
pub(crate) const COUNTER_STRIDE: u32 = 64;

/// The counters' indices (`COUNTER_*` in `batch.wesl`), for tests that read
/// them back.
#[cfg(test)]
pub(crate) mod counter {
    pub(crate) const TRIANGLES: u32 = 0;
    pub(crate) const VERTICES: u32 = 1;
    pub(crate) const WELDED: u32 = 2;
    pub(crate) const OPEN_EDGES: u32 = 3;
    pub(crate) const OVERFLOW: u32 = 4;
}

/// `COUNTER_OVERFLOW` bits (`OVERFLOW_*` in `batch.wesl`).
pub(crate) const OVERFLOW_INPUT: u32 = 1;
pub(crate) const OVERFLOW_VERTICES: u32 = 2;
pub(crate) const OVERFLOW_INDICES: u32 = 4;

/// Region indices (`REGION_*` in `batch.wesl`).
pub(crate) const REGION_POSITIONS: usize = 0;
pub(crate) const REGION_NORMALS: usize = 1;
pub(crate) const REGION_CORNERS: usize = 2;
pub(crate) const REGION_WELD_FLAGS: usize = 3;
pub(crate) const REGION_WELD_LIST_A: usize = 4;
pub(crate) const REGION_WELD_LIST_B: usize = 5;
pub(crate) const REGION_WELD_KEY_X: usize = 6;
pub(crate) const REGION_WELD_KEY_Y: usize = 7;
pub(crate) const REGION_WELD_KEY_Z: usize = 8;
pub(crate) const REGION_WELD_KEY_REQUEST: usize = 9;
pub(crate) const REGION_WELD_STARTS: usize = 10;
pub(crate) const REGION_WELD_HEADS: usize = 11;
pub(crate) const REGION_WELD_RANK: usize = 12;
pub(crate) const REGION_WELD_ID: usize = 13;
pub(crate) const REGION_WELDED_NORMALS: usize = 14;
pub(crate) const REGION_EDGE_FLAGS: usize = 15;
pub(crate) const REGION_EDGE_LIST_A: usize = 16;
pub(crate) const REGION_EDGE_LIST_B: usize = 17;
pub(crate) const REGION_EDGE_KEY_LO: usize = 18;
pub(crate) const REGION_EDGE_KEY_HI: usize = 19;
pub(crate) const REGION_EDGE_KEY_REQUEST: usize = 20;
pub(crate) const REGION_EDGE_STARTS: usize = 21;
pub(crate) const REGION_EDGE_HEADS: usize = 22;
pub(crate) const REGION_WELD_SORTED: usize = 23;
pub(crate) const REGION_EDGE_SORTED: usize = 24;
pub(crate) const REGION_TRI_KEY_MORTON: usize = 25;
pub(crate) const REGION_TRI_KEY_REQUEST: usize = 26;
pub(crate) const REGION_TRI_FLAGS: usize = 27;
pub(crate) const REGION_TRI_LIST_A: usize = 28;
pub(crate) const REGION_TRI_LIST_B: usize = 29;
pub(crate) const REGION_TRI_SORTED: usize = 30;
pub(crate) const REGION_BVH_NODES: usize = 31;
pub(crate) const REGION_BVH_READY: usize = 32;
pub(crate) const REGION_RAYS: usize = 33;
pub(crate) const REGION_SCATTER_CUMULATIVE: usize = 34;
pub(crate) const REGION_SCATTER_FACING: usize = 35;
pub(crate) const REGION_ATTEMPTS: usize = 36;
pub(crate) const REGION_SEEDS: usize = 37;
pub(crate) const REGION_SEED_NEXT: usize = 38;
pub(crate) const REGION_GRID: usize = 39;
pub(crate) const REGION_BLOB_VERTICES: usize = 40;
pub(crate) const REGION_BLOB_TRIANGLES: usize = 41;
pub(crate) const REGION_WORK_POSITIONS: usize = 42;
pub(crate) const REGION_WORK_FADE: usize = 43;
pub(crate) const REGION_WORK_CORNERS: usize = 44;
pub(crate) const REGION_WORK_USED: usize = 45;
pub(crate) const REGION_WORK_KEPT: usize = 46;
pub(crate) const REGION_WORK_BURIED: usize = 47;
pub(crate) const REGION_WORK_NORMALS: usize = 48;
pub(crate) const REGION_BLOB_BOUNDS: usize = 49;
pub(crate) const REGION_BLOB_REJECTED: usize = 50;
pub(crate) const REGION_FACING_KEEP: usize = 51;
pub(crate) const REGION_CORNER_OWNER: usize = 52;
pub(crate) const REGION_POINT_OF_ID: usize = 53;
pub(crate) const REGION_PATCH_POINTS: usize = 54;
pub(crate) const REGION_PATCH_TRIS: usize = 55;
pub(crate) const REGION_POINT_REMOVED: usize = 56;
pub(crate) const REGION_POINT_LOCKED: usize = 57;
pub(crate) const REGION_POINT_BORDER: usize = 58;
pub(crate) const REGION_POINT_CHANGED: usize = 59;
pub(crate) const REGION_FINE_POINTS: usize = 60;
pub(crate) const REGION_PATCH_ALIVE: usize = 61;
pub(crate) const REGION_AROUND_START: usize = 62;
pub(crate) const REGION_AROUND_LIST: usize = 63;
pub(crate) const REGION_CANDIDATE_STATE: usize = 64;
pub(crate) const REGION_CANDIDATE_PRIORITY: usize = 65;
pub(crate) const REGION_CANDIDATE_PLACEMENT: usize = 66;
pub(crate) const REGION_COARSE_POINT_FLAGS: usize = 67;
pub(crate) const REGION_COARSE_TRI_FLAGS: usize = 68;
pub(crate) const REGION_COARSE_POINTS: usize = 69;
pub(crate) const REGION_COARSE_TRIS: usize = 70;
/// Regions of the flags buffer, in the same table
/// (`FLAG_REGION_*` in `batch.wesl`).
pub(crate) const FLAG_REGION_POINT_OWNER: usize = 71;
pub(crate) const FLAG_REGION_AROUND_COUNT: usize = 72;
pub(crate) const FLAG_REGION_AROUND_CURSOR: usize = 73;
pub(crate) const FLAG_REGION_BORDER_EDGES: usize = 74;
pub(crate) const FLAG_REGION_LOW_PRIORITY: usize = 75;
pub(crate) const FLAG_REGION_LOW_SLOT: usize = 76;
pub(crate) const REGION_COARSE_KEY_MORTON: usize = 77;
pub(crate) const REGION_COARSE_KEY_REQUEST: usize = 78;
pub(crate) const REGION_COARSE_BVH_FLAGS: usize = 79;
pub(crate) const REGION_COARSE_LIST_A: usize = 80;
pub(crate) const REGION_COARSE_LIST_B: usize = 81;
pub(crate) const REGION_COARSE_SORTED: usize = 82;
pub(crate) const REGION_COARSE_NODES: usize = 83;
pub(crate) const REGION_COARSE_READY: usize = 84;
pub(crate) const FLAG_REGION_RISE: usize = 85;
pub(crate) const FLAG_REGION_DIP: usize = 86;
pub(crate) const REGION_COVER_FLAGS: usize = 87;
pub(crate) const REGION_LAYER_TRIS: usize = 88;
pub(crate) const REGION_SHEET_POINT_FLAGS: usize = 89;
pub(crate) const REGION_SHEET_POINTS: usize = 90;
pub(crate) const REGION_SHEET_TRIS_A: usize = 91;
pub(crate) const REGION_SHEET_TRIS_B: usize = 92;
pub(crate) const REGION_SHEET_AROUND_START: usize = 93;
pub(crate) const REGION_SHEET_AROUND_LIST: usize = 94;
pub(crate) const REGION_REFINE_MARKS: usize = 95;
pub(crate) const REGION_REFINE_CHILDREN: usize = 96;
pub(crate) const REGION_REFINE_CREATES: usize = 97;
pub(crate) const REGION_REFINE_OWNER: usize = 98;
pub(crate) const FLAG_REGION_SHEET_AROUND_COUNT: usize = 99;
pub(crate) const FLAG_REGION_SHEET_AROUND_CURSOR: usize = 100;
pub(crate) const REGION_SHELL_BLUR_A: usize = 101;
pub(crate) const REGION_SHELL_BLUR_B: usize = 102;
pub(crate) const REGION_SHELL_DIRS: usize = 103;
pub(crate) const REGION_SHELL_ALONG: usize = 104;
pub(crate) const REGION_SHELL_WEIGHTS: usize = 105;
pub(crate) const REGION_SHELL_TRI_STATE: usize = 106;
pub(crate) const FLAG_REGION_SHELL_TOPS: usize = 107;
pub(crate) const FLAG_REGION_SHELL_LOWER: usize = 108;
pub(crate) const REGION_SHELL_EDGES: usize = 109;
pub(crate) const REGION_SHELL_NEIGHBOUR_START: usize = 110;
pub(crate) const REGION_SHELL_NEIGHBOURS: usize = 111;
pub(crate) const REGION_CLOSE_DEPTHS: usize = 112;
pub(crate) const REGION_CLOSE_SINK_DIRS: usize = 113;
pub(crate) const REGION_CLOSE_BORDER: usize = 114;
pub(crate) const REGION_CLOSE_FAN: usize = 115;
pub(crate) const REGION_CLOSE_SPLIT: usize = 116;
pub(crate) const REGION_CLOSE_COPIES: usize = 117;
pub(crate) const REGION_CLOSE_SOURCE: usize = 118;
pub(crate) const REGION_CLOSE_FIRST: usize = 119;
pub(crate) const REGION_CLOSE_BELOW: usize = 120;
pub(crate) const REGION_CLOSE_KEPT: usize = 121;
pub(crate) const REGION_CLOSE_WALLS: usize = 122;
pub(crate) const REGION_CLOSE_AROUND_START: usize = 123;
pub(crate) const REGION_CLOSE_AROUND_LIST: usize = 124;
pub(crate) const FLAG_REGION_CLOSE_AROUND_COUNT: usize = 125;
pub(crate) const FLAG_REGION_CLOSE_AROUND_CURSOR: usize = 126;
/// Entries of the region table in use.
pub(crate) const REGION_COUNT: usize = 127;

/// Most blur and slope passes, and fold passes before folded triangles are
/// left out (`MAX_BLUR_PASSES`, `MAX_SLOPE_PASSES` and `MAX_REPAIR_PASSES`
/// in `shell.wesl`).
pub(crate) const MAX_BLUR_PASSES: u32 = 256;
pub(crate) const MAX_SLOPE_PASSES: u32 = 256;
pub(crate) const MAX_REPAIR_PASSES: u32 = 24;

/// Refine passes (`REFINE_PASSES` in `refine.wesl`).
pub(crate) const REFINE_PASSES: u32 = 16;

/// Simplify sweeps at most (`MAX_SWEEPS` in `simplify.wesl`), and the
/// selection rounds each sweep runs. Sweeps on test rocks needed at most 6
/// rounds; a sweep that runs out is reported, not wrong.
pub(crate) const MAX_SWEEPS: u32 = 36;
pub(crate) const MAX_MIS_ROUNDS: u32 = 16;

/// Words per working-surface point and triangle (`POINT_WORDS` and
/// `PATCH_TRI_WORDS` in `patch.wesl`).
pub(crate) const POINT_WORDS: u32 = 18;
pub(crate) const PATCH_TRI_WORDS: u32 = 6;

/// Region slots in `Batch.regions` (thirty-two `vec4<u32>`).
const REGION_SLOTS: usize = 128;

/// Words per inner BVH node (`NODE_WORDS` in `bvh.wesl`).
pub(crate) const NODE_WORDS: u32 = 16;

/// Words per scatter attempt, seed and grid cell (`ATTEMPT_WORDS`,
/// `SEED_WORDS` and `GRID_WORDS` in `scatter.wesl`).
pub(crate) const ATTEMPT_WORDS: u32 = 12;
pub(crate) const SEED_WORDS: u32 = 9;
pub(crate) const GRID_WORDS: u32 = 4;

/// Bits of the BVH's Morton codes.
const MORTON_BITS: u32 = 30;

/// What a batch needs to know about one request to lay out its arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RequestShape {
    /// The input's vertex and triangle capacities.
    pub(crate) vertex_capacity: u32,
    pub(crate) triangle_capacity: u32,
    /// Scatter attempts and clumps (0 for a layer).
    pub(crate) attempts: u32,
    pub(crate) seeds: u32,
    /// Exposure rays, over all directions.
    pub(crate) rays: u32,
    /// Output vertices, and output indices (a multiple of 3).
    pub(crate) out_vertices: u32,
    pub(crate) out_indices: u32,
    /// Whether strict mode checks scattered clumps for crossings.
    pub(crate) strict_scatter: bool,
    /// Whether the request grows one layer, and whether that layer is
    /// closed into a solid.
    pub(crate) layer: bool,
    pub(crate) closed_layer: bool,
}

impl RequestShape {
    /// Returns the shape of a request with these input and output
    /// capacities and `options`, which must be validated. `out_indices` is
    /// rounded down to whole triangles.
    pub(crate) fn new(
        input: &GpuSurfaceInput,
        options: &ClumpOptions,
        out_vertices: u32,
        out_indices: u32,
    ) -> Self {
        let (attempts, seeds) = match options.placement.mode {
            Placement::Everywhere => (0, 0),
            Placement::Scatter => (
                options.placement.count.saturating_mul(ATTEMPTS_PER_CLUMP),
                options.placement.count,
            ),
        };
        Self {
            vertex_capacity: input.vertex_capacity,
            triangle_capacity: input.triangle_capacity,
            attempts,
            seeds,
            rays: exposure_ray_dirs(options).len() as u32,
            out_vertices,
            out_indices: out_indices - out_indices % 3,
            strict_scatter: options.mesh.strict && options.placement.mode == Placement::Scatter,
            layer: options.placement.mode == Placement::Everywhere,
            closed_layer: options.placement.mode == Placement::Everywhere && options.mesh.closed,
        }
    }

    /// Words of crossing bits this request's strict scatter needs: one bit
    /// per pair of clumps.
    fn pair_words(&self) -> u32 {
        if !self.strict_scatter {
            return 0;
        }
        let n = self.seeds as u64;
        (n * n.saturating_sub(1) / 2)
            .div_ceil(32)
            .min(u32::MAX as u64) as u32
    }

    /// Cells of this request's spacing grid: a power of two with room to
    /// spare for every clump.
    fn grid_cells(&self) -> u32 {
        if self.seeds == 0 {
            0
        } else {
            (2 * self.seeds).next_power_of_two().max(16)
        }
    }
}

/// One request's slices of the batch's global arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RequestSlices {
    pub(crate) vertex_base: u32,
    pub(crate) vertex_capacity: u32,
    pub(crate) triangle_base: u32,
    pub(crate) triangle_capacity: u32,
    pub(crate) attempt_base: u32,
    pub(crate) attempt_count: u32,
    pub(crate) seed_base: u32,
    pub(crate) seed_capacity: u32,
    pub(crate) grid_base: u32,
    pub(crate) grid_capacity: u32,
    pub(crate) ray_base: u32,
    pub(crate) out_vertex_base: u32,
    pub(crate) out_vertex_capacity: u32,
    pub(crate) out_index_base: u32,
    pub(crate) out_index_capacity: u32,
    /// First word of this request's crossing bits in the flags buffer.
    pub(crate) pair_base: u32,
}

/// The arena layout and the planned scans and sorts of a batch.
#[derive(Debug, Clone)]
pub(crate) struct BatchPlan {
    pub(crate) slices: Vec<RequestSlices>,
    /// Sums over every request: input vertex and triangle slots (V, T),
    /// attempt and seed slots (A, S), and output vertex and triangle slots
    /// (W, Wt).
    pub(crate) total_vertices: u32,
    pub(crate) total_triangles: u32,
    pub(crate) total_attempts: u32,
    pub(crate) total_seeds: u32,
    pub(crate) total_work_vertices: u32,
    pub(crate) total_work_triangles: u32,
    /// Word offset of each region in the arena.
    pub(crate) regions: [u32; REGION_COUNT],
    /// Words the arena needs.
    pub(crate) arena_words: u64,
    /// Words the flags buffer needs: one per seed slot, then every strict
    /// request's crossing bits.
    pub(crate) flag_words: u64,
    /// Whether any request grows a layer, closes its layer, or scatters
    /// clumps.
    pub(crate) has_layers: bool,
    pub(crate) has_closed_layers: bool,
    pub(crate) has_scatter: bool,
    pub(crate) weld_flags_scan: Vec<ScanLevel>,
    pub(crate) weld_starts_scan: Vec<ScanLevel>,
    pub(crate) weld_rank_scan: Vec<ScanLevel>,
    pub(crate) weld_sort: SortPlan,
    pub(crate) edge_flags_scan: Vec<ScanLevel>,
    pub(crate) edge_starts_scan: Vec<ScanLevel>,
    pub(crate) edge_sort: SortPlan,
    pub(crate) triangle_flags_scan: Vec<ScanLevel>,
    pub(crate) triangle_sort: SortPlan,
    /// The simplified working surface's BVH: its triangle flags scan and sort.
    pub(crate) coarse_flags_scan: Vec<ScanLevel>,
    pub(crate) coarse_sort: SortPlan,
    /// Refit passes that finish every BVH node: one per tree level, and the
    /// tree is no deeper than its sort key is long.
    pub(crate) refit_passes: u32,
    pub(crate) blob_vertices_scan: Vec<ScanLevel>,
    pub(crate) blob_triangles_scan: Vec<ScanLevel>,
    pub(crate) work_used_scan: Vec<ScanLevel>,
    pub(crate) work_kept_scan: Vec<ScanLevel>,
    pub(crate) corner_owner_scan: Vec<ScanLevel>,
    pub(crate) facing_keep_scan: Vec<ScanLevel>,
    pub(crate) around_start_scan: Vec<ScanLevel>,
    pub(crate) coarse_point_scan: Vec<ScanLevel>,
    pub(crate) coarse_tri_scan: Vec<ScanLevel>,
    pub(crate) cover_scan: Vec<ScanLevel>,
    pub(crate) sheet_point_scan: Vec<ScanLevel>,
    pub(crate) sheet_around_scan: Vec<ScanLevel>,
    pub(crate) refine_children_scan: Vec<ScanLevel>,
    pub(crate) refine_creates_scan: Vec<ScanLevel>,
    pub(crate) shell_neighbour_scan: Vec<ScanLevel>,
    pub(crate) close_border_scan: Vec<ScanLevel>,
    pub(crate) close_copies_scan: Vec<ScanLevel>,
    pub(crate) close_first_scan: Vec<ScanLevel>,
    pub(crate) close_kept_scan: Vec<ScanLevel>,
    pub(crate) close_walls_scan: Vec<ScanLevel>,
    pub(crate) close_around_scan: Vec<ScanLevel>,
}

/// Returns how many low bits hold every value below `n`: at least 1.
fn bits_for(n: u32) -> u32 {
    32 - n.saturating_sub(1).max(1).leading_zeros()
}

impl BatchPlan {
    /// Lays out a batch of requests with these shapes, in order.
    pub(crate) fn new(shapes: &[RequestShape]) -> Self {
        // Each request's slices, back to back. Crossing bits follow the
        // one flag word per seed slot.
        let total_seeds: u32 = shapes.iter().fold(0u32, |n, s| n.saturating_add(s.seeds));
        let mut pair_base = total_seeds;
        let mut slices = Vec::with_capacity(shapes.len());
        let mut next = RequestSlices::default();
        for shape in shapes {
            let pairs = pair_base;
            pair_base = pair_base.saturating_add(shape.pair_words());
            let s = RequestSlices {
                vertex_capacity: shape.vertex_capacity,
                triangle_capacity: shape.triangle_capacity,
                attempt_count: shape.attempts,
                seed_capacity: shape.seeds,
                grid_capacity: shape.grid_cells(),
                out_vertex_capacity: shape.out_vertices,
                out_index_capacity: shape.out_indices,
                pair_base: pairs,
                ..next
            };
            slices.push(s);
            next = RequestSlices {
                vertex_base: s.vertex_base.saturating_add(s.vertex_capacity),
                triangle_base: s.triangle_base.saturating_add(s.triangle_capacity),
                attempt_base: s.attempt_base.saturating_add(s.attempt_count),
                seed_base: s.seed_base.saturating_add(s.seed_capacity),
                grid_base: s.grid_base.saturating_add(s.grid_capacity),
                ray_base: s.ray_base.saturating_add(shape.rays),
                out_vertex_base: s.out_vertex_base.saturating_add(s.out_vertex_capacity),
                out_index_base: s.out_index_base.saturating_add(s.out_index_capacity),
                ..RequestSlices::default()
            };
        }
        let (v, triangles) = (next.vertex_base, next.triangle_base);
        let e = triangles.saturating_mul(3);
        let (a, s) = (next.attempt_base, next.seed_base);
        let (w, wt) = (next.out_vertex_base, next.out_index_base / 3);

        // Arena regions, one after another.
        let mut used = 0u64;
        let mut alloc = |words: u64| {
            let at = used;
            used += words;
            at.min(u32::MAX as u64) as u32
        };
        let mut regions = [0u32; REGION_COUNT];
        let [v64, e64, t64, a64, s64, w64, wt64] = [v, e, triangles, a, s, w, wt].map(u64::from);
        // The layer sheet's output-sized regions, empty when no request
        // grows a layer.
        let has_layers = shapes.iter().any(|s| s.layer);
        let (sheet_w, sheet_wt) = if has_layers { (w, wt) } else { (0, 0) };
        let (sheet_w64, sheet_wt64) = (u64::from(sheet_w), u64::from(sheet_wt));
        regions[REGION_POSITIONS] = alloc(3 * v64);
        regions[REGION_NORMALS] = alloc(3 * v64);
        regions[REGION_CORNERS] = alloc(e64);
        regions[REGION_WELD_FLAGS] = alloc(v64 + 1);
        regions[REGION_WELD_LIST_A] = alloc(v64);
        regions[REGION_WELD_LIST_B] = alloc(v64);
        for key in [
            REGION_WELD_KEY_X,
            REGION_WELD_KEY_Y,
            REGION_WELD_KEY_Z,
            REGION_WELD_KEY_REQUEST,
        ] {
            regions[key] = alloc(v64);
        }
        regions[REGION_WELD_STARTS] = alloc(v64 + 1);
        regions[REGION_WELD_HEADS] = alloc(v64);
        regions[REGION_WELD_RANK] = alloc(v64 + 1);
        regions[REGION_WELD_ID] = alloc(v64);
        regions[REGION_WELDED_NORMALS] = alloc(3 * v64);
        regions[REGION_EDGE_FLAGS] = alloc(e64 + 1);
        regions[REGION_EDGE_LIST_A] = alloc(e64);
        regions[REGION_EDGE_LIST_B] = alloc(e64);
        for key in [
            REGION_EDGE_KEY_LO,
            REGION_EDGE_KEY_HI,
            REGION_EDGE_KEY_REQUEST,
        ] {
            regions[key] = alloc(e64);
        }
        regions[REGION_EDGE_STARTS] = alloc(e64 + 1);
        regions[REGION_EDGE_HEADS] = alloc(e64);
        regions[REGION_TRI_KEY_MORTON] = alloc(t64);
        regions[REGION_TRI_KEY_REQUEST] = alloc(t64);
        regions[REGION_TRI_FLAGS] = alloc(t64 + 1);
        regions[REGION_TRI_LIST_A] = alloc(t64);
        regions[REGION_TRI_LIST_B] = alloc(t64);
        regions[REGION_BVH_NODES] = alloc(NODE_WORDS as u64 * t64);
        regions[REGION_BVH_READY] = alloc(t64);
        regions[REGION_RAYS] = alloc(3 * next.ray_base as u64);
        regions[REGION_SCATTER_CUMULATIVE] = alloc(t64);
        regions[REGION_SCATTER_FACING] = alloc(t64);
        regions[REGION_ATTEMPTS] = alloc(ATTEMPT_WORDS as u64 * a64);
        regions[REGION_SEEDS] = alloc(SEED_WORDS as u64 * s64);
        regions[REGION_SEED_NEXT] = alloc(s64);
        regions[REGION_GRID] = alloc(GRID_WORDS as u64 * next.grid_base as u64);
        regions[REGION_BLOB_VERTICES] = alloc(s64 + 1);
        regions[REGION_BLOB_TRIANGLES] = alloc(s64 + 1);
        regions[REGION_WORK_POSITIONS] = alloc(3 * w64);
        regions[REGION_WORK_FADE] = alloc(w64);
        regions[REGION_WORK_CORNERS] = alloc(3 * wt64);
        regions[REGION_WORK_USED] = alloc(w64 + 1);
        regions[REGION_WORK_KEPT] = alloc(wt64 + 1);
        regions[REGION_WORK_BURIED] = alloc(w64);
        regions[REGION_WORK_NORMALS] = alloc(3 * w64);
        regions[REGION_BLOB_BOUNDS] = alloc(6 * s64);
        regions[REGION_BLOB_REJECTED] = alloc(s64);
        regions[REGION_FACING_KEEP] = alloc(t64 + 1);
        regions[REGION_CORNER_OWNER] = alloc(e64 + 1);
        regions[REGION_POINT_OF_ID] = alloc(v64);
        regions[REGION_PATCH_POINTS] = alloc(POINT_WORDS as u64 * v64);
        regions[REGION_PATCH_TRIS] = alloc(PATCH_TRI_WORDS as u64 * t64);
        for one_per_point in [
            REGION_POINT_REMOVED,
            REGION_POINT_LOCKED,
            REGION_POINT_BORDER,
            REGION_POINT_CHANGED,
        ] {
            regions[one_per_point] = alloc(v64);
        }
        regions[REGION_FINE_POINTS] = alloc(6 * v64);
        regions[REGION_PATCH_ALIVE] = alloc(t64);
        regions[REGION_AROUND_START] = alloc(v64 + 1);
        regions[REGION_AROUND_LIST] = alloc(e64);
        regions[REGION_CANDIDATE_STATE] = alloc(e64);
        regions[REGION_CANDIDATE_PRIORITY] = alloc(e64);
        regions[REGION_CANDIDATE_PLACEMENT] = alloc(6 * e64);
        regions[REGION_COARSE_POINT_FLAGS] = alloc(v64 + 1);
        regions[REGION_COARSE_TRI_FLAGS] = alloc(t64 + 1);
        regions[REGION_COARSE_POINTS] = alloc(POINT_WORDS as u64 * v64);
        regions[REGION_COARSE_TRIS] = alloc(PATCH_TRI_WORDS as u64 * t64);
        regions[REGION_COARSE_KEY_MORTON] = alloc(t64);
        regions[REGION_COARSE_KEY_REQUEST] = alloc(t64);
        regions[REGION_COARSE_BVH_FLAGS] = alloc(t64 + 1);
        regions[REGION_COARSE_LIST_A] = alloc(t64);
        regions[REGION_COARSE_LIST_B] = alloc(t64);
        regions[REGION_COARSE_NODES] = alloc(NODE_WORDS as u64 * t64);
        regions[REGION_COARSE_READY] = alloc(t64);
        regions[REGION_COVER_FLAGS] = alloc(t64 + 1);
        regions[REGION_LAYER_TRIS] = alloc(PATCH_TRI_WORDS as u64 * t64);
        regions[REGION_SHEET_POINT_FLAGS] = alloc(v64 + 1);
        regions[REGION_SHEET_POINTS] = alloc(POINT_WORDS as u64 * sheet_w64);
        regions[REGION_SHEET_TRIS_A] = alloc(PATCH_TRI_WORDS as u64 * sheet_wt64);
        regions[REGION_SHEET_TRIS_B] = alloc(PATCH_TRI_WORDS as u64 * sheet_wt64);
        regions[REGION_SHEET_AROUND_START] = alloc(sheet_w64 + 1);
        regions[REGION_SHEET_AROUND_LIST] = alloc(3 * sheet_wt64);
        regions[REGION_REFINE_MARKS] = alloc(sheet_wt64);
        regions[REGION_REFINE_CHILDREN] = alloc(sheet_wt64 + 1);
        regions[REGION_REFINE_CREATES] = alloc(3 * sheet_wt64 + 1);
        regions[REGION_REFINE_OWNER] = alloc(3 * sheet_wt64);
        for vector_per_point in [
            REGION_SHELL_BLUR_A,
            REGION_SHELL_BLUR_B,
            REGION_SHELL_DIRS,
            REGION_SHELL_ALONG,
        ] {
            regions[vector_per_point] = alloc(3 * sheet_w64);
        }
        regions[REGION_SHELL_WEIGHTS] = alloc(sheet_w64);
        regions[REGION_SHELL_TRI_STATE] = alloc(sheet_wt64);
        regions[REGION_SHELL_EDGES] = alloc(3 * sheet_wt64);
        // Each triangle gives each corner at most two neighbours.
        regions[REGION_SHELL_NEIGHBOUR_START] = alloc(sheet_w64 + 1);
        regions[REGION_SHELL_NEIGHBOURS] = alloc(6 * sheet_wt64);

        // Closing's regions, empty when no layer is closed.
        let has_closed_layers = shapes.iter().any(|s| s.closed_layer);
        let (close_w, close_wt) = if has_closed_layers { (w, wt) } else { (0, 0) };
        let (close_w64, close_wt64) = (u64::from(close_w), u64::from(close_wt));
        regions[REGION_CLOSE_DEPTHS] = alloc(close_w64);
        regions[REGION_CLOSE_SINK_DIRS] = alloc(3 * close_w64);
        regions[REGION_CLOSE_BORDER] = alloc(3 * close_wt64 + 1);
        regions[REGION_CLOSE_FAN] = alloc(3 * close_wt64);
        regions[REGION_CLOSE_SPLIT] = alloc(3 * close_wt64);
        regions[REGION_CLOSE_COPIES] = alloc(close_w64 + 1);
        regions[REGION_CLOSE_SOURCE] = alloc(close_w64);
        regions[REGION_CLOSE_FIRST] = alloc(3 * close_wt64 + 1);
        regions[REGION_CLOSE_BELOW] = alloc(close_w64);
        regions[REGION_CLOSE_KEPT] = alloc(close_wt64 + 1);
        regions[REGION_CLOSE_WALLS] = alloc(close_w64 + 1);
        regions[REGION_CLOSE_AROUND_START] = alloc(close_w64 + 1);
        regions[REGION_CLOSE_AROUND_LIST] = alloc(3 * close_wt64);

        // Scratch shared by the scans and sorts, which run one at a time.
        let largest = [v, e, s, w, wt, sheet_wt.saturating_mul(3)]
            .into_iter()
            .max()
            .unwrap_or(0);
        let scan_scratch = alloc(sums_words(largest + 1) as u64);
        let sort_scratch = alloc(hist_words(v.max(e)) as u64);

        // The scans, each over its region plus the total at the end.
        let scan = |region: usize, count: u32| plan_scan(count + 1, regions[region], scan_scratch);
        let weld_flags_scan = scan(REGION_WELD_FLAGS, v);
        let weld_starts_scan = scan(REGION_WELD_STARTS, v);
        let weld_rank_scan = scan(REGION_WELD_RANK, v);
        let edge_flags_scan = scan(REGION_EDGE_FLAGS, e);
        let edge_starts_scan = scan(REGION_EDGE_STARTS, e);
        let triangle_flags_scan = scan(REGION_TRI_FLAGS, triangles);
        let coarse_flags_scan = scan(REGION_COARSE_BVH_FLAGS, triangles);
        let blob_vertices_scan = scan(REGION_BLOB_VERTICES, s);
        let blob_triangles_scan = scan(REGION_BLOB_TRIANGLES, s);
        let work_used_scan = scan(REGION_WORK_USED, w);
        let work_kept_scan = scan(REGION_WORK_KEPT, wt);
        let corner_owner_scan = scan(REGION_CORNER_OWNER, e);
        let facing_keep_scan = scan(REGION_FACING_KEEP, triangles);
        let around_start_scan = scan(REGION_AROUND_START, v);
        let coarse_point_scan = scan(REGION_COARSE_POINT_FLAGS, v);
        let coarse_tri_scan = scan(REGION_COARSE_TRI_FLAGS, triangles);
        let cover_scan = scan(REGION_COVER_FLAGS, triangles);
        let sheet_point_scan = scan(REGION_SHEET_POINT_FLAGS, v);
        let sheet_around_scan = scan(REGION_SHEET_AROUND_START, sheet_w);
        let refine_children_scan = scan(REGION_REFINE_CHILDREN, sheet_wt);
        let refine_creates_scan = scan(REGION_REFINE_CREATES, sheet_wt.saturating_mul(3));
        let shell_neighbour_scan = scan(REGION_SHELL_NEIGHBOUR_START, sheet_w);
        let close_border_scan = scan(REGION_CLOSE_BORDER, close_wt.saturating_mul(3));
        let close_copies_scan = scan(REGION_CLOSE_COPIES, close_w);
        let close_first_scan = scan(REGION_CLOSE_FIRST, close_wt.saturating_mul(3));
        let close_kept_scan = scan(REGION_CLOSE_KEPT, close_wt);
        let close_walls_scan = scan(REGION_CLOSE_WALLS, close_w);
        let close_around_scan = scan(REGION_CLOSE_AROUND_START, close_w);

        // Sort vertices by position bits then request, edges by their
        // welded ends then request, and triangles by Morton code then
        // request (for the BVH). The counts are the flag scans' totals.
        let request_bits = bits_for(shapes.len() as u32);
        let max_vertices = shapes.iter().map(|c| c.vertex_capacity).max().unwrap_or(1);
        let max_triangles = shapes
            .iter()
            .map(|c| c.triangle_capacity)
            .max()
            .unwrap_or(1);
        let id_bits = bits_for(max_vertices);
        let key = |region: usize, bits: u32| SortKey {
            offset: regions[region],
            bits,
        };
        let weld_sort = plan_sort(
            v,
            regions[REGION_WELD_FLAGS] + v,
            regions[REGION_WELD_LIST_A],
            regions[REGION_WELD_LIST_B],
            &[
                key(REGION_WELD_KEY_X, 32),
                key(REGION_WELD_KEY_Y, 32),
                key(REGION_WELD_KEY_Z, 32),
                key(REGION_WELD_KEY_REQUEST, request_bits),
            ],
            sort_scratch,
        );
        let edge_sort = plan_sort(
            e,
            regions[REGION_EDGE_FLAGS] + e,
            regions[REGION_EDGE_LIST_A],
            regions[REGION_EDGE_LIST_B],
            &[
                key(REGION_EDGE_KEY_LO, id_bits),
                key(REGION_EDGE_KEY_HI, id_bits),
                key(REGION_EDGE_KEY_REQUEST, request_bits),
            ],
            sort_scratch,
        );
        let triangle_sort = plan_sort(
            triangles,
            regions[REGION_TRI_FLAGS] + triangles,
            regions[REGION_TRI_LIST_A],
            regions[REGION_TRI_LIST_B],
            &[
                key(REGION_TRI_KEY_MORTON, MORTON_BITS),
                key(REGION_TRI_KEY_REQUEST, request_bits),
            ],
            sort_scratch,
        );
        let coarse_sort = plan_sort(
            triangles,
            regions[REGION_COARSE_BVH_FLAGS] + triangles,
            regions[REGION_COARSE_LIST_A],
            regions[REGION_COARSE_LIST_B],
            &[
                key(REGION_COARSE_KEY_MORTON, MORTON_BITS),
                key(REGION_COARSE_KEY_REQUEST, request_bits),
            ],
            sort_scratch,
        );
        regions[REGION_WELD_SORTED] = weld_sort.result_offset;
        regions[REGION_EDGE_SORTED] = edge_sort.result_offset;
        regions[REGION_TRI_SORTED] = triangle_sort.result_offset;
        regions[REGION_COARSE_SORTED] = coarse_sort.result_offset;

        // Flags-buffer regions follow the crossing bits.
        let mut flag_words = pair_base as u64;
        for one_per_point in [
            FLAG_REGION_POINT_OWNER,
            FLAG_REGION_AROUND_COUNT,
            FLAG_REGION_AROUND_CURSOR,
            FLAG_REGION_BORDER_EDGES,
            FLAG_REGION_LOW_PRIORITY,
            FLAG_REGION_LOW_SLOT,
            FLAG_REGION_RISE,
            FLAG_REGION_DIP,
        ] {
            regions[one_per_point] = flag_words.min(u32::MAX as u64) as u32;
            flag_words += v64;
        }
        for one_per_sheet_point in [
            FLAG_REGION_SHEET_AROUND_COUNT,
            FLAG_REGION_SHEET_AROUND_CURSOR,
            FLAG_REGION_SHELL_TOPS,
            FLAG_REGION_SHELL_LOWER,
        ] {
            regions[one_per_sheet_point] = flag_words.min(u32::MAX as u64) as u32;
            flag_words += sheet_w64;
        }
        for one_per_closed_vertex in [
            FLAG_REGION_CLOSE_AROUND_COUNT,
            FLAG_REGION_CLOSE_AROUND_CURSOR,
        ] {
            regions[one_per_closed_vertex] = flag_words.min(u32::MAX as u64) as u32;
            flag_words += close_w64;
        }

        Self {
            slices,
            total_vertices: v,
            total_triangles: triangles,
            total_attempts: a,
            total_seeds: s,
            total_work_vertices: w,
            total_work_triangles: wt,
            regions,
            arena_words: used.max(1),
            flag_words: flag_words.max(1),
            weld_flags_scan,
            weld_starts_scan,
            weld_rank_scan,
            weld_sort,
            edge_flags_scan,
            edge_starts_scan,
            edge_sort,
            triangle_flags_scan,
            triangle_sort,
            coarse_flags_scan,
            coarse_sort,
            refit_passes: MORTON_BITS + bits_for(max_triangles) + 1,
            blob_vertices_scan,
            blob_triangles_scan,
            work_used_scan,
            work_kept_scan,
            corner_owner_scan,
            facing_keep_scan,
            around_start_scan,
            coarse_point_scan,
            coarse_tri_scan,
            cover_scan,
            sheet_point_scan,
            sheet_around_scan,
            refine_children_scan,
            refine_creates_scan,
            shell_neighbour_scan,
            close_border_scan,
            close_copies_scan,
            close_first_scan,
            close_kept_scan,
            close_walls_scan,
            close_around_scan,
            has_layers,
            has_closed_layers,
            has_scatter: shapes.iter().any(|s| s.seeds > 0),
        }
    }

    /// Returns the `Batch` uniform.
    pub(crate) fn batch_bytes(&self) -> Vec<u8> {
        let mut words = vec![
            self.slices.len() as u32,
            self.total_vertices,
            self.total_triangles,
            self.total_attempts,
            self.total_seeds,
            self.total_work_vertices,
            self.total_work_triangles,
            0,
        ];
        let mut slots = [0u32; REGION_SLOTS];
        slots[..REGION_COUNT].copy_from_slice(&self.regions);
        words.extend(slots);
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

/// Returns request `slices`' `Request` struct for reading `input` with
/// `options`, which must be validated.
pub(crate) fn pack_request(
    input: &GpuSurfaceInput,
    options: &ClumpOptions,
    slices: &RequestSlices,
) -> [u8; REQUEST_SIZE] {
    let format = input.format;
    let closed_fact = match input.closed {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    };
    let mode = match options.placement.mode {
        Placement::Everywhere => 0,
        Placement::Scatter => 1,
    };
    let rays = exposure_ray_dirs(options).len() as u32 / options.directions.len() as u32;
    let scale = options.scale;
    let mut words = vec![
        format.stride,
        format.position_offset,
        format.normal_offset,
        closed_fact,
        (input.draw_args_offset / 4) as u32,
        slices.vertex_capacity,
        slices.triangle_capacity,
        slices.vertex_base,
        slices.triangle_base,
        mode,
        slices.ray_base,
        rays,
        slices.attempt_base,
        slices.attempt_count,
        slices.seed_base,
        slices.seed_capacity,
        slices.grid_base,
        slices.grid_capacity,
        slices.out_vertex_base,
        slices.out_vertex_capacity,
        slices.out_index_base,
        slices.out_index_capacity,
        options.mesh.closed as u32,
        options.mesh.strict as u32,
        (options.placement.radius * scale).to_bits(),
        (options.placement.min_spacing * scale).to_bits(),
        options.placement.size_variation.to_bits(),
        options.shape.neck.to_bits(),
        (options.mesh.edge_length * scale).to_bits(),
        options.shape.max_slope.to_bits(),
        slices.pair_base,
        0,
    ];

    // Rows of the transform, then rows of the cofactor matrix that turns
    // normals: with columns a, b, c, its columns are b×c, c×a and a×b.
    let m = input.transform.matrix3;
    let t = input.transform.translation;
    for i in 0..3 {
        let row = [m.x_axis[i], m.y_axis[i], m.z_axis[i], t[i]];
        words.extend(row.map(f32::to_bits));
    }
    let (a, b, c): (Vec3A, Vec3A, Vec3A) = (m.x_axis, m.y_axis, m.z_axis);
    let cofactor = Mat3A::from_cols(b.cross(c), c.cross(a), a.cross(b));
    for row in [cofactor.row(0), cofactor.row(1), cofactor.row(2)] {
        words.extend([row.x, row.y, row.z, 0.0].map(f32::to_bits));
    }
    words.extend(field_words(options));

    let mut bytes = [0u8; REQUEST_SIZE];
    for (k, w) in words.iter().enumerate() {
        bytes[4 * k..4 * k + 4].copy_from_slice(&w.to_le_bytes());
    }
    bytes
}

/// Returns every request's exposure ray directions, request after request,
/// packed as the `REGION_RAYS` words.
pub(crate) fn pack_rays(options: &[&ClumpOptions]) -> Vec<u32> {
    options
        .iter()
        .flat_map(|o| exposure_ray_dirs(o))
        .flat_map(|d: Vec3| d.to_array().map(f32::to_bits))
        .collect()
}
