// Pushes each open layer request's refined sheet out into a clump, like the
// Rust crate's `shell::displaced_shell` for open clumps: blurs the normals
// into push directions, works out every vertex's height and its limits,
// limits the slope, sinks buried vertices, keeps the triangles that can be
// seen, lowers the clump around triangles that turn over, and writes the
// result to the working mesh the output passes read.
//
// Pass order, with W sheet point slots and Wt sheet triangle slots, after
// the refine passes and with the triangle lists around each sheet point
// rebuilt for buffer A:
// - `shell_neighbour_count` (W + 1), a scan of `REGION_SHELL_NEIGHBOUR_START`,
//   `shell_neighbour_fill` (W);
// - `shell_edges` (3 Wt), then `shell_edge_sum` (3 Wt) numbered 0, 1, …
//   until the stride covers the largest layer's edge slots;
// - `shell_setup` (requests), `shell_blur_init` (W), then `shell_blur`
//   numbered 0 to `MAX_BLUR_PASSES - 1` (W each);
// - `shell_tops` (W);
// - `shell_slope` then `shell_slope_end` (requests), numbered 0 to
//   `MAX_SLOPE_PASSES - 1`;
// - `shell_sink` (W), `shell_keep` (Wt);
// - `shell_fold_mark` (Wt), `shell_fold_end` (requests), `shell_fold_lower`
//   (W), numbered 0 to `MAX_REPAIR_PASSES`;
// - after any scattered clumps are written, `shell_out_vertices` (W) and
//   `shell_out_tris` (Wt); after the working mesh's scans, `shell_normals`
//   (W).

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, store3, counter,
    request_of_work_vertex, request_of_work_triangle, COUNTER_STRIDE, COUNTER_SHEET_POINTS,
    COUNTER_SHEET_TRIANGLES, COUNTER_SHEET_FAILED, COUNTER_BLUR_PASSES, COUNTER_MAX_RISE,
    COUNTER_THINNED, COUNTER_SLOPE_CHANGED, COUNTER_SLOPE_DONE, COUNTER_FOLD_COUNT,
    COUNTER_FOLD_DONE, COUNTER_FOLDED, MODE_EVERYWHERE, REGION_SHEET_POINTS, REGION_SHEET_TRIS_A, REGION_SHEET_AROUND_START,
    REGION_SHEET_AROUND_LIST, REGION_SHELL_BLUR_A, REGION_SHELL_BLUR_B, REGION_SHELL_DIRS,
    REGION_SHELL_ALONG, REGION_SHELL_WEIGHTS, REGION_SHELL_TRI_STATE, REGION_SHELL_EDGES, REGION_SHELL_NEIGHBOUR_START, REGION_SHELL_NEIGHBOURS, FLAG_REGION_SHELL_TOPS,
    REGION_CLOSE_DEPTHS, REGION_CLOSE_SPLIT,
    FLAG_REGION_SHELL_LOWER, REGION_WORK_POSITIONS, REGION_WORK_FADE, REGION_WORK_CORNERS,
    REGION_WORK_USED, REGION_WORK_KEPT, REGION_WORK_NORMALS, POINT_WORDS, PATCH_TRI_WORDS,
    WORKGROUP_SIZE,
};
import package::bvh::{ray_hit, order_bits, from_order_bits, LEAF};
import package::field::{
    Point, weight, top, fade, layer_top, limit_tilt, lowered, LUMP_RADIUS_MAX, MAX_DIRECTIONS,
};
import package::math::{cross3, dot3, length3, normalize_or, normalize_or_zero, F32_MAX, F32_MIN_POSITIVE};
import package::measure::{epsilon_of, inside_span};

/// The number of the repeated pass being run, from 0.
struct ShellPass {
    number: u32,
}

@group(1) @binding(0) var<uniform> shell_pass: ShellPass;

/// Most blur passes, as the CPU's `MAX_BLUR_PASSES`.
const MAX_BLUR_PASSES: u32 = 256u;
/// Most slope passes. The CPU's slope limit has no cap; a request that
/// still lowers heights after this many is reported as not converged.
const MAX_SLOPE_PASSES: u32 = 256u;
/// Fold passes before folded triangles are left out, as the CPU's
/// `MAX_REPAIR_PASSES`.
const MAX_REPAIR_PASSES: u32 = 24u;
/// Shares of the gap to a facing surface, and of the input's thickness, a
/// clump may fill (`GAP_FILL` and `ROOM_FILL` on the CPU).
const GAP_FILL: f32 = 0.5;
const ROOM_FILL: f32 = 0.5;
/// A closed layer's underside depth below the sink depth, and its thinnest
/// solid, as shares of the thickness (`BOTTOM_GAP` and `MIN_SOLID` on the
/// CPU).
const BOTTOM_GAP: f32 = 0.25;
const MIN_SOLID: f32 = 0.05;

