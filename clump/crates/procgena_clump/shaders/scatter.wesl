// Scattered clumps (`Placement::Scatter`), matching the Rust crate's `blob`
// module: separate rounded blobs, each a ring mesh partly sunk into the
// input. Needs the prepare stage, the BVH and `measure_init`.
//
// Pass order, with A attempt slots, S seed slots, W working vertex slots
// and Wt working triangle slots:
// 1. `scatter_weights` (requests), `scatter_attempts` (A),
//    `scatter_accept` (requests).
// 2. `blob_counts` (S + 1), scan the blob vertices and triangles (S + 1 each),
//    `blob_totals` (requests).
// 3. `blob_vertices` (W + 1), `blob_triangles` (Wt + 1), scan the used
//    vertices (W + 1) and kept triangles (Wt + 1), `blob_normals` (W).
// Then the output passes write each request's mesh.

import package::batch::{
    arena, batch, counters, requests, region, item_index, load3, store3, counter,
    request_of_work_vertex, request_of_work_triangle, flags, COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_SEEDS, COUNTER_WEIGHT_TOTAL,
    COUNTER_WORK_VERTICES, COUNTER_WORK_TRIANGLES, COUNTER_OVERFLOW, COUNTER_CLUMPS,
    REGION_BLOB_BOUNDS, REGION_BLOB_REJECTED,
    OVERFLOW_VERTICES, OVERFLOW_INDICES, MODE_SCATTER,
    REGION_POSITIONS, REGION_NORMALS, REGION_CORNERS, REGION_SCATTER_CUMULATIVE,
    REGION_SCATTER_FACING, REGION_ATTEMPTS, REGION_SEEDS, REGION_SEED_NEXT, REGION_GRID,
    REGION_BLOB_VERTICES, REGION_BLOB_TRIANGLES, REGION_WORK_POSITIONS, REGION_WORK_FADE,
    REGION_WORK_CORNERS, REGION_WORK_USED, REGION_WORK_KEPT, REGION_WORK_BURIED,
    REGION_WORK_NORMALS, WORKGROUP_SIZE,
};
import package::bvh::{closest_point, triangle_corners};
import package::field::{facing, mask, fade, lump_height};
import package::geometry::triangles_cross;
import package::hash::{pcg3d, rng_draw};
import package::math::{cross3, dot3, length3, normalize_or, normalize_or_zero};
import package::measure::measure;

/// Seed attempts per requested clump.
const ATTEMPTS_PER_CLUMP: u32 = 64u;
/// Smallest size factor a clump may get from size variation.
const MIN_SIZE: f32 = 0.05;
/// Extra depth of a blob's buried bottom below the sink, as a fraction of
/// its height.
const BOTTOM_GAP: f32 = 0.25;
/// Fraction of the input's thickness a blob's buried bottom may reach.
const GAP_FILL: f32 = 0.5;
/// Widest a blob may be, as a fraction of the input's thickness under it.
const ROOM_WIDTH: f32 = 0.5;
/// Fewest segments around a blob, and fewest rings from top to bottom.
const MIN_SEGMENTS: u32 = 4u;
const MIN_RINGS: u32 = 3u;
/// π and 2π in `f32`, as Rust rounds them.
const PI_F32: f32 = 3.1415927f;
const TAU_F32: f32 = 6.2831855f;

/// Salt mixed into the seed of the scatter's random stream.
const SCATTER_SALT: u32 = 0x0b10b5edu;

/// Words per attempt: point (3), normal (3), local triangle, size, accept
/// draw, the triangle's facing, mask, room.
const ATTEMPT_WORDS: u32 = 12u;
/// Words per seed: point (3), normal (3), radius, height, depth.
const SEED_WORDS: u32 = 9u;
/// Words per spacing-grid cell: cell coordinates (3), first seed (or
/// `NO_SEED` for an empty cell).
const GRID_WORDS: u32 = 4u;
const NO_SEED: u32 = 0xffffffffu;

