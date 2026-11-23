// Simplifies each layer request's working surface toward its target edge
// length, matching the Rust crate's `patch::simplify`: sweeps of edge
// collapses with the same rules, in the same hashed edge order.
//
// A CPU sweep tries edges in priority order and merges each one whose ends
// nothing merged earlier in the sweep has touched. A merge only reads and
// changes its own neighbourhood, so this picks the same merges as rounds
// of: every undecided candidate posts its priority to the points it would
// block; a candidate that is the lowest at both its ends is accepted; a
// candidate with an end an accepted merge touched is rejected.
//
// Pass order, after `patch.wesl`, with V vertex slots, T triangle slots and
// E = 3T: `simplify_init_points` (V), `simplify_init_tris` (T); then each
// sweep (`Step.sweep` = 0, 1, …, `MAX_SWEEPS` - 1):
// `around_count` (T), `around_copy` (V + 1), scan the starts (V + 1),
// `around_fill` (T); on sweep 0 only `edge_uses` (E) and `point_flags` (V);
// `candidates` (E); per round (`Step.round` = 0, 1, …): `mis_reset` (V),
// `mis_priority` (E), `mis_slot` (E), `mis_accept` (E), `mis_reject` (E);
// `apply_merges` (E), `sweep_end` (requests). Then `coarse_point_flags`
// (V + 1), `coarse_tri_flags` (T + 1), scan both, `coarse_points` (V),
// `coarse_tris` (T), `coarse_counts` (requests).

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, store3, counter,
    request_of_vertex, request_of_triangle,
    COUNTER_STRIDE, COUNTER_POINTS, COUNTER_PATCH_TRIANGLES, COUNTER_MERGED,
    COUNTER_SIMPLIFY_DONE, COUNTER_COLLAPSED, COUNTER_ROUNDS_CAPPED, COUNTER_COARSE_POINTS,
    COUNTER_COARSE_TRIANGLES, MODE_EVERYWHERE, REGION_WELDED_NORMALS, REGION_PATCH_POINTS,
    REGION_PATCH_TRIS, REGION_POINT_REMOVED, REGION_POINT_LOCKED, REGION_POINT_BORDER,
    REGION_POINT_CHANGED, REGION_FINE_POINTS, REGION_PATCH_ALIVE, REGION_AROUND_START,
    REGION_AROUND_LIST, REGION_CANDIDATE_STATE, REGION_CANDIDATE_PRIORITY,
    REGION_CANDIDATE_PLACEMENT, REGION_COARSE_POINT_FLAGS, REGION_COARSE_TRI_FLAGS,
    REGION_COARSE_POINTS, REGION_COARSE_TRIS, FLAG_REGION_AROUND_COUNT,
    FLAG_REGION_AROUND_CURSOR, FLAG_REGION_BORDER_EDGES, FLAG_REGION_LOW_PRIORITY,
    FLAG_REGION_LOW_SLOT, POINT_WORDS, PATCH_TRI_WORDS, WORKGROUP_SIZE,
};
import package::bvh::closest_point;
import package::collapse::{collapse_quality, border_stays_near};
import package::hash::pcg3d;
import package::math::{dot3, length3, normalize_or_zero, F32_MAX};
import package::patch::corner_position_id;

/// Length limits of the first sweeps, as fractions of the target edge
/// length; later sweeps use the full length.
const SWEEP_FRACTIONS: array<f32, 4> = array<f32, 4>(0.25, 0.5, 0.75, 1.0);
/// Sweeps at most: the four growing limits, then up to 32 at full length.
const MAX_SWEEPS: u32 = 36u;
/// Farthest a merged midpoint may move to reach the input, as a fraction of
/// the edge it replaces.
const MAX_SNAP: f32 = 0.5;
/// Mixed into each edge's priority hash.
const SWEEP_ORDER_SALT: u32 = 0x51edc0deu;

/// `REGION_CANDIDATE_STATE` values.
const CANDIDATE_NONE: u32 = 0u;
const CANDIDATE_UNDECIDED: u32 = 1u;
const CANDIDATE_ACCEPTED: u32 = 2u;
const CANDIDATE_REJECTED: u32 = 3u;

