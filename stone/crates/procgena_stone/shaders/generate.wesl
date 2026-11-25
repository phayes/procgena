// Shared bindings, grid helpers, and the sampling pass for GPU rock generation.
//
// One request describes one rock mesh. The passes for a request run in order:
// `sample_field`; `count_mc` or `count_dc`; an exclusive prefix sum over the
// counts (see scan.wesl); the mesher's emit passes; and `finish_mc` or
// `finish_dc`; then `measure_mc` or `measure_dc`. The resulting mesh describes the same surface as the CPU
// mesher's output for the same settings; GPU rounding can change a few
// vertices near the surface and the vertex order.

import package::field::{carve_begin, carve_step, normalize_or, stone_finish, Surface, MAX_CARVE_STEPS};
import package::tables::MC_CORNERS;

/// Workgroup width of the per-item generation passes.
const WORKGROUP_SIZE: u32 = 64u;

/// `Status.overflow` bit: the vertex output needed more than `vertex_capacity` slots.
const OVERFLOW_VERTICES: u32 = 1u;
/// `Status.overflow` bit: the index output needed more than `index_capacity` slots.
const OVERFLOW_INDICES: u32 = 2u;
/// `Status.overflow` bit: dual contouring needed more than `crossing_capacity` crossings.
const OVERFLOW_CROSSINGS: u32 = 4u;

/// Settings and output locations for one rock mesh.
struct Request {
    /// Per-axis factors converting stone space to carve space.
    carve_scale: vec3<f32>,
    /// Radius of the starting sphere, in carve-space units.
    base_radius: f32,
    /// Width of the rounded transitions between cuts.
    smoothness: f32,
    /// Roughness depth; zero disables roughness.
    displacement: f32,
    /// Roughness pattern frequency.
    spread: f32,
    seed: u32,
    /// Number of roughness noise layers.
    octaves: u32,
    /// Number of entries of `spheres` to apply.
    steps: u32,
    /// Grid cells per axis.
    resolution: u32,
    /// Index of this request's entries in `draw_args` and `status`.
    request_index: u32,
    /// First slot of this mesh in the shared vertex buffer.
    vertex_base: u32,
    /// Vertex slots reserved for this mesh.
    vertex_capacity: u32,
    /// First slot of this mesh in the shared index buffer.
    index_base: u32,
    /// Index slots reserved for this mesh.
    index_capacity: u32,
    /// Surface-point slots available to dual contouring or placement analysis.
    crossing_capacity: u32,
    /// Status entry containing the completed ground placement for this mesh.
    placement_index: u32,
    /// Analysis only: 1 to estimate a resting side, 0 to keep the generated orientation.
    settle: u32,
    /// Analysis only: burial cap as a finite fraction of placed height, from 0 to 1.
    max_embed_fraction: f32,
    /// Analysis only: smallest burial as a fraction of placed height, from 0
    /// up to `max_embed_fraction`.
    min_embed_fraction: f32,
    /// Cutting spheres in application order: centre in `xyz`, radius in `w`.
    spheres: array<vec4<f32>, MAX_CARVE_STEPS>,
}

/// One mesh vertex: position in the stone-space cube and outward unit normal.
struct Vertex {
    position: array<f32, 3>,
    normal: array<f32, 3>,
}

/// Arguments for one indexed indirect draw, in the layout GPUs expect.
struct DrawArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

/// A request's required buffer sizes and unscaled ground placement.
struct Status {
    /// Combined `OVERFLOW_*` flags; zero means all reserved buffers were large enough.
    overflow: u32,
    /// Vertices required for the complete mesh, even if it overflowed.
    vertex_count: u32,
    /// Indices required for the complete mesh, even if it overflowed.
    index_count: u32,
    /// Surface crossings counted on grid edges by dual contouring.
    crossing_count: u32,
    /// Rotation from the generated orientation to the chosen ground orientation.
    rotation: mat3x3<f32>,
    /// Unscaled offset applied after rotation to centre and lift the rock.
    translation: vec3<f32>,
    /// Suggested burial depth in unscaled units; not included in `translation`.
    embed_depth: f32,
    /// Diagonal of the placed mesh's axis-aligned bounding box in unscaled
    /// units, written by `measure_bounds`; zero for a mesh that overflowed
    /// or has no vertices.
    bounding_diameter: f32,
    /// Unused; rounds the entry up to `STATUS_SIZE` (96) bytes.
    padding: array<u32, 3>,
}

@group(0) @binding(0) var<uniform> request: Request;
@group(0) @binding(2) var<storage, read_write> samples: array<f32>;
@group(0) @binding(3) var<storage, read_write> prefix: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read_write> vertices: array<Vertex>;
@group(0) @binding(6) var<storage, read_write> indices: array<u32>;
@group(0) @binding(7) var<storage, read_write> draw_args: array<DrawArgs>;
@group(0) @binding(8) var<storage, read_write> status: array<Status>;