/// Returns the request owning attempt slot `slot`.
fn request_of_attempt(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].attempt_base <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Returns the request owning seed slot `slot`.
fn request_of_seed(slot: u32) -> u32 {
    var lo = 0u;
    var hi = batch.request_count - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if requests[mid].seed_base <= slot {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    return lo;
}

/// Writes each triangle's running weight total (area × its largest corner
/// facing, summed in triangle order, as the CPU does) and its facing. One
/// item per request; requests that don't scatter do nothing.
@compute @workgroup_size(WORKGROUP_SIZE)
fn scatter_weights(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_SCATTER {
        return;
    }
    let request = requests[r];
    let n = min(counter(r, COUNTER_TRIANGLES), request.triangle_capacity);
    var total = 0.0;
    for (var t = 0u; t < n; t++) {
        let slot = request.triangle_base + t;
        let c = triangle_corners(r, slot);
        let area = 0.5 * length3(cross3(c[1] - c[0], c[2] - c[0]));
        var most = 0.0;
        for (var k = 0u; k < 3u; k++) {
            let v = request.vertex_base + arena[region(REGION_CORNERS) + 3u * slot + k];
            let normal = normalize_or_zero(load3(region(REGION_NORMALS) + 3u * v));
            most = max(most, facing(request.field, normal));
        }
        total += area * most;
        arena[region(REGION_SCATTER_CUMULATIVE) + slot] = bitcast<u32>(total);
        arena[region(REGION_SCATTER_FACING) + slot] = bitcast<u32>(most);
    }
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_WEIGHT_TOTAL], bitcast<u32>(total));
}

/// Draws and measures every seed attempt. One item per attempt slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn scatter_attempts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if batch.request_count == 0u {
        return;
    }
    let r = request_of_attempt(slot);
    let request = requests[r];
    let i = slot - request.attempt_base;
    if slot < request.attempt_base || i >= request.attempt_count || request.mode != MODE_SCATTER {
        return;
    }
    let total = bitcast<f32>(counter(r, COUNTER_WEIGHT_TOTAL));
    let n = min(counter(r, COUNTER_TRIANGLES), request.triangle_capacity);
    if total <= 0.0 || n == 0u {
        return;
    }

    // This attempt's five draws, in the CPU's order.
    let seed = request.field.seed ^ SCATTER_SALT;
    let pick = rng_draw(seed, 5u * i) * total;
    let r1 = sqrt(rng_draw(seed, 5u * i + 1u));
    let r2 = rng_draw(seed, 5u * i + 2u);
    let variation = request.size_variation;
    let lo = 1.0 - variation;
    let size = max(lo + ((1.0 + variation) - lo) * rng_draw(seed, 5u * i + 3u), MIN_SIZE);
    let accept = rng_draw(seed, 5u * i + 4u);

    // The first triangle whose running total passes the pick.
    let cumulative = region(REGION_SCATTER_CUMULATIVE) + request.triangle_base;
    var first = 0u;
    var count = n;
    while count > 0u {
        let half = count / 2u;
        if bitcast<f32>(arena[cumulative + first + half]) <= pick {
            first += half + 1u;
            count -= half + 1u;
        } else {
            count = half;
        }
    }
    let t = min(first, n - 1u);

    // A point on it, and the input's normal there.
    let tri = request.triangle_base + t;
    let c = triangle_corners(r, tri);
    let w = vec3<f32>(1.0 - r1, r1 * (1.0 - r2), r1 * r2);
    var point = vec3<f32>(0.0);
    var normal = vec3<f32>(0.0);
    for (var k = 0u; k < 3u; k++) {
        let v = request.vertex_base + arena[region(REGION_CORNERS) + 3u * tri + k];
        point += c[k] * w[k];
        normal += load3(region(REGION_NORMALS) + 3u * v) * w[k];
    }
    let face = normalize_or(cross3(c[1] - c[0], c[2] - c[0]), vec3<f32>(0.0, 1.0, 0.0));
    normal = normalize_or(normal, face);

    // Measure the spot.
    let here = measure(r, point, normal);
    let at = region(REGION_ATTEMPTS) + ATTEMPT_WORDS * slot;
    store3(at, point);
    store3(at + 3u, normal);
    arena[at + 6u] = t;
    arena[at + 7u] = bitcast<u32>(size);
    arena[at + 8u] = bitcast<u32>(accept);
    arena[at + 9u] = arena[region(REGION_SCATTER_FACING) + tri];
    arena[at + 10u] = bitcast<u32>(mask(request.field, here));
    arena[at + 11u] = bitcast<u32>(here.room);
}