/// No point.
const NO_POINT: u32 = 0xffffffffu;

/// Which sweep, and which selection round within it, this dispatch is.
struct Step {
    sweep: u32,
    round: u32,
}

@group(1) @binding(0) var<uniform> step: Step;

/// Returns whether request `r` is a layer whose simplification goes on.
fn simplifying(r: u32) -> bool {
    return requests[r].mode == MODE_EVERYWHERE && counter(r, COUNTER_SIMPLIFY_DONE) == 0u;
}

/// Returns point `p` of request `r` as a global point slot.
fn point_slot(r: u32, p: u32) -> u32 {
    return requests[r].vertex_base + p;
}

/// Returns the position, and the normal, of point slot `slot`.
fn point_position(slot: u32) -> vec3<f32> {
    return load3(region(REGION_PATCH_POINTS) + POINT_WORDS * slot);
}
fn point_normal(slot: u32) -> vec3<f32> {
    return load3(region(REGION_PATCH_POINTS) + POINT_WORDS * slot + 3u);
}

/// Returns corner `k` of triangle slot `t`, as a point of its request.
fn corner(t: u32, k: u32) -> u32 {
    return arena[region(REGION_PATCH_TRIS) + PATCH_TRI_WORDS * t + k];
}

/// Returns whether triangle slot `t` has point `p` as a corner.
fn has_corner(t: u32, p: u32) -> bool {
    return corner(t, 0u) == p || corner(t, 1u) == p || corner(t, 2u) == p;
}

/// Returns where point slot `slot`'s list of triangles starts and ends.
fn around(slot: u32) -> vec2<u32> {
    let at = region(REGION_AROUND_START) + slot;
    return vec2<u32>(arena[at], arena[at + 1u]);
}

/// Returns entry `i` of the triangle lists: a triangle slot.
fn around_triangle(i: u32) -> u32 {
    return arena[region(REGION_AROUND_LIST) + i];
}

/// Returns whether triangle slot `slot` is a live working-surface triangle
/// of a request whose simplification goes on.
fn live_triangle(slot: u32) -> bool {
    if slot >= batch.total_triangles {
        return false;
    }
    let r = request_of_triangle(slot);
    return simplifying(r) && arena[region(REGION_PATCH_ALIVE) + slot] != 0u;
}

/// Sets up every working-surface point for simplifying. One item per
/// vertex slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn simplify_init_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    arena[region(REGION_POINT_REMOVED) + slot] = 0u;
    arena[region(REGION_POINT_LOCKED) + slot] = 0u;
    arena[region(REGION_POINT_BORDER) + slot] = 0u;
    arena[region(REGION_POINT_CHANGED) + slot] = 0u;
    atomicStore(&flags[region(FLAG_REGION_AROUND_COUNT) + slot], 0u);
    atomicStore(&flags[region(FLAG_REGION_AROUND_CURSOR) + slot], 0u);
    atomicStore(&flags[region(FLAG_REGION_BORDER_EDGES) + slot], 0u);

    // Keep the surface as it was before simplifying, for the envelope.
    let r = request_of_vertex(slot);
    if requests[r].mode == MODE_EVERYWHERE && slot - requests[r].vertex_base < counter(r, COUNTER_POINTS) {
        store3(region(REGION_FINE_POINTS) + 6u * slot, point_position(slot));
        store3(region(REGION_FINE_POINTS) + 6u * slot + 3u, point_normal(slot));
    }
}

/// Marks every working-surface triangle alive. One item per triangle slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn simplify_init_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_triangles {
        return;
    }
    let r = request_of_triangle(slot);
    let alive = requests[r].mode == MODE_EVERYWHERE
        && slot - requests[r].triangle_base < counter(r, COUNTER_PATCH_TRIANGLES);
    arena[region(REGION_PATCH_ALIVE) + slot] = select(0u, 1u, alive);
}

