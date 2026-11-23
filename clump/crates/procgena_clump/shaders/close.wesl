// Closes each closed layer request's sheet into a solid, like the Rust
// crate's `shell::close_volume`: sinks an underside below the sheet, splits
// pinch points (where the sheet's border passes through a point more than
// once) so every wall edge is shared by exactly two triangles, and joins the
// top and underside with walls along the border.
//
// A closed layer's working mesh holds, in order, the sheet points (P), one
// copy of each pinch point per fan of triangles after the first (C), and
// one underside vertex per point after splitting (U); then the sheet
// triangles, one underside triangle per kept top triangle (K), and two wall
// triangles per border edge (B). That is the CPU's order once unused
// vertices are removed.
//
// Pass order, with W working vertex slots and Wt working triangle slots:
// - between `shell_keep` and the fold passes: `close_depth` (W);
// - after the fold passes: `close_border` (3 Wt + 1) and `close_kept`
//   (Wt + 1), then scans of `REGION_CLOSE_BORDER` and `REGION_CLOSE_KEPT`;
//   `close_fans` (W + 1), a scan of `REGION_CLOSE_COPIES`; `close_split`
//   (3 Wt); `close_first` (3 Wt + 1), a scan of `REGION_CLOSE_FIRST`;
//   `close_below` (3 Wt); `close_walls` (W + 1), a scan of
//   `REGION_CLOSE_WALLS`; `close_size` (requests);
// - after `shell_out_vertices` and `shell_out_tris`: `close_out_vertices`
//   (W), `close_out_tris` (Wt), `close_out_walls` (3 Wt), then
//   `close_around_count` (Wt), `close_around_copy` (W + 1), a scan of
//   `REGION_CLOSE_AROUND_START`, `close_around_fill` (Wt);
// - after the working mesh's scans: `close_normals` (W).

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, store3, counter,
    request_of_work_vertex, request_of_work_triangle, COUNTER_STRIDE, COUNTER_SHEET_POINTS,
    COUNTER_SHEET_TRIANGLES, COUNTER_SHEET_FAILED, COUNTER_WORK_VERTICES, COUNTER_WORK_TRIANGLES,
    COUNTER_OVERFLOW, OVERFLOW_VERTICES, OVERFLOW_INDICES, REGION_SHELL_DIRS, REGION_SHELL_ALONG,
    REGION_SHELL_WEIGHTS, REGION_SHELL_TRI_STATE, REGION_SHEET_AROUND_LIST, REGION_CLOSE_DEPTHS,
    REGION_CLOSE_SINK_DIRS, REGION_CLOSE_BORDER, REGION_CLOSE_FAN, REGION_CLOSE_SPLIT,
    REGION_CLOSE_COPIES, REGION_CLOSE_SOURCE, REGION_CLOSE_FIRST, REGION_CLOSE_BELOW,
    REGION_CLOSE_KEPT, REGION_CLOSE_WALLS, REGION_CLOSE_AROUND_START, REGION_CLOSE_AROUND_LIST,
    FLAG_REGION_CLOSE_AROUND_COUNT, FLAG_REGION_CLOSE_AROUND_CURSOR, REGION_WORK_POSITIONS,
    REGION_WORK_FADE, REGION_WORK_CORNERS, REGION_WORK_USED, REGION_WORK_KEPT,
    REGION_WORK_NORMALS, WORKGROUP_SIZE,
};
import package::field::{top, fade};
import package::math::{cross3, normalize_or_zero};
import package::shell::{
    shell_open, sheet_vertex, sheet_triangle, corners_of, point_at, sheet_point, top_of, set_top,
    top_position, around, sink, min_solid, Sunk, BOTTOM_GAP, TRI_KEPT, NONE,
};

// ---------------------------------------------------------------------------
// Helpers

/// Returns whether sheet triangle slot `t` of request `r` is a kept top
/// triangle of a closed layer.
fn closed_kept(r: u32, t: u32) -> bool {
    return requests[r].closed != 0u && sheet_triangle(r, t)
        && arena[region(REGION_SHELL_TRI_STATE) + t] == TRI_KEPT;
}

/// Returns the corner of sheet triangle slot `t` at request-local point `p`,
/// or 3 if `p` isn't a corner.
fn corner_at(t: u32, p: u32) -> u32 {
    let v = corners_of(t);
    for (var k = 0u; k < 3u; k++) {
        if v[k] == p {
            return k;
        }
    }
    return 3u;
}

