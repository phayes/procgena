// Bounding volume hierarchies (BVHs) over each request's triangles, built on
// the GPU, and the ray and closest-point queries that use them. There are
// two meshes to build over (`MESH_*`): the input, and a layer's simplified
// working surface.
//
// The tree is a linear BVH (Karras 2012): triangles sorted by the Morton
// code of their centroid are the leaves, and node `i` of a request with
// `n` triangles (`0 <= i < n - 1`) is an inner node; node 0 is the root.
// Each inner node stores both children and both children's bounds.
//
// Build one mesh's BVHs, with T triangle slots and `BvhPass.mesh` set:
// 1. `bvh_init` (requests), `bvh_bounds` (T), `bvh_keys` (T + 1).
// 2. Scan the mesh's flags (T + 1), `bvh_list` (T), and sort the list by
//    Morton code then request.
// 3. `bvh_build` (T), then `bvh_refit` (T) once per pass, with
//    `BvhPass.number` counting 1, 2, … up to the batch's refit pass count.

import package::batch::{
    arena, batch, counters, requests, region, item_index, load3, store3, counter,
    request_of_triangle, COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_COARSE_TRIANGLES,
    COUNTER_BOUNDS_MIN, COUNTER_BOUNDS_MAX, COUNTER_VERTEX_MIN, COUNTER_VERTEX_MAX,
    REGION_POSITIONS, REGION_CORNERS, REGION_TRI_KEY_MORTON, REGION_TRI_KEY_REQUEST,
    REGION_TRI_FLAGS, REGION_TRI_LIST_A, REGION_TRI_SORTED, REGION_BVH_NODES,
    REGION_BVH_READY, REGION_COARSE_POINTS, REGION_COARSE_TRIS, REGION_COARSE_KEY_MORTON,
    REGION_COARSE_KEY_REQUEST, REGION_COARSE_BVH_FLAGS, REGION_COARSE_LIST_A,
    REGION_COARSE_SORTED, REGION_COARSE_NODES, REGION_COARSE_READY, POINT_WORDS,
    PATCH_TRI_WORDS, WORKGROUP_SIZE, request_of_work_triangle, MODE_EVERYWHERE,
    REGION_WORK_POSITIONS, REGION_WORK_CORNERS, REGION_WORK_KEPT, REGION_WORK_KEY_MORTON,
    REGION_WORK_KEY_REQUEST, REGION_WORK_BVH_FLAGS, REGION_WORK_LIST_A, REGION_WORK_SORTED,
    REGION_WORK_NODES, REGION_WORK_READY,
};
import package::math::{dot3, length_squared, F32_MAX};

/// `BvhPass.mesh` values: each request's input, a layer's simplified
/// working surface, or a strict layer's assembled working mesh.
const MESH_INPUT: u32 = 0u;
const MESH_COARSE: u32 = 1u;
const MESH_WORK: u32 = 2u;

/// Where one mesh's triangles and BVH live, as word offsets into the arena.
struct MeshSource {
    /// Points, `position_stride` words apart, position first.
    positions: u32,
    position_stride: u32,
    /// Triangles, `corner_stride` words apart: three point numbers first,
    /// counted from the request's first vertex slot.
    corners: u32,
    corner_stride: u32,
    /// The BVH's arrays: sort keys, flags (scanned: where each request's
    /// triangles start in the sorted list), list, sorted list, nodes and
    /// refit passes.
    morton: u32,
    request_key: u32,
    flags: u32,
    list: u32,
    sorted: u32,
    nodes: u32,
    ready: u32,
    /// The counter holding each request's triangle count (unused for the
    /// working mesh).
    count: u32,
    /// `MESH_*`: which mesh this is.
    kind: u32,
}

/// Returns where the input's triangles and BVH are.
fn input_mesh() -> MeshSource {
    return MeshSource(
        region(REGION_POSITIONS), 3u, region(REGION_CORNERS), 3u,
        region(REGION_TRI_KEY_MORTON), region(REGION_TRI_KEY_REQUEST), region(REGION_TRI_FLAGS),
        region(REGION_TRI_LIST_A), region(REGION_TRI_SORTED), region(REGION_BVH_NODES),
        region(REGION_BVH_READY), COUNTER_TRIANGLES, MESH_INPUT,
    );
}