/// Counts each point's live triangles.
@compute @workgroup_size(WORKGROUP_SIZE)
fn around_count(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if !live_triangle(slot) {
        return;
    }
    let r = request_of_triangle(slot);
    for (var k = 0u; k < 3u; k++) {
        atomicAdd(&flags[region(FLAG_REGION_AROUND_COUNT) + point_slot(r, corner(slot, k))], 1u);
    }
}

/// Copies the counts out for scanning, and clears them and the fill
/// cursors. Dispatch V + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn around_copy(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_vertices {
        return;
    }
    var count = 0u;
    if slot < batch.total_vertices {
        count = atomicExchange(&flags[region(FLAG_REGION_AROUND_COUNT) + slot], 0u);
        atomicStore(&flags[region(FLAG_REGION_AROUND_CURSOR) + slot], 0u);
    }
    arena[region(REGION_AROUND_START) + slot] = count;
}

/// Lists each live triangle under its corners, after the counts are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn around_fill(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if !live_triangle(slot) {
        return;
    }
    let r = request_of_triangle(slot);
    for (var k = 0u; k < 3u; k++) {
        let p = point_slot(r, corner(slot, k));
        let at = arena[region(REGION_AROUND_START) + p] + atomicAdd(&flags[region(FLAG_REGION_AROUND_CURSOR) + p], 1u);
        arena[region(REGION_AROUND_LIST) + at] = slot;
    }
}

/// Returns how many of point slot `slot`'s triangles have point `p`.
fn triangles_with(slot: u32, p: u32) -> u32 {
    let span = around(slot);
    var n = 0u;
    for (var i = span.x; i < span.y; i++) {
        n += select(0u, 1u, has_corner(around_triangle(i), p));
    }
    return n;
}

/// Before the first sweep: locks the ends of every edge with more than two
/// triangles, and counts each point's border edges (edges with one).
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_uses(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    let t = e / 3u;
    if !live_triangle(t) {
        return;
    }
    let r = request_of_triangle(t);
    let a = corner(t, e % 3u);
    let b = corner(t, (e % 3u + 1u) % 3u);
    let uses = triangles_with(point_slot(r, a), b);
    if uses > 2u {
        arena[region(REGION_POINT_LOCKED) + point_slot(r, a)] = 1u;
        arena[region(REGION_POINT_LOCKED) + point_slot(r, b)] = 1u;
    } else if uses == 1u {
        atomicAdd(&flags[region(FLAG_REGION_BORDER_EDGES) + point_slot(r, a)], 1u);
        atomicAdd(&flags[region(FLAG_REGION_BORDER_EDGES) + point_slot(r, b)], 1u);
    }
}

/// Before the first sweep: marks border points, and locks border points
/// where the border doesn't simply pass through.
@compute @workgroup_size(WORKGROUP_SIZE)
fn point_flags(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let edges = atomicLoad(&flags[region(FLAG_REGION_BORDER_EDGES) + slot]);
    if edges != 0u && edges != 2u {
        arena[region(REGION_POINT_LOCKED) + slot] = 1u;
    }
    arena[region(REGION_POINT_BORDER) + slot] = select(0u, 1u, edges > 0u);
}

/// Returns the order in which sweep `pass` tries the edge between points
/// `a` and `b`, like the CPU's `edge_priority`.
fn edge_priority(a: u32, b: u32, sweep_number: u32) -> u32 {
    return pcg3d(vec3<u32>(min(a, b), max(a, b), sweep_number ^ SWEEP_ORDER_SALT)).x;
}

/// A place a merged point may go.
struct Placement {
    point: vec3<f32>,
    normal: vec3<f32>,
}

/// Returns the border neighbour of point `v` other than `other`, or
/// `NO_POINT`: a corner of `v`'s triangles shared by only one of them.
fn outer_neighbour(r: u32, v: u32, other: u32) -> u32 {
    let span = around(point_slot(r, v));
    var found = NO_POINT;
    for (var i = span.x; i < span.y; i++) {
        let t = around_triangle(i);
        for (var k = 0u; k < 3u; k++) {
            let w = corner(t, k);
            if w != v && w != other && triangles_with(point_slot(r, v), w) == 1u {
                found = w;
            }
        }
    }
    return found;
}

