// Builds each layer request's sheet: the covered part of its working
// surface, renumbered into its output-sized slots, then refined so no edge a
// clump may cover is longer than the request's edge length. Matches the Rust
// crate's `shell::refine`, point for point and triangle for triangle, up to
// renumbering: the sheet keeps only points a covered triangle uses, in their
// order.
//
// Setup, with V vertex slots, T triangle slots, and W, Wt output vertex and
// triangle slots: `sheet_clear` (W), `sheet_flag_clear` (V + 1),
// `sheet_flag_points` (T), scan the point flags (V + 1), `sheet_points` (V),
// `sheet_tris` (T), `sheet_counts` (requests).
//
// Then `REFINE_PASSES` passes, numbered from 0 in `refine_pass`. Even passes
// read triangles from buffer A and write B; odd passes the reverse, so A
// holds the result. A pass that splits nothing copies the triangles
// unchanged, as the CPU's refinement would if it went on. Each pass:
// `refine_marks` (Wt + 1), `sheet_around_copy` (W + 1), scan the around
// starts (W + 1), `sheet_around_fill` (Wt), `refine_owner` (3 Wt + 1), scan
// the creates flags (3 Wt + 1), scan the child counts (Wt + 1),
// `refine_points` (3 Wt), `refine_tris` (Wt), `refine_counts` (requests).
//
// A request whose sheet outgrows its output capacity is marked failed and
// skipped from then on; its sheet counts then give the size it needed at
// that pass, which is a lower bound on what the finished sheet needs.

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, store3, counter,
    request_of_vertex, request_of_triangle, request_of_work_vertex, request_of_work_triangle,
    COUNTER_STRIDE, COUNTER_COARSE_POINTS, COUNTER_LAYER_TRIANGLES, COUNTER_SHEET_POINTS,
    COUNTER_SHEET_TRIANGLES, COUNTER_SHEET_FAILED, COUNTER_WORK_VERTICES,
    COUNTER_WORK_TRIANGLES, COUNTER_OVERFLOW, OVERFLOW_VERTICES,
    OVERFLOW_INDICES, MODE_EVERYWHERE, REGION_COARSE_POINTS, REGION_LAYER_TRIS,
    REGION_SHEET_POINT_FLAGS, REGION_SHEET_POINTS, REGION_SHEET_TRIS_A, REGION_SHEET_TRIS_B,
    REGION_SHEET_AROUND_START, REGION_SHEET_AROUND_LIST, REGION_REFINE_MARKS,
    REGION_REFINE_CHILDREN, REGION_REFINE_CREATES, REGION_REFINE_OWNER,
    FLAG_REGION_SHEET_AROUND_COUNT, FLAG_REGION_SHEET_AROUND_CURSOR, POINT_WORDS,
    PATCH_TRI_WORDS, WORKGROUP_SIZE,
};
import package::field::{Point, mask, MAX_DIRECTIONS};
import package::math::{length_squared, normalize_or, F32_MAX};

/// The number of the refine pass being run, from 0.
struct RefinePass {
    number: u32,
}

@group(1) @binding(0) var<uniform> refine_pass: RefinePass;

/// Refine passes, as the CPU's `MAX_PASSES`.
const REFINE_PASSES: u32 = 16u;

/// No call slot.
const NONE: u32 = 0xffffffffu;

/// Returns the first output triangle slot of request `r`.
fn tri_base(r: u32) -> u32 {
    return requests[r].out_index_base / 3u;
}

/// Returns whether request `r` grows a sheet that is still being refined.
fn sheet_open(r: u32) -> bool {
    return requests[r].mode == MODE_EVERYWHERE && counter(r, COUNTER_SHEET_FAILED) == 0u;
}

/// Returns the region the current pass reads triangles from, or with
/// `write`, the one it writes them to.
fn tris_region(write: bool) -> u32 {
    let odd = refine_pass.number % 2u == 1u;
    return region(select(REGION_SHEET_TRIS_A, REGION_SHEET_TRIS_B, odd != write));
}

/// Returns whether output triangle slot `t` holds a live sheet triangle of
/// its request `r` in the current pass.
fn live(r: u32, t: u32) -> bool {
    return sheet_open(r) && t - tri_base(r) < counter(r, COUNTER_SHEET_TRIANGLES);
}