/// Returns the total that a scan of `region_index`, over a request's slots
/// from `first` to before `end`, counted.
fn scanned_total(region_index: u32, first: u32, end: u32) -> u32 {
    return arena[region(region_index) + end] - arena[region(region_index) + first];
}

/// Returns whether entry `s` of scanned region `region_index` was 1.
fn scanned_flag(region_index: u32, s: u32) -> bool {
    return arena[region(region_index) + s + 1u] != arena[region(region_index) + s];
}

/// Returns whether a kept top triangle of request `r` around request-local
/// point `a`, in a slot below `before`, has the directed edge from `a` to
/// `b`.
fn has_edge(r: u32, a: u32, b: u32, before: u32) -> bool {
    let span = around(requests[r].out_vertex_base + a);
    for (var i = span.x; i < span.y; i++) {
        let u = arena[region(REGION_SHEET_AROUND_LIST) + i];
        if u >= before || !closed_kept(r, u) {
            continue;
        }
        let k = corner_at(u, a);
        if corners_of(u)[(k + 1u) % 3u] == b {
            return true;
        }
    }
    return false;
}

/// A request's closing counts: copies of pinch points, underside vertices,
/// kept top triangles and border edges.
struct Closing {
    copies: u32,
    undersides: u32,
    kept: u32,
    border: u32,
}

/// Returns request `r`'s closing counts, once their scans have run.
fn closing(r: u32) -> Closing {
    let request = requests[r];
    let first_vertex = request.out_vertex_base;
    let first_tri = request.out_index_base / 3u;
    let end_tri = first_tri + request.out_index_capacity / 3u;
    return Closing(
        scanned_total(REGION_CLOSE_COPIES, first_vertex, first_vertex + request.out_vertex_capacity),
        scanned_total(REGION_CLOSE_FIRST, 3u * first_tri, 3u * end_tri),
        scanned_total(REGION_CLOSE_KEPT, first_tri, end_tri),
        scanned_total(REGION_CLOSE_BORDER, 3u * first_tri, 3u * end_tri),
    );
}

/// Returns the working vertex slot of request `r`'s point `p`, after
/// splitting, if it is within the request's capacity, or `NONE`.
fn vertex_slot(r: u32, p: u32) -> u32 {
    if p >= requests[r].out_vertex_capacity {
        return NONE;
    }
    return requests[r].out_vertex_base + p;
}

// ---------------------------------------------------------------------------
// Underside depths (before the fold passes)

/// Places the underside below each point of a closed layer's kept
/// triangles, never more than part of the way through the input, and
/// raises its top to stay at least the thinnest solid above it. Points in
/// no kept triangle get depth 0, sunk along their push direction.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_depth(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if requests[r].closed == 0u || !sheet_vertex(r, slot) {
        return;
    }

    // Whether the point is a corner of a kept triangle.
    let span = around(slot);
    var in_kept = false;
    for (var i = span.x; i < span.y; i++) {
        let t = arena[region(REGION_SHEET_AROUND_LIST) + i];
        in_kept = in_kept || arena[region(REGION_SHELL_TRI_STATE) + t] == TRI_KEPT;
    }

    // Sink the underside by the sink depth plus a gap, and further by the
    // dip, so it clears input the simplified surface passes over.
    let dir = load3(region(REGION_SHELL_DIRS) + 3u * slot);
    var sunk = Sunk(dir, 0.0);
    if in_kept {
        let p = sheet_point(slot);
        let field = requests[r].field;
        sunk = sink(r, p, dir, field.sink + BOTTOM_GAP * field.thickness + p.dip);
        let least = min_solid(r, p) - sunk.depth;
        if top_of(slot) < least {
            set_top(slot, least);
            store3(region(REGION_SHELL_ALONG) + 3u * slot, select(dir, sunk.dir, least < 0.0));
        }
    }
    arena[region(REGION_CLOSE_DEPTHS) + slot] = bitcast<u32>(sunk.depth);
    store3(region(REGION_CLOSE_SINK_DIRS) + 3u * slot, sunk.dir);
}

// ---------------------------------------------------------------------------
// Border edges, pinch points and numbering (after the fold passes)

