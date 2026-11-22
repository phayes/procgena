//! procgena_plant::shaders::generate — prepares visible grass and foliage for each camera.
//!
//! These compute passes generate instances once for reuse by the camera's draws:
//!
//! 1. `plan_buckets` divides storage using last frame's demand, then clears counters.
//! 2. `cull` finds visible chunks and divides their candidates into work slices.
//! 3. `size_dispatch` schedules generation for the slices that fit.
//! 4. `generate` processes each slice, choosing visible plants, detail and wind bend.
//! 5. `write_draw_args` prepares draws, limited to the instances that fit in storage.
//!
//! The passes in `trample.wesl` update trampling before `cull`; `generate` samples
//! the result at each blade's root.

import package::shaders::common::{
    Frame, View, Field, Chunk, ShapeTable, BladeData, TILE_POINTS, NO_MAP, NO_CLASS, WIDTH_RANGE, TAU,
    MAX_BUCKETS, DRAW_ARGS_COUNT, class_stride, hash, hash2, unit, density_falloff, fade_falloff, height_thinning, select_lod,
    coverage_width, reach_factor, stem_facing, pack, trample_unpack, trample_slot, in_trample_window,
};
import package::shaders::wind::{Wind, wind_bend};

/// The height a ground baked from a mesh has where the mesh does not reach:
/// no blade grows there. Twin of `ground::HOLE`.
const HOLE: f32 = -1e30;

// Shared by every view.
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> fields: array<Field>;
@group(0) @binding(2) var<storage, read> chunks: array<Chunk>;
/// Every mask, two R8G8 texels per word, then every heightmap as `f32` bits.
/// A field's `placement.w` already counts the mask words.
@group(0) @binding(3) var<storage, read> scene: array<u32>;
@group(0) @binding(4) var<storage, read> tile: array<vec2<f32>>;
@group(0) @binding(5) var gust_texture: texture_2d<f32>;
@group(0) @binding(6) var gust_sampler: sampler;
@group(0) @binding(7) var<uniform> shapes: ShapeTable;
/// `mirror::clump_table`: a clump per 25 cm square of a 64 m tile, four bytes
/// to a word.
@group(0) @binding(8) var<storage, read> clumps: array<u32>;
/// `mirror::tuft_table`: the nearest tuft centre for every quarter of a
/// tuft spacing of a 64-tuft tile.
@group(0) @binding(9) var<storage, read> tuft_table: array<u32>;
@group(0) @binding(10) var<uniform> wind: Wind;

/// One slice of a visible chunk's candidates: the chunk, the slice's first
/// candidate and how many it holds. A dense chunk is several slices, so
/// several workgroups generate it at once.
struct VisibleChunk {
    chunk: u32,
    first: u32,
    count: u32,
    _pad: u32,
}

struct DispatchArgs {
    x: u32,
    y: u32,
    z: u32,
}

struct DrawArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

// This view.
@group(1) @binding(0) var<uniform> view: View;
@group(1) @binding(1) var<storage, read_write> visible: array<VisibleChunk>;
@group(1) @binding(2) var<storage, read_write> counters: BucketCounters;
@group(1) @binding(3) var<storage, read_write> instances: array<vec4<u32>>;
@group(1) @binding(4) var<storage, read_write> draw_args: array<DrawArgs, DRAW_ARGS_COUNT>;
/// One packed push per trample cell, addressed by `trample_slot`. Read with
/// plain loads: `trample.wesl` writes it in earlier dispatches.
@group(1) @binding(5) var<storage, read_write> trample: array<u32>;
// Only `size_dispatch` writes the dispatch arguments, in a group of their own:
// `generate` reads them as its indirect arguments, and one dispatch may not
// use a buffer both ways.
@group(2) @binding(0) var<storage, read_write> dispatch: DispatchArgs;