/// Returns the spacing-grid slot of `cell` in request `r`'s grid: the cell
/// itself, or the empty slot where it would go.
fn grid_find(r: u32, cell: vec3<i32>) -> u32 {
    let request = requests[r];
    let mask_bits = request.grid_capacity - 1u;
    var slot = pcg3d(bitcast<vec3<u32>>(cell)).x & mask_bits;
    // The grid has room for twice the clumps, so a probe always ends at the
    // cell or an empty slot within the grid's size.
    for (var probe = 0u; probe < request.grid_capacity; probe++) {
        let at = region(REGION_GRID) + GRID_WORDS * (request.grid_base + slot);
        if arena[at + 3u] == NO_SEED {
            return slot;
        }
        if bitcast<i32>(arena[at]) == cell.x && bitcast<i32>(arena[at + 1u]) == cell.y
            && bitcast<i32>(arena[at + 2u]) == cell.z {
            return slot;
        }
        slot = (slot + 1u) & mask_bits;
    }
    return slot;
}

/// Returns the spacing-grid cell holding `p`, for cells `spacing` wide.
fn grid_cell(p: vec3<f32>, spacing: f32) -> vec3<i32> {
    return vec3<i32>(floor(clamp(p / spacing, vec3<f32>(-1e9), vec3<f32>(1e9))));
}

/// Returns whether any placed seed of request `r` lies closer than
/// `spacing` to `p`.
fn too_close(r: u32, p: vec3<f32>, spacing: f32) -> bool {
    let cell = grid_cell(p, spacing);
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let slot = grid_find(r, cell + vec3<i32>(dx, dy, dz));
                var s = arena[region(REGION_GRID) + GRID_WORDS * (requests[r].grid_base + slot) + 3u];
                while s != NO_SEED {
                    let q = load3(region(REGION_SEEDS) + SEED_WORDS * s);
                    if length3(q - p) < spacing {
                        return true;
                    }
                    s = arena[region(REGION_SEED_NEXT) + s];
                }
            }
        }
    }
    return false;
}

/// Accepts attempts in order, as the CPU does, until the requested number
/// of clumps is placed. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn scatter_accept(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_SCATTER {
        return;
    }
    let request = requests[r];
    let field = request.field;
    let total = bitcast<f32>(counter(r, COUNTER_WEIGHT_TOTAL));
    let n = min(counter(r, COUNTER_TRIANGLES), request.triangle_capacity);
    var seeds = 0u;
    if total > 0.0 && n > 0u {
        // An empty spacing grid.
        for (var cell = 0u; cell < request.grid_capacity; cell++) {
            arena[region(REGION_GRID) + GRID_WORDS * (request.grid_base + cell) + 3u] = NO_SEED;
        }
        let spacing = request.min_spacing;
        for (var i = 0u; i < request.attempt_count && seeds < request.seed_capacity; i++) {
            let at = region(REGION_ATTEMPTS) + ATTEMPT_WORDS * (request.attempt_base + i);
            let point = load3(at);
            if spacing > 0.0 && too_close(r, point, spacing) {
                continue;
            }

            // Keep it in proportion to its mask.
            let size = bitcast<f32>(arena[at + 7u]);
            let accept = bitcast<f32>(arena[at + 8u]);
            let most = bitcast<f32>(arena[at + 9u]);
            let full = bitcast<f32>(arena[at + 10u]);
            let room = bitcast<f32>(arena[at + 11u]);
            if full <= 0.0 || accept * most >= full {
                continue;
            }
            let height = field.thickness * size * min(full, 1.0);
            let s = request.seed_base + seeds;
            let seed_at = region(REGION_SEEDS) + SEED_WORDS * s;
            store3(seed_at, point);
            store3(seed_at + 3u, load3(at + 3u));
            arena[seed_at + 6u] = bitcast<u32>(min(request.clump_radius * size, ROOM_WIDTH * room));
            arena[seed_at + 7u] = bitcast<u32>(height);
            arena[seed_at + 8u] = bitcast<u32>(min(field.sink * size + BOTTOM_GAP * height, GAP_FILL * room));
            seeds += 1u;

            // File it in the spacing grid.
            if spacing > 0.0 {
                let cell = grid_cell(point, spacing);
                let slot = grid_find(r, cell);
                let cell_at = region(REGION_GRID) + GRID_WORDS * (request.grid_base + slot);
                arena[cell_at] = bitcast<u32>(cell.x);
                arena[cell_at + 1u] = bitcast<u32>(cell.y);
                arena[cell_at + 2u] = bitcast<u32>(cell.z);
                arena[region(REGION_SEED_NEXT) + s] = arena[cell_at + 3u];
                arena[cell_at + 3u] = s;
            }
        }
    }
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_SEEDS], seeds);
}