/// Returns the worst quality among the triangles a merge of `a` and `b` at
/// `place` changes, or -1 if it isn't allowed.
fn worst_quality(r: u32, a: u32, b: u32, place: Placement, along_border: bool) -> f32 {
    let edge_length = requests[r].edge_length;
    if along_border {
        let na = outer_neighbour(r, a, b);
        let nb = outer_neighbour(r, b, a);
        if na == NO_POINT || nb == NO_POINT {
            return -1.0;
        }
        let near = border_stays_near(
            place.point,
            point_position(point_slot(r, na)),
            point_position(point_slot(r, nb)),
            point_position(point_slot(r, a)),
            point_position(point_slot(r, b)),
            edge_length,
        );
        if !near {
            return -1.0;
        }
    }
    var worst = F32_MAX;
    let ends = vec2<u32>(a, b);
    for (var side = 0u; side < 2u; side++) {
        let span = around(point_slot(r, ends[side]));
        for (var i = span.x; i < span.y; i++) {
            let t = around_triangle(i);
            if has_corner(t, a) && has_corner(t, b) {
                continue;
            }
            var before: array<vec3<f32>, 3>;
            var normals: array<vec3<f32>, 3>;
            var moved = vec3<bool>(false);
            for (var k = 0u; k < 3u; k++) {
                let w = corner(t, k);
                before[k] = point_position(point_slot(r, w));
                normals[k] = point_normal(point_slot(r, w));
                moved[k] = w == a || w == b;
            }
            let q = collapse_quality(before, normals, moved, place.point, place.normal, edge_length);
            if q < 0.0 {
                return -1.0;
            }
            worst = min(worst, q);
        }
    }
    return worst;
}

/// Returns the midpoint of edge `a`–`b` moved onto the input, and whether
/// that is allowed: the input there faces the ends' way and isn't too far.
fn snapped_midpoint(r: u32, a: u32, b: u32, place: ptr<function, Placement>) -> bool {
    let pa = point_position(point_slot(r, a));
    let pb = point_position(point_slot(r, b));
    let mid = (pa + pb) * 0.5;
    let near = closest_point(r, mid);
    var normal = vec3<f32>(0.0);
    for (var k = 0u; k < 3u; k++) {
        let id = corner_position_id(r, near.slot, k);
        normal += load3(region(REGION_WELDED_NORMALS) + 3u * id) * near.barycentric[k];
    }
    normal = normalize_or_zero(normal);
    *place = Placement(near.point, normal);
    let facing = dot3(normal, point_normal(point_slot(r, a)) + point_normal(point_slot(r, b))) > 0.0;
    return facing && length3(near.point - mid) <= MAX_SNAP * length3(pa - pb);
}