/// Flags each corner of a closed layer's kept top triangles whose edge to
/// the next corner is a border edge: no kept triangle has the reverse edge.
/// A directed edge in more than one triangle is flagged in the first.
/// Writes 0 everywhere else, including the total slot. Dispatch 3 Wt + 1
/// items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_border(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s > 3u * batch.total_work_triangles {
        return;
    }
    var border = false;
    if s < 3u * batch.total_work_triangles {
        let t = s / 3u;
        let r = request_of_work_triangle(t);
        if closed_kept(r, t) {
            let v = corners_of(t);
            let a = v[s % 3u];
            let b = v[(s + 1u) % 3u];
            border = !has_edge(r, b, a, NONE) && !has_edge(r, a, b, t);
        }
    }
    arena[region(REGION_CLOSE_BORDER) + s] = select(0u, 1u, border);
}

/// Flags each kept top triangle of a closed layer, and writes 0 everywhere
/// else, including the total slot. Dispatch Wt + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_kept(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t > batch.total_work_triangles {
        return;
    }
    let kept = t < batch.total_work_triangles && closed_kept(request_of_work_triangle(t), t);
    arena[region(REGION_CLOSE_KEPT) + t] = select(0u, 1u, kept);
}

/// Finds each closed layer's pinch points, where more than one border edge
/// leaves the point, and groups the kept triangles around each into fans:
/// triangles joined through edges at the point. Labels each such corner
/// with its fan's lowest triangle slot, and counts the copies the point
/// gets, one per fan after the first; 0 for every other slot, including the
/// total slot. Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_fans(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_work_vertices {
        return;
    }
    var copies = 0u;
    if slot < batch.total_work_vertices {
        let r = request_of_work_vertex(slot);
        if requests[r].closed != 0u && sheet_vertex(r, slot) {
            copies = split_fans(r, slot);
        }
    }
    arena[region(REGION_CLOSE_COPIES) + slot] = copies;
}

/// Labels the fans around sheet point slot `slot` of closed request `r` if
/// it is a pinch point, and returns how many copies it gets.
fn split_fans(r: u32, slot: u32) -> u32 {
    let p = slot - requests[r].out_vertex_base;
    let span = around(slot);
    let list = region(REGION_SHEET_AROUND_LIST);
    let fan = region(REGION_CLOSE_FAN);

    // Count the border edges leaving the point.
    var leaving = 0u;
    for (var i = span.x; i < span.y; i++) {
        let t = arena[list + i];
        if closed_kept(r, t) && scanned_flag(REGION_CLOSE_BORDER, 3u * t + corner_at(t, p)) {
            leaving += 1u;
        }
    }
    if leaving <= 1u {
        return 0u;
    }

    // Start each triangle in its own fan, then join triangles that share
    // another corner, keeping the lower label, until nothing changes.
    for (var i = span.x; i < span.y; i++) {
        let t = arena[list + i];
        if closed_kept(r, t) {
            arena[fan + 3u * t + corner_at(t, p)] = t;
        }
    }
    var changed = true;
    while changed {
        changed = false;
        for (var i = span.x; i < span.y; i++) {
            let t = arena[list + i];
            if !closed_kept(r, t) {
                continue;
            }
            let at = fan + 3u * t + corner_at(t, p);
            for (var j = span.x; j < span.y; j++) {
                let u = arena[list + j];
                if u == t || !closed_kept(r, u) || !share_other_corner(t, u, p) {
                    continue;
                }
                let label = arena[fan + 3u * u + corner_at(u, p)];
                if label < arena[at] {
                    arena[at] = label;
                    changed = true;
                }
            }
        }
    }

    // A fan's label is its lowest triangle, so its heads count the fans.
    var fans = 0u;
    for (var i = span.x; i < span.y; i++) {
        let t = arena[list + i];
        if closed_kept(r, t) && arena[fan + 3u * t + corner_at(t, p)] == t {
            fans += 1u;
        }
    }
    return fans - 1u;
}

/// Returns whether sheet triangle slots `t` and `u` share a corner other
/// than request-local point `p`.
fn share_other_corner(t: u32, u: u32, p: u32) -> bool {
    let a = corners_of(t);
    let b = corners_of(u);
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            if a[i] != p && a[i] == b[j] {
                return true;
            }
        }
    }
    return false;
}