/// `REGION_SHELL_TRI_STATE` values: a triangle that can't be seen, one that
/// is kept, and one left out because it stayed turned over.
const TRI_HIDDEN: u32 = 0u;
const TRI_KEPT: u32 = 1u;
const TRI_FOLDED: u32 = 2u;
/// A kept triangle cut out in strict mode because it crossed another.
const TRI_CUT: u32 = 3u;

/// No point.
const NONE: u32 = 0xffffffffu;

/// Returns whether request `r` grows a sheet that refined within its
/// capacity.
fn shell_open(r: u32) -> bool {
    return requests[r].mode == MODE_EVERYWHERE && counter(r, COUNTER_SHEET_FAILED) == 0u;
}

/// Returns whether output vertex slot `slot` holds a sheet point of its
/// request `r`.
fn sheet_vertex(r: u32, slot: u32) -> bool {
    return shell_open(r) && slot - requests[r].out_vertex_base < counter(r, COUNTER_SHEET_POINTS);
}

/// Returns whether output triangle slot `t` holds a sheet triangle of its
/// request `r`.
fn sheet_triangle(r: u32, t: u32) -> bool {
    return shell_open(r) && t - requests[r].out_index_base / 3u < counter(r, COUNTER_SHEET_TRIANGLES);
}

/// Returns the corners of sheet triangle slot `t`, as point numbers of its
/// request.
fn corners_of(t: u32) -> array<u32, 3> {
    let at = region(REGION_SHEET_TRIS_A) + PATCH_TRI_WORDS * t;
    return array<u32, 3>(arena[at], arena[at + 1u], arena[at + 2u]);
}

/// Returns the word offset of sheet point slot `slot`.
fn point_at(slot: u32) -> u32 {
    return region(REGION_SHEET_POINTS) + POINT_WORDS * slot;
}

/// Returns sheet point slot `slot`.
fn sheet_point(slot: u32) -> Point {
    let at = point_at(slot);
    var exposure: array<f32, MAX_DIRECTIONS>;
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        exposure[d] = bitcast<f32>(arena[at + 6u + d]);
    }
    return Point(
        load3(at),
        load3(at + 3u),
        exposure,
        bitcast<f32>(arena[at + 14u]),
        bitcast<f32>(arena[at + 15u]),
        bitcast<f32>(arena[at + 16u]),
    );
}

/// Returns the height of sheet point slot `slot`.
fn top_of(slot: u32) -> f32 {
    return from_order_bits(atomicLoad(&flags[region(FLAG_REGION_SHELL_TOPS) + slot]));
}

/// Sets the height of sheet point slot `slot`.
fn set_top(slot: u32, height: f32) {
    atomicStore(&flags[region(FLAG_REGION_SHELL_TOPS) + slot], order_bits(height));
}

/// Returns where the top of sheet point slot `slot` sits: pushed out along
/// its push direction, or when sunk, in along its sink direction.
fn top_position(slot: u32) -> vec3<f32> {
    let height = top_of(slot);
    let dir = select(
        load3(region(REGION_SHELL_DIRS) + 3u * slot),
        load3(region(REGION_SHELL_ALONG) + 3u * slot),
        height < 0.0,
    );
    return load3(point_at(slot)) + dir * height;
}

/// Returns the range of `REGION_SHEET_AROUND_LIST` holding the triangles
/// around sheet point slot `slot`.
fn around(slot: u32) -> vec2<u32> {
    let start = region(REGION_SHEET_AROUND_START) + slot;
    return vec2<u32>(arena[start], arena[start + 1u]);
}

/// Returns the lowest neighbour of request `r`'s sheet point `p` along a
/// triangle edge that is at least `lowest`, or `NONE`. Called with `lowest`
/// rising past each result, it lists the neighbours once each in
/// ascending order, as the CPU's `neighbour_lists` holds them.
fn next_neighbour(r: u32, p: u32, lowest: u32) -> u32 {
    let span = around(requests[r].out_vertex_base + p);
    var best = NONE;
    for (var i = span.x; i < span.y; i++) {
        let v = corners_of(arena[region(REGION_SHEET_AROUND_LIST) + i]);
        for (var k = 0u; k < 3u; k++) {
            if v[k] != p && v[k] >= lowest {
                best = min(best, v[k]);
            }
        }
    }
    return best;
}