/// Plans merging point `b` into point `a` of request `r`: returns whether
/// it is allowed, and where the merged point goes.
fn plan_merge(r: u32, a: u32, b: u32, place: ptr<function, Placement>) -> bool {
    let sa = point_slot(r, a);
    let sb = point_slot(r, b);
    let locked_a = arena[region(REGION_POINT_LOCKED) + sa] != 0u;
    let locked_b = arena[region(REGION_POINT_LOCKED) + sb] != 0u;
    let border_a = arena[region(REGION_POINT_BORDER) + sa] != 0u;
    let border_b = arena[region(REGION_POINT_BORDER) + sb] != 0u;
    if locked_a && locked_b {
        return false;
    }

    // An inner edge has two triangles, a border edge one. An inner edge
    // between two border points would pinch the surface.
    let on_edge = triangles_with(sa, b);
    var along_border = false;
    if on_edge == 1u {
        along_border = true;
    } else if !(on_edge == 2u && !(border_a && border_b)) {
        return false;
    }

    // The ends may share no neighbours beyond the shared triangles' third
    // corners, or the surface tears.
    var shared_neighbours = 0u;
    let span = around(sa);
    for (var i = span.x; i < span.y; i++) {
        let t = around_triangle(i);
        for (var k = 0u; k < 3u; k++) {
            let w = corner(t, k);
            if w == a {
                continue;
            }
            // Count each neighbour once: skip it if an earlier triangle had it.
            var seen = false;
            for (var j = span.x; j < i && !seen; j++) {
                seen = has_corner(around_triangle(j), w);
            }
            if seen {
                continue;
            }
            if w != b && triangles_with(sb, w) > 0u {
                shared_neighbours += 1u;
            }
        }
    }
    if shared_neighbours != on_edge {
        return false;
    }

    // Where the merged point may go: a fixed end stays put, a border point
    // stays on the border, and otherwise the midpoint is tried too. Keep
    // the place with the best worst triangle; among equals, the last.
    let fixed_a = locked_a || (!along_border && border_a);
    let fixed_b = locked_b || (!along_border && border_b);
    if fixed_a && fixed_b {
        return false;
    }
    let end_a = Placement(point_position(sa), point_normal(sa));
    let end_b = Placement(point_position(sb), point_normal(sb));

    // A fixed end is the only place; otherwise try both ends, then (away
    // from the border) the midpoint.
    var options: array<Placement, 3>;
    var count = 0u;
    if fixed_a {
        options[0] = end_a;
        count = 1u;
    } else if fixed_b {
        options[0] = end_b;
        count = 1u;
    } else {
        options[0] = end_a;
        options[1] = end_b;
        count = 2u;
        var mid: Placement;
        if !along_border && snapped_midpoint(r, a, b, &mid) {
            options[2] = mid;
            count = 3u;
        }
    }
    var best = -1.0;
    for (var i = 0u; i < count; i++) {
        let q = worst_quality(r, a, b, options[i], along_border);
        if q >= 0.0 && q >= best {
            best = q;
            *place = options[i];
        }
    }
    return best >= 0.0;
}

/// Returns the ends of triangle edge slot `e`, as points of its request.
fn edge_ends(e: u32) -> vec2<u32> {
    let t = e / 3u;
    let k = e % 3u;
    return vec2<u32>(corner(t, k), corner(t, (k + 1u) % 3u));
}

/// Returns edge slot `e`'s number within its request, as the CPU numbers
/// triangle edge slots.
fn local_edge(r: u32, e: u32) -> u32 {
    return e - 3u * requests[r].triangle_base;
}

/// Plans every edge this sweep may try: the edge's chosen side, ends that
/// haven't changed, shorter than the sweep's limit, and allowed to merge.
@compute @workgroup_size(WORKGROUP_SIZE)
fn candidates(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    if e >= 3u * batch.total_triangles {
        return;
    }
    arena[region(REGION_CANDIDATE_STATE) + e] = CANDIDATE_NONE;
    if !live_triangle(e / 3u) {
        return;
    }
    let r = request_of_triangle(e / 3u);
    let ends = edge_ends(e);
    let a = ends.x;
    let b = ends.y;
    let border_a = arena[region(REGION_POINT_BORDER) + point_slot(r, a)] != 0u;
    let border_b = arena[region(REGION_POINT_BORDER) + point_slot(r, b)] != 0u;
    if a > b && !(border_a && border_b) {
        return;
    }
    let sweep = step.sweep;
    let limit = SWEEP_FRACTIONS[min(sweep, 3u)] * requests[r].edge_length;
    if length3(point_position(point_slot(r, a)) - point_position(point_slot(r, b))) >= limit {
        return;
    }
    var place: Placement;
    if !plan_merge(r, a, b, &place) {
        return;
    }
    arena[region(REGION_CANDIDATE_STATE) + e] = CANDIDATE_UNDECIDED;
    arena[region(REGION_CANDIDATE_PRIORITY) + e] = edge_priority(a, b, sweep + 1u);
    store3(region(REGION_CANDIDATE_PLACEMENT) + 6u * e, place.point);
    store3(region(REGION_CANDIDATE_PLACEMENT) + 6u * e + 3u, place.normal);
}

