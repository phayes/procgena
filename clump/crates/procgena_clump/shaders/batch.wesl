// Bindings and lookups shared by every generator pass over a batch.
//
// A batch holds several clump requests, run in lockstep: each pass covers
// every request at once. Each request owns a fixed slice of each global
// array, sized by its capacities: vertex slots
// `vertex_base .. vertex_base + vertex_capacity` and triangle slots
// `triangle_base .. triangle_base + triangle_capacity`. Slots past the
// request's actual counts are unused.
//
// Arrays live in regions of `arena`, at word offsets listed in
// `batch.regions` (see the `REGION_*` indices). Per-request counts and flags
// live in `counters`, `COUNTER_STRIDE` words per request.

import package::field::Field;

/// Work items per workgroup of the per-item passes.
const WORKGROUP_SIZE: u32 = 64u;

/// Words of `counters` per request.
const COUNTER_STRIDE: u32 = 64u;
/// Input triangles the request has (at most its capacity).
const COUNTER_TRIANGLES: u32 = 0u;
/// Input vertices the request uses: its highest referenced index + 1.
const COUNTER_VERTICES: u32 = 1u;
/// Distinct input positions after welding.
const COUNTER_WELDED: u32 = 2u;
/// Welded input edges not shared by exactly two triangles.
const COUNTER_OPEN_EDGES: u32 = 3u;
/// `OVERFLOW_*` bits.
const COUNTER_OVERFLOW: u32 = 4u;
/// The bounds of the request's triangle centroids, three words each,
/// encoded so that unsigned order is float order.
const COUNTER_BOUNDS_MIN: u32 = 5u;
const COUNTER_BOUNDS_MAX: u32 = 8u;
/// The bounds of the request's input vertices, encoded the same way.
const COUNTER_VERTEX_MIN: u32 = 11u;
const COUNTER_VERTEX_MAX: u32 = 14u;
/// `f32` bits of the step off the input before casting rays: a millionth of
/// the input's bounding diagonal.
const COUNTER_EPSILON: u32 = 17u;
/// 1 if the input is closed, by its own measure or the request's fact.
const COUNTER_CLOSED: u32 = 18u;
/// Scattered clumps placed.
const COUNTER_SEEDS: u32 = 19u;
/// `f32` bits of the total triangle weight scattered clumps are drawn by.
const COUNTER_WEIGHT_TOTAL: u32 = 20u;
/// Vertices and triangles the clump mesh needs before unused parts are
/// dropped, and the vertices and indices it has after.
const COUNTER_WORK_VERTICES: u32 = 21u;
const COUNTER_WORK_TRIANGLES: u32 = 22u;
const COUNTER_OUT_VERTICES: u32 = 23u;
const COUNTER_OUT_INDICES: u32 = 24u;
/// Separate clumps in the finished mesh: the seeds, less any strict mode
/// left out.
const COUNTER_CLUMPS: u32 = 25u;
/// Layer: points and triangles of the working surface.
const COUNTER_POINTS: u32 = 26u;
const COUNTER_PATCH_TRIANGLES: u32 = 27u;
/// Layer simplification: merges in the current sweep; 1 once the request's
/// sweeps are done; 1 if any sweep merged; 1 if a sweep ran out of
/// selection rounds before every candidate was decided.
const COUNTER_MERGED: u32 = 28u;
const COUNTER_SIMPLIFY_DONE: u32 = 29u;
const COUNTER_COLLAPSED: u32 = 30u;
const COUNTER_ROUNDS_CAPPED: u32 = 31u;
/// Layer: points and triangles of the simplified working surface.
const COUNTER_COARSE_POINTS: u32 = 32u;
const COUNTER_COARSE_TRIANGLES: u32 = 33u;
/// Layer: triangles of the final working surface, after the cover filter.
const COUNTER_LAYER_TRIANGLES: u32 = 34u;
/// Layer sheet: points and triangles while refining; 1 once refining has
/// run out of output capacity, after which the request stops.
const COUNTER_SHEET_POINTS: u32 = 35u;
const COUNTER_SHEET_TRIANGLES: u32 = 36u;
const COUNTER_SHEET_FAILED: u32 = 37u;
/// Layer shell: blur passes the push directions need; the largest rise of
/// the simplified surface (`f32` bits); vertices thinned to fit a gap or
/// the input's thickness.
const COUNTER_BLUR_PASSES: u32 = 38u;
const COUNTER_MAX_RISE: u32 = 39u;
const COUNTER_THINNED: u32 = 40u;
/// Slope passes: heights lowered in even and odd passes; 1 once a pass
/// lowers nothing.
const COUNTER_SLOPE_CHANGED: u32 = 41u;
const COUNTER_SLOPE_DONE: u32 = 43u;
/// Fold passes: triangles turned over in even and odd passes; 1 once none
/// are; triangles left out because lowering couldn't fix them.
const COUNTER_FOLD_COUNT: u32 = 44u;
const COUNTER_FOLD_DONE: u32 = 46u;
const COUNTER_FOLDED: u32 = 47u;