/// Returns where the simplified working surface's triangles and BVH are.
fn coarse_mesh() -> MeshSource {
    return MeshSource(
        region(REGION_COARSE_POINTS), POINT_WORDS, region(REGION_COARSE_TRIS), PATCH_TRI_WORDS,
        region(REGION_COARSE_KEY_MORTON), region(REGION_COARSE_KEY_REQUEST),
        region(REGION_COARSE_BVH_FLAGS), region(REGION_COARSE_LIST_A), region(REGION_COARSE_SORTED),
        region(REGION_COARSE_NODES), region(REGION_COARSE_READY), COUNTER_COARSE_TRIANGLES,
        MESH_COARSE,
    );
}

/// Returns where the strict layers' assembled working meshes and their BVH
/// are.
fn work_mesh() -> MeshSource {
    return MeshSource(
        region(REGION_WORK_POSITIONS), 3u, region(REGION_WORK_CORNERS), 3u,
        region(REGION_WORK_KEY_MORTON), region(REGION_WORK_KEY_REQUEST),
        region(REGION_WORK_BVH_FLAGS), region(REGION_WORK_LIST_A), region(REGION_WORK_SORTED),
        region(REGION_WORK_NODES), region(REGION_WORK_READY), 0u, MESH_WORK,
    );
}

/// Returns the mesh this build dispatch works on.
fn build_mesh() -> MeshSource {
    if bvh_pass.mesh == MESH_COARSE {
        return coarse_mesh();
    }
    if bvh_pass.mesh == MESH_WORK {
        return work_mesh();
    }
    return input_mesh();
}

/// Returns how many triangle slots mesh `m` has over the whole batch.
fn slot_count(m: MeshSource) -> u32 {
    return select(batch.total_triangles, batch.total_work_triangles, m.kind == MESH_WORK);
}

/// Returns the request owning triangle slot `slot` of mesh `m`.
fn request_in(m: MeshSource, slot: u32) -> u32 {
    if m.kind == MESH_WORK {
        return request_of_work_triangle(slot);
    }
    return request_of_triangle(slot);
}

/// Returns request `r`'s first triangle slot in mesh `m`.
fn first_in(m: MeshSource, r: u32) -> u32 {
    return select(requests[r].triangle_base, requests[r].out_index_base / 3u, m.kind == MESH_WORK);
}

/// Returns request `r`'s triangle slots in mesh `m`.
fn capacity_in(m: MeshSource, r: u32) -> u32 {
    return select(requests[r].triangle_capacity, requests[r].out_index_capacity / 3u, m.kind == MESH_WORK);
}

/// Returns whether triangle slot `slot` of request `r` holds a triangle of
/// mesh `m`, before the mesh's flags are scanned. The working mesh holds
/// only strict layers' kept triangles.
fn used_in(m: MeshSource, r: u32, slot: u32) -> bool {
    if m.kind == MESH_WORK {
        return requests[r].mode == MODE_EVERYWHERE && requests[r].strict != 0u
            && arena[region(REGION_WORK_KEPT) + slot] != 0u;
    }
    return slot - requests[r].triangle_base < count_in(m, r);
}

/// Words per inner node: left and right child, then each child's bounds
/// (min xyz, max xyz), padded.
const NODE_WORDS: u32 = 16u;

/// Child references with this bit set are leaves: sorted triangle positions.
const LEAF: u32 = 0x80000000u;

/// `REGION_BVH_READY` value of a node not finished yet.
const NOT_READY: u32 = 0xffffffffu;

/// Deepest a traversal stack goes. The tree is at most as deep as its sort
/// key is long: 30 Morton bits plus the tie-breaking position bits.
const STACK_SIZE: u32 = 64u;

/// Which mesh (`MESH_*`) a build dispatch works on, and which refit pass it
/// is, counting from 1.
struct BvhPass {
    number: u32,
    mesh: u32,
}

@group(1) @binding(0) var<uniform> bvh_pass: BvhPass;

/// Returns `x` encoded so unsigned order matches float order.
fn order_bits(x: f32) -> u32 {
    let bits = bitcast<u32>(x);
    return select(bits | 0x80000000u, ~bits, (bits & 0x80000000u) != 0u);
}

/// Undoes [`order_bits`].
fn from_order_bits(bits: u32) -> f32 {
    return bitcast<f32>(select(~bits, bits & 0x7fffffffu, (bits & 0x80000000u) != 0u));
}

