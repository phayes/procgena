// The working surface a layer clump grows on, matching the Rust crate's
// `patch` module: the input triangles that face some direction, with one
// point per welded position, later simplified toward the target edge length.
// Needs the prepare stage.
//
// Pass order, with V vertex slots, T triangle slots and E = 3T:
// `patch_reset` (V), `facing_keep` (T + 1), `point_owner` (T),
// `corner_owner` (E + 1), scan the corner owners (E + 1), scan the kept
// triangles (T + 1), `patch_points` (E), `patch_tris` (T),
// `patch_counts` (requests).
//
// Each request's points live in its vertex slots and its triangles in its
// triangle slots, numbered from 0 within the request.

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, store3, counter,
    request_of_triangle, COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_POINTS,
    COUNTER_PATCH_TRIANGLES, MODE_EVERYWHERE, REGION_POSITIONS, REGION_CORNERS,
    REGION_WELD_ID, REGION_WELD_RANK, REGION_WELDED_NORMALS, REGION_FACING_KEEP,
    REGION_CORNER_OWNER, REGION_POINT_OF_ID, REGION_PATCH_POINTS, REGION_PATCH_TRIS,
    FLAG_REGION_POINT_OWNER, POINT_WORDS, PATCH_TRI_WORDS, WORKGROUP_SIZE,
};
import package::bvh::triangle_corners;
import package::field::facing;
import package::math::{cross3, F32_MAX};

/// `FLAG_REGION_POINT_OWNER` value of a position no triangle uses yet.
const NO_OWNER: u32 = 0xffffffffu;

/// Returns the global welded index of corner `k` of request `r`'s triangle
/// slot `slot`.
fn corner_position_id(r: u32, slot: u32, k: u32) -> u32 {
    let base = requests[r].vertex_base;
    let v = base + arena[region(REGION_CORNERS) + 3u * slot + k];
    return arena[region(REGION_WELD_ID) + v] + arena[region(REGION_WELD_RANK) + base];
}

/// Returns whether request `r` grows a layer, and triangle slot `slot` is
/// one of its input triangles.
fn layer_triangle(r: u32, slot: u32) -> bool {
    let request = requests[r];
    return request.mode == MODE_EVERYWHERE
        && slot - request.triangle_base < min(counter(r, COUNTER_TRIANGLES), request.triangle_capacity);
}

/// Clears the first-use record of every welded position. One item per
/// vertex slot (welded positions never outnumber them).
@compute @workgroup_size(WORKGROUP_SIZE)
fn patch_reset(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot < batch.total_vertices {
        atomicStore(&flags[region(FLAG_REGION_POINT_OWNER) + slot], NO_OWNER);
    }
}

/// Flags each layer input triangle that faces some direction at a corner
/// and doesn't collapse once copies of a position are merged. Dispatch
/// T + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn facing_keep(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_triangles {
        return;
    }
    var keep = 0u;
    if slot < batch.total_triangles {
        let r = request_of_triangle(slot);
        if layer_triangle(r, slot) {
            let ids = vec3<u32>(
                corner_position_id(r, slot, 0u),
                corner_position_id(r, slot, 1u),
                corner_position_id(r, slot, 2u),
            );
            let collapsed = ids.x == ids.y || ids.y == ids.z || ids.z == ids.x;
            var faces = false;
            for (var k = 0u; k < 3u; k++) {
                let normal = load3(region(REGION_WELDED_NORMALS) + 3u * ids[k]);
                faces = faces || facing(requests[r].field, normal) > 0.0;
            }
            keep = select(0u, 1u, !collapsed && faces);
        }
    }
    arena[region(REGION_FACING_KEEP) + slot] = keep;
}

/// Records, per welded position, the first kept corner slot that uses it.
@compute @workgroup_size(WORKGROUP_SIZE)
fn point_owner(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_triangles || arena[region(REGION_FACING_KEEP) + slot] == 0u {
        return;
    }
    let r = request_of_triangle(slot);
    for (var k = 0u; k < 3u; k++) {
        atomicMin(&flags[region(FLAG_REGION_POINT_OWNER) + corner_position_id(r, slot, k)], 3u * slot + k);
    }
}

/// Flags each corner slot that is the first use of its position: these
/// become the working surface's points, numbered in corner order. Dispatch
/// E + 1 items, before the kept triangles are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn corner_owner(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    let edges = 3u * batch.total_triangles;
    if e > edges {
        return;
    }
    var owner = 0u;
    if e < edges && arena[region(REGION_FACING_KEEP) + e / 3u] != 0u {
        let r = request_of_triangle(e / 3u);
        let id = corner_position_id(r, e / 3u, e % 3u);
        owner = select(0u, 1u, atomicLoad(&flags[region(FLAG_REGION_POINT_OWNER) + id]) == e);
    }
    arena[region(REGION_CORNER_OWNER) + e] = owner;
}

/// Writes each point: its input position and its position's smooth normal,
/// fully exposed with unlimited room. Records the point of each position.
@compute @workgroup_size(WORKGROUP_SIZE)
fn patch_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    let owners = region(REGION_CORNER_OWNER);
    if e >= 3u * batch.total_triangles || arena[owners + e + 1u] == arena[owners + e] {
        return;
    }
    let t = e / 3u;
    let r = request_of_triangle(t);
    let request = requests[r];
    let point = arena[owners + e] - arena[owners + 3u * request.triangle_base];
    let id = corner_position_id(r, t, e % 3u);
    let v = request.vertex_base + arena[region(REGION_CORNERS) + e];
    let at = region(REGION_PATCH_POINTS) + POINT_WORDS * (request.vertex_base + point);
    store3(at, load3(region(REGION_POSITIONS) + 3u * v));
    store3(at + 3u, load3(region(REGION_WELDED_NORMALS) + 3u * id));
    for (var d = 0u; d < 8u; d++) {
        arena[at + 6u + d] = bitcast<u32>(1.0);
    }
    arena[at + 14u] = bitcast<u32>(F32_MAX);
    arena[at + 15u] = 0u;
    arena[at + 16u] = 0u;
    arena[region(REGION_POINT_OF_ID) + id] = point;
}

/// Writes each kept triangle: its corners as points, and its input face
/// normal (not normalized). After the kept triangles are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn patch_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let keep = region(REGION_FACING_KEEP);
    if slot >= batch.total_triangles || arena[keep + slot + 1u] == arena[keep + slot] {
        return;
    }
    let r = request_of_triangle(slot);
    let request = requests[r];
    let tri = request.triangle_base + arena[keep + slot] - arena[keep + request.triangle_base];
    let at = region(REGION_PATCH_TRIS) + PATCH_TRI_WORDS * tri;
    for (var k = 0u; k < 3u; k++) {
        arena[at + k] = arena[region(REGION_POINT_OF_ID) + corner_position_id(r, slot, k)];
    }
    let c = triangle_corners(r, slot);
    store3(at + 3u, cross3(c[1] - c[0], c[2] - c[0]));
}

/// Records each layer request's numbers of points and triangles.
@compute @workgroup_size(WORKGROUP_SIZE)
fn patch_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_EVERYWHERE {
        return;
    }
    let request = requests[r];
    let owners = region(REGION_CORNER_OWNER);
    let keep = region(REGION_FACING_KEEP);
    let end = request.triangle_base + request.triangle_capacity;
    let points = arena[owners + 3u * end] - arena[owners + 3u * request.triangle_base];
    let triangles = arena[keep + end] - arena[keep + request.triangle_base];
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_POINTS], points);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_PATCH_TRIANGLES], triangles);
}