/// Returns the range of `REGION_SHELL_NEIGHBOURS` holding the neighbours of
/// sheet point slot `slot`, as point slots in ascending order.
fn neighbours(slot: u32) -> vec2<u32> {
    let start = region(REGION_SHELL_NEIGHBOUR_START) + slot;
    return vec2<u32>(arena[start], arena[start + 1u]);
}

// ---------------------------------------------------------------------------
// Neighbour lists

/// Counts each sheet point's neighbours into `REGION_SHELL_NEIGHBOUR_START`
/// for scanning: 0 for slots past the sheet and for the total slot.
/// Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_neighbour_count(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_work_vertices {
        return;
    }
    var count = 0u;
    if slot < batch.total_work_vertices {
        let r = request_of_work_vertex(slot);
        if sheet_vertex(r, slot) {
            let p = slot - requests[r].out_vertex_base;
            var n = next_neighbour(r, p, 0u);
            while n != NONE {
                count += 1u;
                n = next_neighbour(r, p, n + 1u);
            }
        }
    }
    arena[region(REGION_SHELL_NEIGHBOUR_START) + slot] = count;
}

/// Lists each sheet point's neighbours, in ascending order, after the
/// counts are scanned. Dispatch W items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_neighbour_fill(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    let base = requests[r].out_vertex_base;
    let p = slot - base;
    var at = region(REGION_SHELL_NEIGHBOURS) + neighbours(slot).x;
    var n = next_neighbour(r, p, 0u);
    while n != NONE {
        arena[at] = base + n;
        at += 1u;
        n = next_neighbour(r, p, n + 1u);
    }
}

// ---------------------------------------------------------------------------
// Push directions

/// Writes each sheet edge's length to `REGION_SHELL_EDGES`, and 0 to the
/// edge slots past each layer's sheet. Dispatch 3 Wt items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_edges(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s >= 3u * batch.total_work_triangles {
        return;
    }
    let t = s / 3u;
    let r = request_of_work_triangle(t);
    if !shell_open(r) {
        return;
    }
    var edge = 0.0;
    if sheet_triangle(r, t) {
        let base = requests[r].out_vertex_base;
        let v = corners_of(t);
        let k = s % 3u;
        let a = load3(point_at(base + v[k]));
        let b = load3(point_at(base + v[(k + 1u) % 3u]));
        edge = length3(a - b);
    }
    arena[region(REGION_SHELL_EDGES) + s] = bitcast<u32>(edge);
}

/// One level of summing each layer's edge lengths in pairs, as the CPU's
/// `pairwise_sum` does: at level `shell_pass.number`, each slot at a
/// multiple of twice the level's stride, counted from its request's first
/// edge slot, adds the slot one stride on. Run levels 0, 1, … until the
/// stride covers the largest request's edge slots. Dispatch 3 Wt items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_edge_sum(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s >= 3u * batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(s / 3u);
    if !shell_open(r) {
        return;
    }
    let first = 3u * (requests[r].out_index_base / 3u);
    let stride = 1u << shell_pass.number;
    let i = s - first;
    if i % (2u * stride) != 0u || i + stride >= 3u * (requests[r].out_index_capacity / 3u) {
        return;
    }
    let at = region(REGION_SHELL_EDGES) + s;
    let sum = bitcast<f32>(arena[at]) + bitcast<f32>(arena[at + stride]);
    arena[at] = bitcast<u32>(sum);
}

/// Works out each layer request's blur pass count: enough for averaging to
/// spread about the clump's tallest height, given the mean edge length.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_setup(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !shell_open(r) {
        return;
    }
    let request = requests[r];
    let triangles = counter(r, COUNTER_SHEET_TRIANGLES);
    var passes = 0u;
    if triangles > 0u {
        // The mean edge length, from the sum `shell_edge_sum` left in the
        // request's first edge slot.
        let first = 3u * (request.out_index_base / 3u);
        let total = bitcast<f32>(arena[region(REGION_SHELL_EDGES) + first]);
        let edge = max(total / f32(3u * triangles), F32_MIN_POSITIVE);

        // Averaging spreads about one edge per sqrt(pass).
        let field = request.field;
        let max_lump = field.lumpiness * LUMP_RADIUS_MAX * field.lump_size;
        let tallest = field.thickness + max_lump + bitcast<f32>(counter(r, COUNTER_MAX_RISE));
        let reach = tallest / edge;
        passes = u32(min(ceil(reach * reach), f32(MAX_BLUR_PASSES)));
    }
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_BLUR_PASSES], passes);
}