/// Returns the corners of sheet triangle slot `t` read this pass, as point
/// numbers of its request.
fn corners_of(t: u32) -> array<u32, 3> {
    let at = tris_region(false) + PATCH_TRI_WORDS * t;
    return array<u32, 3>(arena[at], arena[at + 1u], arena[at + 2u]);
}

/// Returns the word offset of request `r`'s sheet point `p`.
fn point_at(r: u32, p: u32) -> u32 {
    return region(REGION_SHEET_POINTS) + POINT_WORDS * (requests[r].out_vertex_base + p);
}

/// Returns request `r`'s sheet point `p`.
fn sheet_point(r: u32, p: u32) -> Point {
    let at = point_at(r, p);
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

/// Stores `p` as request `r`'s sheet point `index`.
fn store_point(r: u32, index: u32, p: Point) {
    let at = point_at(r, index);
    store3(at, p.point);
    store3(at + 3u, p.normal);
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        arena[at + 6u + d] = bitcast<u32>(p.exposure[d]);
    }
    arena[at + 14u] = bitcast<u32>(p.room);
    arena[at + 15u] = bitcast<u32>(p.rise);
    arena[at + 16u] = bitcast<u32>(p.dip);
    arena[at + 17u] = 0u;
}

// ---------------------------------------------------------------------------
// Setup

/// Clears every sheet point's triangle count and fill cursor.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_clear(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot < batch.total_work_vertices {
        atomicStore(&flags[region(FLAG_REGION_SHEET_AROUND_COUNT) + slot], 0u);
        atomicStore(&flags[region(FLAG_REGION_SHEET_AROUND_CURSOR) + slot], 0u);
    }
}

/// Clears the point flags. Dispatch V + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_flag_clear(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot <= batch.total_vertices {
        arena[region(REGION_SHEET_POINT_FLAGS) + slot] = 0u;
    }
}

/// Flags each simplified point a covered triangle uses.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_flag_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_triangles {
        return;
    }
    let r = request_of_triangle(slot);
    let request = requests[r];
    if request.mode != MODE_EVERYWHERE || slot - request.triangle_base >= counter(r, COUNTER_LAYER_TRIANGLES) {
        return;
    }
    let tri = region(REGION_LAYER_TRIS) + PATCH_TRI_WORDS * slot;
    for (var k = 0u; k < 3u; k++) {
        arena[region(REGION_SHEET_POINT_FLAGS) + request.vertex_base + arena[tri + k]] = 1u;
    }
}

/// Returns the sheet point number of simplified point slot `slot`, after
/// the point flags are scanned.
fn sheet_number(r: u32, slot: u32) -> u32 {
    let flags_at = region(REGION_SHEET_POINT_FLAGS);
    return arena[flags_at + slot] - arena[flags_at + requests[r].vertex_base];
}

/// Copies each flagged simplified point to the sheet.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let flags_at = region(REGION_SHEET_POINT_FLAGS);
    if slot >= batch.total_vertices || arena[flags_at + slot + 1u] == arena[flags_at + slot] {
        return;
    }
    let r = request_of_vertex(slot);
    let p = sheet_number(r, slot);
    if p >= requests[r].out_vertex_capacity {
        return;
    }
    let source = region(REGION_COARSE_POINTS) + POINT_WORDS * slot;
    let to = point_at(r, p);
    for (var k = 0u; k < POINT_WORDS; k++) {
        arena[to + k] = arena[source + k];
    }
}

/// Copies each covered triangle to the sheet's buffer A, renumbering its
/// corners.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_triangles {
        return;
    }
    let r = request_of_triangle(slot);
    let request = requests[r];
    let i = slot - request.triangle_base;
    if request.mode != MODE_EVERYWHERE || i >= counter(r, COUNTER_LAYER_TRIANGLES) || i >= request.out_index_capacity / 3u {
        return;
    }
    let source = region(REGION_LAYER_TRIS) + PATCH_TRI_WORDS * slot;
    let to = region(REGION_SHEET_TRIS_A) + PATCH_TRI_WORDS * (tri_base(r) + i);
    for (var k = 0u; k < 3u; k++) {
        arena[to + k] = sheet_number(r, request.vertex_base + arena[source + k]);
    }
    for (var k = 3u; k < PATCH_TRI_WORDS; k++) {
        arena[to + k] = arena[source + k];
    }
}