/// Gives each corner of a closed layer's kept top triangles its point
/// number after splitting: at a pinch point, a fan after the first uses
/// the point's copy for that fan, numbered after the sheet points in point
/// then fan order. Records the sheet point behind each copy. Dispatch 3 Wt
/// items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_split(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s >= 3u * batch.total_work_triangles {
        return;
    }
    let t = s / 3u;
    let r = request_of_work_triangle(t);
    if !closed_kept(r, t) {
        return;
    }
    let base = requests[r].out_vertex_base;
    let p = corners_of(t)[s % 3u];
    var split = p;
    if scanned_flag(REGION_CLOSE_COPIES, base + p) {
        // The fan's place among the fans around the point, by lowest
        // triangle.
        let label = arena[region(REGION_CLOSE_FAN) + s];
        let span = around(base + p);
        var rank = 0u;
        for (var i = span.x; i < span.y; i++) {
            let u = arena[region(REGION_SHEET_AROUND_LIST) + i];
            if u < label && closed_kept(r, u) && arena[region(REGION_CLOSE_FAN) + 3u * u + corner_at(u, p)] == u {
                rank += 1u;
            }
        }
        if rank > 0u {
            let before = arena[region(REGION_CLOSE_COPIES) + base + p] - arena[region(REGION_CLOSE_COPIES) + base];
            split = counter(r, COUNTER_SHEET_POINTS) + before + rank - 1u;
            let copy = vertex_slot(r, split);
            if copy != NONE {
                arena[region(REGION_CLOSE_SOURCE) + copy] = p;
            }
        }
    }
    arena[region(REGION_CLOSE_SPLIT) + s] = split;
}

/// Flags each corner of a closed layer's kept top triangles where its point,
/// after splitting, first appears in triangle then corner order; 0 for
/// every other slot, including the total slot. Dispatch 3 Wt + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_first(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s > 3u * batch.total_work_triangles {
        return;
    }
    var first = false;
    if s < 3u * batch.total_work_triangles {
        let t = s / 3u;
        let r = request_of_work_triangle(t);
        if closed_kept(r, t) {
            // A point appears once per triangle, so look for it in earlier
            // triangles around the sheet point behind it.
            let p = corners_of(t)[s % 3u];
            let split = arena[region(REGION_CLOSE_SPLIT) + s];
            let span = around(requests[r].out_vertex_base + p);
            first = true;
            for (var i = span.x; i < span.y; i++) {
                let u = arena[region(REGION_SHEET_AROUND_LIST) + i];
                if u < t && closed_kept(r, u) && arena[region(REGION_CLOSE_SPLIT) + 3u * u + corner_at(u, p)] == split {
                    first = false;
                }
            }
        }
    }
    arena[region(REGION_CLOSE_FIRST) + s] = select(0u, 1u, first);
}

/// Numbers the underside vertices in order of first appearance, after the
/// sheet points and copies: records each split point's underside vertex and
/// the sheet point behind it. Dispatch 3 Wt items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_below(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s >= 3u * batch.total_work_triangles {
        return;
    }
    let t = s / 3u;
    let r = request_of_work_triangle(t);
    if !closed_kept(r, t) || !scanned_flag(REGION_CLOSE_FIRST, s) {
        return;
    }
    let first_corner = 3u * (requests[r].out_index_base / 3u);
    let rank = arena[region(REGION_CLOSE_FIRST) + s] - arena[region(REGION_CLOSE_FIRST) + first_corner];
    let below = counter(r, COUNTER_SHEET_POINTS) + closing(r).copies + rank;
    let split = vertex_slot(r, arena[region(REGION_CLOSE_SPLIT) + s]);
    if split != NONE {
        arena[region(REGION_CLOSE_BELOW) + split] = below;
    }
    let under = vertex_slot(r, below);
    if under != NONE {
        arena[region(REGION_CLOSE_SOURCE) + under] = corners_of(t)[s % 3u];
    }
}

/// Counts the border edges leaving each split point of a closed layer; 0
/// for every other slot, including the total slot. Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_walls(
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
        let request = requests[r];
        let x = slot - request.out_vertex_base;
        let points = counter(r, COUNTER_SHEET_POINTS);
        if request.closed != 0u && shell_open(r) && x < points + closing(r).copies {
            let p = select(arena[region(REGION_CLOSE_SOURCE) + slot], x, x < points);
            count = leaving_border(r, p, x, NONE);
        }
    }
    arena[region(REGION_CLOSE_WALLS) + slot] = count;
}

/// Returns how many border edges leave split point `x` of closed request
/// `r`, whose sheet point is `p`, toward split points below `below`.
fn leaving_border(r: u32, p: u32, x: u32, below: u32) -> u32 {
    let span = around(requests[r].out_vertex_base + p);
    var count = 0u;
    for (var i = span.x; i < span.y; i++) {
        let u = arena[region(REGION_SHEET_AROUND_LIST) + i];
        if !closed_kept(r, u) {
            continue;
        }
        let k = corner_at(u, p);
        let s = 3u * u + k;
        let next = arena[region(REGION_CLOSE_SPLIT) + 3u * u + (k + 1u) % 3u];
        if arena[region(REGION_CLOSE_SPLIT) + s] == x && scanned_flag(REGION_CLOSE_BORDER, s) && next < below {
            count += 1u;
        }
    }
    return count;
}