/// Starts the blur from each sheet point's normal.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_blur_init(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices || !sheet_vertex(request_of_work_vertex(slot), slot) {
        return;
    }
    store3(region(REGION_SHELL_BLUR_A) + 3u * slot, load3(point_at(slot) + 3u));
}

/// Averages each direction with its neighbours' and makes it unit length,
/// keeping it where the average is zero. Passes past the request's count
/// copy. Even passes read buffer A and write B; odd passes the reverse.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_blur(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    let odd = shell_pass.number % 2u == 1u;
    let source = region(select(REGION_SHELL_BLUR_A, REGION_SHELL_BLUR_B, odd));
    let target_at = region(select(REGION_SHELL_BLUR_B, REGION_SHELL_BLUR_A, odd)) + 3u * slot;
    let d = load3(source + 3u * slot);
    if shell_pass.number >= counter(r, COUNTER_BLUR_PASSES) {
        store3(target_at, d);
        return;
    }
    var sum = d;
    let span = neighbours(slot);
    for (var i = span.x; i < span.y; i++) {
        let n = arena[region(REGION_SHELL_NEIGHBOURS) + i];
        sum += load3(source + 3u * n);
    }
    store3(target_at, normalize_or(sum, d));
}

// ---------------------------------------------------------------------------
// Heights

/// Returns the distance from `point` along unit `dir` to request `r`'s
/// input, or `F32_MAX` if nothing is within about `limit`, starting just off
/// the surface.
fn clearance(r: u32, point: vec3<f32>, dir: vec3<f32>, limit: f32) -> f32 {
    let epsilon = epsilon_of(r);
    let hit = ray_hit(r, point + dir * epsilon, dir, limit);
    if hit.slot == LEAF {
        return F32_MAX;
    }
    return hit.t + epsilon;
}

/// Sets each sheet point's push direction, close to its normal; its
/// coverage weight; and its height, kept under part of the gap to any
/// surface facing it and part of the input's thickness.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_tops(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    let field = requests[r].field;
    let p = sheet_point(slot);
    let dir = limit_tilt(load3(region(REGION_SHELL_BLUR_A) + 3u * slot), p.normal);
    store3(region(REGION_SHELL_DIRS) + 3u * slot, dir);
    let w = weight(field, p);
    arena[region(REGION_SHELL_WEIGHTS) + slot] = bitcast<u32>(w);

    // Neither limit can lower a top at or below the surface.
    var height = layer_top(field, p, w);
    if height > 0.0 {
        let gap = clearance(r, p.point, dir, height / GAP_FILL);
        let limit = min(GAP_FILL * gap, ROOM_FILL * p.room);
        if height > limit {
            atomicAdd(&counters[r * COUNTER_STRIDE + COUNTER_THINNED], 1u);
            height = limit;
        }
    }
    set_top(slot, height);
}

/// Lowers the heights next to each sheet point so no edge rises faster than
/// the request's slope limit from it. Heights only fall, so the result is
/// the CPU's shortest-path one once a pass lowers nothing.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_slope(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) || counter(r, COUNTER_SLOPE_DONE) != 0u {
        return;
    }
    let max_slope = requests[r].max_slope;
    let height = top_of(slot);
    let p = load3(point_at(slot));
    let changed = r * COUNTER_STRIDE + COUNTER_SLOPE_CHANGED + shell_pass.number % 2u;
    let span = neighbours(slot);
    for (var i = span.x; i < span.y; i++) {
        let n = arena[region(REGION_SHELL_NEIGHBOURS) + i];
        let allowed = order_bits(height + max_slope * length3(p - load3(point_at(n))));
        if atomicMin(&flags[region(FLAG_REGION_SHELL_TOPS) + n], allowed) > allowed {
            atomicAdd(&counters[changed], 1u);
        }
    }
}

