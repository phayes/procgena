// Finishes each layer request's working surface, matching the end of the
// Rust crate's `Patch::new`: records how far the input rises above and dips
// below the simplified surface, measures each point, and keeps only the
// triangles a clump may cover. Needs `simplify.wesl` and the simplified
// surface's BVH.
//
// Pass order, with V vertex slots and T triangle slots: `envelope_reset`
// (V), `envelope_fine` (V), `envelope_store` (V), `measure_points` (V),
// `cover_flags` (T + 1), scan the cover flags (T + 1), `cover_tris` (T),
// `cover_counts` (requests).
//
// The finished surface is the simplified points (`REGION_COARSE_POINTS`,
// now measured) and the covered triangles (`REGION_LAYER_TRIS`).

import package::batch::{
    arena, batch, counters, flags, requests, region, item_index, load3, counter,
    request_of_vertex, request_of_triangle, COUNTER_STRIDE, COUNTER_POINTS, COUNTER_COLLAPSED,
    COUNTER_COARSE_POINTS, COUNTER_COARSE_TRIANGLES, COUNTER_LAYER_TRIANGLES, COUNTER_MAX_RISE,
    MODE_EVERYWHERE,
    REGION_FINE_POINTS, REGION_COARSE_POINTS, REGION_COARSE_TRIS, REGION_COVER_FLAGS,
    REGION_LAYER_TRIS, FLAG_REGION_RISE, FLAG_REGION_DIP, POINT_WORDS, PATCH_TRI_WORDS,
    WORKGROUP_SIZE,
};
import package::bvh::{closest_point_in, coarse_mesh, corners_in};
import package::field::{Point, mask, MAX_DIRECTIONS};
import package::math::{cross3, dot3, normalize_or_zero};
import package::measure::{exposure, room};

/// Returns whether point slot `slot` is one of request `r`'s first `count`
/// points of a layer.
fn layer_point(r: u32, slot: u32, count: u32) -> bool {
    return requests[r].mode == MODE_EVERYWHERE && slot - requests[r].vertex_base < count;
}

/// Clears every point's rise and dip.
@compute @workgroup_size(WORKGROUP_SIZE)
fn envelope_reset(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot < batch.total_vertices {
        atomicStore(&flags[region(FLAG_REGION_RISE) + slot], 0u);
        atomicStore(&flags[region(FLAG_REGION_DIP) + slot], 0u);
    }
}

/// Matches each point of the surface before simplifying to the closest
/// simplified triangle facing the same way, and raises that triangle's
/// corners' rise (or dip) to the point's height above (or below) it. Only
/// for requests where something merged.
@compute @workgroup_size(WORKGROUP_SIZE)
fn envelope_fine(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let r = request_of_vertex(slot);
    if !layer_point(r, slot, counter(r, COUNTER_POINTS)) || counter(r, COUNTER_COLLAPSED) == 0u {
        return;
    }
    let q = load3(region(REGION_FINE_POINTS) + 6u * slot);
    let n = load3(region(REGION_FINE_POINTS) + 6u * slot + 3u);
    let m = coarse_mesh();
    let closest = closest_point_in(m, r, q);
    let c = corners_in(m, r, closest.slot);
    let face = normalize_or_zero(cross3(c[1] - c[0], c[2] - c[0]));
    if dot3(face, n) <= 0.0 {
        return;
    }

    // Heights of 0 or less change nothing: rise and dip start at 0.
    let h = dot3(q - closest.point, face);
    let side = select(FLAG_REGION_DIP, FLAG_REGION_RISE, h > 0.0);
    let height = abs(h);
    if height > 0.0 {
        let tri = region(REGION_COARSE_TRIS) + PATCH_TRI_WORDS * closest.slot;
        for (var k = 0u; k < 3u; k++) {
            let corner = requests[r].vertex_base + arena[tri + k];
            // Non-negative floats order like their bits.
            atomicMax(&flags[region(side) + corner], bitcast<u32>(height));
        }
    }
}