const WORKGROUP: u32 = 64u;
/// Candidates per slice: one workgroup's share of a chunk.
const SLICE: u32 = 512u;
const FIXED_LOD_NONE: u32 = 3u;
/// Extra stem slots requested per branched bucket to allow new plants to appear.
///
/// This allowance may shrink if the budget is insufficient. Twin of `mirror::MIN_SLACK`.
const MIN_SLACK: u32 = 256u;
/// Precision used to scale plant allocations with integer arithmetic.
///
/// Fractional shares use this many bits to match the CPU. Twin of `mirror::SHARE_BITS`.
const SHARE_BITS: u32 = 12u;

/// Writable bucket table shared by the compute passes.
///
/// Matches `BucketTable` in `common.wesl`, with atomic counters so generating
/// workgroups can safely add to the same totals.
struct BucketCounters {
    /// Requested instances per bucket, including those that do not fit.
    count: array<atomic<u32>, MAX_BUCKETS>,
    /// Number of visible chunk slices requested for generation.
    slices: atomic<u32>,
    /// Starting offset of each bucket in the instance buffer.
    base: array<u32, MAX_BUCKETS>,
    /// Maximum number of instances each bucket can store this frame.
    capacity: array<u32, MAX_BUCKETS>,
}

fn is_orthographic() -> bool {
    return view.camera_position.w > 0.5;
}

fn sphere_visible(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0; i < 6; i++) {
        if dot(view.planes[i].xyz, center) + view.planes[i].w < -radius {
            return false;
        }
    }
    return true;
}

fn box_visible(lo: vec3<f32>, hi: vec3<f32>) -> bool {
    for (var i = 0; i < 6; i++) {
        let n = view.planes[i].xyz;
        // The corner furthest along the plane's normal.
        let corner = select(lo, hi, n > vec3<f32>(0.0));
        if dot(n, corner) + view.planes[i].w < 0.0 {
            return false;
        }
    }
    return true;
}

/// `v`, given along the field's own axes, in world XZ. Unturned it is `v`
/// exactly. Twin of `fields::turn`.
fn field_turn(field: Field, v: vec2<f32>) -> vec2<f32> {
    let x = field.axis.xy;
    return x * v.x + vec2<f32>(-x.y, x.x) * v.y;
}

/// A point of the field, in metres along its axes from its minimum corner,
/// in world XZ. Twin of `FieldGpu::world_of`.
fn field_to_world(field: Field, local: vec2<f32>) -> vec2<f32> {
    return field.min_corner.xz + field_turn(field, local);
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= frame.chunk_count {
        return;
    }
    let chunk = chunks[index];
    let field = fields[chunk.ids.x];
    // Hidden fields, and fields on layers this view does not draw.
    if (field.access.x & view.layers) == 0u {
        return;
    }
    // Include branch reach in the culling bound so foliage remains visible beyond its
    // root.
    let tallest = field.min_corner.w;
    // The chunk's square in world XZ, turned with its field, and the box
    // around it. Unturned, the box is the square exactly.
    let origin = field_to_world(field, chunk.bounds.xy);
    let side_x = field_turn(field, vec2<f32>(frame.chunk_size, 0.0));
    let side_z = field_turn(field, vec2<f32>(0.0, frame.chunk_size));
    let low = origin + min(side_x, vec2<f32>(0.0)) + min(side_z, vec2<f32>(0.0));
    let high = origin + max(side_x, vec2<f32>(0.0)) + max(side_z, vec2<f32>(0.0));
    // Tufts pull blades up to one and a half tuft spacings from where they
    // were placed; `variation.w` is the widest pull of any of the field's
    // tufted plants, or 0.
    let spread = tallest + field.variation.w;
    // The chunk's ground box, grown by the tallest blade: up for its height,
    // sideways for how far it can lean or bend.
    var lo = vec3<f32>(low.x - spread, chunk.bounds.z, low.y - spread);
    var hi = vec3<f32>(high.x + spread, chunk.bounds.w + tallest, high.y + spread);
    if view.shadow_offset.w > 0.5 {
        // Blades just outside the view can still shadow ground inside it.
        let reach = view.shadow_offset.xyz * tallest;
        lo = min(lo, lo + reach);
        hi = max(hi, hi + reach);
    }
    if !box_visible(lo, hi) {
        return;
    }

    var falloff = 1.0;
    if !is_orthographic() {
        let nearest = clamp(view.camera_position.xyz, lo, hi);
        let d = max(distance(nearest, view.camera_position.xyz) - view.distance_offset, 0.0);
        if d >= view.radius {
            return;
        }
        // Retain enough candidates for branched plants, which thin less with distance.
        // `evaluate` applies the picked plant's thinning rule.
        let thinning = select(density_falloff(view, d), fade_falloff(view, d), field.access.z != 0u);
        falloff = thinning * height_thinning(view);
    }
    let tile_size = field.extent.w;
    let candidate_density = f32(TILE_POINTS) / (tile_size * tile_size);
    let keep = clamp(field.shape.w * view.density_scale * falloff / candidate_density, 0.0, 1.0);
    let cells = field.placement.y * field.placement.y;
    // Twin of `ChunkLayout::candidates_needed`.
    let needed = u32(ceil(keep * f32(TILE_POINTS))) * cells;
    if needed == 0u {
        return;
    }
    let slices = (needed + SLICE - 1u) / SLICE;
    let slot = atomicAdd(&counters.slices, slices);
    for (var s = 0u; s < slices; s++) {
        if slot + s < view.max_visible_chunks {
            visible[slot + s] = VisibleChunk(index, s * SLICE, min(SLICE, needed - s * SLICE), 0u);
        }
    }
}