/// Ends a slope pass: a request whose pass lowered nothing is done. Clears
/// the next pass's count.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_slope_end(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !shell_open(r) {
        return;
    }
    let c = r * COUNTER_STRIDE;
    let parity = shell_pass.number % 2u;
    if atomicLoad(&counters[c + COUNTER_SLOPE_CHANGED + parity]) == 0u {
        atomicStore(&counters[c + COUNTER_SLOPE_DONE], 1u);
    }
    atomicStore(&counters[c + COUNTER_SLOPE_CHANGED + 1u - parity], 0u);
}

/// Sinks each buried sheet point into the input along a direction that
/// really leads inside it, no more than part of the way through: against
/// its push direction, else against its normal, else not at all. Sets each
/// point's direction to move along.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_sink(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    let dir = load3(region(REGION_SHELL_DIRS) + 3u * slot);
    let height = top_of(slot);
    var along = dir;
    if height < 0.0 {
        let sunk = sink(r, sheet_point(slot), dir, -height);
        along = sunk.dir;
        set_top(slot, -sunk.depth);
    }
    store3(region(REGION_SHELL_ALONG) + 3u * slot, along);
}

/// Where a point sinks into the input: the outward direction to move
/// against, and how deep.
struct Sunk {
    dir: vec3<f32>,
    depth: f32,
}

/// Sinks point `p` of request `r` up to `wanted` into the input, as the
/// CPU's `shell::sink`: against push direction `dir`, else against the
/// normal, taking the first that leads inside, and no more than
/// `GAP_FILL` of the way through. The depth is 0, along `dir`, where
/// neither does.
fn sink(r: u32, p: Point, dir: vec3<f32>, wanted: f32) -> Sunk {
    var tries = array<vec3<f32>, 2>(dir, p.normal);
    for (var i = 0u; i < 2u; i++) {
        let span = inside_span(r, p.point, -tries[i]);
        if span.found {
            return Sunk(tries[i], min(wanted, span.enter + GAP_FILL * (span.leave - span.enter)));
        }
    }
    return Sunk(dir, 0.0);
}

/// Keeps each sheet triangle with a corner that is covered and not sunk
/// well below the surface.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_keep(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !sheet_triangle(r, t) {
        return;
    }
    let hidden = -0.5 * requests[r].field.sink;
    let v = corners_of(t);
    var kept = false;
    for (var k = 0u; k < 3u; k++) {
        let slot = requests[r].out_vertex_base + v[k];
        let w = bitcast<f32>(arena[region(REGION_SHELL_WEIGHTS) + slot]);
        kept = kept || (w > 0.0 && top_of(slot) >= hidden);
    }
    arena[region(REGION_SHELL_TRI_STATE) + t] = select(TRI_HIDDEN, TRI_KEPT, kept);
}

// ---------------------------------------------------------------------------
// Folds

/// Returns whether kept sheet triangle slot `t` of request `r` has turned
/// over relative to the input triangle it came from.
fn is_folded(r: u32, t: u32) -> bool {
    let v = corners_of(t);
    let base = requests[r].out_vertex_base;
    let a = top_position(base + v[0]);
    let b = top_position(base + v[1]);
    let c = top_position(base + v[2]);
    let source = load3(region(REGION_SHEET_TRIS_A) + PATCH_TRI_WORDS * t + 3u);
    return dot3(cross3(b - a, c - a), source) < 0.0;
}

/// Counts the kept triangles that have turned over and flags their corners
/// for lowering; on the last pass, leaves them out instead.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_fold_mark(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !sheet_triangle(r, t) || counter(r, COUNTER_FOLD_DONE) != 0u {
        return;
    }
    let state = region(REGION_SHELL_TRI_STATE) + t;
    if arena[state] != TRI_KEPT || !is_folded(r, t) {
        return;
    }
    let c = r * COUNTER_STRIDE;
    atomicAdd(&counters[c + COUNTER_FOLD_COUNT + shell_pass.number % 2u], 1u);
    if shell_pass.number == MAX_REPAIR_PASSES {
        // Leaving a triangle out moves no vertex, so no other turns over.
        arena[state] = TRI_FOLDED;
        atomicAdd(&counters[c + COUNTER_FOLDED], 1u);
        return;
    }
    let v = corners_of(t);
    for (var k = 0u; k < 3u; k++) {
        atomicStore(&flags[region(FLAG_REGION_SHELL_LOWER) + requests[r].out_vertex_base + v[k]], 1u);
    }
}

