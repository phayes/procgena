// Measures points on a request's input, matching `ClumpField::measure` and
// the ray tests it uses. Needs the prepare stage and the BVH.
//
// Run `measure_bounds` (V items) then `measure_init` (one per request)
// before any pass calls `measure` or `room`.
//
// Unbounded distances are `F32_MAX` here rather than infinity, because GPU
// fast math may assume no float is infinite.

import package::batch::{
    arena, batch, counters, requests, region, item_index, load3, counter,
    request_of_vertex, COUNTER_STRIDE, COUNTER_VERTICES, COUNTER_OPEN_EDGES,
    COUNTER_VERTEX_MIN, COUNTER_VERTEX_MAX, COUNTER_EPSILON, COUNTER_CLOSED,
    REGION_POSITIONS, REGION_RAYS, WORKGROUP_SIZE,
};
import package::bvh::{ray_hit, ray_blocked, triangle_corners, order_bits, from_order_bits, LEAF};
import package::field::{Point, MAX_DIRECTIONS};
import package::math::{cross3, dot3, length3, F32_MAX};

/// Grows each request's vertex bounds to hold its used input vertices.
@compute @workgroup_size(WORKGROUP_SIZE)
fn measure_bounds(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let r = request_of_vertex(slot);
    let local = slot - requests[r].vertex_base;
    if local >= min(counter(r, COUNTER_VERTICES), requests[r].vertex_capacity) {
        return;
    }
    let p = load3(region(REGION_POSITIONS) + 3u * slot);
    for (var k = 0u; k < 3u; k++) {
        atomicMin(&counters[r * COUNTER_STRIDE + COUNTER_VERTEX_MIN + k], order_bits(p[k]));
        atomicMax(&counters[r * COUNTER_STRIDE + COUNTER_VERTEX_MAX + k], order_bits(p[k]));
    }
}

/// Records each request's ray step and closedness. One item per request.
/// The vertex-bounds counters must start at `0xffffffff` (min) and 0 (max).
@compute @workgroup_size(WORKGROUP_SIZE)
fn measure_init(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count {
        return;
    }
    var lo = vec3<f32>(0.0);
    var hi = vec3<f32>(0.0);
    for (var k = 0u; k < 3u; k++) {
        lo[k] = from_order_bits(counter(r, COUNTER_VERTEX_MIN + k));
        hi[k] = from_order_bits(counter(r, COUNTER_VERTEX_MAX + k));
    }
    // Small enough to stay inside fine features, large enough to step off
    // a triangle in f32.
    let epsilon = max(length3(hi - lo), 1e-6) * 1e-6;
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_EPSILON], bitcast<u32>(epsilon));
    let fact = requests[r].closed_fact;
    let closed = fact == 2u || (fact == 0u && counter(r, COUNTER_OPEN_EDGES) == 0u);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_CLOSED], select(0u, 1u, closed));
}

/// Returns request `r`'s ray step.
fn epsilon_of(r: u32) -> f32 {
    return bitcast<f32>(counter(r, COUNTER_EPSILON));
}

/// Returns whether a ray along `dir` crosses triangle slot `slot` from
/// behind, leaving the solid.
fn leaves(r: u32, slot: u32, dir: vec3<f32>) -> bool {
    let c = triangle_corners(r, slot);
    return dot3(cross3(c[1] - c[0], c[2] - c[0]), dir) > 0.0;
}

/// Returns whether a ray from surface point `point` along unit `dir` first
/// meets a surface from behind, meaning `point` lies inside some part of
/// request `r`'s input.
fn exits_through(r: u32, point: vec3<f32>, dir: vec3<f32>) -> bool {
    let hit = ray_hit(r, point + dir * epsilon_of(r), dir, F32_MAX);
    return hit.slot != LEAF && leaves(r, hit.slot, dir);
}

/// The first stretch of a ray inside the input: where it enters and leaves.
/// `found` is false if the ray never gets inside.
struct Span {
    enter: f32,
    leave: f32,
    found: bool,
}

/// Returns the first stretch of the ray from `point` along unit `dir` that
/// lies inside request `r`'s input. `enter` is 0 when `point` is already
/// inside, or on the surface heading in. On an open input, a ray that is
/// inside and meets nothing leaves at `F32_MAX`.
fn inside_span(r: u32, point: vec3<f32>, dir: vec3<f32>) -> Span {
    let epsilon = epsilon_of(r);
    let open = counter(r, COUNTER_CLOSED) == 0u;
    let first = ray_hit(r, point + dir * epsilon, dir, F32_MAX);
    if first.slot == LEAF {
        return Span(0.0, F32_MAX, open);
    }
    let t = first.t + epsilon;
    if leaves(r, first.slot, dir) {
        return Span(0.0, t, true);
    }

    // The point is outside: go in through the surface just met.
    let entry = point + dir * t;
    let second = ray_hit(r, entry + dir * epsilon, dir, F32_MAX);
    if second.slot == LEAF {
        return Span(t, F32_MAX, open);
    }
    if leaves(r, second.slot, dir) {
        return Span(t, t + second.t + epsilon, true);
    }
    return Span(0.0, 0.0, false);
}

/// Returns how thick request `r`'s input is at surface point `p` with unit
/// normal `n`, straight in along the normal: 0 where the input can't carry
/// a clump, `F32_MAX` on an open input where nothing is met.
fn room(r: u32, p: vec3<f32>, n: vec3<f32>) -> f32 {
    if all(n == vec3<f32>(0.0)) {
        return F32_MAX;
    }
    if exits_through(r, p, n) {
        return 0.0;
    }
    let span = inside_span(r, p, -n);
    if span.found && span.enter == 0.0 {
        return span.leave;
    }
    return 0.0;
}

/// Returns how exposed point `p` with normal `n` is toward each of request
/// `r`'s directions, from 0 to 1: the share of its rays that escape. 1 for
/// every direction when exposure isn't measured.
fn exposure(r: u32, p: vec3<f32>, n: vec3<f32>) -> array<f32, MAX_DIRECTIONS> {
    var open: array<f32, MAX_DIRECTIONS>;
    for (var d = 0u; d < MAX_DIRECTIONS; d++) {
        open[d] = 1.0;
    }
    let rays = requests[r].ray_count;
    if rays == 0u {
        return open;
    }
    let origin = p + n * epsilon_of(r);
    let sources = requests[r].field.source_count;
    for (var d = 0u; d < sources; d++) {
        var escaped = 0u;
        for (var k = 0u; k < rays; k++) {
            let at = region(REGION_RAYS) + 3u * (requests[r].ray_base + d * rays + k);
            if !ray_blocked(r, origin, load3(at), F32_MAX) {
                escaped += 1u;
            }
        }
        open[d] = f32(escaped) / f32(rays);
    }
    return open;
}

/// Returns the point at `p` with normal `n` on request `r`'s input,
/// measured: its exposure and room, with no rise or dip.
fn measure(r: u32, p: vec3<f32>, n: vec3<f32>) -> Point {
    return Point(p, n, exposure(r, p, n), room(r, p, n), 0.0, 0.0);
}