/// A blob's ring counts: segments around, rings from top to bottom.
fn blob_rings(seed_at: u32, edge_length: f32) -> vec2<u32> {
    let radius = bitcast<f32>(arena[seed_at + 6u]);
    let height = bitcast<f32>(arena[seed_at + 7u]);
    let depth = bitcast<f32>(arena[seed_at + 8u]);
    let segments = max(u32(ceil(TAU_F32 * radius / edge_length)), MIN_SEGMENTS);
    let meridian = 0.5 * PI_F32 * (radius + 0.5 * (height + depth));
    let rings = max(u32(ceil(meridian / edge_length)), MIN_RINGS);
    return vec2<u32>(segments, rings);
}

/// Writes each seed's blob vertex and triangle counts (0 for unused seed
/// slots). Dispatch S + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn blob_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot > batch.total_seeds {
        return;
    }
    var vertices = 0u;
    var triangles = 0u;
    if slot < batch.total_seeds {
        let r = request_of_seed(slot);
        if slot - requests[r].seed_base < counter(r, COUNTER_SEEDS) {
            let rings = blob_rings(region(REGION_SEEDS) + SEED_WORDS * slot, requests[r].edge_length);
            vertices = 2u + (rings.y - 1u) * rings.x;
            triangles = 2u * rings.x * (rings.y - 1u);
        }
    }
    arena[region(REGION_BLOB_VERTICES) + slot] = vertices;
    arena[region(REGION_BLOB_TRIANGLES) + slot] = triangles;
}

/// Returns where request `r`'s blobs start in the scanned counts at
/// `counts` (a region index), as `(first seed's offset, total)`.
fn blob_span(r: u32, counts: u32) -> vec2<u32> {
    let at = region(counts) + requests[r].seed_base;
    let first = arena[at];
    return vec2<u32>(first, arena[at + counter(r, COUNTER_SEEDS)] - first);
}

/// Records how many working vertices and triangles each request's blobs
/// need, and flags requests whose output can't hold them. One item per
/// request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn blob_totals(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || requests[r].mode != MODE_SCATTER {
        return;
    }
    let vertices = blob_span(r, REGION_BLOB_VERTICES).y;
    let triangles = blob_span(r, REGION_BLOB_TRIANGLES).y;
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_WORK_VERTICES], vertices);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_WORK_TRIANGLES], triangles);
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_CLUMPS], counter(r, COUNTER_SEEDS));
    var overflow = 0u;
    if vertices > requests[r].out_vertex_capacity {
        overflow |= OVERFLOW_VERTICES;
    }
    if triangles > requests[r].out_index_capacity / 3u {
        overflow |= OVERFLOW_INDICES;
    }
    atomicOr(&counters[r * COUNTER_STRIDE + COUNTER_OVERFLOW], overflow);
}

/// Returns whether request `r`'s clump mesh doesn't fit its output.
fn output_overflows(r: u32) -> bool {
    return (counter(r, COUNTER_OVERFLOW) & (OVERFLOW_VERTICES | OVERFLOW_INDICES)) != 0u;
}

/// Returns the seed of request `r` whose blob holds item `local` of the
/// scanned counts at `counts`, as `(seed slot, item within the blob, the
/// blob's first item)`.
fn blob_of(r: u32, counts: u32, local: u32) -> vec3<u32> {
    let base = region(counts) + requests[r].seed_base;
    let first = arena[base];
    var lo = 0u;
    var hi = counter(r, COUNTER_SEEDS) - 1u;
    while lo < hi {
        let mid = (lo + hi + 1u) / 2u;
        if arena[base + mid] - first <= local {
            lo = mid;
        } else {
            hi = mid - 1u;
        }
    }
    let start = arena[base + lo] - first;
    return vec3<u32>(requests[r].seed_base + lo, local - start, start);
}

/// Returns a unit vector perpendicular to unit vector `n`, like glam's
/// `any_orthonormal_vector`.
fn any_orthonormal(n: vec3<f32>) -> vec3<f32> {
    // The sign of z by its sign bit, so -0.0 counts as negative.
    let sign = select(1.0, -1.0, (bitcast<u32>(n.z) & 0x80000000u) != 0u);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    return vec3<f32>(b, sign + n.y * n.y * a, -n.y);
}