/// The push of the cell `point` is in. One cell rather than a bilinear
/// blend of four: at 0.25 m a cell is about one blade wide, screenshots
/// could not tell the two apart, and four loads made generation under a
/// 512-interactor horde 0.2 ms slower on the development GPU.
fn trample_push(point: vec2<f32>) -> vec2<f32> {
    let cell = vec2<i32>(floor(point / view.trample_step.x));
    if !in_trample_window(cell, view.trample_origin.xy) {
        return vec2<f32>(0.0);
    }
    return trample_unpack(trample[trample_slot(cell)]);
}

@compute @workgroup_size(1)
fn size_dispatch() {
    dispatch = DispatchArgs(min(atomicLoad(&counters.slices), view.max_visible_chunks), 1u, 1u);
}

/// Twin of `mirror::clump_of`.
fn clump_of(point: vec2<f32>) -> u32 {
    let entry = vec2<i32>(floor(point * 4.0)) & vec2<i32>(255);
    let index = u32(entry.y * 256 + entry.x);
    return (clumps[index / 4u] >> ((index % 4u) * 8u)) & 63u;
}

/// The nearest tuft centre to `q`, in tuft spacings, and the tuft's random
/// value in [0, 1]. Twin of `mirror::tuft_at`.
fn tuft_at(q: vec2<f32>) -> vec3<f32> {
    let entry = floor(q * 4.0);
    let wrapped = vec2<i32>(entry) & vec2<i32>(255);
    let word = tuft_table[u32(wrapped.y * 256 + wrapped.x)];
    let offset = (vec2<f32>(f32(word & 0xffu), f32((word >> 8u) & 0xffu)) / 255.0 - 0.5) * 4.0;
    return vec3<f32>((entry + 0.5) / 4.0 + offset, f32((word >> 16u) & 0xffu) / 255.0);
}

/// The noise bare patches are cut from. Twin of `mirror::bare_noise`.
fn bare_noise(at: vec2<f32>, size: f32) -> f32 {
    let uv = at / (size * 8.0);
    let turned = vec2<f32>(uv.x * 0.8 - uv.y * 0.6, uv.x * 0.6 + uv.y * 0.8) * 1.37 + 0.5;
    return 0.5 * (textureSampleLevel(gust_texture, gust_sampler, uv, 0.0).r
        + textureSampleLevel(gust_texture, gust_sampler, turned, 0.0).r);
}

