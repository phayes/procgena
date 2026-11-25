// Dual contouring passes. Each cell crossed by the surface gets one fitted
// vertex; each crossing grid edge joins its four surrounding cells with two
// triangles. Follows the Rust crate's `Mesher::DualContouring`.

import package::generate::{
    Vertex, cell_case, crossing_count, crossing_id, crossing_vertex, crosses, inside, is_cell,
    item_index, make_vertex, measure_bounds, place_vertex, sample_coords, sample_count,
    sample_index, sample_position, stone_gradient, vertex_normal, vertex_position, write_result,
    OVERFLOW_CROSSINGS, OVERFLOW_INDICES, OVERFLOW_VERTICES, WORKGROUP_SIZE, indices, prefix,
    request, vertices,
};
import package::field::normalize_or;
import package::tables::{MC_EDGE_AXES, MC_EDGE_OFFSETS};

/// Strength of the pull towards the average crossing position when fitting a
/// cell's vertex. Matches the Rust crate's dual contouring.
const REGULARIZATION: f32 = 0.05;

/// Surface crossings on grid edges, in crossing ID order. Needs room for
/// `request.crossing_capacity` entries.
@group(0) @binding(4) var<storage, read_write> crossings: array<Vertex>;

/// Solves `a * x = rhs` for an invertible `a`, rounding as glam's
/// `Mat3::inverse() * rhs` does.
fn solve3(a: mat3x3<f32>, rhs: vec3<f32>) -> vec3<f32> {
    let tmp0 = cross(a[1], a[2]);
    let tmp1 = cross(a[2], a[0]);
    let tmp2 = cross(a[0], a[1]);
    let inv_det = 1.0 / dot(a[0], tmp0);
    let inv = transpose(mat3x3<f32>(tmp0 * inv_det, tmp1 * inv_det, tmp2 * inv_det));
    return inv[0] * rhs.x + inv[1] * rhs.y + inv[2] * rhs.z;
}

/// Fits a vertex within cell `c` from its edge crossings, which must exist.
/// `points` and `normals` hold `count` crossings relative to the cell: its
/// minimum corner is zero and one unit is the cell width.
fn fit_vertex(points: array<vec3<f32>, 12>, normals: array<vec3<f32>, 12>, count: u32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var k = 0u; k < count; k++) {
        sum += points[k];
    }
    let mass = sum / f32(count);

    // Fit the planes through the crossings, pulled towards their average.
    var a = mat3x3<f32>(
        vec3<f32>(REGULARIZATION, 0.0, 0.0),
        vec3<f32>(0.0, REGULARIZATION, 0.0),
        vec3<f32>(0.0, 0.0, REGULARIZATION),
    );
    var rhs = vec3<f32>(0.0);
    for (var k = 0u; k < count; k++) {
        let n = normals[k];
        a += mat3x3<f32>(n * n.x, n * n.y, n * n.z);
        rhs += n * dot(n, points[k] - mass);
    }
    return clamp(mass + solve3(a, rhs), vec3<f32>(0.0), vec3<f32>(1.0));
}

/// Counts crossings and crossed cells per sample. Dispatch `sample_count() + 1` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn count_dc(
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
    var cell = 0u;
    if is_cell(s) {
        let case_index = cell_case(s);
        cell = select(0u, 1u, case_index != 0u && case_index != 255u);
    }
    prefix[i] = vec2<u32>(crossing_count(s), cell);
}

/// Writes every edge crossing to `crossings`. Run after the prefix sum;
/// dispatch `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn emit_dc_crossings(
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
            if id < request.crossing_capacity {
                crossings[id] = crossing_vertex(s, axis);
            }
            id += 1u;
        }
    }
}