/// Returns the direction of vertex `q` of a blob with `rings`
/// (segments, rings) on the unit sphere: the top pole, the rings, then the
/// bottom pole.
fn blob_direction(q: u32, rings: vec2<u32>) -> vec3<f32> {
    let last = 1u + (rings.y - 1u) * rings.x;
    if q == 0u {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    if q == last {
        return vec3<f32>(0.0, -1.0, 0.0);
    }
    let i = 1u + (q - 1u) / rings.x;
    let j = (q - 1u) % rings.x;
    let theta = PI_F32 * f32(i) / f32(rings.y);
    let phi = TAU_F32 * f32(j) / f32(rings.x);
    return vec3<f32>(sin(theta) * cos(phi), cos(theta), sin(theta) * sin(phi));
}

/// Places every blob vertex: on the blob's shape, draped over the input's
/// curvature, with lumps on its visible part. Dispatch W + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn blob_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        if slot == batch.total_work_vertices {
            arena[region(REGION_WORK_USED) + slot] = 0u;
        }
        return;
    }
    arena[region(REGION_WORK_USED) + slot] = 0u;
    arena[region(REGION_WORK_BURIED) + slot] = 0u;
    let r = request_of_work_vertex(slot);
    let request = requests[r];
    let local = slot - request.out_vertex_base;
    if request.mode != MODE_SCATTER || local >= counter(r, COUNTER_WORK_VERTICES) || output_overflows(r) {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_VERTICES, local);
    let seed_at = region(REGION_SEEDS) + SEED_WORDS * blob.x;
    let point = load3(seed_at);
    let normal = load3(seed_at + 3u);
    let radius = bitcast<f32>(arena[seed_at + 6u]);
    let height = bitcast<f32>(arena[seed_at + 7u]);
    let depth = bitcast<f32>(arena[seed_at + 8u]);
    let tangent = any_orthonormal(normal);
    let bitangent = cross3(tangent, normal);

    // The upper part is an ellipsoid with its widest ring at `centre`; below
    // the surface it is squashed to `depth`.
    let neck = 1.0 - request.neck;
    let k = sqrt(1.0 - neck * neck);
    let semi = height / (1.0 + k);
    let centre = k * semi;
    let below = semi - centre;

    let dir = blob_direction(blob.y, blob_rings(seed_at, request.edge_length));
    var y = centre + dir.y * semi;
    if y < 0.0 {
        y = y * depth / max(below, 1.17549435e-38f);
    }
    let offset = (tangent * dir.x + bitangent * dir.z) * radius;
    let drape = dot3(closest_point(r, point + offset).point - point, normal);
    var p = point + offset + normal * (y + drape);
    if y > 0.0 {
        let outward = normalize_or(tangent * dir.x + normal * dir.y + bitangent * dir.z, normal);
        p += outward * lump_height(request.field, p) * fade(request.field, y);
    }
    store3(region(REGION_WORK_POSITIONS) + 3u * slot, p);
    arena[region(REGION_WORK_FADE) + slot] = bitcast<u32>(clamp(y / max(height, 1.17549435e-38f), 0.0, 1.0));
    arena[region(REGION_WORK_BURIED) + slot] = select(0u, 1u, y <= 0.0);
}

/// Returns the corners of triangle `q` of a blob with `rings`, as blob
/// vertex numbers, counter-clockwise seen from outside.
fn blob_triangle(q: u32, rings: vec2<u32>) -> vec3<u32> {
    let segments = rings.x;
    let per_segment = 2u * (rings.y - 1u);
    let j = q / per_segment;
    let m = q % per_segment;
    let bottom = 1u + (rings.y - 1u) * segments;
    if m == 0u {
        return vec3<u32>(0u, ring_vertex(1u, j + 1u, segments), ring_vertex(1u, j, segments));
    }
    if m == per_segment - 1u {
        return vec3<u32>(ring_vertex(rings.y - 1u, j, segments), ring_vertex(rings.y - 1u, j + 1u, segments), bottom);
    }
    let g = 1u + (m - 1u) / 2u;
    let a = ring_vertex(g, j, segments);
    let b = ring_vertex(g, j + 1u, segments);
    let c = ring_vertex(g + 1u, j, segments);
    let d = ring_vertex(g + 1u, j + 1u, segments);
    if (m - 1u) % 2u == 0u {
        return vec3<u32>(a, b, c);
    }
    return vec3<u32>(b, d, c);
}