/// Returns the corners of mesh `m`'s triangle slot `slot`, which belongs to
/// request `r`.
fn corners_in(m: MeshSource, r: u32, slot: u32) -> array<vec3<f32>, 3> {
    let at = m.corners + m.corner_stride * slot;
    // The working mesh's corners are already global vertex slots.
    let base = select(requests[r].vertex_base, 0u, m.kind == MESH_WORK);
    return array<vec3<f32>, 3>(
        load3(m.positions + m.position_stride * (base + arena[at])),
        load3(m.positions + m.position_stride * (base + arena[at + 1u])),
        load3(m.positions + m.position_stride * (base + arena[at + 2u])),
    );
}

/// Returns how many triangles request `r` has in mesh `m`. For the working
/// mesh, only once its flags are scanned.
fn count_in(m: MeshSource, r: u32) -> u32 {
    if m.kind == MESH_WORK {
        let first = first_in(m, r);
        return arena[m.flags + first + capacity_in(m, r)] - arena[m.flags + first];
    }
    return min(counter(r, m.count), requests[r].triangle_capacity);
}

/// Returns the global triangle slot at sorted position `position` of
/// request `r`'s triangles in mesh `m`.
fn sorted_in(m: MeshSource, r: u32, position: u32) -> u32 {
    return arena[m.sorted + arena[m.flags + first_in(m, r)] + position];
}

/// Returns the corners of the input's triangle slot `slot`, which belongs
/// to request `r`.
fn triangle_corners(r: u32, slot: u32) -> array<vec3<f32>, 3> {
    return corners_in(input_mesh(), r, slot);
}

/// Sets up each request's bounds counters. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_init(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count {
        return;
    }
    // Empty centroid bounds, ready to grow; with the input, empty vertex
    // bounds for measuring too.
    for (var k = 0u; k < 3u; k++) {
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_BOUNDS_MIN + k], 0xffffffffu);
        atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_BOUNDS_MAX + k], 0u);
        if bvh_pass.mesh == MESH_INPUT {
            atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_VERTEX_MIN + k], 0xffffffffu);
            atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_VERTEX_MAX + k], 0u);
        }
    }
}

/// Returns the centroid of triangle `corners`.
fn centroid(corners: array<vec3<f32>, 3>) -> vec3<f32> {
    return (corners[0] + corners[1] + corners[2]) / 3.0;
}

/// Grows each request's bounds to hold its triangles' centroids.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_bounds(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let m = build_mesh();
    if slot >= slot_count(m) {
        return;
    }
    let r = request_in(m, slot);
    if !used_in(m, r, slot) {
        return;
    }
    let c = centroid(corners_in(m, r, slot));
    for (var k = 0u; k < 3u; k++) {
        atomicMin(&counters[r * COUNTER_STRIDE + COUNTER_BOUNDS_MIN + k], order_bits(c[k]));
        atomicMax(&counters[r * COUNTER_STRIDE + COUNTER_BOUNDS_MAX + k], order_bits(c[k]));
    }
}

/// Spreads the low 10 bits of `x` to every third bit.
fn spread_bits(x: u32) -> u32 {
    var v = x & 0x3ffu;
    v = (v | (v << 16u)) & 0x030000ffu;
    v = (v | (v << 8u)) & 0x0300f00fu;
    v = (v | (v << 4u)) & 0x030c30c3u;
    v = (v | (v << 2u)) & 0x09249249u;
    return v;
}

/// Flags each used triangle slot and writes its sort keys: the 30-bit
/// Morton code of its centroid within its request's bounds, and its
/// request. Dispatch T + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_keys(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let m = build_mesh();
    if slot >= slot_count(m) {
        if slot == slot_count(m) {
            arena[m.flags + slot] = 0u;
        }
        return;
    }
    let r = request_in(m, slot);
    let used = used_in(m, r, slot);
    arena[m.flags + slot] = select(0u, 1u, used);
    arena[m.request_key + slot] = r;
    if !used {
        return;
    }

    // The centroid's place in the bounds, in 1024 steps per axis.
    let c = centroid(corners_in(m, r, slot));
    var code = 0u;
    for (var k = 0u; k < 3u; k++) {
        let lo = from_order_bits(counter(r, COUNTER_BOUNDS_MIN + k));
        let hi = from_order_bits(counter(r, COUNTER_BOUNDS_MAX + k));
        var f = 0.0;
        if hi > lo {
            f = clamp((c[k] - lo) / (hi - lo), 0.0, 1.0);
        }
        code |= spread_bits(min(u32(f * 1024.0), 1023u)) << (2u - k);
    }
    arena[m.morton + slot] = code;
}