/// Ends a fold pass: a request with nothing turned over is done. Clears the
/// next pass's count.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_fold_end(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !shell_open(r) {
        return;
    }
    let c = r * COUNTER_STRIDE;
    let parity = shell_pass.number % 2u;
    if atomicLoad(&counters[c + COUNTER_FOLD_COUNT + parity]) == 0u {
        atomicStore(&counters[c + COUNTER_FOLD_DONE], 1u);
    }
    atomicStore(&counters[c + COUNTER_FOLD_COUNT + 1u - parity], 0u);
}

/// Moves each flagged sheet point halfway down to the surface, once.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_fold_lower(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    if atomicExchange(&flags[region(FLAG_REGION_SHELL_LOWER) + slot], 0u) == 0u || counter(r, COUNTER_FOLD_DONE) != 0u {
        return;
    }
    set_top(slot, lowered(top_of(slot), floor_of(r, slot)));
}

/// Returns the height that lowering sheet point slot `slot` of request `r`
/// heads toward: the surface, or for a closed layer, just thick enough
/// above its underside.
fn floor_of(r: u32, slot: u32) -> f32 {
    if requests[r].closed == 0u {
        return 0.0;
    }
    let depth = bitcast<f32>(arena[region(REGION_CLOSE_DEPTHS) + slot]);
    return max(min_solid(r, sheet_point(slot)) - depth, 0.0);
}

/// Returns the thinnest a closed layer of request `r` may be at point `p`:
/// part of the thickness, shrinking on thin input like the clump itself.
fn min_solid(r: u32, p: Point) -> f32 {
    return min(MIN_SOLID * requests[r].field.thickness, ROOM_FILL * p.room);
}

// ---------------------------------------------------------------------------
// Output

/// Writes each sheet point's top position and fade to the working mesh.
/// The fade comes from the coverage weight alone.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_out_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !sheet_vertex(r, slot) {
        return;
    }
    let field = requests[r].field;
    let w = bitcast<f32>(arena[region(REGION_SHELL_WEIGHTS) + slot]);
    store3(region(REGION_WORK_POSITIONS) + 3u * slot, top_position(slot));
    arena[region(REGION_WORK_FADE) + slot] = bitcast<u32>(fade(field, top(field, w)));
}

/// Writes each kept sheet triangle to the working mesh and marks its
/// corners used. A closed layer's triangles use its points after pinch
/// points are split.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_out_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !sheet_triangle(r, t) || arena[region(REGION_SHELL_TRI_STATE) + t] != TRI_KEPT {
        return;
    }
    var v = corners_of(t);
    if requests[r].closed != 0u {
        for (var k = 0u; k < 3u; k++) {
            v[k] = arena[region(REGION_CLOSE_SPLIT) + 3u * t + k];
        }
    }
    for (var k = 0u; k < 3u; k++) {
        let slot = requests[r].out_vertex_base + v[k];
        arena[region(REGION_WORK_CORNERS) + 3u * t + k] = slot;
        arena[region(REGION_WORK_USED) + slot] = 1u;
    }
    arena[region(REGION_WORK_KEPT) + t] = 1u;
}

/// Sets each used sheet vertex's normal for open layers: the sum of its kept
/// triangles' faces, each weighted by its area, in triangle order, made
/// unit length.
@compute @workgroup_size(WORKGROUP_SIZE)
fn shell_normals(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    let used = region(REGION_WORK_USED);
    if requests[r].closed != 0u || !sheet_vertex(r, slot) || arena[used + slot + 1u] == arena[used + slot] {
        return;
    }

    // The kept triangles around the vertex, lowest slot first.
    let span = around(slot);
    let list = region(REGION_SHEET_AROUND_LIST);
    let state = region(REGION_SHELL_TRI_STATE);
    var sum = vec3<f32>(0.0);
    var lowest = 0u;
    loop {
        var next = NONE;
        for (var i = span.x; i < span.y; i++) {
            let t = arena[list + i];
            if t >= lowest && arena[state + t] == TRI_KEPT {
                next = min(next, t);
            }
        }
        if next == NONE {
            break;
        }
        let corners = region(REGION_WORK_CORNERS) + 3u * next;
        let a = load3(region(REGION_WORK_POSITIONS) + 3u * arena[corners]);
        let b = load3(region(REGION_WORK_POSITIONS) + 3u * arena[corners + 1u]);
        let c = load3(region(REGION_WORK_POSITIONS) + 3u * arena[corners + 2u]);
        sum += cross3(b - a, c - a);
        lowest = next + 1u;
    }
    store3(region(REGION_WORK_NORMALS) + 3u * slot, normalize_or_zero(sum));
}