/// Returns blob vertex number of ring `ring` (from 1), segment `j`.
fn ring_vertex(ring: u32, j: u32, segments: u32) -> u32 {
    return 1u + (ring - 1u) * segments + j % segments;
}

/// Writes every blob triangle's corners and whether it is kept: all are in
/// a closed blob, and those with a visible corner in an open one. Marks the
/// kept triangles' corners as used. Dispatch Wt + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn blob_triangles(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_triangles {
        if slot == batch.total_work_triangles {
            arena[region(REGION_WORK_KEPT) + slot] = 0u;
        }
        return;
    }
    arena[region(REGION_WORK_KEPT) + slot] = 0u;
    let r = request_of_work_triangle(slot);
    let request = requests[r];
    let local = slot - request.out_index_base / 3u;
    if request.mode != MODE_SCATTER || local >= counter(r, COUNTER_WORK_TRIANGLES) || output_overflows(r) {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_TRIANGLES, local);
    let rings = blob_rings(region(REGION_SEEDS) + SEED_WORDS * blob.x, request.edge_length);
    let corners = blob_triangle(blob.y, rings);

    // The blob's first vertex: its seed's offset in the scanned vertex counts.
    let first = arena[region(REGION_BLOB_VERTICES) + blob.x] - arena[region(REGION_BLOB_VERTICES) + request.seed_base];
    let base = request.out_vertex_base + first;
    let at = region(REGION_WORK_CORNERS) + 3u * slot;
    var visible = false;
    for (var k = 0u; k < 3u; k++) {
        arena[at + k] = base + corners[k];
        visible = visible || arena[region(REGION_WORK_BURIED) + base + corners[k]] == 0u;
    }
    if request.closed == 0u && !visible {
        return;
    }
    arena[region(REGION_WORK_KEPT) + slot] = 1u;
    for (var k = 0u; k < 3u; k++) {
        arena[region(REGION_WORK_USED) + base + corners[k]] = 1u;
    }
}

/// Returns the face normal, not normalized, of working triangle `slot`.
fn work_face(slot: u32) -> vec3<f32> {
    let at = region(REGION_WORK_CORNERS) + 3u * slot;
    let a = load3(region(REGION_WORK_POSITIONS) + 3u * arena[at]);
    let b = load3(region(REGION_WORK_POSITIONS) + 3u * arena[at + 1u]);
    let c = load3(region(REGION_WORK_POSITIONS) + 3u * arena[at + 2u]);
    return cross3(b - a, c - a);
}