/// Lists the used triangle slots in order, after the flags are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_list(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let m = build_mesh();
    if slot >= slot_count(m) {
        return;
    }
    if arena[m.flags + slot + 1u] != arena[m.flags + slot] {
        arena[m.list + arena[m.flags + slot]] = slot;
    }
}

/// Returns the length of the common prefix of the sort keys at sorted
/// positions `i` and `j` of request `r`'s triangles in mesh `m`, which has
/// `n` of them: the Morton codes, then the positions themselves to break
/// ties. -1 if `j` is out of range.
fn common_prefix(m: MeshSource, r: u32, n: u32, i: i32, j: i32) -> i32 {
    if j < 0 || j >= i32(n) {
        return -1;
    }
    let a = arena[m.morton + sorted_in(m, r, u32(i))];
    let b = arena[m.morton + sorted_in(m, r, u32(j))];
    if a != b {
        return i32(countLeadingZeros(a ^ b));
    }
    return 32 + i32(countLeadingZeros(u32(i) ^ u32(j)));
}

/// Finds each inner node's children (Karras 2012). One item per triangle
/// slot; slot `i` of a request is its inner node `i`.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_build(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let m = build_mesh();
    if slot >= slot_count(m) {
        return;
    }
    let r = request_in(m, slot);
    let n = count_in(m, r);
    let node = slot - first_in(m, r);
    arena[m.ready + slot] = NOT_READY;
    if n < 2u || node >= n - 1u {
        return;
    }
    let i = i32(node);

    // Which way this node's range runs, and how far.
    let d = select(-1, 1, common_prefix(m, r, n, i, i + 1) > common_prefix(m, r, n, i, i - 1));
    let shortest = common_prefix(m, r, n, i, i - d);
    var span = 2;
    while common_prefix(m, r, n, i, i + span * d) > shortest {
        span *= 2;
    }
    var length = 0;
    for (var step = span / 2; step >= 1; step /= 2) {
        if common_prefix(m, r, n, i, i + (length + step) * d) > shortest {
            length += step;
        }
    }
    let j = i + length * d;

    // Where the range splits.
    let prefix = common_prefix(m, r, n, i, j);
    var split = 0;
    var step = length;
    loop {
        step = (step + 1) / 2;
        if common_prefix(m, r, n, i, i + (split + step) * d) > prefix {
            split += step;
        }
        if step <= 1 {
            break;
        }
    }
    let gamma = i + split * d + min(d, 0);
    let first = min(i, j);
    let last = max(i, j);
    let at = m.nodes + NODE_WORDS * slot;
    arena[at] = select(u32(gamma), LEAF | u32(gamma), first == gamma);
    arena[at + 1u] = select(u32(gamma + 1), LEAF | u32(gamma + 1), last == gamma + 1);
}

/// Returns the bounds of triangle `corners`, as (min, max).
fn triangle_bounds(corners: array<vec3<f32>, 3>) -> array<vec3<f32>, 2> {
    return array<vec3<f32>, 2>(
        min(min(corners[0], corners[1]), corners[2]),
        max(max(corners[0], corners[1]), corners[2]),
    );
}

/// Returns the bounds of child `child` of request `r` in mesh `m`, and
/// whether they are known: a leaf's always are, an inner node's once it was
/// finished in a pass before pass `current`.
fn child_bounds(
    m: MeshSource,
    r: u32,
    child: u32,
    current: u32,
    bounds: ptr<function, array<vec3<f32>, 2>>,
) -> bool {
    if (child & LEAF) != 0u {
        *bounds = triangle_bounds(corners_in(m, r, sorted_in(m, r, child & ~LEAF)));
        return true;
    }
    let slot = first_in(m, r) + child;
    if arena[m.ready + slot] >= current {
        return false;
    }
    let at = m.nodes + NODE_WORDS * slot;
    (*bounds)[0] = min(load3(at + 2u), load3(at + 8u));
    (*bounds)[1] = max(load3(at + 5u), load3(at + 11u));
    return true;
}