/// What `visit_marks` does at each point.
const POST_PRIORITY: u32 = 0u;
const POST_SLOT: u32 = 1u;
const MARK_CHANGED: u32 = 2u;

/// Visits every point candidate `e` (merging `b` into `a`) blocks: the
/// corners of the triangles around `a` afterwards, and `a` and `b`. At
/// each it posts the candidate's priority or slot, or marks it changed.
fn visit_marks(r: u32, e: u32, a: u32, b: u32, what: u32) {
    let priority = arena[region(REGION_CANDIDATE_PRIORITY) + e];
    let slot = local_edge(r, e);
    let sweep_number = step.sweep + 1u;
    let ends = vec2<u32>(a, b);
    for (var side = 0u; side < 4u; side++) {
        // Sides 0 and 1 walk the ends' triangles; 2 and 3 are the ends.
        var first = 0u;
        var last = 1u;
        if side < 2u {
            let span = around(point_slot(r, ends[side]));
            first = span.x;
            last = span.y;
        }
        for (var i = first; i < last; i++) {
            var points = vec3<u32>(NO_POINT);
            if side < 2u {
                let t = around_triangle(i);
                if has_corner(t, a) && has_corner(t, b) {
                    continue;
                }
                points = vec3<u32>(corner(t, 0u), corner(t, 1u), corner(t, 2u));
            } else {
                points.x = ends[side - 2u];
            }
            for (var k = 0u; k < 3u; k++) {
                if points[k] == NO_POINT {
                    continue;
                }
                let w = point_slot(r, select(points[k], a, points[k] == b));
                if what == POST_PRIORITY {
                    atomicMin(&flags[region(FLAG_REGION_LOW_PRIORITY) + w], priority);
                } else if what == POST_SLOT {
                    if atomicLoad(&flags[region(FLAG_REGION_LOW_PRIORITY) + w]) == priority {
                        atomicMin(&flags[region(FLAG_REGION_LOW_SLOT) + w], slot);
                    }
                } else {
                    arena[region(REGION_POINT_CHANGED) + w] = sweep_number;
                }
            }
        }
    }
}

/// Returns candidate `e`'s request, and whether it is still undecided.
fn undecided(e: u32, r: ptr<function, u32>) -> bool {
    if e >= 3u * batch.total_triangles || arena[region(REGION_CANDIDATE_STATE) + e] != CANDIDATE_UNDECIDED {
        return false;
    }
    *r = request_of_triangle(e / 3u);
    return true;
}

/// Starts a selection round: clears every point's lowest posted priority.
@compute @workgroup_size(WORKGROUP_SIZE)
fn mis_reset(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot < batch.total_vertices {
        atomicStore(&flags[region(FLAG_REGION_LOW_PRIORITY) + slot], 0xffffffffu);
        atomicStore(&flags[region(FLAG_REGION_LOW_SLOT) + slot], 0xffffffffu);
    }
}

/// Posts each undecided candidate's priority at the points it blocks.
@compute @workgroup_size(WORKGROUP_SIZE)
fn mis_priority(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    var r = 0u;
    if undecided(e, &r) {
        let ends = edge_ends(e);
        visit_marks(r, e, ends.x, ends.y, POST_PRIORITY);
    }
}

/// Among candidates tied at a point's lowest priority, posts the lowest slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn mis_slot(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    var r = 0u;
    if undecided(e, &r) {
        let ends = edge_ends(e);
        visit_marks(r, e, ends.x, ends.y, POST_SLOT);
    }
}

/// Returns whether nothing earlier than candidate `e` blocks point `w`.
fn first_at(r: u32, e: u32, w: u32) -> bool {
    let slot = point_slot(r, w);
    let low = atomicLoad(&flags[region(FLAG_REGION_LOW_PRIORITY) + slot]);
    let priority = arena[region(REGION_CANDIDATE_PRIORITY) + e];
    if low != priority {
        return low > priority;
    }
    return atomicLoad(&flags[region(FLAG_REGION_LOW_SLOT) + slot]) >= local_edge(r, e);
}

