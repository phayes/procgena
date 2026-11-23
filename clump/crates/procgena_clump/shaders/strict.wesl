// Strict mode for layers, like the strict loop of the Rust crate's
// `shell::displaced_shell`: finds the triangles of each strict layer's
// assembled mesh that pass through another, then thins the clump around
// them, or once thinning stops helping, cuts them out, and checks again.
//
// Each strict pass, numbered from 0, after the fold passes and closing:
// - `strict_clear_tris` (Wt) and `strict_clear_points` (W);
// - closing's numbering passes again, for closed layers;
// - the working mesh: `shell_out_vertices`, `shell_out_tris`, and for
//   closed layers `close_out_vertices`, `close_out_tris`, `close_out_walls`;
// - the working mesh's BVH (`bvh.wesl` with `MESH_WORK`);
// - `strict_cross` (Wt), then `strict_decide` (requests);
// - except after the last pass: `strict_mark` (Wt), `strict_cut` (Wt) and
//   `strict_relax` (W).

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, counter,
    request_of_work_vertex, request_of_work_triangle, COUNTER_STRIDE, COUNTER_SHEET_POINTS,
    COUNTER_SHEET_TRIANGLES, COUNTER_CROSSINGS, COUNTER_STRICT_FOUND, COUNTER_STRICT_FEWEST,
    COUNTER_STRICT_STALLED, COUNTER_STRICT_CUTTING, COUNTER_STRICT_DONE, MODE_EVERYWHERE,
    REGION_SHELL_TRI_STATE, REGION_CLOSE_DEPTHS, REGION_CLOSE_SOURCE, REGION_CLOSE_KEPT,
    REGION_WORK_CORNERS, REGION_WORK_USED, REGION_WORK_KEPT, REGION_STRICT_CROSSING,
    FLAG_REGION_STRICT_TOUCH, WORKGROUP_SIZE,
};
import package::bvh::{
    work_mesh, count_in, sorted_in, corners_in, first_in, triangle_bounds, NODE_WORDS, LEAF,
    STACK_SIZE,
};
import package::close::{closing};
import package::field::{lowered};
import package::geometry::{triangles_cross};
import package::shell::{
    shell_open, sheet_vertex, sheet_triangle, corners_of, sheet_point, top_of, set_top, floor_of,
    min_solid, TRI_KEPT, TRI_CUT,
};

/// The number of the strict pass being run, from 0.
struct StrictPass {
    number: u32,
}

@group(1) @binding(0) var<uniform> strict_pass: StrictPass;

/// Thinning passes before cutting starts, and thinning passes in a row
/// without fewer crossings after which it starts anyway (the CPU's
/// `MAX_REPAIR_PASSES` and `MAX_STALLED_PASSES`).
const MAX_REPAIR_PASSES: u32 = 24u;
const MAX_STALLED_PASSES: u32 = 2u;

/// `FLAG_REGION_STRICT_TOUCH` bits: lower the point's top; raise its
/// underside; cut out the top triangles using the point.
const TOUCH_TOP: u32 = 1u;
const TOUCH_BOTTOM: u32 = 2u;
const TOUCH_CUT: u32 = 4u;

/// Returns whether request `r` is a strict layer still being repaired.
fn strict_active(r: u32) -> bool {
    return requests[r].mode == MODE_EVERYWHERE && requests[r].strict != 0u && shell_open(r)
        && counter(r, COUNTER_STRICT_DONE) == 0u;
}

/// Clears the strict layers' kept working triangles before the mesh is
/// assembled again.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_clear_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if requests[r].mode == MODE_EVERYWHERE && requests[r].strict != 0u {
        arena[region(REGION_WORK_KEPT) + t] = 0u;
    }
}

/// Clears the strict layers' used working vertices and the marks of the
/// pass before.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_clear_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if requests[r].mode == MODE_EVERYWHERE && requests[r].strict != 0u {
        arena[region(REGION_WORK_USED) + slot] = 0u;
        atomicStore(&flags[region(FLAG_REGION_STRICT_TOUCH) + slot], 0u);
    }
}

// ---------------------------------------------------------------------------
// Finding crossings