/// Chooses the field entry for a root, testing plants in list order.
/// The last plant with a positive density has chance 1 and fills the gaps; `first`
/// is a fallback for malformed tables. Twin of `mirror::pick_plant`.
fn pick_plant(first: u32, field: Field, seed: u32, clump: u32) -> u32 {
    let count = u32(field.mix.z);
    for (var k = 0u; k < count; k++) {
        let plant = fields[first + k];
        let salt = (k + 1u) * 0x9e3779b9u;
        let scattered = unit(hash(seed ^ salt));
        let mixer = hash(salt ^ 0x5bd1e995u);
        let shuffled = (clump * ((mixer & 0x3eu) | 1u) + (mixer >> 8u)) & 63u;
        let patched = (f32(shuffled) + unit(hash(seed ^ salt ^ 0x68e31da4u))) / 64.0;
        let clustered = unit(hash(seed ^ salt ^ 0x1b873593u)) < plant.mix.y;
        if select(scattered, patched, clustered) < plant.mix.x {
            return first + k;
        }
    }
    return first;
}

/// Tests whether this root uses the plant's accent appearance.
/// Uses a separate random choice from plant selection, with probability `share`.
/// Twin of `mirror::is_accent`.
fn is_accent(seed: u32, share: f32) -> bool {
    return unit(hash(seed ^ 0x2c1b3c6du)) < share;
}

/// Returns the chosen plant entry or its accent entry for this root.
/// `accent.y` is the offset from the plant entry to its accent.
fn with_accent(entry: u32, look: Field, seed: u32) -> u32 {
    if look.accent.x > 0.0 && is_accent(seed, look.accent.x) {
        return entry + u32(look.accent.y);
    }
    return entry;
}

