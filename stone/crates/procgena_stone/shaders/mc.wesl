// Marching cubes passes. Vertices sit on grid edges that cross the surface and
// are shared between neighbouring cells; each cell adds triangles from the
// triangle table. Follows the Rust crate's `Mesher::MarchingCubes`.

import package::generate::{
    cell_case, crossing_count, crossing_id, crossing_vertex, crosses, is_cell, item_index,
    place_vertex, sample_coords, sample_count, sample_index, write_result, OVERFLOW_INDICES,
    OVERFLOW_VERTICES, WORKGROUP_SIZE, indices, prefix, request, vertices,
};
import package::tables::{MC_EDGE_AXES, MC_EDGE_OFFSETS};

/// Triangle table, one entry per cell case. `x` holds edges 0 to 7 and `y`
/// edges 8 to 14 of the case's edge list, four bits each, lowest bits first;
/// `z` is the number of edges (three per triangle). Each edge is an index into
/// `MC_EDGE_OFFSETS` and `MC_EDGE_AXES`.
@group(0) @binding(1) var<uniform> tri_table: array<vec4<u32>, 256>;

/// Returns edge `k` of case `case_index`'s triangle list.
fn tri_edge(case_index: u32, k: u32) -> u32 {
    let entry = tri_table[case_index];
    let word = select(entry.y, entry.x, k < 8u);
    return (word >> (4u * (k % 8u))) & 15u;
}

/// Counts vertices and indices per sample. Dispatch `sample_count() + 1` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn count_mc(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    let n = sample_count();
    if i > n {
        return;
    }
    if i == n {
        prefix[i] = vec2<u32>(0u);
        return;
    }
    let s = sample_coords(i);
    var index_count = 0u;
    if is_cell(s) {
        index_count = tri_table[cell_case(s)].z;
    }
    prefix[i] = vec2<u32>(crossing_count(s), index_count);
}

/// Writes one vertex per crossing edge. Run after the prefix sum; dispatch
/// `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn emit_mc_vertices(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    if i >= sample_count() {
        return;
    }
    let s = sample_coords(i);
    var id = prefix[i].x;
    for (var axis = 0u; axis < 3u; axis++) {
        if crosses(s, axis) {
            if id < request.vertex_capacity {
                vertices[request.vertex_base + id] = place_vertex(crossing_vertex(s, axis));
            }
            id += 1u;
        }
    }
}

/// Writes each cell's triangles. Run after the prefix sum; dispatch
/// `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn emit_mc_indices(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    if i >= sample_count() {
        return;
    }
    let s = sample_coords(i);
    if !is_cell(s) {
        return;
    }
    let case_index = cell_case(s);
    let first = prefix[i].y;
    let count = tri_table[case_index].z;
    for (var k = 0u; k < count; k++) {
        let slot = first + k;
        if slot < request.index_capacity {
            let edge = tri_edge(case_index, k);
            let id = crossing_id(s + MC_EDGE_OFFSETS[edge], MC_EDGE_AXES[edge]);
            indices[request.index_base + slot] = id;
        }
    }
}

/// Writes the draw arguments and status. Run last; dispatch one item.
@compute @workgroup_size(1)
fn finish_mc() {
    let totals = prefix[sample_count()];
    var overflow = 0u;
    if totals.x > request.vertex_capacity {
        overflow |= OVERFLOW_VERTICES;
    }
    if totals.y > request.index_capacity {
        overflow |= OVERFLOW_INDICES;
    }
    write_result(overflow, totals.x, totals.y, 0u);
}