/// Writes one fitted vertex per crossed cell. Run after `emit_dc_crossings`;
/// dispatch `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn emit_dc_vertices(
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
    if case_index == 0u || case_index == 255u {
        return;
    }
    let id = prefix[i].y;
    if id >= request.vertex_capacity {
        return;
    }

    // Gather the cell's crossings relative to the cell, in table edge order.
    let r = f32(request.resolution);
    let origin = sample_position(s);
    var points: array<vec3<f32>, 12>;
    var normals: array<vec3<f32>, 12>;
    var count = 0u;
    for (var e = 0u; e < 12u; e++) {
        let start = s + MC_EDGE_OFFSETS[e];
        let axis = MC_EDGE_AXES[e];
        if crosses(start, axis) {
            let cid = crossing_id(start, axis);
            if cid >= request.crossing_capacity {
                return;
            }
            let crossing = crossings[cid];
            points[count] = (vertex_position(crossing) - origin) * r;
            normals[count] = vertex_normal(crossing);
            count += 1u;
        }
    }

    let position = origin + fit_vertex(points, normals, count) / r;

    // Use the averaged crossing normals if the field gradient cannot be normalized.
    var normal_sum = vec3<f32>(0.0);
    for (var k = 0u; k < count; k++) {
        normal_sum += normals[k];
    }
    let average = normalize_or(normal_sum, normals[0]);
    let normal = normalize_or(stone_gradient(position, 0.5 / r), average);
    vertices[request.vertex_base + id] = place_vertex(make_vertex(position, normal));
}

/// Vertex ID of the crossed cell whose minimum corner is sample `c`.
fn cell_id(c: vec3<u32>) -> u32 {
    return prefix[sample_index(c)].y;
}

/// Returns `s` moved back one sample along `axis`.
fn back(s: vec3<u32>, axis: u32) -> vec3<u32> {
    var t = s;
    t[axis] -= 1u;
    return t;
}

/// Position of output vertex `id` of this request.
fn output_position(id: u32) -> vec3<f32> {
    return vertex_position(vertices[request.vertex_base + id]);
}

/// Writes two outward-facing triangles per crossing edge. Run after
/// `emit_dc_vertices`; dispatch `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn emit_dc_indices(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    if i >= sample_count() {
        return;
    }
    let s = sample_coords(i);
    var edge_id = prefix[i].x;
    for (var axis = 0u; axis < 3u; axis++) {
        if !crosses(s, axis) {
            continue;
        }
        let first = 6u * edge_id;
        edge_id += 1u;

        // Corners face the positive axis direction; reverse them when that
        // direction points into the rock.
        let b = (axis + 1u) % 3u;
        let c = (axis + 2u) % 3u;
        var quad = array<u32, 4>(
            cell_id(back(back(s, b), c)),
            cell_id(back(s, c)),
            cell_id(s),
            cell_id(back(s, b)),
        );
        if !inside(s) {
            quad = array<u32, 4>(quad[3], quad[2], quad[1], quad[0]);
        }
        if first + 6u > request.index_capacity || max(max(quad[0], quad[1]), max(quad[2], quad[3])) >= request.vertex_capacity {
            continue;
        }

        // Split along the shorter diagonal; ties use the 0-2 diagonal.
        let d02 = output_position(quad[0]) - output_position(quad[2]);
        let d13 = output_position(quad[1]) - output_position(quad[3]);
        var tri: array<u32, 6>;
        if dot(d02, d02) <= dot(d13, d13) {
            tri = array<u32, 6>(quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]);
        } else {
            tri = array<u32, 6>(quad[1], quad[2], quad[3], quad[1], quad[3], quad[0]);
        }
        for (var k = 0u; k < 6u; k++) {
            indices[request.index_base + first + k] = tri[k];
        }
    }
}

/// Writes the draw arguments and status. Run last; dispatch one item.
@compute @workgroup_size(1)
fn finish_dc() {
    let totals = prefix[sample_count()];
    let index_count = 6u * totals.x;
    var overflow = 0u;
    if totals.y > request.vertex_capacity {
        overflow |= OVERFLOW_VERTICES;
    }
    if index_count > request.index_capacity {
        overflow |= OVERFLOW_INDICES;
    }
    if totals.x > request.crossing_capacity {
        overflow |= OVERFLOW_CROSSINGS;
    }
    write_result(overflow, totals.y, index_count, totals.x);
}

/// Measures the mesh's bounds into its status. Run after `finish_dc`;
/// dispatch one workgroup.
@compute @workgroup_size(WORKGROUP_SIZE)
fn measure_dc(@builtin(local_invocation_index) local: u32) {
    measure_bounds(local);
}