/// Bilinear R8G8 mask sample: density and height multiplier.
fn sample_mask(field: Field, uv: vec2<f32>) -> vec2<f32> {
    if field.placement.z == NO_MAP {
        return vec2<f32>(1.0);
    }
    let size = field.map_sizes.xy;
    let p = clamp(uv * vec2<f32>(size) - 0.5, vec2<f32>(0.0), vec2<f32>(size) - 1.0);
    let base = vec2<u32>(floor(p));
    let t = fract(p);
    let next = min(base + 1u, size - 1u);
    let a = mask_texel(field, base.x, base.y);
    let b = mask_texel(field, next.x, base.y);
    let c = mask_texel(field, base.x, next.y);
    let d = mask_texel(field, next.x, next.y);
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

fn mask_texel(field: Field, x: u32, y: u32) -> vec2<f32> {
    let texel = field.placement.z + y * field.map_sizes.x + x;
    let half = (scene[texel / 2u] >> ((texel % 2u) * 16u)) & 0xffffu;
    return vec2<f32>(f32(half & 0xffu), f32(half >> 8u)) / 255.0;
}

/// Ground height in world units at a point of the field (bilinear), or
/// `HOLE` where a mesh ground does not reach. The hole test has a CPU twin,
/// `edit::ground_is_hole`, for `PlantSampler`.
fn ground_height(field: Field, local: vec2<f32>) -> f32 {
    if field.placement.w == NO_MAP {
        return field.min_corner.y;
    }
    let size = field.map_sizes.zw;
    let p = clamp(local / field.extent.xy * vec2<f32>(size) - 0.5, vec2<f32>(0.0), vec2<f32>(size) - 1.0);
    let base = vec2<u32>(floor(p));
    let t = fract(p);
    let next = min(base + 1u, size - 1u);
    let row = field.placement.w;
    let a = bitcast<f32>(scene[row + base.y * size.x + base.x]);
    let b = bitcast<f32>(scene[row + base.y * size.x + next.x]);
    let c = bitcast<f32>(scene[row + next.y * size.x + base.x]);
    let d = bitcast<f32>(scene[row + next.y * size.x + next.x]);
    if min(min(a, b), min(c, d)) < HOLE * 0.5 {
        return HOLE;
    }
    return field.min_corner.y + mix(mix(a, b, t.x), mix(c, d, t.x), t.y) * field.extent.z;
}

/// Surviving plant candidate: the packed first stem and shared data for expanding its
/// crown with `stem_words`.
struct Generated {
    keep: bool,
    bucket: u32,
    /// Packed first stem, ready for the instance buffer.
    words: vec4<u32>,
    /// Stems sharing this root; 1 for a single blade.
    stems: u32,
    /// Unpacked first stem used as the template for additional stems.
    blade: BladeData,
    /// Full candidate seed, before instance packing reduces its precision.
    seed: u32,
    /// Height scale sampled from the mask at the shared root.
    mask_height: f32,
    /// Width multiplier compensating for distance thinning.
    coverage: f32,
    /// Wind scaling and trample displacement shared by the crown's stems.
    calm: f32,
    push: vec2<f32>,
}

/// Candidate `index` of a visible chunk: twin of `mirror::candidate` followed
/// by the tests a blade must pass to be drawn.
fn evaluate(chunk: Chunk, field: Field, index: u32) -> Generated {
    var out: Generated;
    out.keep = false;

    let tiles = field.placement.y;
    let cells = tiles * tiles;
    let point = index / cells;
    let cell = index % cells;
    let cell_hash = hash2(hash2(chunk.ids.y, chunk.ids.z), hash2(cell, field.placement.x));
    let offset = vec2<f32>(unit(cell_hash), unit(hash(cell_hash)));
    let p = fract(tile[point] + offset);
    var local = chunk.bounds.xy + (vec2<f32>(f32(cell % tiles), f32(cell / tiles)) + p) * field.extent.w;
    let rank = (f32(point) + 0.5) / f32(TILE_POINTS);
    let seed = hash2(cell_hash, point);

    // A field with tufted or branched plants picks the plant here, where the
    // root was placed: the tuft pull and the distance rule depend on it.
    // Other fields pick after thinning, so rejected candidates skip the work;
    // with no tufts the root does not move, so both pick the same plant.
    let branched_field = field.access.z != 0u;
    let early = (branched_field || field.variation.w > 0.0) && field.mix.z > 1.5;
    var entry = chunk.ids.x;
    var look = field;
    if early {
        entry = pick_plant(chunk.ids.x, field, seed, clump_of(field_to_world(field, local)));
        look = fields[entry];
    }
    if early || field.mix.z < 1.5 {
        let grown = with_accent(entry, look, seed);
        if grown != entry {
            entry = grown;
            look = fields[entry];
        }
    }

    // Tufts: the root moves part of the way to its tuft's centre, the tuft's
    // own share of the plant's pull, so tufts differ in size.
    var outward = vec2<f32>(0.0);
    if look.tufts.x > 0.0 {
        let size = look.tufts.y;
        let q = (field.min_corner.xz + local) / size;
        let tuft = tuft_at(q);
        outward = q - tuft.xy;
        local -= outward * min(look.tufts.x * (0.6 + 0.8 * tuft.z), 0.95) * size;
    }
    if any(local >= field.extent.xy) || any(local < vec2<f32>(0.0)) {
        return out;
    }

    let mask = sample_mask(field, local / field.extent.xy);
    let ground = ground_height(field, local);
    if ground < HOLE * 0.5 {
        return out;
    }
    let at = field_to_world(field, local);
    let root = vec3<f32>(at.x, ground, at.y);

    let shape = look.access.y;
    if shape == NO_CLASS {
        return out;
    }

    var falloff = 1.0;
    var distance_to_eye = 0.0;
    if !is_orthographic() {
        distance_to_eye = distance(root, view.camera_position.xyz);
        // A raised view thins as if it stood on the ground below it, and
        // for its height; its blades' detail still follows their distance.
        let from_ground = max(distance_to_eye - view.distance_offset, 0.0);
        // Branched plants retain full density until the final fade. Fields without
        // branched plants take the same branch for every candidate.
        var thinning: f32;
        if shape == 0u {
            thinning = density_falloff(view, from_ground);
        } else {
            thinning = fade_falloff(view, from_ground);
        }
        falloff = thinning * height_thinning(view);
    }
    var bare = 1.0;
    if field.tufts.z >= 0.0 {
        let threshold = field.tufts.z;
        bare = smoothstep(threshold - 0.05, threshold + 0.05, bare_noise(root.xz, field.tufts.w));
    }
    let tile_size = field.extent.w;
    let candidate_density = f32(TILE_POINTS) / (tile_size * tile_size);
    let keep = clamp(field.shape.w * mask.x * bare * view.density_scale * falloff / candidate_density, 0.0, 1.0);
    if rank >= keep {
        return out;
    }

    // A mixed field without tufts or branches picks its plant only for the
    // candidates that survived; a field of one plant skips the pick.
    if !early && field.mix.z > 1.5 {
        entry = pick_plant(chunk.ids.x, field, seed, clump_of(root.xz));
        look = fields[entry];
        let grown = with_accent(entry, look, seed);
        if grown != entry {
            entry = grown;
            look = fields[entry];
        }
    }
    let h = unit(hash(seed ^ 0x68u));
    let height = (look.shape.x + h * (look.shape.y - look.shape.x)) * mask.y;
    if height < 0.01 {
        return out;
    }
    // Cull the whole crown using its maximum possible stem height and branch reach.
    let stems = look.access.w;
    let reach = select(height, look.shape.y * mask.y, stems > 1u) * reach_factor(look);
    let visible_blade = sphere_visible(root + vec3<f32>(0.0, height * 0.5, 0.0), reach);
    var visible_shadow = false;
    if view.shadow_offset.w > 0.5 {
        let shadow = view.shadow_offset.xyz * reach;
        visible_shadow = sphere_visible(root + shadow * 0.5, length(shadow) * 0.5 + reach * 0.25);
    }
    if !(visible_blade || visible_shadow) {
        return out;
    }

    var lod = view.fixed_lod;
    if lod == 3u {
        lod = select_lod(view, distance_to_eye, unit(hash(seed ^ 0x10du)));
    }
    // Widen thinned blades to preserve coverage. Branched plants need no compensation
    // for mid-range thinning.
    let coverage = select(coverage_width(falloff, view.limits.x), 1.0, shape != 0u);
    let width = look.shape.z * (0.75 + 0.5 * unit(hash(seed ^ 0x77u))) * coverage;
    let seed_unit = f32(seed >> 24u) / 255.0;

    var blade: BladeData;
    blade.root = root;
    blade.facing = unit(hash(seed ^ 0x66u));
    if dot(outward, outward) > 1e-8 {
        // Tufted blades lean out from the tuft's centre: the tighter the
        // tufts, the more of them.
        let random = vec2<f32>(sin(blade.facing * TAU), cos(blade.facing * TAU));
        let away = mix(random, normalize(field_turn(field, outward)), min(look.tufts.x * 1.5, 1.0));
        if dot(away, away) > 1e-6 {
            blade.facing = fract(atan2(away.x, away.y) / TAU);
        }
    }
    blade.height = height / max(look.shape.y, 1e-3);
    blade.width = width / (WIDTH_RANGE * max(look.shape.z, 1e-5));
    blade.seed = seed >> 24u;
    blade.lod = lod;
    blade.field = entry;
    blade.clump = clump_of(root.xz);
    // A trampled blade lies down away from what pushed it, and the wind moves
    // it less the flatter it lies.
    let push = trample_push(root.xz);
    // Flattened blades and sheltered fields move less.
    let calm = (1.0 - min(length(push), 1.0)) * (1.0 - look.motion.z);
    blade.bend = wind_bend(wind, root.xz, seed_unit, look.motion.y, frame.time, false, gust_texture, gust_sampler) * calm + push;
    blade.previous_bend = wind_bend(wind, root.xz, seed_unit, look.motion.y, frame.previous_time, true, gust_texture, gust_sampler) * calm + push;

    out.keep = true;
    out.bucket = shape * 3u + select(lod, 0u, view.fixed_lod != 3u);
    out.words = pack(blade, view.instance_origin, view.limits.w);
    out.stems = stems;
    out.blade = blade;
    out.seed = seed;
    out.mask_height = mask.y;
    out.coverage = coverage;
    out.calm = calm;
    out.push = push;
    return out;
}

/// Packs additional stem `k` (at least 1) at the crown's shared root, plant and
/// detail level. A separate seed varies its height, width, direction and sway. The
/// packing twin for words 0–2 is `render::gpu_test::crown_words`.
fn stem_words(crown: Generated, k: u32) -> vec4<u32> {
    let look = fields[crown.blade.field];
    let seed = hash2(crown.seed, k);
    var blade = crown.blade;
    blade.facing = stem_facing(crown.blade.facing, k, crown.stems, seed);
    let h = unit(hash(seed ^ 0x68u));
    let height = (look.shape.x + h * (look.shape.y - look.shape.x)) * crown.mask_height;
    let width = look.shape.z * (0.75 + 0.5 * unit(hash(seed ^ 0x77u))) * crown.coverage;
    blade.height = height / max(look.shape.y, 1e-3);
    blade.width = width / (WIDTH_RANGE * max(look.shape.z, 1e-5));
    blade.seed = seed >> 24u;
    let seed_unit = f32(seed >> 24u) / 255.0;
    let root = blade.root.xz;
    blade.bend = wind_bend(wind, root, seed_unit, look.motion.y, frame.time, false, gust_texture, gust_sampler) * crown.calm + crown.push;
    blade.previous_bend = wind_bend(wind, root, seed_unit, look.motion.y, frame.previous_time, true, gust_texture, gust_sampler) * crown.calm + crown.push;
    return pack(blade, view.instance_origin, view.limits.w);
}

var<workgroup> group_total: u32;
var<workgroup> group_counts: array<atomic<u32>, MAX_BUCKETS>;
var<workgroup> group_base: array<u32, MAX_BUCKETS>;

/// One workgroup per slice of a visible chunk. Survivors are counted per bucket inside
/// the workgroup first, so the global counters see one atomic per bucket per
/// 64 candidates instead of one per blade.
@compute @workgroup_size(64)
fn generate(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let entry = visible[group.x];
    if lane == 0u {
        group_total = entry.count;
    }
    let total = workgroupUniformLoad(&group_total);
    let chunk = chunks[entry.chunk];
    let field = fields[chunk.ids.x];

    let rounds = (total + WORKGROUP - 1u) / WORKGROUP;
    // Each of the 64 threads handles every 64th bucket, so buckets numbered
    // 64 and above are processed too.
    let buckets = view.bucket_count;
    for (var round = 0u; round < rounds; round++) {
        for (var b = lane; b < buckets; b += WORKGROUP) {
            atomicStore(&group_counts[b], 0u);
        }
        workgroupBarrier();

        let index = round * WORKGROUP + lane;
        var result: Generated;
        result.keep = false;
        if index < total {
            result = evaluate(chunk, field, entry.first + index);
        }
        // Reserve adjacent slots so a crown can be accepted or dropped as a whole.
        var slot = 0u;
        if result.keep {
            slot = atomicAdd(&group_counts[result.bucket], result.stems);
        }
        workgroupBarrier();

        // Avoid global atomics for empty buckets, including classes absent from this
        // view.
        for (var b = lane; b < buckets; b += WORKGROUP) {
            let count = atomicLoad(&group_counts[b]);
            if count > 0u {
                group_base[b] = atomicAdd(&counters.count[b], count);
            }
        }
        workgroupBarrier();

        if result.keep {
            let b = result.bucket;
            let at = group_base[b] + slot;
            let capacity = counters.capacity[b];
            let base = counters.base[b];
            if at + result.stems <= capacity {
                instances[base + at] = result.words;
                for (var k = 1u; k < result.stems; k++) {
                    instances[base + at + k] = stem_words(result, k);
                }
            } else {
                // Drop an overflowing crown as a whole. Clear any reserved slots below
                // capacity with zero-height stems, since the clamped draw still visits
                // them; otherwise it could show stale instances.
                let hidden = vec4<u32>(result.words.xy, result.words.z & 0xffffff00u, result.words.w);
                for (var k = 0u; at + k < capacity; k++) {
                    instances[base + at + k] = hidden;
                }
            }
        }
        workgroupBarrier();
    }
}

/// Calculates a branched bucket's requested stem slots from last frame's demand.
///
/// Adds 25% and `MIN_SLACK` for growth, capped at the entire plant budget. Demand
/// includes stems that did not fit. Twin of `mirror::plant_want`.
fn plant_want(demand: u32, budget: u32) -> u32 {
    // Cap large demands before adding headroom to avoid integer overflow.
    if demand >= budget {
        return budget;
    }
    return min(demand + demand / 4u + MIN_SLACK, budget);
}

/// Divides instance storage between buckets using last frame's demand.
///
/// Blade allocations stay fixed. Branched buckets share `View::plant_capacity`:
/// requests from `plant_want` receive equal shares of any remainder, or shrink
/// proportionally if the budget is insufficient. Clears counters before this
/// frame's culling and generation. Twin of `mirror::plan_buckets`.
@compute @workgroup_size(1)
fn plan_buckets() {
    let buckets = view.bucket_count;
    let budget = view.plant_capacity;

    // Request room for last frame's demand plus growth, including previously dropped stems.
    var wanted = 0u;
    for (var b = 3u; b < buckets; b++) {
        let want = plant_want(atomicLoad(&counters.count[b]), budget);
        counters.capacity[b] = want;
        wanted += want;
    }

    // Share spare slots equally, or reduce every request by the same proportion.
    let branched = buckets - 3u;
    if branched > 0u {
        if wanted <= budget {
            let extra = (budget - wanted) / branched;
            for (var b = 3u; b < buckets; b++) {
                counters.capacity[b] += extra;
            }
        } else {
            let share = (budget << SHARE_BITS) / wanted;
            for (var b = 3u; b < buckets; b++) {
                counters.capacity[b] = (counters.capacity[b] * share) >> SHARE_BITS;
            }
        }
    }

    // Keep blade allocations fixed, empty unused buckets and assign consecutive
    // buffer offsets. Reset counters for this frame's generation.
    counters.capacity[0] = view.blade_capacity;
    counters.capacity[1] = view.blade_capacity / 2u;
    counters.capacity[2] = view.blade_capacity / 2u;
    var base = 0u;
    for (var b = 0u; b < MAX_BUCKETS; b++) {
        if b >= buckets {
            counters.capacity[b] = 0u;
        }
        counters.base[b] = base;
        base += counters.capacity[b];
        atomicStore(&counters.count[b], 0u);
    }
    atomicStore(&counters.slices, 0u);
}

@compute @workgroup_size(1)
fn write_draw_args() {
    // Only active buckets are drawn; arguments for unused buckets need no update.
    for (var bucket = 0u; bucket < view.bucket_count; bucket++) {
        let word = bucket / 4u;
        let lane = bucket % 4u;
        let shape = bucket / 3u;
        let lod = view.bucket_lod[word][lane];
        // Select the shared index-buffer range for this class and detail level.
        let range = shape * 3u + lod;
        let instances = min(atomicLoad(&counters.count[bucket]), counters.capacity[bucket]);
        // The vertex shader decodes bucket and LOD from the vertex index.
        let base_vertex = i32((bucket * 3u + lod) * class_stride(shape));
        draw_args[bucket] = DrawArgs(
            shapes.index_count[range / 4u][range % 4u],
            instances,
            shapes.first_index[range / 4u][range % 4u],
            base_vertex,
            0u,
        );

        // Give each branched class's first bucket a separate shadow draw command.
        // Near-detail shadows use simpler leaves. Other detail levels, selected
        // by an orthographic view, use the same geometry as the main draw.
        if shape > 0u && bucket % 3u == 0u {
            var count = shapes.index_count[range / 4u][range % 4u];
            var first = shapes.first_index[range / 4u][range % 4u];
            if lod == 0u {
                count = shapes.shadow_index_count[shape / 4u][shape % 4u];
                first = shapes.shadow_first_index[shape / 4u][shape % 4u];
            }
            draw_args[MAX_BUCKETS + shape] = DrawArgs(count, instances, first, base_vertex, 0u);
        }
    }
}