/// Sets each closed layer's working mesh size: the sheet points, copies and
/// underside vertices; and the sheet triangle slots, underside triangles
/// and two wall triangles per border edge. A layer that doesn't fit its
/// output capacity fails with the matching overflow bits and writes
/// nothing. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_size(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].closed == 0u || !shell_open(r) {
        return;
    }
    let c = closing(r);
    let vertices = counter(r, COUNTER_SHEET_POINTS) + c.copies + c.undersides;
    let triangles = counter(r, COUNTER_SHEET_TRIANGLES) + c.kept + 2u * c.border;
    let at = r * COUNTER_STRIDE;
    atomicStore(&counters[at + COUNTER_WORK_VERTICES], vertices);
    atomicStore(&counters[at + COUNTER_WORK_TRIANGLES], triangles);
    var overflow = 0u;
    if vertices > requests[r].out_vertex_capacity {
        overflow |= OVERFLOW_VERTICES;
    }
    if triangles > requests[r].out_index_capacity / 3u {
        overflow |= OVERFLOW_INDICES;
    }
    if overflow != 0u {
        atomicOr(&counters[at + COUNTER_OVERFLOW], overflow);
        atomicStore(&counters[at + COUNTER_SHEET_FAILED], 1u);
    }
}

// ---------------------------------------------------------------------------
// Output

/// Returns whether request `r` is a closed layer that fits its output.
fn closing_fits(r: u32) -> bool {
    return requests[r].closed != 0u && shell_open(r);
}

/// Writes each closed layer's pinch copies, at their sheet point's top, and
/// its underside vertices, below the sheet point with fade 0, to the
/// working mesh.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_out_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !closing_fits(r) {
        return;
    }
    let base = requests[r].out_vertex_base;
    let x = slot - base;
    let points = counter(r, COUNTER_SHEET_POINTS);
    let c = closing(r);
    if x < points || x >= points + c.copies + c.undersides {
        return;
    }
    let source = base + arena[region(REGION_CLOSE_SOURCE) + slot];
    if x < points + c.copies {
        let field = requests[r].field;
        let w = bitcast<f32>(arena[region(REGION_SHELL_WEIGHTS) + source]);
        store3(region(REGION_WORK_POSITIONS) + 3u * slot, top_position(source));
        arena[region(REGION_WORK_FADE) + slot] = bitcast<u32>(fade(field, top(field, w)));
    } else {
        let depth = bitcast<f32>(arena[region(REGION_CLOSE_DEPTHS) + source]);
        let dir = load3(region(REGION_CLOSE_SINK_DIRS) + 3u * source);
        store3(region(REGION_WORK_POSITIONS) + 3u * slot, load3(point_at(source)) - dir * depth);
        arena[region(REGION_WORK_FADE) + slot] = 0u;
    }
}

/// Returns the working vertex slot of the underside vertex below split
/// point `x` of request `r`.
fn below_slot(r: u32, x: u32) -> u32 {
    let base = requests[r].out_vertex_base;
    return base + arena[region(REGION_CLOSE_BELOW) + base + x];
}

/// Writes a triangle with working vertex slots `a`, `b` and `c` to working
/// triangle slot `t`, keeping it and marking its corners used.
fn write_triangle(t: u32, a: u32, b: u32, c: u32) {
    let at = region(REGION_WORK_CORNERS) + 3u * t;
    arena[at] = a;
    arena[at + 1u] = b;
    arena[at + 2u] = c;
    arena[region(REGION_WORK_USED) + a] = 1u;
    arena[region(REGION_WORK_USED) + b] = 1u;
    arena[region(REGION_WORK_USED) + c] = 1u;
    arena[region(REGION_WORK_KEPT) + t] = 1u;
}

/// Writes each closed layer's underside triangles, below its kept top
/// triangles in the same order and wound the other way, so they face into
/// the input.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_out_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !closing_fits(r) || !closed_kept(r, t) {
        return;
    }
    let first = requests[r].out_index_base / 3u;
    let rank = arena[region(REGION_CLOSE_KEPT) + t] - arena[region(REGION_CLOSE_KEPT) + first];
    let under = first + counter(r, COUNTER_SHEET_TRIANGLES) + rank;
    let split = region(REGION_CLOSE_SPLIT) + 3u * t;
    let a = below_slot(r, arena[split]);
    let b = below_slot(r, arena[split + 1u]);
    let c = below_slot(r, arena[split + 2u]);
    write_triangle(under, a, c, b);
}