/// The input has more triangles than `triangle_capacity`, or references a
/// vertex past `vertex_capacity`.
const OVERFLOW_INPUT: u32 = 1u;
/// The clump needs more vertices, or indices, than its output reserves.
const OVERFLOW_VERTICES: u32 = 2u;
const OVERFLOW_INDICES: u32 = 4u;

/// How a request's input mesh is read and placed, its settings, and its
/// slices of the batch's global arrays. 416 bytes.
struct Request {
    /// Byte stride of one input vertex, and byte offsets of its position
    /// and normal (three `f32` each). All multiples of 4.
    input_stride: u32,
    position_offset: u32,
    normal_offset: u32,
    /// Whether the input is closed: 0 to measure it, 1 open, 2 closed.
    closed_fact: u32,
    /// Word of the input's draw arguments in its draw-args buffer.
    draw_args_word: u32,
    /// The input's capacities, and its slices of the vertex and triangle slots.
    vertex_capacity: u32,
    triangle_capacity: u32,
    vertex_base: u32,
    triangle_base: u32,
    /// `MODE_*`: one layer, or scattered clumps.
    mode: u32,
    /// First exposure ray in `REGION_RAYS`, and rays per direction (0 when
    /// exposure isn't measured).
    ray_base: u32,
    ray_count: u32,
    /// Scatter: the request's slices of the attempt and seed slots, and of
    /// the spacing grid (a power of two).
    attempt_base: u32,
    attempt_count: u32,
    seed_base: u32,
    seed_capacity: u32,
    grid_base: u32,
    grid_capacity: u32,
    /// The request's slices of the output: its vertices and indices.
    out_vertex_base: u32,
    out_vertex_capacity: u32,
    out_index_base: u32,
    out_index_capacity: u32,
    /// 1 to close each clump into a solid, and 1 for strict mode.
    closed: u32,
    strict: u32,
    /// Scatter settings, lengths after `scale`.
    clump_radius: f32,
    min_spacing: f32,
    size_variation: f32,
    neck: f32,
    /// Target triangle edge length, after `scale`, and steepest slope.
    edge_length: f32,
    max_slope: f32,
    /// Strict scatter: first word in `flags` of this request's blob-pair
    /// crossing bits.
    pair_base: u32,
    _pad0: u32,
    /// Rows of the affine transform into clump space: `xyz` is the matrix
    /// row and `w` the translation.
    transform: array<vec4<f32>, 3>,
    /// Rows of the matrix applied to normals (the cofactor matrix of the
    /// transform's linear part).
    normal_transform: array<vec4<f32>, 3>,
    field: Field,
}