/// Copies each simplified point's rise and dip into it, and raises its
/// request's largest rise to match.
@compute @workgroup_size(WORKGROUP_SIZE)
fn envelope_store(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let r = request_of_vertex(slot);
    if !layer_point(r, slot, counter(r, COUNTER_COARSE_POINTS)) {
        return;
    }
    let at = region(REGION_COARSE_POINTS) + POINT_WORDS * slot;
    let rise = atomicLoad(&flags[region(FLAG_REGION_RISE) + slot]);
    arena[at + 15u] = rise;
    arena[at + 16u] = atomicLoad(&flags[region(FLAG_REGION_DIP) + slot]);
    // Non-negative floats order like their bits.
    atomicMax(&counters[r * COUNTER_STRIDE + COUNTER_MAX_RISE], rise);
}

/// Returns simplified point slot `slot` as a `Point`.
fn coarse_point_at(slot: u32) -> Point {
    let at = region(REGION_COARSE_POINTS) + POINT_WORDS * slot;
    var exposure_values: array<f32, MAX_DIRECTIONS>;
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        exposure_values[d] = bitcast<f32>(arena[at + 6u + d]);
    }
    return Point(
        load3(at),
        load3(at + 3u),
        exposure_values,
        bitcast<f32>(arena[at + 14u]),
        bitcast<f32>(arena[at + 15u]),
        bitcast<f32>(arena[at + 16u]),
    );
}

/// Measures each simplified point's exposure and room against the input.
@compute @workgroup_size(WORKGROUP_SIZE)
fn measure_points(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let r = request_of_vertex(slot);
    if !layer_point(r, slot, counter(r, COUNTER_COARSE_POINTS)) {
        return;
    }
    let at = region(REGION_COARSE_POINTS) + POINT_WORDS * slot;
    let p = load3(at);
    let n = load3(at + 3u);
    var open = exposure(r, p, n);
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        arena[at + 6u + d] = bitcast<u32>(open[d]);
    }
    arena[at + 14u] = bitcast<u32>(room(r, p, n));
}

/// Flags each simplified triangle a clump may cover: one with a corner whose
/// mask is above 0. Dispatch T + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn cover_flags(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_triangles {
        return;
    }
    var covered = false;
    if slot < batch.total_triangles {
        let r = request_of_triangle(slot);
        let request = requests[r];
        if request.mode == MODE_EVERYWHERE && slot - request.triangle_base < counter(r, COUNTER_COARSE_TRIANGLES) {
            let tri = region(REGION_COARSE_TRIS) + PATCH_TRI_WORDS * slot;
            for (var k = 0u; k < 3u; k++) {
                let point = coarse_point_at(request.vertex_base + arena[tri + k]);
                covered = covered || mask(request.field, point) > 0.0;
            }
        }
    }
    arena[region(REGION_COVER_FLAGS) + slot] = select(0u, 1u, covered);
}

/// Copies each covered triangle, in order, to the final surface.
@compute @workgroup_size(WORKGROUP_SIZE)
fn cover_tris(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let cover = region(REGION_COVER_FLAGS);
    if slot >= batch.total_triangles || arena[cover + slot + 1u] == arena[cover + slot] {
        return;
    }
    let r = request_of_triangle(slot);
    let base = requests[r].triangle_base;
    let to = region(REGION_LAYER_TRIS) + PATCH_TRI_WORDS * (base + arena[cover + slot] - arena[cover + base]);
    let source = region(REGION_COARSE_TRIS) + PATCH_TRI_WORDS * slot;
    for (var k = 0u; k < PATCH_TRI_WORDS; k++) {
        arena[to + k] = arena[source + k];
    }
}

/// Records each layer request's final triangle count.
@compute @workgroup_size(WORKGROUP_SIZE)
fn cover_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_EVERYWHERE {
        return;
    }
    let request = requests[r];
    let cover = region(REGION_COVER_FLAGS);
    let end = request.triangle_base + request.triangle_capacity;
    atomicStore(
        &counters[r * COUNTER_STRIDE + COUNTER_LAYER_TRIANGLES],
        arena[cover + end] - arena[cover + request.triangle_base],
    );
}