/// Sets each used blob vertex's normal: the area-weighted sum of its kept
/// triangles' faces, in triangle order, made unit length. Dispatch W items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn blob_normals(
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
    let r = request_of_work_vertex(slot);
    let request = requests[r];
    if request.mode != MODE_SCATTER {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_VERTICES, slot - request.out_vertex_base);
    let rings = blob_rings(region(REGION_SEEDS) + SEED_WORDS * blob.x, request.edge_length);
    let segments = rings.x;
    let per_segment = 2u * (rings.y - 1u);
    let q = blob.y;

    // The blob's first triangle slot.
    let first = arena[region(REGION_BLOB_TRIANGLES) + blob.x] - arena[region(REGION_BLOB_TRIANGLES) + request.seed_base];
    let tri_base = request.out_index_base / 3u + first;
    let kept = region(REGION_WORK_KEPT);
    var sum = vec3<f32>(0.0);
    let last = 1u + (rings.y - 1u) * segments;
    if q == 0u || q == last {
        // A pole touches one triangle per segment.
        let m = select(per_segment - 1u, 0u, q == 0u);
        for (var j = 0u; j < segments; j++) {
            let t = tri_base + j * per_segment + m;
            if arena[kept + t + 1u] != arena[kept + t] {
                sum += work_face(t);
            }
        }
    } else {
        // A ring vertex touches up to three triangles in its own segment and
        // three in the segment before; gather them in triangle order.
        let i = 1u + (q - 1u) / segments;
        let j = (q - 1u) % segments;
        let before = (j + segments - 1u) % segments;
        var found: array<u32, 6>;
        var count = 0u;
        if i == 1u {
            found[count] = j * per_segment; count += 1u;
            found[count] = before * per_segment; count += 1u;
        }
        if i <= rings.y - 2u {
            // As corner a of its own segment's first triangle of ring i, and
            // corner b of both of the previous segment's.
            found[count] = j * per_segment + 1u + 2u * (i - 1u); count += 1u;
            found[count] = before * per_segment + 1u + 2u * (i - 1u); count += 1u;
            found[count] = before * per_segment + 2u + 2u * (i - 1u); count += 1u;
        }
        if i >= 2u {
            // As corner c of both of its own segment's triangles of ring
            // i - 1, and corner d of the previous segment's second.
            found[count] = j * per_segment + 1u + 2u * (i - 2u); count += 1u;
            found[count] = j * per_segment + 2u + 2u * (i - 2u); count += 1u;
            found[count] = before * per_segment + 2u + 2u * (i - 2u); count += 1u;
        }
        if i == rings.y - 1u {
            found[count] = j * per_segment + per_segment - 1u; count += 1u;
            found[count] = before * per_segment + per_segment - 1u; count += 1u;
        }
        // Insertion sort into triangle order.
        for (var a = 1u; a < count; a++) {
            let value = found[a];
            var b = a;
            while b > 0u && found[b - 1u] > value {
                found[b] = found[b - 1u];
                b -= 1u;
            }
            found[b] = value;
        }
        for (var a = 0u; a < count; a++) {
            let t = tri_base + found[a];
            if arena[kept + t + 1u] != arena[kept + t] {
                sum += work_face(t);
            }
        }
    }
    store3(region(REGION_WORK_NORMALS) + 3u * slot, normalize_or_zero(sum));
}

// Strict mode for scattered clumps: a blob is left out if it crosses
// itself or a blob kept before it, as on the CPU. After `blob_triangles`:
// `strict_bounds` (S), `strict_cross` (Wt), `strict_decide` (requests),
// `strict_drop_vertices` (W) and `strict_drop_triangles` (Wt), then the
// scans of used vertices and kept triangles as usual.

/// Returns whether request `r` is a strict scatter whose clumps are built.
fn strict_scatter(r: u32) -> bool {
    return requests[r].mode == MODE_SCATTER && requests[r].strict != 0u && !output_overflows(r);
}

/// Returns seed slot `seed`'s first working triangle slot and how many
/// triangles its blob has.
fn blob_triangle_span(r: u32, seed: u32) -> vec2<u32> {
    let counts = region(REGION_BLOB_TRIANGLES);
    let first = arena[counts + seed] - arena[counts + requests[r].seed_base];
    return vec2<u32>(requests[r].out_index_base / 3u + first, arena[counts + seed + 1u] - arena[counts + seed]);
}

/// Returns working triangle `slot`'s corner positions.
fn work_triangle(slot: u32) -> array<vec3<f32>, 3> {
    let at = region(REGION_WORK_CORNERS) + 3u * slot;
    let positions = region(REGION_WORK_POSITIONS);
    return array<vec3<f32>, 3>(
        load3(positions + 3u * arena[at]),
        load3(positions + 3u * arena[at + 1u]),
        load3(positions + 3u * arena[at + 2u]),
    );
}

/// Records each strict blob's bounds: over its kept triangles' corners.
/// One item per seed slot.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_bounds(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let seed = item_index(global_id, num_workgroups);
    if seed >= batch.total_seeds {
        return;
    }
    let r = request_of_seed(seed);
    if !strict_scatter(r) || seed - requests[r].seed_base >= counter(r, COUNTER_SEEDS) {
        return;
    }
    let span = blob_triangle_span(r, seed);
    var lo = vec3<f32>(3.40282347e38f);
    var hi = vec3<f32>(-3.40282347e38f);
    for (var t = span.x; t < span.x + span.y; t++) {
        if arena[region(REGION_WORK_KEPT) + t] != 0u {
            let c = work_triangle(t);
            lo = min(lo, min(min(c[0], c[1]), c[2]));
            hi = max(hi, max(max(c[0], c[1]), c[2]));
        }
    }
    store3(region(REGION_BLOB_BOUNDS) + 6u * seed, lo);
    store3(region(REGION_BLOB_BOUNDS) + 6u * seed + 3u, hi);
}