/// Packs a position and normal into a `Vertex`.
fn make_vertex(position: vec3<f32>, normal: vec3<f32>) -> Vertex {
    return Vertex(
        array<f32, 3>(position.x, position.y, position.z),
        array<f32, 3>(normal.x, normal.y, normal.z),
    );
}

/// Returns a vertex's position as a vector.
fn vertex_position(v: Vertex) -> vec3<f32> {
    return vec3<f32>(v.position[0], v.position[1], v.position[2]);
}

/// Returns a vertex's normal as a vector.
fn vertex_normal(v: Vertex) -> vec3<f32> {
    return vec3<f32>(v.normal[0], v.normal[1], v.normal[2]);
}

/// Rotates and offsets an unplaced vertex, including rotating its normal.
/// Complete analysis in `status[request.placement_index]` first. The result
/// remains unscaled and does not include suggested burial.
fn place_vertex(v: Vertex) -> Vertex {
    let placement = status[request.placement_index];
    return make_vertex(
        placement.rotation * vertex_position(v) + placement.translation,
        placement.rotation * vertex_normal(v),
    );
}

/// Samples the requested rock at stone-space position `p`: negative inside,
/// zero or more outside.
fn stone_distance(p: vec3<f32>) -> f32 {
    let q = p * request.carve_scale;
    var c = carve_begin(q, request.base_radius);
    for (var i = 0u; i < request.steps; i++) {
        c = carve_step(c, q, request.spheres[i], request.smoothness);
    }
    let s = Surface(request.carve_scale, request.displacement, request.seed, request.spread, request.octaves);
    return stone_finish(p, c, s);
}

/// Estimates the field's gradient at `p` with central differences of size `step`.
fn stone_gradient(p: vec3<f32>, step: f32) -> vec3<f32> {
    let dx = vec3<f32>(step, 0.0, 0.0);
    let dy = vec3<f32>(0.0, step, 0.0);
    let dz = vec3<f32>(0.0, 0.0, step);
    let g = vec3<f32>(
        stone_distance(p + dx) - stone_distance(p - dx),
        stone_distance(p + dy) - stone_distance(p - dy),
        stone_distance(p + dz) - stone_distance(p - dz),
    );
    return g / (2.0 * step);
}

/// Returns the item this invocation handles. Dispatch rows of workgroups along
/// x, adding rows along y when one row cannot cover every item.
fn item_index(gid: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return gid.x + gid.y * num_workgroups.x * WORKGROUP_SIZE;
}

/// Grid samples per axis, including both cube faces.
fn samples_per_axis() -> u32 {
    return request.resolution + 1u;
}

/// Total grid samples. Count passes handle one more item than this, which
/// receives zero counts so the prefix sum ends with the totals.
fn sample_count() -> u32 {
    let n = samples_per_axis();
    return n * n * n;
}

/// Flat index of sample `s`, with x changing fastest.
fn sample_index(s: vec3<u32>) -> u32 {
    let n = samples_per_axis();
    return s.x + n * (s.y + n * s.z);
}

/// Sample coordinates of flat index `i`.
fn sample_coords(i: u32) -> vec3<u32> {
    let n = samples_per_axis();
    return vec3<u32>(i % n, i / n % n, i / (n * n));
}

/// Stone-space coordinate of sample index `i` along one axis.
fn grid_coordinate(i: u32) -> f32 {
    return f32(i) / f32(request.resolution) - 0.5;
}

/// Stone-space position of sample `s`.
fn sample_position(s: vec3<u32>) -> vec3<f32> {
    return vec3<f32>(grid_coordinate(s.x), grid_coordinate(s.y), grid_coordinate(s.z));
}

/// Field value at sample `s`, after `sample_field` has run.
fn sample_value(s: vec3<u32>) -> f32 {
    return samples[sample_index(s)];
}

/// Whether sample `s` is inside the rock.
fn inside(s: vec3<u32>) -> bool {
    return sample_value(s) < 0.0;
}

/// Unit step along `axis` (0 = x, 1 = y, 2 = z) in sample coordinates.
fn axis_step(axis: u32) -> vec3<u32> {
    var v = vec3<u32>(0u);
    v[axis] = 1u;
    return v;
}

/// Unit direction along `axis` in stone space.
fn axis_direction(axis: u32) -> vec3<f32> {
    var v = vec3<f32>(0.0);
    v[axis] = 1.0;
    return v;
}