/// Returns whether boxes `a` and `b`, as (min, max), overlap or touch.
fn boxes_overlap(a: array<vec3<f32>, 2>, b: array<vec3<f32>, 2>) -> bool {
    return all(a[0] <= b[1]) && all(b[0] <= a[1]);
}

/// Returns whether triangles `a` and `b` share a corner position.
fn share_position(a: array<vec3<f32>, 3>, b: array<vec3<f32>, 3>) -> bool {
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            if all(a[i] == b[j]) {
                return true;
            }
        }
    }
    return false;
}

/// Returns whether working triangle slot `t` of strict layer `r` passes
/// through another triangle of its mesh that shares no corner position
/// with it.
fn crosses_another(r: u32, t: u32) -> bool {
    let m = work_mesh();
    let n = count_in(m, r);
    let corners = corners_in(m, r, t);
    let bounds = triangle_bounds(corners);
    let nodes = m.nodes + NODE_WORDS * first_in(m, r);
    var stack: array<u32, STACK_SIZE>;
    var top = 1u;
    stack[0] = select(0u, LEAF, n == 1u);
    while top > 0u {
        top -= 1u;
        let entry = stack[top];
        if (entry & LEAF) != 0u {
            let u = sorted_in(m, r, entry & ~LEAF);
            if u == t {
                continue;
            }
            let other = corners_in(m, r, u);
            if boxes_overlap(bounds, triangle_bounds(other)) && !share_position(corners, other)
                && triangles_cross(corners, other) {
                return true;
            }
            continue;
        }
        let at = nodes + NODE_WORDS * entry;
        for (var side = 0u; side < 2u; side++) {
            let child = array<vec3<f32>, 2>(load3(at + 2u + 6u * side), load3(at + 5u + 6u * side));
            if boxes_overlap(bounds, child) && top < STACK_SIZE {
                stack[top] = arena[at + side];
                top += 1u;
            }
        }
    }
    return false;
}

/// Marks each triangle of the strict layers' working meshes that passes
/// through another, and counts them per request; writes 0 for every other
/// slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_cross(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    var crossing = false;
    if strict_active(r) && arena[region(REGION_WORK_KEPT) + t] != 0u {
        crossing = crosses_another(r, t);
    }
    arena[region(REGION_STRICT_CROSSING) + t] = select(0u, 1u, crossing);
    if crossing {
        atomicAdd(&counters[r * COUNTER_STRIDE + COUNTER_STRICT_FOUND], 1u);
    }
}

/// Ends each strict layer's pass: done once nothing crosses; otherwise
/// cutting starts once thinning has run `MAX_REPAIR_PASSES` passes or
/// stopped helping. Records the crossings found. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_decide(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !strict_active(r) {
        return;
    }
    let c = r * COUNTER_STRIDE;
    let pass_number = strict_pass.number;
    let found = atomicExchange(&counters[c + COUNTER_STRICT_FOUND], 0u);
    atomicStore(&counters[c + COUNTER_CROSSINGS], found);
    if found == 0u {
        atomicStore(&counters[c + COUNTER_STRICT_DONE], 1u);
        return;
    }
    let fewest = select(counter(r, COUNTER_STRICT_FEWEST), 0xffffffffu, pass_number == 0u);
    var stalled = counter(r, COUNTER_STRICT_STALLED);
    if found < fewest {
        atomicStore(&counters[c + COUNTER_STRICT_FEWEST], found);
        stalled = 0u;
    } else {
        stalled += 1u;
    }
    atomicStore(&counters[c + COUNTER_STRICT_STALLED], stalled);
    if pass_number >= MAX_REPAIR_PASSES || stalled >= MAX_STALLED_PASSES {
        atomicStore(&counters[c + COUNTER_STRICT_CUTTING], 1u);
    }
}

// ---------------------------------------------------------------------------
// Repairing

/// Where a working vertex came from: a sheet point slot, and whether it is
/// that point's underside vertex.
struct Origin {
    point: u32,
    bottom: bool,
}