/// Returns whether triangles `t` and `u` share a corner position.
fn share_corner(t: array<vec3<f32>, 3>, u: array<vec3<f32>, 3>) -> bool {
    var a = t;
    var b = u;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            if all(a[i] == b[j]) {
                return true;
            }
        }
    }
    return false;
}

/// Flags each strict blob that crosses itself, and each pair of a blob and
/// an earlier blob of the same request that cross. One item per working
/// triangle slot: it tests its own triangle.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_cross(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(slot);
    let request = requests[r];
    let local = slot - request.out_index_base / 3u;
    let kept = region(REGION_WORK_KEPT);
    if !strict_scatter(r) || local >= counter(r, COUNTER_WORK_TRIANGLES) || arena[kept + slot] == 0u {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_TRIANGLES, local).x;
    let t = work_triangle(slot);

    // Against the blob's later triangles that share no corner.
    let own = blob_triangle_span(r, blob);
    for (var u = slot + 1u; u < own.x + own.y; u++) {
        if arena[kept + u] != 0u {
            let other = work_triangle(u);
            if !share_corner(t, other) && triangles_cross(t, other) {
                atomicStore(&flags[blob], 1u);
                break;
            }
        }
    }

    // Against every earlier blob whose bounds this triangle overlaps.
    let lo = min(min(t[0], t[1]), t[2]);
    let hi = max(max(t[0], t[1]), t[2]);
    let b = blob - request.seed_base;
    for (var a = 0u; a < b; a++) {
        let seed = request.seed_base + a;
        let bounds = region(REGION_BLOB_BOUNDS) + 6u * seed;
        if any(lo > load3(bounds + 3u)) || any(hi < load3(bounds)) {
            continue;
        }
        let bit = b * (b - 1u) / 2u + a;
        let word = request.pair_base + bit / 32u;
        let mask_bit = 1u << (bit % 32u);
        if (atomicLoad(&flags[word]) & mask_bit) != 0u {
            continue;
        }
        let span = blob_triangle_span(r, seed);
        for (var u = span.x; u < span.x + span.y; u++) {
            if arena[kept + u] != 0u && triangles_cross(t, work_triangle(u)) {
                atomicOr(&flags[word], mask_bit);
                break;
            }
        }
    }
}

/// Decides, in seed order as the CPU does, which strict blobs are kept: a
/// blob is left out if it crosses itself or crosses a blob kept before it.
/// One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_decide(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count || !strict_scatter(r) {
        return;
    }
    let request = requests[r];
    let rejected = region(REGION_BLOB_REJECTED) + request.seed_base;
    var clumps = 0u;
    for (var b = 0u; b < counter(r, COUNTER_SEEDS); b++) {
        var out = atomicLoad(&flags[request.seed_base + b]) != 0u;
        for (var a = 0u; a < b && !out; a++) {
            let bit = b * (b - 1u) / 2u + a;
            let crossed = (atomicLoad(&flags[request.pair_base + bit / 32u]) >> (bit % 32u)) & 1u;
            out = arena[rejected + a] == 0u && crossed != 0u;
        }
        arena[rejected + b] = select(0u, 1u, out);
        clumps += select(1u, 0u, out);
    }
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_CLUMPS], clumps);
}

/// Marks every vertex of a left-out strict blob as unused.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_drop_vertices(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_vertices {
        return;
    }
    let r = request_of_work_vertex(slot);
    let local = slot - requests[r].out_vertex_base;
    if !strict_scatter(r) || local >= counter(r, COUNTER_WORK_VERTICES) {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_VERTICES, local).x;
    if arena[region(REGION_BLOB_REJECTED) + blob] != 0u {
        arena[region(REGION_WORK_USED) + slot] = 0u;
    }
}

/// Marks every triangle of a left-out strict blob as dropped.
@compute @workgroup_size(WORKGROUP_SIZE)
fn strict_drop_triangles(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_work_triangles {
        return;
    }
    let r = request_of_work_triangle(slot);
    let local = slot - requests[r].out_index_base / 3u;
    if !strict_scatter(r) || local >= counter(r, COUNTER_WORK_TRIANGLES) {
        return;
    }
    let blob = blob_of(r, REGION_BLOB_TRIANGLES, local).x;
    if arena[region(REGION_BLOB_REJECTED) + blob] != 0u {
        arena[region(REGION_WORK_KEPT) + slot] = 0u;
    }
}