/// Batch-wide sizes and region offsets. Usable as a uniform.
struct Batch {
    request_count: u32,
    /// Sums over all requests of: input vertex and triangle capacities
    /// (V, T), attempt and seed slots (A, S), and output vertex and triangle
    /// slots (W, Wt; Wt is the index capacity over 3).
    total_vertices: u32,
    total_triangles: u32,
    total_attempts: u32,
    total_seeds: u32,
    total_work_vertices: u32,
    total_work_triangles: u32,
    _pad: u32,
    /// Word offset of each region; index with `REGION_*` (in `arena`) and
    /// `FLAG_REGION_*` (in `flags`).
    regions: array<vec4<u32>, 32>,
}

/// Region indices into `Batch.regions`. Sizes are per global vertex slot
/// (V), triangle slot (T) or triangle edge slot (E = 3T).
/// Transformed input positions and normals, 3 words per V.
const REGION_POSITIONS: u32 = 0u;
const REGION_NORMALS: u32 = 1u;
/// Local input vertex index of each triangle corner, 3 words per T.
const REGION_CORNERS: u32 = 2u;
/// Weld: 1 per active V, then the scan total (V + 1).
const REGION_WELD_FLAGS: u32 = 3u;
/// Weld: the vertex list being sorted, two halves of V each.
const REGION_WELD_LIST_A: u32 = 4u;
const REGION_WELD_LIST_B: u32 = 5u;
/// Weld sort keys: position bits and request, V each.
const REGION_WELD_KEY_X: u32 = 6u;
const REGION_WELD_KEY_Y: u32 = 7u;
const REGION_WELD_KEY_Z: u32 = 8u;
const REGION_WELD_KEY_REQUEST: u32 = 9u;
/// Weld: 1 where a sorted run starts, then the scan total (V + 1).
const REGION_WELD_STARTS: u32 = 10u;
/// Weld: sorted position of each run's start, V.
const REGION_WELD_HEADS: u32 = 11u;
/// Weld: 1 for each run's first vertex, by vertex slot, then the scan
/// total (V + 1). After the scan, a representative's global welded index.
const REGION_WELD_RANK: u32 = 12u;
/// Weld: the local welded id of each vertex slot, V.
const REGION_WELD_ID: u32 = 13u;
/// Welded normals by global welded index, 3 words per V.
const REGION_WELDED_NORMALS: u32 = 14u;
/// Edges: the same layout as the weld regions, per E.
const REGION_EDGE_FLAGS: u32 = 15u;
const REGION_EDGE_LIST_A: u32 = 16u;
const REGION_EDGE_LIST_B: u32 = 17u;
const REGION_EDGE_KEY_LO: u32 = 18u;
const REGION_EDGE_KEY_HI: u32 = 19u;
const REGION_EDGE_KEY_REQUEST: u32 = 20u;
const REGION_EDGE_STARTS: u32 = 21u;
const REGION_EDGE_HEADS: u32 = 22u;
/// Whichever of each sort's two lists holds its sorted result.
const REGION_WELD_SORTED: u32 = 23u;
const REGION_EDGE_SORTED: u32 = 24u;
/// BVH: Morton code and request of each triangle slot, T each.
const REGION_TRI_KEY_MORTON: u32 = 25u;
const REGION_TRI_KEY_REQUEST: u32 = 26u;
/// BVH: 1 per used triangle slot, then the scan total (T + 1). After the
/// scan, each request's first entry is where its triangles start in the
/// sorted list.
const REGION_TRI_FLAGS: u32 = 27u;
/// BVH: the triangle list being sorted, two halves of T each, and whichever
/// holds the sorted list.
const REGION_TRI_LIST_A: u32 = 28u;
const REGION_TRI_LIST_B: u32 = 29u;
const REGION_TRI_SORTED: u32 = 30u;
/// BVH: inner nodes, `NODE_WORDS` (in `bvh.wesl`) per triangle slot, and the
/// refit pass each was finished in (T).
const REGION_BVH_NODES: u32 = 31u;
const REGION_BVH_READY: u32 = 32u;
/// Exposure ray directions of every request, 3 words per ray.
const REGION_RAYS: u32 = 33u;
/// Scatter: per triangle slot, the running total of the weights clumps are
/// drawn by (`f32`), and the triangle's largest corner facing (`f32`).
const REGION_SCATTER_CUMULATIVE: u32 = 34u;
const REGION_SCATTER_FACING: u32 = 35u;
/// Scatter: `ATTEMPT_WORDS` (in `scatter.wesl`) per attempt slot.
const REGION_ATTEMPTS: u32 = 36u;
/// Scatter: `SEED_WORDS` per seed slot, and each seed's next seed in its
/// grid cell.
const REGION_SEEDS: u32 = 37u;
const REGION_SEED_NEXT: u32 = 38u;
/// Scatter: the spacing grid, `GRID_WORDS` per cell.
const REGION_GRID: u32 = 39u;
/// Per seed slot, then the scan total (S + 1): the vertices and the
/// triangles of each clump's mesh.
const REGION_BLOB_VERTICES: u32 = 40u;
const REGION_BLOB_TRIANGLES: u32 = 41u;
/// The clump mesh being built, per output vertex slot (W): position (3),
/// fade (1); and per output triangle slot (W/3 of the index slots): corners
/// (3, as working vertex slots).
const REGION_WORK_POSITIONS: u32 = 42u;
const REGION_WORK_FADE: u32 = 43u;
const REGION_WORK_CORNERS: u32 = 44u;
/// 1 per used working vertex, and 1 per kept working triangle, then the
/// scan totals.
const REGION_WORK_USED: u32 = 45u;
const REGION_WORK_KEPT: u32 = 46u;
/// 1 per working vertex at or below the input surface (W).
const REGION_WORK_BURIED: u32 = 47u;
/// Working vertex normals, 3 words per W.
const REGION_WORK_NORMALS: u32 = 48u;
/// Strict scatter: each blob's bounds (min xyz, max xyz; 6 per S), and 1
/// for each blob left out (S).
const REGION_BLOB_BOUNDS: u32 = 49u;
const REGION_BLOB_REJECTED: u32 = 50u;
/// Layer: 1 per input triangle slot that faces some direction and doesn't
/// collapse once welded, then the scan total (T + 1). After the scan, its
/// working-surface triangle number.
const REGION_FACING_KEEP: u32 = 51u;
/// Layer: 1 per triangle corner slot that is the first use of its welded
/// position, then the scan total (E + 1). After the scan, its point number.
const REGION_CORNER_OWNER: u32 = 52u;
/// Layer: the working-surface point of each welded position (global welded
/// index, V).
const REGION_POINT_OF_ID: u32 = 53u;
/// Layer: working-surface points, `POINT_WORDS` (in `patch.wesl`) per
/// vertex slot, and triangles, `PATCH_TRI_WORDS` per triangle slot.
const REGION_PATCH_POINTS: u32 = 54u;
const REGION_PATCH_TRIS: u32 = 55u;
/// Simplify, per working-surface point slot (V): 1 once merged away, 1 if
/// locked in place, 1 on the surface's border, the sweep it last changed in;
/// and its position and normal before simplifying (6 words).
const REGION_POINT_REMOVED: u32 = 56u;
const REGION_POINT_LOCKED: u32 = 57u;
const REGION_POINT_BORDER: u32 = 58u;
const REGION_POINT_CHANGED: u32 = 59u;
const REGION_FINE_POINTS: u32 = 60u;
/// Simplify, per triangle slot (T): 1 while the triangle is alive.
const REGION_PATCH_ALIVE: u32 = 61u;
/// Simplify: each point's triangles: where its list starts, then the scan
/// total (V + 1), and the lists (E words, as triangle slots).
const REGION_AROUND_START: u32 = 62u;
const REGION_AROUND_LIST: u32 = 63u;
/// Simplify, per triangle edge slot (E): the candidate merge's state
/// (`CANDIDATE_*`), priority, and placement (6 words).
const REGION_CANDIDATE_STATE: u32 = 64u;
const REGION_CANDIDATE_PRIORITY: u32 = 65u;
const REGION_CANDIDATE_PLACEMENT: u32 = 66u;
/// Simplify, then compaction: 1 per point slot (V + 1) and per triangle
/// slot (T + 1) that survives, then the scan totals; and the compacted
/// points and triangles, in the working surface's layout.
const REGION_COARSE_POINT_FLAGS: u32 = 67u;
const REGION_COARSE_TRI_FLAGS: u32 = 68u;
const REGION_COARSE_POINTS: u32 = 69u;
const REGION_COARSE_TRIS: u32 = 70u;
/// The simplified surface's BVH, laid out as the input's (`REGION_TRI_*`,
/// `REGION_BVH_*`): Morton code and request per triangle slot, flags then
/// the scan total (T + 1), two lists and whichever holds the sorted one,
/// inner nodes, and the refit pass each node was finished in.
const REGION_COARSE_KEY_MORTON: u32 = 77u;
const REGION_COARSE_KEY_REQUEST: u32 = 78u;
const REGION_COARSE_BVH_FLAGS: u32 = 79u;
const REGION_COARSE_LIST_A: u32 = 80u;
const REGION_COARSE_LIST_B: u32 = 81u;
const REGION_COARSE_SORTED: u32 = 82u;
const REGION_COARSE_NODES: u32 = 83u;
const REGION_COARSE_READY: u32 = 84u;
/// Envelope, in `flags`, per point slot (V): how far the input rises above,
/// and dips below, the simplified surface around each point (`f32` bits).
const FLAG_REGION_RISE: u32 = 85u;
const FLAG_REGION_DIP: u32 = 86u;
/// Cover: 1 per simplified triangle a clump may cover, then the scan total
/// (T + 1); and the layer's final working-surface triangles, as
/// `PATCH_TRI_WORDS` per triangle slot.
const REGION_COVER_FLAGS: u32 = 87u;
const REGION_LAYER_TRIS: u32 = 88u;
/// Layer sheet, the surface the clump is refined and pushed out from. It
/// lives in the output-sized slots: points per output vertex slot (W) and
/// triangles per output triangle slot (Wt).
/// 1 per simplified point slot a covered triangle uses, then the scan total
/// (V + 1); after the scan, its sheet point number.
const REGION_SHEET_POINT_FLAGS: u32 = 89u;
/// Sheet points, `POINT_WORDS` per W; triangles, `PATCH_TRI_WORDS` per Wt,
/// in two buffers that refine passes alternate between (A holds the
/// result).
const REGION_SHEET_POINTS: u32 = 90u;
const REGION_SHEET_TRIS_A: u32 = 91u;
const REGION_SHEET_TRIS_B: u32 = 92u;
/// Each sheet point's triangles: where its list starts, then the scan total
/// (W + 1), and the lists (3 Wt words, as triangle slots, in no order).
const REGION_SHEET_AROUND_START: u32 = 93u;
const REGION_SHEET_AROUND_LIST: u32 = 94u;
/// Refine, per triangle slot (Wt): bit k set if edge k (corner k to k + 1)
/// is split this pass. Per triangle slot, then the scan total (Wt + 1): the
/// triangles it becomes.
const REGION_REFINE_MARKS: u32 = 95u;
const REGION_REFINE_CHILDREN: u32 = 96u;
/// Refine, per edge call slot (3 Wt + 1, numbered 3 × triangle + the order
/// the CPU makes that triangle's midpoints in): 1 where a new midpoint is
/// made, then the scan total. Per edge slot (3 Wt, 3 × triangle + edge):
/// the call slot that makes the edge's midpoint.
const REGION_REFINE_CREATES: u32 = 97u;
const REGION_REFINE_OWNER: u32 = 98u;
/// In `flags`, per sheet point slot (W): triangles around it, counted, then
/// a fill cursor.
const FLAG_REGION_SHEET_AROUND_COUNT: u32 = 99u;
const FLAG_REGION_SHEET_AROUND_CURSOR: u32 = 100u;
/// Shell, per sheet point slot (W): blurred normals in two buffers that
/// blur passes alternate between (A holds the result); push directions;
/// directions to move along when sunk; and coverage weights. Vectors take 3
/// words.
const REGION_SHELL_BLUR_A: u32 = 101u;
const REGION_SHELL_BLUR_B: u32 = 102u;
const REGION_SHELL_DIRS: u32 = 103u;
const REGION_SHELL_ALONG: u32 = 104u;
const REGION_SHELL_WEIGHTS: u32 = 105u;
/// Shell, per sheet triangle slot (Wt): `TRI_*` state.
const REGION_SHELL_TRI_STATE: u32 = 106u;
/// In `flags`, per sheet point slot (W): heights, encoded so unsigned order
/// is float order; and 1 for points to lower in the current fold pass.
const FLAG_REGION_SHELL_TOPS: u32 = 107u;
const FLAG_REGION_SHELL_LOWER: u32 = 108u;
/// Shell, per edge slot (3 Wt, 3 × triangle + edge): sheet edge lengths, 0
/// past the sheet, summed in place in pairs so each request's first slot
/// ends up holding its total.
const REGION_SHELL_EDGES: u32 = 109u;
/// Shell, per sheet point slot: where its neighbour list starts, then the
/// scan total (W + 1); and the lists (6 Wt words), each in ascending point
/// number order.
const REGION_SHELL_NEIGHBOUR_START: u32 = 110u;
const REGION_SHELL_NEIGHBOURS: u32 = 111u;
/// Closing, per sheet point slot (W): the underside's depth below the
/// surface, and the direction it is sunk along (3 words).
const REGION_CLOSE_DEPTHS: u32 = 112u;
const REGION_CLOSE_SINK_DIRS: u32 = 113u;
/// Closing, per corner slot of the kept top triangles (3 Wt + 1, 3 ×
/// triangle + corner): 1 where the edge from that corner to the next is a
/// border edge, counting each directed edge once; scanned.
const REGION_CLOSE_BORDER: u32 = 114u;
/// Closing, per corner slot (3 Wt): at a pinch point, the lowest triangle
/// slot of the corner's fan; and every kept corner's point number after
/// pinch points are split.
const REGION_CLOSE_FAN: u32 = 115u;
const REGION_CLOSE_SPLIT: u32 = 116u;
/// Closing, per sheet point slot (W + 1): copies a pinch point gets, one per
/// fan after the first; scanned.
const REGION_CLOSE_COPIES: u32 = 117u;
/// Closing, per working vertex slot (W): the sheet point behind each pinch
/// copy and underside vertex.
const REGION_CLOSE_SOURCE: u32 = 118u;
/// Closing, per corner slot (3 Wt + 1): 1 where a point, after splitting,
/// first appears in the kept top triangles; scanned to number the
/// underside vertices.
const REGION_CLOSE_FIRST: u32 = 119u;
/// Closing, per working vertex slot of a split point (W): the point number
/// of its underside vertex.
const REGION_CLOSE_BELOW: u32 = 120u;
/// Closing, per sheet triangle slot (Wt + 1): 1 for each kept top
/// triangle; scanned to number the underside triangles.
const REGION_CLOSE_KEPT: u32 = 121u;
/// Closing, per working vertex slot of a split point (W + 1): border edges
/// leaving it; scanned to number the walls.
const REGION_CLOSE_WALLS: u32 = 122u;
/// Closing, per working vertex slot (W + 1): where the list of triangles
/// around it in the finished mesh starts, then the scan total; and the
/// lists (3 Wt), in no particular order.
const REGION_CLOSE_AROUND_START: u32 = 123u;
const REGION_CLOSE_AROUND_LIST: u32 = 124u;
/// In `flags`, per working vertex slot (W): triangles around it in the
/// finished mesh, counted, then a fill cursor.
const FLAG_REGION_CLOSE_AROUND_COUNT: u32 = 125u;
const FLAG_REGION_CLOSE_AROUND_CURSOR: u32 = 126u;
/// Entries of the region table in use.
const REGION_COUNT: u32 = 127u;