/// Records each layer request's sheet size, and fails a request whose
/// sheet doesn't fit its output capacity.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_EVERYWHERE {
        return;
    }
    let request = requests[r];
    let flags_at = region(REGION_SHEET_POINT_FLAGS);
    let points = arena[flags_at + request.vertex_base + request.vertex_capacity] - arena[flags_at + request.vertex_base];
    let triangles = counter(r, COUNTER_LAYER_TRIANGLES);
    record_size(r, points, triangles);
}

/// Sets request `r`'s sheet size, which is also the working mesh size it
/// needs, failing it with the matching overflow bits if that doesn't fit
/// its output capacity.
fn record_size(r: u32, points: u32, triangles: u32) {
    let c = r * COUNTER_STRIDE;
    atomicStore(&counters[c + COUNTER_SHEET_POINTS], points);
    atomicStore(&counters[c + COUNTER_SHEET_TRIANGLES], triangles);
    atomicStore(&counters[c + COUNTER_WORK_VERTICES], points);
    atomicStore(&counters[c + COUNTER_WORK_TRIANGLES], triangles);
    var overflow = 0u;
    if points > requests[r].out_vertex_capacity {
        overflow |= OVERFLOW_VERTICES;
    }
    if triangles > requests[r].out_index_capacity / 3u {
        overflow |= OVERFLOW_INDICES;
    }
    if overflow != 0u {
        atomicOr(&counters[c + COUNTER_OVERFLOW], overflow);
        atomicStore(&counters[c + COUNTER_SHEET_FAILED], 1u);
    }
}

// ---------------------------------------------------------------------------
// Refine passes

/// Returns the edge of a triangle with split-edge bits `marks` whose
/// midpoint the CPU makes `j`-th, or 3 if it makes fewer.
fn call_edge(marks: u32, j: u32) -> u32 {
    let count = countOneBits(marks);
    if j >= count {
        return 3u;
    }
    if count == 1u {
        return firstTrailingBit(marks);
    }
    if count == 2u {
        // From the edge after the one left whole.
        let whole = firstTrailingBit(~marks & 7u);
        return (whole + 1u + j) % 3u;
    }
    return j;
}

/// Returns when, among a triangle's midpoints, the CPU makes the midpoint
/// of split edge `k`, given its split-edge bits `marks`.
fn call_rank(marks: u32, k: u32) -> u32 {
    let count = countOneBits(marks);
    if count == 1u {
        return 0u;
    }
    if count == 2u {
        let whole = firstTrailingBit(~marks & 7u);
        return (k + 2u - whole) % 3u;
    }
    return k;
}

/// Flags the edges of each live triangle a clump may cover that are longer
/// than the edge length, counts the triangles it becomes, and counts it
/// around its corners. Dispatch Wt + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn refine_marks(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t > batch.total_work_triangles {
        return;
    }
    var children = 0u;
    if t < batch.total_work_triangles {
        let r = request_of_work_triangle(t);
        var marks = 0u;
        if live(r, t) {
            let v = corners_of(t);
            let field = requests[r].field;
            var points: array<Point, 3>;
            var covered = false;
            for (var k = 0u; k < 3u; k++) {
                points[k] = sheet_point(r, v[k]);
                covered = covered || mask(field, points[k]) > 0.0;
            }
            let limit = requests[r].edge_length * requests[r].edge_length;
            for (var k = 0u; k < 3u; k++) {
                let gap = points[k].point - points[(k + 1u) % 3u].point;
                if covered && length_squared(gap) > limit {
                    marks |= 1u << k;
                }
                let at = requests[r].out_vertex_base + v[k];
                atomicAdd(&flags[region(FLAG_REGION_SHEET_AROUND_COUNT) + at], 1u);
            }
            children = 1u + countOneBits(marks);
        }
        arena[region(REGION_REFINE_MARKS) + t] = marks;
    }
    arena[region(REGION_REFINE_CHILDREN) + t] = children;
}