/// Returns where working vertex slot `slot` of layer `r` came from: a sheet
/// point, a pinch copy of one, or an underside vertex below one.
fn origin_of(r: u32, slot: u32) -> Origin {
    let base = requests[r].out_vertex_base;
    let x = slot - base;
    let points = counter(r, COUNTER_SHEET_POINTS);
    if x < points {
        return Origin(slot, false);
    }
    let source = base + arena[region(REGION_CLOSE_SOURCE) + slot];
    return Origin(source, x >= points + closing(r).copies);
}

/// Returns the sheet triangle slot of closed layer `r` behind its `rank`-th
/// underside triangle: its `rank`-th kept top triangle.
fn top_of_underside(r: u32, rank: u32) -> u32 {
    let kept = region(REGION_CLOSE_KEPT);
    let first = requests[r].out_index_base / 3u;
    var lo = first;
    var hi = first + counter(r, COUNTER_SHEET_TRIANGLES) - 1u;
    // The last slot whose count of kept triangles before it is `rank`.
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if arena[kept + mid] - arena[kept + first] <= rank {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Marks what each crossing triangle changes. While thinning: its corners'
/// tops to lower and undersides to raise. While cutting: a top or underside
/// triangle cuts out its top triangle; a wall marks its top corners' sheet
/// points, whose top triangles are cut out next.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_mark(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !strict_active(r) || arena[region(REGION_STRICT_CROSSING) + t] == 0u {
        return;
    }
    let corners = region(REGION_WORK_CORNERS) + 3u * t;
    if counter(r, COUNTER_STRICT_CUTTING) == 0u {
        for (var k = 0u; k < 3u; k++) {
            let o = origin_of(r, arena[corners + k]);
            atomicOr(&flags[region(FLAG_REGION_STRICT_TOUCH) + o.point], select(TOUCH_TOP, TOUCH_BOTTOM, o.bottom));
        }
        return;
    }
    let first = requests[r].out_index_base / 3u;
    let local = t - first;
    let tops = counter(r, COUNTER_SHEET_TRIANGLES);
    if local < tops {
        arena[region(REGION_SHELL_TRI_STATE) + t] = TRI_CUT;
    } else if local < tops + closing(r).kept {
        arena[region(REGION_SHELL_TRI_STATE) + top_of_underside(r, local - tops)] = TRI_CUT;
    } else {
        for (var k = 0u; k < 3u; k++) {
            let o = origin_of(r, arena[corners + k]);
            if !o.bottom {
                atomicOr(&flags[region(FLAG_REGION_STRICT_TOUCH) + o.point], TOUCH_CUT);
            }
        }
    }
}

/// While cutting, cuts out each kept top triangle with a corner a crossing
/// wall marked.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_cut(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let t = item_index(global_id, num_workgroups);
    if t >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(t);
    if !strict_active(r) || counter(r, COUNTER_STRICT_CUTTING) == 0u || !sheet_triangle(r, t) {
        return;
    }
    let state = region(REGION_SHELL_TRI_STATE) + t;
    if arena[state] != TRI_KEPT {
        return;
    }
    let v = corners_of(t);
    for (var k = 0u; k < 3u; k++) {
        let p = requests[r].out_vertex_base + v[k];
        if (atomicLoad(&flags[region(FLAG_REGION_STRICT_TOUCH) + p]) & TOUCH_CUT) != 0u {
            arena[state] = TRI_CUT;
        }
    }
}

/// While thinning, moves each marked top halfway down toward its floor,
/// then halves each marked underside's depth, keeping the top at least the
/// thinnest solid above it.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_relax(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    if !strict_active(r) || counter(r, COUNTER_STRICT_CUTTING) != 0u || !sheet_vertex(r, slot) {
        return;
    }
    let touch = atomicLoad(&flags[region(FLAG_REGION_STRICT_TOUCH) + slot]);
    if (touch & TOUCH_TOP) != 0u {
        set_top(slot, lowered(top_of(slot), floor_of(r, slot)));
    }
    if (touch & TOUCH_BOTTOM) != 0u {
        let at = region(REGION_CLOSE_DEPTHS) + slot;
        let depth = 0.5 * bitcast<f32>(arena[at]);
        arena[at] = bitcast<u32>(depth);
        set_top(slot, max(top_of(slot), min_solid(r, sheet_point(slot)) - depth));
    }
}