/// Stores each inner node's children's bounds once both are known. Run once
/// per pass, counting passes from 1; after enough passes every node is done.
@compute @workgroup_size(WORKGROUP_SIZE)
fn bvh_refit(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    let m = build_mesh();
    if slot >= slot_count(m) {
        return;
    }
    let r = request_in(m, slot);
    let n = count_in(m, r);
    let node = slot - first_in(m, r);
    if n < 2u || node >= n - 1u || arena[m.ready + slot] != NOT_READY {
        return;
    }
    let current = bvh_pass.number;
    let at = m.nodes + NODE_WORDS * slot;
    var left: array<vec3<f32>, 2>;
    var right: array<vec3<f32>, 2>;
    if !child_bounds(m, r, arena[at], current, &left)
        || !child_bounds(m, r, arena[at + 1u], current, &right) {
        return;
    }
    store3(at + 2u, left[0]);
    store3(at + 5u, left[1]);
    store3(at + 8u, right[0]);
    store3(at + 11u, right[1]);
    arena[m.ready + slot] = current;
}

// Queries. Each takes a request and searches only its triangles; the
// `_in` forms take the mesh, and the others search the input.

/// Returns the entry distance of a ray from `origin` with direction
/// reciprocal `inv_dir` into box `lo`..`hi`, or -1 if it misses the box
/// before `t_max`.
fn ray_entry(lo: vec3<f32>, hi: vec3<f32>, origin: vec3<f32>, inv_dir: vec3<f32>, t_max: f32) -> f32 {
    let t0 = (lo - origin) * inv_dir;
    let t1 = (hi - origin) * inv_dir;
    let near_t = max(max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z)), 0.0);
    let far_t = min(min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z)), t_max);
    return select(-1.0, near_t, near_t <= far_t);
}

/// Returns the squared distance from `p` to box `lo`..`hi`; 0 inside it.
fn box_distance_squared(lo: vec3<f32>, hi: vec3<f32>, p: vec3<f32>) -> f32 {
    return length_squared(max(max(lo - p, p - hi), vec3<f32>(0.0)));
}

/// A ray set up for the watertight triangle test (Woop, Benthin and Wald
/// 2013): rows mapping a point relative to the origin into a frame where
/// the ray runs along z.
struct Ray {
    origin: vec3<f32>,
    x: vec3<f32>,
    y: vec3<f32>,
    z: vec3<f32>,
    z_scale: f32,
}

/// Prepares a ray from `origin` along unit direction `dir`.
fn make_ray(origin: vec3<f32>, dir: vec3<f32>) -> Ray {
    // Run the ray along its largest axis, swapping the other two when it
    // points the negative way, so triangles keep their winding.
    let a = abs(dir);
    var kz = 2u;
    if a.x >= a.y && a.x >= a.z {
        kz = 0u;
    } else if a.y >= a.z {
        kz = 1u;
    }
    var kx = (kz + 1u) % 3u;
    var ky = (kz + 2u) % 3u;
    if dir[kz] < 0.0 {
        let swap = kx;
        kx = ky;
        ky = swap;
    }
    var ux = vec3<f32>(0.0);
    ux[kx] = 1.0;
    var uy = vec3<f32>(0.0);
    uy[ky] = 1.0;
    var uz = vec3<f32>(0.0);
    uz[kz] = 1.0;
    return Ray(origin, ux - uz * (dir[kx] / dir[kz]), uy - uz * (dir[ky] / dir[kz]), uz, 1.0 / dir[kz]);
}

/// Returns which side of the edge from `p` to `q`, in a ray's frame, the ray
/// passes: `p.x q.y - p.y q.x`. Swapping `p` and `q` gives exactly the
/// negated value, as the triangle on the other side of a shared edge needs,
/// however the GPU fuses the multiplies.
fn edge_side(p: vec3<f32>, q: vec3<f32>) -> f32 {
    let in_order = p.x < q.x || (p.x == q.x && p.y < q.y);
    let a = select(q, p, in_order);
    let b = select(p, q, in_order);
    let side = a.x * b.y - a.y * b.x;
    return select(-side, side, in_order);
}

/// Returns the distance along `ray` where it meets triangle `corners` from
/// either side, or -1 if it misses. A ray starting on the triangle hits it
/// at 0; a triangle seen exactly edge-on is never hit.
fn ray_triangle(ray: Ray, corners: array<vec3<f32>, 3>) -> f32 {
    var p: array<vec3<f32>, 3>;
    for (var k = 0u; k < 3u; k++) {
        let q = corners[k] - ray.origin;
        p[k] = vec3<f32>(dot3(ray.x, q), dot3(ray.y, q), dot3(ray.z, q));
    }
    let u = edge_side(p[2], p[1]);
    let v = edge_side(p[0], p[2]);
    if (u < 0.0 && v > 0.0) || (u > 0.0 && v < 0.0) {
        return -1.0;
    }
    let w = edge_side(p[1], p[0]);
    if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
        return -1.0;
    }
    let det = u + v + w;
    if det == 0.0 {
        return -1.0;
    }
    let t = (u * p[0].z + v * p[1].z + w * p[2].z) * ray.z_scale / det;
    return select(-1.0, t, t >= 0.0);
}

