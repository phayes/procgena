// Reads one request's input mesh into the batch's arrays. These passes run
// once per request, with that request's input buffers bound; every later
// pass covers the whole batch.
//
// Run `ingest_triangles` over the request's triangle capacity, then
// `ingest_vertices` over its vertex capacity.

import package::batch::{
    arena, counters, requests, region, item_index, store3, counter,
    COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_VERTICES, COUNTER_OVERFLOW,
    OVERFLOW_INPUT, REGION_CORNERS, REGION_POSITIONS, REGION_NORMALS, WORKGROUP_SIZE,
};

/// Which request this dispatch reads.
struct IngestParams {
    request: u32,
}

@group(1) @binding(0) var<storage, read> input_vertices: array<u32>;
@group(1) @binding(1) var<storage, read> input_indices: array<u32>;
@group(1) @binding(2) var<storage, read> input_draw_args: array<u32>;
@group(1) @binding(3) var<uniform> ingest: IngestParams;

/// Copies each input triangle's corners (as indices relative to the input's
/// base vertex) and records the triangle count and the vertices used.
/// Dispatch one item per triangle of capacity.
@compute @workgroup_size(WORKGROUP_SIZE)
fn ingest_triangles(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = ingest.request;
    let request = requests[r];
    let t = item_index(global_id, num_workgroups);
    if t >= request.triangle_capacity {
        return;
    }

    // The input's draw arguments: index count, instances, first index, base vertex.
    let index_count = input_draw_args[request.draw_args_word];
    let first_index = input_draw_args[request.draw_args_word + 2u];
    let wanted = index_count / 3u;
    if t == 0u {
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_TRIANGLES], min(wanted, request.triangle_capacity));
        if wanted > request.triangle_capacity {
            atomicOr(&counters[r * COUNTER_STRIDE + COUNTER_OVERFLOW], OVERFLOW_INPUT);
        }
    }
    if t >= wanted {
        return;
    }

    let at = region(REGION_CORNERS) + 3u * (request.triangle_base + t);
    for (var k = 0u; k < 3u; k++) {
        let index = input_indices[first_index + 3u * t + k];
        arena[at + k] = index;
        atomicMax(&counters[r * COUNTER_STRIDE + COUNTER_VERTICES], index + 1u);
    }
}

/// Copies each used input vertex, transformed into clump space. Dispatch one
/// item per vertex of capacity, after `ingest_triangles`.
@compute @workgroup_size(WORKGROUP_SIZE)
fn ingest_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = ingest.request;
    let request = requests[r];
    let v = item_index(global_id, num_workgroups);
    let used = counter(r, COUNTER_VERTICES);
    if v == 0u && used > request.vertex_capacity {
        atomicOr(&counters[r * COUNTER_STRIDE + COUNTER_OVERFLOW], OVERFLOW_INPUT);
    }
    if v >= min(used, request.vertex_capacity) {
        return;
    }

    // The vertex's words in the input, counted from the base vertex.
    let base_vertex = bitcast<i32>(input_draw_args[request.draw_args_word + 3u]);
    let first = u32(i32(v) + base_vertex) * (request.input_stride / 4u);
    let p_at = first + request.position_offset / 4u;
    let n_at = first + request.normal_offset / 4u;
    let p = vec3<f32>(
        bitcast<f32>(input_vertices[p_at]),
        bitcast<f32>(input_vertices[p_at + 1u]),
        bitcast<f32>(input_vertices[p_at + 2u]),
    );
    let n = vec3<f32>(
        bitcast<f32>(input_vertices[n_at]),
        bitcast<f32>(input_vertices[n_at + 1u]),
        bitcast<f32>(input_vertices[n_at + 2u]),
    );

    // Place it: rows of the affine transform, and of the normal matrix.
    let m = request.transform;
    let c = request.normal_transform;
    let placed = vec3<f32>(
        m[0].x * p.x + m[0].y * p.y + m[0].z * p.z + m[0].w,
        m[1].x * p.x + m[1].y * p.y + m[1].z * p.z + m[1].w,
        m[2].x * p.x + m[2].y * p.y + m[2].z * p.z + m[2].w,
    );
    let turned = vec3<f32>(
        c[0].x * n.x + c[0].y * n.y + c[0].z * n.z,
        c[1].x * n.x + c[1].y * n.y + c[1].z * n.z,
        c[2].x * n.x + c[2].y * n.y + c[2].z * n.z,
    );
    let slot = request.vertex_base + v;
    store3(region(REGION_POSITIONS) + 3u * slot, placed);
    store3(region(REGION_NORMALS) + 3u * slot, turned);
}