/// Words per working-surface point: position (3), smooth normal (3),
/// exposure per direction (8), room, rise, dip, unused.
const POINT_WORDS: u32 = 18u;
/// Words per working-surface triangle: corners (3, as point numbers), and
/// the face normal of the input triangle it came from (3).
const PATCH_TRI_WORDS: u32 = 6u;

/// Regions of `flags` rather than `arena`, by the same table. Per global
/// welded index (V): the first corner slot that uses the position.
const FLAG_REGION_POINT_OWNER: u32 = 71u;
/// Simplify, per point slot (V): triangles around it (counted, then a fill
/// cursor), its border edges, and the lowest blocking priority and slot
/// of a selection round.
const FLAG_REGION_AROUND_COUNT: u32 = 72u;
const FLAG_REGION_AROUND_CURSOR: u32 = 73u;
const FLAG_REGION_BORDER_EDGES: u32 = 74u;
const FLAG_REGION_LOW_PRIORITY: u32 = 75u;
const FLAG_REGION_LOW_SLOT: u32 = 76u;

/// `Request.mode` values.
const MODE_EVERYWHERE: u32 = 0u;
const MODE_SCATTER: u32 = 1u;

@group(0) @binding(0) var<storage, read> requests: array<Request>;
@group(0) @binding(1) var<uniform> batch: Batch;
@group(0) @binding(2) var<storage, read_write> arena: array<u32>;
@group(0) @binding(3) var<storage, read_write> counters: array<atomic<u32>>;
/// Bit flags many threads set at once. Strict scatter: one word per seed
/// slot (1 if its blob crosses itself), then each strict request's
/// blob-pair crossing bits from its `pair_base`.
@group(0) @binding(4) var<storage, read_write> flags: array<atomic<u32>>;