/// Writes each closed layer's walls: for each border edge from split point
/// `a` to `b`, the quad joining it to the underside edge below it, as two
/// triangles, in order of `a` then `b`. Dispatch 3 Wt items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_out_walls(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let s = item_index(global_id, num_workgroups);
    if s >= 3u * batch.total_work_triangles {
        return;
    }
    let t = s / 3u;
    let r = request_of_work_triangle(t);
    if !closing_fits(r) || !closed_kept(r, t) || !scanned_flag(REGION_CLOSE_BORDER, s) {
        return;
    }
    let base = requests[r].out_vertex_base;
    let a = arena[region(REGION_CLOSE_SPLIT) + s];
    let b = arena[region(REGION_CLOSE_SPLIT) + 3u * t + (s + 1u) % 3u];

    // Walls go in order of their border edge's ends.
    let p = corners_of(t)[s % 3u];
    let wall = arena[region(REGION_CLOSE_WALLS) + base + a] - arena[region(REGION_CLOSE_WALLS) + base]
        + leaving_border(r, p, a, b);
    let first = requests[r].out_index_base / 3u;
    let walls = first + counter(r, COUNTER_SHEET_TRIANGLES) + closing(r).kept;
    let a_below = below_slot(r, a);
    let b_below = below_slot(r, b);
    write_triangle(walls + 2u * wall, base + b, base + a, a_below);
    write_triangle(walls + 2u * wall + 1u, base + b, a_below, b_below);
}

// ---------------------------------------------------------------------------
// Normals

/// Returns whether working triangle slot `t` is a kept triangle of a closed
/// layer, before the working mesh's scans.
fn closed_work_triangle(t: u32) -> bool {
    return closing_fits(request_of_work_triangle(t)) && arena[region(REGION_WORK_KEPT) + t] != 0u;
}

/// Counts the triangles around each vertex of the closed layers' working
/// meshes.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_around_count(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles || !closed_work_triangle(t) {
        return;
    }
    for (var k = 0u; k < 3u; k++) {
        let v = arena[region(REGION_WORK_CORNERS) + 3u * t + k];
        atomicAdd(&flags[region(FLAG_REGION_CLOSE_AROUND_COUNT) + v], 1u);
    }
}

/// Copies the triangle counts out for scanning, and clears the fill
/// cursors. Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_around_copy(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_work_vertices {
        return;
    }
    var count = 0u;
    if slot < batch.total_work_vertices {
        count = atomicExchange(&flags[region(FLAG_REGION_CLOSE_AROUND_COUNT) + slot], 0u);
        atomicStore(&flags[region(FLAG_REGION_CLOSE_AROUND_CURSOR) + slot], 0u);
    }
    arena[region(REGION_CLOSE_AROUND_START) + slot] = count;
}

/// Lists each kept triangle of the closed layers' working meshes under its
/// corners, after the counts are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_around_fill(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles || !closed_work_triangle(t) {
        return;
    }
    for (var k = 0u; k < 3u; k++) {
        let v = arena[region(REGION_WORK_CORNERS) + 3u * t + k];
        let at = arena[region(REGION_CLOSE_AROUND_START) + v] + atomicAdd(&flags[region(FLAG_REGION_CLOSE_AROUND_CURSOR) + v], 1u);
        arena[region(REGION_CLOSE_AROUND_LIST) + at] = t;
    }
}

/// Sets each used vertex normal of the closed layers: the sum of its
/// triangles' faces, each weighted by its area, in triangle order, made
/// unit length.
@compute @workgroup_size(WORKGROUP_SIZE)
fn close_normals(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let used = region(REGION_WORK_USED);
    if !closing_fits(request_of_work_vertex(slot)) || arena[used + slot + 1u] == arena[used + slot] {
        return;
    }

    // The triangles around the vertex, lowest slot first.
    let start = region(REGION_CLOSE_AROUND_START) + slot;
    let list = region(REGION_CLOSE_AROUND_LIST);
    var sum = vec3<f32>(0.0);
    var lowest = 0u;
    loop {
        var next = NONE;
        for (var i = arena[start]; i < arena[start + 1u]; i++) {
            let t = arena[list + i];
            if t >= lowest {
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