/// Accepts each undecided candidate that nothing earlier blocks at either
/// end, and marks the points it changes.
@compute @workgroup_size(WORKGROUP_SIZE)
fn mis_accept(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    var r = 0u;
    if !undecided(e, &r) {
        return;
    }
    let ends = edge_ends(e);
    if first_at(r, e, ends.x) && first_at(r, e, ends.y) {
        arena[region(REGION_CANDIDATE_STATE) + e] = CANDIDATE_ACCEPTED;
        visit_marks(r, e, ends.x, ends.y, MARK_CHANGED);
    }
}

/// Rejects each undecided candidate with an end an accepted merge changed.
@compute @workgroup_size(WORKGROUP_SIZE)
fn mis_reject(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    var r = 0u;
    if !undecided(e, &r) {
        return;
    }
    let ends = edge_ends(e);
    let sweep_number = step.sweep + 1u;
    let changed = region(REGION_POINT_CHANGED);
    if arena[changed + point_slot(r, ends.x)] == sweep_number
        || arena[changed + point_slot(r, ends.y)] == sweep_number {
        arena[region(REGION_CANDIDATE_STATE) + e] = CANDIDATE_REJECTED;
    }
}

/// Merges each accepted candidate's `b` into its `a`: moves `a`, removes
/// the triangles on the edge, hands `b`'s other triangles to `a`, and
/// records the new triangles' normals.
@compute @workgroup_size(WORKGROUP_SIZE)
fn apply_merges(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    if e >= 3u * batch.total_triangles {
        return;
    }
    let state = arena[region(REGION_CANDIDATE_STATE) + e];
    let r = request_of_triangle(e / 3u);
    if state == CANDIDATE_UNDECIDED {
        // The rounds ran out before this candidate was decided.
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_ROUNDS_CAPPED], 1u);
        return;
    }
    if state != CANDIDATE_ACCEPTED {
        return;
    }
    let ends = edge_ends(e);
    let a = ends.x;
    let b = ends.y;
    let sa = point_slot(r, a);
    let sb = point_slot(r, b);

    // Move a, and give it b's flags.
    let at = region(REGION_PATCH_POINTS) + POINT_WORDS * sa;
    store3(at, load3(region(REGION_CANDIDATE_PLACEMENT) + 6u * e));
    store3(at + 3u, load3(region(REGION_CANDIDATE_PLACEMENT) + 6u * e + 3u));
    let locked = region(REGION_POINT_LOCKED);
    let border = region(REGION_POINT_BORDER);
    arena[locked + sa] = arena[locked + sa] | arena[locked + sb];
    arena[border + sa] = arena[border + sa] | arena[border + sb];
    arena[region(REGION_POINT_REMOVED) + sb] = 1u;

    // Drop the shared triangles; point b's others at a, and record every
    // remaining triangle's new face.
    let ends_slots = vec2<u32>(sa, sb);
    for (var side = 0u; side < 2u; side++) {
        let span = around(ends_slots[side]);
        for (var i = span.x; i < span.y; i++) {
            let t = around_triangle(i);
            if has_corner(t, a) && has_corner(t, b) {
                arena[region(REGION_PATCH_ALIVE) + t] = 0u;
                continue;
            }
            let tri = region(REGION_PATCH_TRIS) + PATCH_TRI_WORDS * t;
            for (var k = 0u; k < 3u; k++) {
                if arena[tri + k] == b {
                    arena[tri + k] = a;
                }
            }
            let p0 = point_position(point_slot(r, arena[tri]));
            let p1 = point_position(point_slot(r, arena[tri + 1u]));
            let p2 = point_position(point_slot(r, arena[tri + 2u]));
            store3(tri + 3u, cross_product(p1 - p0, p2 - p0));
        }
    }
    atomicAdd(&counters[r * COUNTER_STRIDE + COUNTER_MERGED], 1u);
}

/// The cross product, like glam's.
fn cross_product(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(a.y * b.z - b.y * a.z, a.z * b.x - b.z * a.x, a.x * b.y - b.x * a.y);
}