/// Copies the triangle counts around each sheet point out for scanning, and
/// clears them and the fill cursors. Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_around_copy(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_work_vertices {
        return;
    }
    var count = 0u;
    if slot < batch.total_work_vertices {
        count = atomicExchange(&flags[region(FLAG_REGION_SHEET_AROUND_COUNT) + slot], 0u);
        atomicStore(&flags[region(FLAG_REGION_SHEET_AROUND_CURSOR) + slot], 0u);
    }
    arena[region(REGION_SHEET_AROUND_START) + slot] = count;
}

/// Lists each live triangle under its corners, after the counts are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sheet_around_fill(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !live(r, t) {
        return;
    }
    let v = corners_of(t);
    for (var k = 0u; k < 3u; k++) {
        let p = requests[r].out_vertex_base + v[k];
        let at = arena[region(REGION_SHEET_AROUND_START) + p] + atomicAdd(&flags[region(FLAG_REGION_SHEET_AROUND_CURSOR) + p], 1u);
        arena[region(REGION_SHEET_AROUND_LIST) + at] = t;
    }
}

/// For each call slot that splits an edge, finds the call slot that makes
/// the edge's midpoint: the first, in triangle and call order, of the live
/// triangles that split the same edge. Flags the call slots that make one.
/// Dispatch 3 Wt + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn refine_owner(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s > 3u * batch.total_work_triangles {
        return;
    }
    var creates = 0u;
    if s < 3u * batch.total_work_triangles {
        let t = s / 3u;
        let r = request_of_work_triangle(t);
        let marks = arena[region(REGION_REFINE_MARKS) + t];
        let k = call_edge(marks, s % 3u);
        if live(r, t) && k < 3u {
            let v = corners_of(t);
            let a = v[k];
            let b = v[(k + 1u) % 3u];

            // Every live triangle around `a` that splits edge {a, b}.
            let start = region(REGION_SHEET_AROUND_START) + requests[r].out_vertex_base + a;
            var owner = NONE;
            for (var i = arena[start]; i < arena[start + 1u]; i++) {
                let other = arena[region(REGION_SHEET_AROUND_LIST) + i];
                let other_marks = arena[region(REGION_REFINE_MARKS) + other];
                let w = corners_of(other);
                for (var e = 0u; e < 3u; e++) {
                    let x = w[e];
                    let y = w[(e + 1u) % 3u];
                    let same = (x == a && y == b) || (x == b && y == a);
                    if same && (other_marks & (1u << e)) != 0u {
                        owner = min(owner, 3u * other + call_rank(other_marks, e));
                    }
                }
            }
            arena[region(REGION_REFINE_OWNER) + 3u * t + k] = owner;
            creates = select(0u, 1u, owner == s);
        }
    }
    arena[region(REGION_REFINE_CREATES) + s] = creates;
}

/// Returns the sheet point number of the midpoint of split edge `k` of live
/// triangle slot `t` of request `r`, after the creates flags are scanned.
fn midpoint_number(r: u32, t: u32, k: u32) -> u32 {
    let owner = arena[region(REGION_REFINE_OWNER) + 3u * t + k];
    let creates = region(REGION_REFINE_CREATES);
    return counter(r, COUNTER_SHEET_POINTS) + arena[creates + owner] - arena[creates + 3u * tri_base(r)];
}

/// Returns the point halfway between `a` and `b`, with every value blended,
/// like the CPU's `ClumpField::midpoint`. Unlimited room stays unlimited.
fn midpoint(a: Point, b: Point) -> Point {
    var exposure: array<f32, MAX_DIRECTIONS>;
    var ea = a.exposure;
    var eb = b.exposure;
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        exposure[d] = (ea[d] + eb[d]) * 0.5;
    }
    let unlimited = a.room >= F32_MAX || b.room >= F32_MAX;
    return Point(
        (a.point + b.point) * 0.5,
        normalize_or(a.normal + b.normal, a.normal),
        exposure,
        select((a.room + b.room) * 0.5, F32_MAX, unlimited),
        (a.rise + b.rise) * 0.5,
        (a.dip + b.dip) * 0.5,
    );
}

/// Makes each new midpoint, at the end of its request's points.
@compute @workgroup_size(WORKGROUP_SIZE)
fn refine_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    let creates = region(REGION_REFINE_CREATES);
    if s >= 3u * batch.total_work_triangles || arena[creates + s + 1u] == arena[creates + s] {
        return;
    }
    let t = s / 3u;
    let r = request_of_work_triangle(t);
    let k = call_edge(arena[region(REGION_REFINE_MARKS) + t], s % 3u);
    let v = corners_of(t);
    let index = midpoint_number(r, t, k);
    if index < requests[r].out_vertex_capacity {
        store_point(r, index, midpoint(sheet_point(r, v[k]), sheet_point(r, v[(k + 1u) % 3u])));
    }
}