/// Returns the word offset of region `index`.
fn region(index: u32) -> u32 {
    return batch.regions[index / 4u][index % 4u];
}

/// Returns the work item of this invocation. Dispatch rows of
/// `WORKGROUP_SIZE`-wide workgroups along x, adding rows along y when one
/// row is not enough; check the item against the dispatch's size.
fn item_index(global_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return global_id.x + global_id.y * num_workgroups.x * WORKGROUP_SIZE;
}

/// Returns the request owning global vertex slot `slot`.
fn request_of_vertex(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].vertex_base <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Returns the request owning global triangle slot `slot`.
fn request_of_triangle(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].triangle_base <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Returns the request owning working (output-sized) vertex slot `slot`.
fn request_of_work_vertex(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].out_vertex_base <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Returns the request owning working (output-sized) triangle slot `slot`.
fn request_of_work_triangle(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].out_index_base / 3u <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Returns counter `which` of request `r`.
fn counter(r: u32, which: u32) -> u32 {
    return atomicLoad(&counters[r * COUNTER_STRIDE + which]);
}

/// Reads three floats from the arena at word `at`.
fn load3(at: u32) -> vec3<f32> {
    return vec3<f32>(bitcast<f32>(arena[at]), bitcast<f32>(arena[at + 1u]), bitcast<f32>(arena[at + 2u]));
}

/// Writes three floats to the arena at word `at`.
fn store3(at: u32, v: vec3<f32>) {
    arena[at] = bitcast<u32>(v.x);
    arena[at + 1u] = bitcast<u32>(v.y);
    arena[at + 2u] = bitcast<u32>(v.z);
}