/// A ray hit: the distance and global triangle slot, or `slot` = `LEAF` for
/// no hit.
struct Hit {
    t: f32,
    slot: u32,
}

/// Returns request `r`'s first input triangle hit before `t_max` by the ray
/// from `origin` along unit direction `dir`, from either side.
fn ray_hit(r: u32, origin: vec3<f32>, dir: vec3<f32>, t_max: f32) -> Hit {
    return ray_hit_in(input_mesh(), r, origin, dir, t_max);
}

/// Returns request `r`'s first triangle in mesh `m` hit before `t_max` by
/// the ray from `origin` along unit direction `dir`, from either side. Ties
/// go to the lowest sorted position.
fn ray_hit_in(m: MeshSource, r: u32, origin: vec3<f32>, dir: vec3<f32>, t_max: f32) -> Hit {
    let n = count_in(m, r);
    var best = Hit(t_max, LEAF);
    var best_position = 0xffffffffu;
    if n == 0u {
        return best;
    }
    let ray = make_ray(origin, dir);
    let inv_dir = 1.0 / dir;
    let nodes = m.nodes + NODE_WORDS * first_in(m, r);

    // Walk nodes nearest first; a lone triangle is its own root.
    var stack: array<vec2<u32>, STACK_SIZE>;
    var top = 1u;
    stack[0] = vec2<u32>(select(0u, LEAF, n == 1u), bitcast<u32>(0.0));
    while top > 0u {
        top -= 1u;
        let entry = stack[top];
        if bitcast<f32>(entry.y) > best.t {
            continue;
        }
        if (entry.x & LEAF) != 0u {
            let position = entry.x & ~LEAF;
            let slot = sorted_in(m, r, position);
            let t = ray_triangle(ray, corners_in(m, r, slot));
            let found = best.slot != LEAF;
            if t >= 0.0 && ((!found && t < best.t) || (found && (t < best.t || (t == best.t && position < best_position)))) {
                best = Hit(t, slot);
                best_position = position;
            }
            continue;
        }
        let at = nodes + NODE_WORDS * entry.x;
        let left = ray_entry(load3(at + 2u), load3(at + 5u), origin, inv_dir, best.t);
        let right = ray_entry(load3(at + 8u), load3(at + 11u), origin, inv_dir, best.t);
        // Push the farther child first, so the nearer one is popped first.
        var near_child = vec2<u32>(arena[at], bitcast<u32>(left));
        var far_child = vec2<u32>(arena[at + 1u], bitcast<u32>(right));
        if left >= 0.0 && right >= 0.0 && right < left {
            let swap = near_child;
            near_child = far_child;
            far_child = swap;
        }
        if bitcast<f32>(far_child.y) >= 0.0 && top < STACK_SIZE {
            stack[top] = far_child;
            top += 1u;
        }
        if bitcast<f32>(near_child.y) >= 0.0 && top < STACK_SIZE {
            stack[top] = near_child;
            top += 1u;
        }
    }
    return best;
}

/// Returns whether the ray from `origin` along unit direction `dir` hits any
/// of request `r`'s input triangles before `t_max`.
fn ray_blocked(r: u32, origin: vec3<f32>, dir: vec3<f32>, t_max: f32) -> bool {
    let m = input_mesh();
    let n = count_in(m, r);
    if n == 0u {
        return false;
    }
    let ray = make_ray(origin, dir);
    let inv_dir = 1.0 / dir;
    let nodes = m.nodes + NODE_WORDS * first_in(m, r);
    var stack: array<u32, STACK_SIZE>;
    var top = 1u;
    stack[0] = select(0u, LEAF, n == 1u);
    while top > 0u {
        top -= 1u;
        let node = stack[top];
        if (node & LEAF) != 0u {
            let t = ray_triangle(ray, corners_in(m, r, sorted_in(m, r, node & ~LEAF)));
            if t >= 0.0 && t < t_max {
                return true;
            }
            continue;
        }
        let at = nodes + NODE_WORDS * node;
        if ray_entry(load3(at + 8u), load3(at + 11u), origin, inv_dir, t_max) >= 0.0 && top < STACK_SIZE {
            stack[top] = arena[at + 1u];
            top += 1u;
        }
        if ray_entry(load3(at + 2u), load3(at + 5u), origin, inv_dir, t_max) >= 0.0 && top < STACK_SIZE {
            stack[top] = arena[at];
            top += 1u;
        }
    }
    return false;
}