/// Writes each live triangle's children, in order, to the other buffer:
/// itself if no edge splits, or the CPU's split by how many edges do.
@compute @workgroup_size(WORKGROUP_SIZE)
fn refine_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !live(r, t) {
        return;
    }
    let base = tri_base(r);
    let children = region(REGION_REFINE_CHILDREN);
    let first = base + arena[children + t] - arena[children + base];
    let capacity = base + requests[r].out_index_capacity / 3u;
    let source = tris_region(false) + PATCH_TRI_WORDS * t;
    let normal = vec3<u32>(arena[source + 3u], arena[source + 4u], arena[source + 5u]);
    let v = corners_of(t);
    let marks = arena[region(REGION_REFINE_MARKS) + t];

    // The children's corners, by how many edges split.
    var out: array<array<u32, 3>, 4>;
    let count = countOneBits(marks);
    if count == 0u {
        out[0] = v;
    } else if count == 1u {
        // Rotate so the long edge is a→b, then halve it.
        let k = firstTrailingBit(marks);
        let a = v[k];
        let b = v[(k + 1u) % 3u];
        let c = v[(k + 2u) % 3u];
        let ab = midpoint_number(r, t, k);
        out[0] = array<u32, 3>(a, ab, c);
        out[1] = array<u32, 3>(ab, b, c);
    } else if count == 2u {
        // Rotate so the short edge is c→a, halve a→b and b→c, and cut the
        // remaining quad along its shorter diagonal.
        let whole = firstTrailingBit(~marks & 7u);
        let a = v[(whole + 1u) % 3u];
        let b = v[(whole + 2u) % 3u];
        let c = v[whole];
        let ab = midpoint_number(r, t, (whole + 1u) % 3u);
        let bc = midpoint_number(r, t, (whole + 2u) % 3u);
        out[0] = array<u32, 3>(ab, b, bc);
        let pa = load3(point_at(r, a));
        let pb = load3(point_at(r, b));
        let pc = load3(point_at(r, c));
        let pab = (pa + pb) * 0.5;
        let pbc = (pb + pc) * 0.5;
        if length_squared(pa - pbc) <= length_squared(pab - pc) {
            out[1] = array<u32, 3>(a, ab, bc);
            out[2] = array<u32, 3>(a, bc, c);
        } else {
            out[1] = array<u32, 3>(a, ab, c);
            out[2] = array<u32, 3>(ab, bc, c);
        }
    } else {
        let ab = midpoint_number(r, t, 0u);
        let bc = midpoint_number(r, t, 1u);
        let ca = midpoint_number(r, t, 2u);
        out[0] = array<u32, 3>(v[0], ab, ca);
        out[1] = array<u32, 3>(ab, v[1], bc);
        out[2] = array<u32, 3>(ca, bc, v[2]);
        out[3] = array<u32, 3>(ab, bc, ca);
    }

    // Each keeps the input triangle's face normal.
    for (var i = 0u; i < 1u + count; i++) {
        if first + i >= capacity {
            return;
        }
        let to = tris_region(true) + PATCH_TRI_WORDS * (first + i);
        for (var k = 0u; k < 3u; k++) {
            arena[to + k] = out[i][k];
        }
        arena[to + 3u] = normal.x;
        arena[to + 4u] = normal.y;
        arena[to + 5u] = normal.z;
    }
}

/// Adds each request's new midpoints and children to its sheet size.
@compute @workgroup_size(WORKGROUP_SIZE)
fn refine_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !sheet_open(r) {
        return;
    }
    let base = tri_base(r);
    let end = base + requests[r].out_index_capacity / 3u;
    let creates = region(REGION_REFINE_CREATES);
    let children = region(REGION_REFINE_CHILDREN);
    let points = counter(r, COUNTER_SHEET_POINTS) + arena[creates + 3u * end] - arena[creates + 3u * base];
    record_size(r, points, arena[children + end] - arena[children + base]);
}
