// Writes each request's finished clump mesh, draw arguments and status.
//
// Pass order, after the used vertices (W + 1) and kept triangles (Wt + 1)
// are scanned: `output_vertices` (W), `output_indices` (Wt),
// `output_finish` (requests).

import package::batch::{
    arena, batch, counters, requests, region, item_index, load3, counter,
    COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_VERTICES, COUNTER_WELDED, COUNTER_CLOSED,
    COUNTER_FOLDED, COUNTER_THINNED, COUNTER_SLOPE_DONE, COUNTER_SHEET_FAILED,
    COUNTER_ROUNDS_CAPPED, MODE_EVERYWHERE,
    COUNTER_CLUMPS, COUNTER_WORK_VERTICES, COUNTER_WORK_TRIANGLES, COUNTER_OUT_VERTICES,
    COUNTER_OUT_INDICES, COUNTER_OVERFLOW, REGION_WORK_POSITIONS, REGION_WORK_FADE,
    REGION_WORK_CORNERS, REGION_WORK_USED, REGION_WORK_KEPT, REGION_WORK_NORMALS,
    WORKGROUP_SIZE,
};
import package::batch::{request_of_work_vertex, request_of_work_triangle};

/// Floats per output vertex: position (3), normal (3), fade.
const VERTEX_FLOATS: u32 = 7u;
/// Words per request of `status`.
const STATUS_WORDS: u32 = 16u;

@group(1) @binding(0) var<storage, read_write> out_vertices: array<f32>;
@group(1) @binding(1) var<storage, read_write> out_indices: array<u32>;
@group(1) @binding(2) var<storage, read_write> draw_args: array<u32>;
@group(1) @binding(3) var<storage, read_write> status: array<u32>;

/// Copies each used working vertex to its request's output.
@compute @workgroup_size(WORKGROUP_SIZE)
fn output_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let used = region(REGION_WORK_USED);
    if arena[used + slot + 1u] == arena[used + slot] {
        return;
    }
    let base = requests[request_of_work_vertex(slot)].out_vertex_base;
    let at = VERTEX_FLOATS * (base + arena[used + slot] - arena[used + base]);
    let p = load3(region(REGION_WORK_POSITIONS) + 3u * slot);
    let n = load3(region(REGION_WORK_NORMALS) + 3u * slot);
    out_vertices[at] = p.x;
    out_vertices[at + 1u] = p.y;
    out_vertices[at + 2u] = p.z;
    out_vertices[at + 3u] = n.x;
    out_vertices[at + 4u] = n.y;
    out_vertices[at + 5u] = n.z;
    out_vertices[at + 6u] = bitcast<f32>(arena[region(REGION_WORK_FADE) + slot]);
}

/// Copies each kept working triangle to its request's output, with indices
/// relative to the request's first output vertex.
@compute @workgroup_size(WORKGROUP_SIZE)
fn output_indices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_triangles {
        return;
    }
    let kept = region(REGION_WORK_KEPT);
    if arena[kept + slot + 1u] == arena[kept + slot] {
        return;
    }
    let request = requests[request_of_work_triangle(slot)];
    let first = request.out_index_base / 3u;
    let at = request.out_index_base + 3u * (arena[kept + slot] - arena[kept + first]);
    let used = region(REGION_WORK_USED);
    let corners = region(REGION_WORK_CORNERS) + 3u * slot;
    for (var k = 0u; k < 3u; k++) {
        out_indices[at + k] = arena[used + arena[corners + k]] - arena[used + request.out_vertex_base];
    }
}

/// Writes each request's draw arguments and status. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn output_finish(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count {
        return;
    }
    let request = requests[r];
    let used = region(REGION_WORK_USED) + request.out_vertex_base;
    let kept = region(REGION_WORK_KEPT) + request.out_index_base / 3u;
    let vertices = arena[used + request.out_vertex_capacity] - arena[used];
    let indices = 3u * (arena[kept + request.out_index_capacity / 3u] - arena[kept]);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_OUT_VERTICES], vertices);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_OUT_INDICES], indices);
    let overflow = counter(r, COUNTER_OVERFLOW);

    // One indexed draw; a request that overflowed draws nothing.
    let d = 5u * r;
    draw_args[d] = select(indices, 0u, overflow != 0u);
    draw_args[d + 1u] = 1u;
    draw_args[d + 2u] = request.out_index_base;
    draw_args[d + 3u] = request.out_vertex_base;
    draw_args[d + 4u] = 0u;

    // Status, as `GpuClumpStatus::decode` reads it.
    let s = STATUS_WORDS * r;
    status[s] = overflow;
    status[s + 1u] = vertices;
    status[s + 2u] = indices;
    status[s + 3u] = counter(r, COUNTER_WORK_VERTICES);
    status[s + 4u] = 3u * counter(r, COUNTER_WORK_TRIANGLES);
    status[s + 5u] = counter(r, COUNTER_CLUMPS);
    status[s + 6u] = counter(r, COUNTER_TRIANGLES);
    status[s + 7u] = counter(r, COUNTER_VERTICES);
    status[s + 8u] = counter(r, COUNTER_WELDED);
    status[s + 9u] = counter(r, COUNTER_CLOSED);
    status[s + 10u] = counter(r, COUNTER_FOLDED);
    status[s + 11u] = counter(r, COUNTER_THINNED);
    // A layer's slope passes either ran until nothing changed, or didn't
    // run because the sheet didn't fit.
    let layer = request.mode == MODE_EVERYWHERE;
    let settled = counter(r, COUNTER_SLOPE_DONE) != 0u || counter(r, COUNTER_SHEET_FAILED) != 0u;
    status[s + 12u] = select(0u, 1u, layer && !settled);
    status[s + 13u] = counter(r, COUNTER_ROUNDS_CAPPED);
    for (var k = 14u; k < STATUS_WORDS; k++) {
        status[s + k] = 0u;
    }
}