/// Ends a sweep: a request stops once a sweep at the full length merges
/// nothing. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sweep_end(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !simplifying(r) {
        return;
    }
    let merged = atomicExchange(&counters[r * COUNTER_STRIDE + COUNTER_MERGED], 0u);
    if merged > 0u {
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_COLLAPSED], 1u);
    } else if step.sweep >= 3u {
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_SIMPLIFY_DONE], 1u);
    }
    if step.sweep + 1u >= MAX_SWEEPS {
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_SIMPLIFY_DONE], 1u);
    }
}

/// Flags each point that survives simplifying. Dispatch V + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn coarse_point_flags(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_vertices {
        return;
    }
    var keep = false;
    if slot < batch.total_vertices {
        let r = request_of_vertex(slot);
        keep = requests[r].mode == MODE_EVERYWHERE
            && slot - requests[r].vertex_base < counter(r, COUNTER_POINTS)
            && arena[region(REGION_POINT_REMOVED) + slot] == 0u;
    }
    arena[region(REGION_COARSE_POINT_FLAGS) + slot] = select(0u, 1u, keep);
}

/// Flags each triangle that survives simplifying. Dispatch T + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn coarse_tri_flags(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_triangles {
        return;
    }
    var keep = 0u;
    if slot < batch.total_triangles {
        keep = arena[region(REGION_PATCH_ALIVE) + slot];
    }
    arena[region(REGION_COARSE_TRI_FLAGS) + slot] = keep;
}

/// Returns point slot `slot`'s number after compaction, within its request.
fn coarse_point(r: u32, slot: u32) -> u32 {
    let flags_at = region(REGION_COARSE_POINT_FLAGS);
    return arena[flags_at + slot] - arena[flags_at + requests[r].vertex_base];
}

/// Copies each surviving point to its compacted place.
@compute @workgroup_size(WORKGROUP_SIZE)
fn coarse_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let flags_at = region(REGION_COARSE_POINT_FLAGS);
    if slot >= batch.total_vertices || arena[flags_at + slot + 1u] == arena[flags_at + slot] {
        return;
    }
    let r = request_of_vertex(slot);
    let source = region(REGION_PATCH_POINTS) + POINT_WORDS * slot;
    let to = region(REGION_COARSE_POINTS) + POINT_WORDS * point_slot(r, coarse_point(r, slot));
    for (var k = 0u; k < POINT_WORDS; k++) {
        arena[to + k] = arena[source + k];
    }
}

/// Copies each surviving triangle to its compacted place, with its corners
/// renumbered.
@compute @workgroup_size(WORKGROUP_SIZE)
fn coarse_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let flags_at = region(REGION_COARSE_TRI_FLAGS);
    if slot >= batch.total_triangles || arena[flags_at + slot + 1u] == arena[flags_at + slot] {
        return;
    }
    let r = request_of_triangle(slot);
    let base = requests[r].triangle_base;
    let tri = base + arena[flags_at + slot] - arena[flags_at + base];
    let source = region(REGION_PATCH_TRIS) + PATCH_TRI_WORDS * slot;
    let to = region(REGION_COARSE_TRIS) + PATCH_TRI_WORDS * tri;
    for (var k = 0u; k < 3u; k++) {
        arena[to + k] = coarse_point(r, point_slot(r, arena[source + k]));
    }
    for (var k = 3u; k < 6u; k++) {
        arena[to + k] = arena[source + k];
    }
}

/// Records each layer request's simplified point and triangle counts.
@compute @workgroup_size(WORKGROUP_SIZE)
fn coarse_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_EVERYWHERE {
        return;
    }
    let request = requests[r];
    let points = region(REGION_COARSE_POINT_FLAGS);
    let tris = region(REGION_COARSE_TRI_FLAGS);
    let point_end = request.vertex_base + request.vertex_capacity;
    let tri_end = request.triangle_base + request.triangle_capacity;
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_COARSE_POINTS],
        arena[points + point_end] - arena[points + request.vertex_base]);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_COARSE_TRIANGLES],
        arena[tris + tri_end] - arena[tris + request.triangle_base]);
}