/// The closest point on a request's input to a query point.
struct Closest {
    point: vec3<f32>,
    /// Global triangle slot it lies on.
    slot: u32,
    /// Barycentric weights of `point` on that triangle's corners.
    barycentric: vec3<f32>,
    distance_squared: f32,
}

/// Returns the closest point to `p` on triangle `corners` and its
/// barycentric weights (Ericson, "Real-Time Collision Detection", 5.1.5).
fn closest_on_triangle(p: vec3<f32>, corners: array<vec3<f32>, 3>) -> Closest {
    let a = corners[0];
    let b = corners[1];
    let c = corners[2];
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return Closest(a, 0u, vec3<f32>(1.0, 0.0, 0.0), 0.0);
    }
    let bp = p - b;
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return Closest(b, 0u, vec3<f32>(0.0, 1.0, 0.0), 0.0);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return Closest(a + ab * v, 0u, vec3<f32>(1.0 - v, v, 0.0), 0.0);
    }
    let cp = p - c;
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return Closest(c, 0u, vec3<f32>(0.0, 0.0, 1.0), 0.0);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return Closest(a + ac * w, 0u, vec3<f32>(1.0 - w, 0.0, w), 0.0);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return Closest(b + (c - b) * w, 0u, vec3<f32>(0.0, 1.0 - w, w), 0.0);
    }
    let denom = va + vb + vc;
    if abs(denom) <= 1.17549435e-38f {
        return Closest(a, 0u, vec3<f32>(1.0, 0.0, 0.0), 0.0);
    }
    let v = vb / denom;
    let w = vc / denom;
    return Closest(a + ab * v + ac * w, 0u, vec3<f32>(1.0 - v - w, v, w), 0.0);
}

/// Returns the closest point to `p` on any of request `r`'s input
/// triangles. With no triangles, `distance_squared` is `F32_MAX`.
fn closest_point(r: u32, p: vec3<f32>) -> Closest {
    return closest_point_in(input_mesh(), r, p);
}

/// Returns the closest point to `p` on any of request `r`'s triangles in
/// mesh `m`. With no triangles, `distance_squared` is `F32_MAX`.
fn closest_point_in(m: MeshSource, r: u32, p: vec3<f32>) -> Closest {
    var best = Closest(vec3<f32>(0.0), 0u, vec3<f32>(1.0, 0.0, 0.0), F32_MAX);
    let n = count_in(m, r);
    if n == 0u {
        return best;
    }
    let nodes = m.nodes + NODE_WORDS * first_in(m, r);
    var stack: array<vec2<u32>, STACK_SIZE>;
    var top = 1u;
    stack[0] = vec2<u32>(select(0u, LEAF, n == 1u), 0u);
    while top > 0u {
        top -= 1u;
        let entry = stack[top];
        if bitcast<f32>(entry.y) >= best.distance_squared {
            continue;
        }
        if (entry.x & LEAF) != 0u {
            let slot = sorted_in(m, r, entry.x & ~LEAF);
            var found = closest_on_triangle(p, corners_in(m, r, slot));
            let d = length_squared(found.point - p);
            if d < best.distance_squared {
                found.slot = slot;
                found.distance_squared = d;
                best = found;
            }
            continue;
        }
        // Visit the nearer child first, so the far one is more often pruned.
        let at = nodes + NODE_WORDS * entry.x;
        let dl = box_distance_squared(load3(at + 2u), load3(at + 5u), p);
        let dr = box_distance_squared(load3(at + 8u), load3(at + 11u), p);
        var first = vec2<u32>(arena[at], bitcast<u32>(dl));
        var second = vec2<u32>(arena[at + 1u], bitcast<u32>(dr));
        if dr <= dl {
            let swap = first;
            first = second;
            second = swap;
        }
        if top + 2u <= STACK_SIZE {
            stack[top] = second;
            stack[top + 1u] = first;
            top += 2u;
        }
    }
    return best;
}