/// Whether the grid edge from sample `s` along `axis` crosses the surface.
fn crosses(s: vec3<u32>, axis: u32) -> bool {
    return s[axis] + 1u < samples_per_axis() && inside(s) != inside(s + axis_step(axis));
}

/// Number of surface crossings on the edges starting at sample `s`.
fn crossing_count(s: vec3<u32>) -> u32 {
    var count = 0u;
    for (var axis = 0u; axis < 3u; axis++) {
        if crosses(s, axis) {
            count += 1u;
        }
    }
    return count;
}

/// Crossing ID of the edge from sample `s` along `axis`, which must cross the
/// surface. Valid after the prefix sum.
fn crossing_id(s: vec3<u32>, axis: u32) -> u32 {
    var rank = 0u;
    for (var a = 0u; a < axis; a++) {
        if crosses(s, a) {
            rank += 1u;
        }
    }
    return prefix[sample_index(s)].x + rank;
}

/// Surface position and outward normal on the crossing edge from sample `s` along `axis`.
fn crossing_vertex(s: vec3<u32>, axis: u32) -> Vertex {
    let a = sample_value(s);
    let b = sample_value(s + axis_step(axis));
    let t = a / (a - b);
    let r = f32(request.resolution);
    let dir = axis_direction(axis);
    let position = sample_position(s) + dir * (t / r);
    // Without a usable gradient, point from the inside endpoint to the outside one.
    let outward = select(-dir, dir, inside(s));
    let normal = normalize_or(stone_gradient(position, 0.5 / r), outward);
    return make_vertex(position, normal);
}

/// Whether sample `s` is the minimum corner of a grid cell.
fn is_cell(s: vec3<u32>) -> bool {
    return all(s < vec3<u32>(request.resolution));
}

/// Case index of cell `c`: bit `i` is set when corner `i` of `MC_CORNERS` is outside.
fn cell_case(c: vec3<u32>) -> u32 {
    var case_index = 0u;
    for (var i = 0u; i < 8u; i++) {
        if !inside(c + MC_CORNERS[i]) {
            case_index |= 1u << i;
        }
    }
    return case_index;
}

/// Stores required counts, overflow flags, and the placement applied to this
/// request. Any overflow produces an empty draw; counts still report needed capacity.
fn write_result(overflow: u32, vertex_count: u32, index_count: u32, crossing_count: u32) {
    let drawn = select(0u, index_count, overflow == 0u);
    draw_args[request.request_index] = DrawArgs(
        drawn,
        1u,
        request.index_base,
        i32(request.vertex_base),
        0u,
    );
    let placement = status[request.placement_index];
    status[request.request_index] = Status(
        overflow,
        vertex_count,
        index_count,
        crossing_count,
        placement.rotation,
        placement.translation,
        placement.embed_depth,
        0.0,
        array<u32, 3>(),
    );
}

/// Each invocation's share of the bounding box in `measure_bounds`.
var<workgroup> bounds_min: array<vec3<f32>, WORKGROUP_SIZE>;
var<workgroup> bounds_max: array<vec3<f32>, WORKGROUP_SIZE>;

/// Measures the placed mesh's axis-aligned bounding box and stores its
/// diagonal in the request's status, the same measure as the CPU's
/// `Stone::bounding_diameter`. Call from an entry point run after the
/// mesher's finish pass, with one workgroup of `WORKGROUP_SIZE` invocations;
/// `local` is the invocation's index in it.
fn measure_bounds(local: u32) {
    let result = status[request.request_index];
    let count = select(0u, result.vertex_count, result.overflow == 0u);

    // Each invocation covers every `WORKGROUP_SIZE`th vertex.
    var low = vec3<f32>(3.4e38);
    var high = vec3<f32>(-3.4e38);
    for (var i = local; i < count; i += WORKGROUP_SIZE) {
        let p = vertex_position(vertices[request.vertex_base + i]);
        low = min(low, p);
        high = max(high, p);
    }
    bounds_min[local] = low;
    bounds_max[local] = high;
    workgroupBarrier();

    // Halve the number of boxes until one holds them all.
    for (var stride = WORKGROUP_SIZE / 2u; stride > 0u; stride /= 2u) {
        if local < stride {
            bounds_min[local] = min(bounds_min[local], bounds_min[local + stride]);
            bounds_max[local] = max(bounds_max[local], bounds_max[local + stride]);
        }
        workgroupBarrier();
    }
    if local == 0u && count > 0u {
        status[request.request_index].bounding_diameter = length(bounds_max[0] - bounds_min[0]);
    }
}

/// Samples the field at every grid point. Dispatch at least `sample_count()` items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn sample_field(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    if i >= sample_count() {
        return;
    }
    samples[i] = stone_distance(sample_position(sample_coords(i)));
}
