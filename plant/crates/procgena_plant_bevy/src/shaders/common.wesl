//! procgena_plant_bevy::shaders::common — data layouts and pure functions shared by the
//! compute passes and the draws. No bindings live here.
//!
//! Functions marked "twin" must stay identical to their counterparts in
//! `src/mirror.rs`, which is where their properties are tested.

const TAU: f32 = 6.283185307;
/// Seconds after which `Frame::time` starts again. Twin of
/// `clock::TIME_PERIOD`.
const TIME_PERIOD: f32 = 3600.0;
const TILE_POINTS: u32 = 1024u;

/// The parts a field's fill is counted in. Twin of
/// `shader_model::FILL_PARTS`.
const FILL_PARTS: u32 = 16777216u;

/// Whether candidate `point` of its tile is placed, in a field with `fill`
/// (`axis.z`) where its mask reads `mask` (0 to 255). Twin of
/// `shader_model::is_placed`.
fn is_placed(point: u32, fill: u32, mask: u32) -> bool {
    return point * (FILL_PARTS / TILE_POINTS * 255u) < fill * mask;
}
const WIDTH_RANGE: f32 = 4.0;
const LOD_VERTEX_STRIDE: u32 = 64u;
/// Vertex-index space reserved per branched stem, limiting geometry to 1024 vertices.
const BRANCHED_VERTEX_STRIDE: u32 = 1024u;
/// Maximum number of plant shapes a view supports.
///
/// Plants with matching branch counts share a class; class 0 holds unbranched
/// blades. Twin of `view::SHAPE_CLASSES`.
const SHAPE_CLASSES: u32 = 33u;
/// Maximum number of instance groups, called buckets, per view.
///
/// Each shape class has three detail levels. Class `c` uses buckets `3c`, `3c + 1`
/// and `3c + 2`; `View::bucket_count` gives the number in use.
/// Twin of `view::MAX_BUCKETS`.
const MAX_BUCKETS: u32 = 99u;
/// Packed `vec4<u32>` count for one value per bucket. Twin of `view::BUCKET_WORDS`.
const BUCKET_WORDS: u32 = 25u;
/// Number of four-value vectors needed for one value per shape class.
///
/// Twin of `view::CLASS_WORDS`.
const CLASS_WORDS: u32 = 9u;
/// Number of draw-command slots reserved for each view.
///
/// Each bucket has a slot, followed by shadow slots indexed by shape class.
/// Class `c` uses shadow slot `MAX_BUCKETS + c`. Twin of `view::DRAW_ARGS_COUNT`.
const DRAW_ARGS_COUNT: u32 = 132u;
/// Twin of `gpu_layout::NO_MAP`.
const NO_MAP: u32 = 0xffffffffu;
/// Twin of `trample::TEXELS`.
const TRAMPLE_TEXELS: i32 = 256;

/// Per frame, shared by every view. The wind lives in its own `Wind`
/// uniform (`wind.wesl`), so another wind model can be dropped in.
struct Frame {
    time: f32,
    previous_time: f32,
    chunk_size: f32,
    chunk_count: u32,
    /// Rows in a blade of each LOD (`PlantSettings::blade_rows`), x for LOD 0.
    lod_rows: vec4<u32>,
}

/// `frequency` (radians per second) rounded to a whole number of cycles per
/// `TIME_PERIOD`, so that `sin(time * periodic(f))` is continuous where
/// `time` wraps. The rounding is below 0.1% for any frequency above 1.
fn periodic(frequency: f32) -> f32 {
    let step = TAU / TIME_PERIOD;
    return round(frequency / step) * step;
}

/// Per view.
struct View {
    /// Frustum planes, normals pointing inward, normalized.
    planes: array<vec4<f32>, 6>,
    /// xyz: camera position; w: 1 for orthographic views.
    camera_position: vec4<f32>,
    /// xyz: camera forward; w: metres per pixel per unit of clip-space w.
    camera_forward: vec4<f32>,
    /// xyz: where a blade's shadow lands, per metre of blade height; w: 1
    /// when a directional light casts shadows.
    shadow_offset: vec4<f32>,
    /// xyz: the origin instance positions are relative to; w: their extent.
    instance_origin: vec4<f32>,
    radius: f32,
    lod1_start: f32,
    lod2_start: f32,
    lod_band: f32,
    /// Where the mid-range thinning begins, in metres.
    mid_range_start: f32,
    density_scale: f32,
    /// LOD for every blade, or 3 for distance-based LOD.
    fixed_lod: u32,
    max_visible_chunks: u32,
    /// Interactors stamped this frame.
    interactor_count: u32,
    /// The render layers the view draws, one bit each.
    layers: u32,
    /// Narrowest a blade is drawn, in pixels.
    min_width_px: f32,
    /// Metres taken off every distance to the camera before the radius, the
    /// thinning and the levels of detail apply (`radius_from_ground`).
    distance_offset: f32,
    /// Number of buckets in use: three for blades and three per branched shape class.
    /// Later buckets hold no instances and are not drawn.
    bucket_count: u32,
    /// Slots reserved for the first blade bucket (`PlantSettings::instance_capacity`).
    /// The other two blade buckets each receive half this number.
    blade_capacity: u32,
    /// Total stem slots shared by branched buckets (`PlantSettings::plant_capacity`).
    /// Zero when there are no branched shape classes.
    plant_capacity: u32,
    /// Drawing detail level for each bucket, packed four values per vector.
    /// Read bucket `b` at `[b / 4u][b % 4u]`, matching `view::bucket_value`.
    bucket_lod: array<vec4<u32>, BUCKET_WORDS>,
    /// xy: the trample window's lowest cell this frame; zw: last frame's.
    trample_origin: vec4<i32>,
    /// x: cell size in metres; y: flattening recovered this frame (0–1).
    trample_step: vec4<f32>,
    /// xyz: direction to the first directional light; w: 1 when there is one.
    sun: vec4<f32>,
    /// Density along the distance table (`PlantSettings`): x: the share kept
    /// where LOD 2 starts; y: the share kept where the last fade starts; z:
    /// the extra share kept from LOD 1 on; w: where the last fade starts, as
    /// a share of the radius.
    lod_density: vec4<f32>,
    /// x: the most a thinned blade widens; y: the largest interactor radius;
    /// z: how far an edge-on blade turns toward the camera; w: the vertical
    /// range instance positions are stored over, in metres either side of
    /// the origin.
    limits: vec4<f32>,
}

/// Demand and storage allocation for each bucket in a view.
///
/// Drawing reads this table to locate instances. `generate.wesl` writes the same
/// buffer through `BucketCounters`, whose atomic counters allow concurrent updates.
/// Mirrors the offsets in `view.rs` (`SLICE_COUNTER`, `BASE_AT`, `CAPACITY_AT`).
struct BucketTable {
    /// Requested instances this frame, including those that did not fit.
    count: array<u32, MAX_BUCKETS>,
    /// Number of visible chunk slices requested for generation this frame.
    slices: u32,
    /// Starting offset of each bucket in the instance buffer.
    base: array<u32, MAX_BUCKETS>,
    /// Maximum number of instances each bucket can store this frame.
    capacity: array<u32, MAX_BUCKETS>,
}

/// Sentinel for plants with no drawable mesh class. Twin of `gpu_layout::NO_CLASS`.
const NO_CLASS: u32 = 0xffffffffu;

/// Packed branch settings for one level. Mirrors `LevelGpu` in `gpu_layout.rs`.
struct Level {
    /// x: rendered children per parent (0 disables); y: arrangement (0 opposite, 1
    /// alternate); z: bare base fraction; w: longest-child position within the leafy
    /// part.
    attach: vec4<f32>,
    /// x: maximum child-to-parent length ratio; y: child width-to-length ratio; z:
    /// direction (0 perpendicular, 1 along parent); w: wind stiffness.
    size: vec4<f32>,
    /// x: outline profile; y: downward droop; z: twist in turns; w: centre-line fold.
    form: vec4<f32>,
    /// rgb: segment base colour; w: rotation between attachment points, in half-turns.
    root_color: vec4<f32>,
    tip_color: vec4<f32>,
}

/// One plant, or one plant's accent, of one field. Mirrors `FieldGpu` in
/// `gpu_layout.rs`.
struct Field {
    /// xyz: minimum field corner; w: conservative root-to-tip reach for culling, stored
    /// on the field's first entry.
    min_corner: vec4<f32>,
    /// x, y: size; z: heightmap scale; w: tile size.
    extent: vec4<f32>,
    /// x: shortest, y: tallest, z: width, w: density.
    shape: vec4<f32>,
    /// x: lean, y: stiffness, z: how much less the wind moves the field's
    /// blades (1 - its wind scale), w: root occlusion.
    motion: vec4<f32>,
    root_color: vec4<f32>,
    tip_color: vec4<f32>,
    /// x: seed, y: tiles per chunk side, z: mask offset, w: height offset.
    placement: vec4<u32>,
    map_sizes: vec4<u32>,
    /// rgb: the ground's colour; w: how far roots blend toward it.
    ground: vec4<f32>,
    /// x: specular strength; y: toon bands (0 uses physical shading); z: main-segment
    /// outline profile.
    look: vec4<f32>,
    /// x: chance of selecting this plant after earlier plants declined a root
    /// (1 for the last plant with a positive density, 0 for accents); y: clustering;
    /// z: plant count, excluding accents; w: maximum stem length across all
    /// plants and accents. The count and maximum are repeated on every entry.
    mix: vec4<f32>,
    /// x: twist, in turns; y: droop; z: fold; w: head, a share of the length.
    form: vec4<f32>,
    /// rgb: head colour; w: head width.
    head: vec4<f32>,
    /// rgb: patch colour over tip colour; w: patch amount.
    patches: vec4<f32>,
    /// x: patch size; y: dry tips; z: dead blades; w: how far tufts can pull
    /// a root, the widest of the field's tufted plants, or 0, for culling.
    variation: vec4<f32>,
    /// x: this plant's tuft pull; y: its tuft size; z: the field's bare-patch
    /// noise threshold (-1: none); w: the field's bare patch size.
    tufts: vec4<f32>,
    /// x: render layers the field is drawn on, one bit each; 0 while hidden.
    /// y: mesh class (`NO_CLASS` skips drawing); z: 1 if the field contains branched
    /// plants; w: stems sharing each root.
    access: vec4<u32>,
    /// x, y: the field's own X axis in world XZ; its Z axis is (-y, x).
    /// z: the field's fill, a whole number of 1/`FILL_PARTS`.
    axis: vec4<f32>,
    /// x: accent share (0 disables it); y: offset from this entry to its
    /// accent entry. A relative offset stays valid when the field moves
    /// within the table.
    accent: vec4<f32>,
    levels: array<Level, 2>,
}

/// One chunk. Mirrors `ChunkGpu` in `gpu_layout.rs`.
struct Chunk {
    bounds: vec4<f32>,
    ids: vec4<u32>,
}

/// Locations and lengths of mesh index ranges for drawing plants.
///
/// Entry `3c + l` selects shape class `c` at detail level `l`. Shadow entry `c`
/// selects that branched class's near-detail geometry with simpler leaves.
/// Values are packed four per vector. Twin of `resources::ShapeTable`.
struct ShapeTable {
    index_count: array<vec4<u32>, BUCKET_WORDS>,
    first_index: array<vec4<u32>, BUCKET_WORDS>,
    shadow_index_count: array<vec4<u32>, CLASS_WORDS>,
    shadow_first_index: array<vec4<u32>, CLASS_WORDS>,
}

/// Reserved vertex-index space for a draw of this mesh class.
fn class_stride(shape: u32) -> u32 {
    return select(BRANCHED_VERTEX_STRIDE, LOD_VERTEX_STRIDE, shape == 0u);
}

/// Twin of `mirror::hash`.
fn hash(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

/// Twin of `mirror::hash2`.
fn hash2(a: u32, b: u32) -> u32 {
    return hash(a ^ hash(b));
}

/// Twin of `mirror::unit`.
fn unit(h: u32) -> f32 {
    return f32(h >> 8u) / 16777216.0;
}

/// Twin of `DistanceLod::falloff`.
fn density_falloff(view: View, distance: f32) -> f32 {
    let into_mid = clamp((distance - (view.mid_range_start - view.lod_band * 0.5)) / view.lod_band, 0.0, 1.0);
    return distance_thinning(view, distance) * mix(1.0, view.lod_density.z, into_mid);
}

/// The distance table, before the mid-range thinning: 1 to the start of LOD 1,
/// `lod_density.x` where LOD 2 starts, `lod_density.y` where the fade starts,
/// 0 at the radius. A boundary that coincides with the next (an empty span)
/// is never divided by.
fn distance_thinning(view: View, distance: f32) -> f32 {
    let edge = view.radius * view.lod_density.w;
    if distance <= view.lod1_start {
        return 1.0;
    }
    if distance <= view.lod2_start {
        return mix(1.0, view.lod_density.x, clamp((distance - view.lod1_start) / (view.lod2_start - view.lod1_start), 0.0, 1.0));
    }
    if distance <= edge {
        return mix(view.lod_density.x, view.lod_density.y, clamp((distance - view.lod2_start) / (edge - view.lod2_start), 0.0, 1.0));
    }
    return mix(view.lod_density.y, 0.0, clamp((distance - edge) / max(view.radius - edge, 1e-4), 0.0, 1.0));
}

/// The share of blades a view raised by `distance_offset` keeps
/// (`radius_from_ground`): the ground it sees grows with the square of its
/// height, so the blades thin by as much and widen to cover, and their
/// number stays about the same at any height. 1 when the offset is 0. Twin
/// of `mirror::height_thinning`.
fn height_thinning(view: View) -> f32 {
    let share = view.radius / (view.radius + view.distance_offset);
    return share * share;
}

/// Twin of `DistanceLod::lod`.
fn select_lod(view: View, distance: f32, dither: f32) -> u32 {
    var lod = 0u;
    if dither < (distance - (view.lod1_start - view.lod_band * 0.5)) / view.lod_band {
        lod += 1u;
    }
    if dither < (distance - (view.lod2_start - view.lod_band * 0.5)) / view.lod_band {
        lod += 1u;
    }
    return lod;
}

/// Twin of `mirror::coverage_width`; `widest` is `View::limits.x`.
fn coverage_width(falloff: f32, widest: f32) -> f32 {
    return min(1.0 / sqrt(max(falloff, 1.0 / (widest * widest))), widest);
}

/// Fraction of branched plants retained at this distance. Keeps all plants until the
/// final fade, then reaches zero at the view radius. This avoids obvious gaps from
/// thinning large plants at mid range. Twin of `mirror::DistanceLod::fade`.
fn fade_falloff(view: View, distance: f32) -> f32 {
    let edge = view.radius * view.lod_density.w;
    return 1.0 - min(max((distance - edge) / max(view.radius - edge, 1e-4), 0.0), 1.0);
}

/// Width multiplier along a segment, with `t` from base (0) to tip (1). Profile 0
/// preserves the grass blade's taper; profile 1 gives an oval leaf. Twin of
/// `mirror::profile_width`.
fn profile_width(t: f32, profile: f32) -> f32 {
    let pointed = 1.0 - pow(t, 1.4);
    let oval = sqrt(max(4.0 * t * (1.0 - t), 0.0));
    return mix(pointed, oval, profile);
}

/// Conservative root-to-tip reach relative to stem height; 1 for unbranched plants.
/// Twin of `scatters::reach_factor`.
fn reach_factor(field: Field) -> f32 {
    return 1.0 + field.levels[0].size.x * (1.0 + field.levels[1].size.x);
}

/// Direction of stem `k` in a shared-root crown. Stem 0 uses `crown`; the others are
/// evenly spaced with seed-based jitter. Twin of `mirror::stem_facing`.
fn stem_facing(crown: f32, k: u32, count: u32, seed: u32) -> f32 {
    if k == 0u {
        return crown;
    }
    let jitter = 0.35 * (unit(hash(seed ^ 0x5e3u)) - 0.5);
    return fract(crown + (f32(k) + jitter) / f32(count));
}

fn quantize(value: f32, max_value: f32) -> u32 {
    return u32(round(clamp(value, 0.0, 1.0) * max_value));
}

/// Steps of last frame's bend per unit of bend. Twin of
/// `mirror::BEND_DELTA_STEPS`.
const BEND_DELTA_STEPS: f32 = 128.0;

/// One axis of a blade's bend in 16 bits: the bend in the low 12 (8 bits
/// made the blades step visibly as the wind moved them), and last frame's
/// bend, for motion vectors, as a 4-bit step from it. Twin of
/// `mirror::pack_bend`.
fn pack_bend(bend: f32, previous: f32) -> u32 {
    let now = quantize(bend * 0.5 + 0.5, 4095.0);
    let decoded = f32(now) / 4095.0 * 2.0 - 1.0;
    let step = clamp(round((previous - decoded) * BEND_DELTA_STEPS), -8.0, 7.0);
    return now | (u32(i32(step) + 8) << 12u);
}

/// This frame's and last frame's bend from `pack_bend`'s 16 bits.
fn unpack_bend(bits: u32) -> vec2<f32> {
    let now = f32(bits & 0xfffu) / 4095.0 * 2.0 - 1.0;
    let step = f32(i32((bits >> 12u) & 0xfu) - 8);
    return vec2<f32>(now, now + step / BEND_DELTA_STEPS);
}

/// A blade as the generation pass produces it.
struct BladeData {
    /// World-space root.
    root: vec3<f32>,
    facing: f32,
    /// Fractions of the style's tallest blade and of WIDTH_RANGE × its width.
    height: f32,
    width: f32,
    seed: u32,
    lod: u32,
    field: u32,
    /// 0–63; see `clump_of`.
    clump: u32,
    bend: vec2<f32>,
    previous_bend: vec2<f32>,
}

/// Twin of `mirror::pack`, with positions made relative to the view's origin.
/// `height_range` is `View::limits.w`.
fn pack(blade: BladeData, origin: vec4<f32>, height_range: f32) -> vec4<u32> {
    let relative = blade.root - origin.xyz;
    let x = quantize(relative.x / (2.0 * origin.w) + 0.5, 65535.0);
    let z = quantize(relative.z / (2.0 * origin.w) + 0.5, 65535.0);
    let y = quantize(relative.y / (2.0 * height_range) + 0.5, 65535.0);
    let facing = u32(fract(blade.facing) * 256.0) & 0xffu;
    return vec4<u32>(
        x | (z << 16u),
        y | (facing << 16u) | ((blade.seed & 0xffu) << 24u),
        quantize(blade.height, 255.0) | (quantize(blade.width, 255.0) << 8u) | ((blade.field & 0xffu) << 16u)
            | ((blade.lod & 3u) << 24u) | ((blade.clump & 63u) << 26u),
        pack_bend(blade.bend.x, blade.previous_bend.x) | (pack_bend(blade.bend.y, blade.previous_bend.y) << 16u),
    );
}

/// An instance, unpacked into metres.
struct Instance {
    root: vec3<f32>,
    facing: f32,
    height: f32,
    width: f32,
    /// In [0, 1].
    seed: f32,
    /// The blade's clump tint, in [0, 1].
    clump: f32,
    field: u32,
    bend: vec2<f32>,
    previous_bend: vec2<f32>,
}

/// Twin of `mirror::unpack`, scaled by the field's style. `height_range` is
/// `View::limits.w`.
fn unpack(words: vec4<u32>, origin: vec4<f32>, height_range: f32) -> Instance {
    var out: Instance;
    out.field = (words.z >> 16u) & 0xffu;
    out.root = origin.xyz + vec3<f32>(
        (f32(words.x & 0xffffu) / 65535.0 - 0.5) * 2.0 * origin.w,
        (f32(words.y & 0xffffu) / 65535.0 - 0.5) * 2.0 * height_range,
        (f32(words.x >> 16u) / 65535.0 - 0.5) * 2.0 * origin.w,
    );
    out.facing = f32((words.y >> 16u) & 0xffu) / 256.0;
    out.seed = f32(words.y >> 24u) / 255.0;
    out.clump = f32(words.z >> 26u) / 63.0;
    out.height = f32(words.z & 0xffu) / 255.0;
    out.width = f32((words.z >> 8u) & 0xffu) / 255.0;
    let bend_x = unpack_bend(words.w & 0xffffu);
    let bend_z = unpack_bend(words.w >> 16u);
    out.bend = vec2<f32>(bend_x.x, bend_z.x);
    out.previous_bend = vec2<f32>(bend_x.y, bend_z.y);
    return out;
}

/// One stem's droop and arch, for `blade_curve`, from a main segment's droop
/// setting (0 to 2), the stem's random number and its normalised height. The
/// part of the setting up to 1 varies per stem, tall stems drooping the most;
/// the part above 1 lowers every stem's tip onto the ground alike. Twin of
/// `mirror::stem_droop`.
fn stem_droop(setting: f32, random: f32, height: f32) -> vec2<f32> {
    return vec2<f32>(min(setting, 1.0) * random * (0.5 + 0.5 * height), clamp(setting - 1.0, 0.0, 1.0));
}

/// The three control points of a blade, in world space relative to its root.
/// Twin of `mirror::blade_curve`, with the lean and bend already in world XZ.
/// `droop` is 0 without `PLANT_SHAPE`, and the curve is then exactly the
/// plain blade's. `arch_wanted` (0 to 1) lowers the tip onto the ground; with 0
/// the curve is exactly the drooping blade's.
fn blade_curve(height: f32, lean: vec2<f32>, bend: vec2<f32>, droop_wanted: f32, arch_wanted: f32) -> array<vec3<f32>, 3> {
    let wanted = (lean + bend) * height;
    // An upright blade has no side to droop toward.
    let droop = select(0.0, droop_wanted, dot(wanted, wanted) > 1e-12);
    let arch = select(0.0, arch_wanted, dot(wanted, wanted) > 1e-12);
    let horizontal = min(length(wanted) + droop * 0.7 * height, height * 0.95);
    let offset = select(vec2<f32>(0.0), normalize(wanted) * horizontal, horizontal > 1e-6 && dot(wanted, wanted) > 1e-12);
    let p0 = vec3<f32>(0.0);
    var p2 = vec3<f32>(offset.x, sqrt(max(height * height - horizontal * horizontal, 0.0)) * (1.0 - 0.7 * droop), offset.y);
    let ratio = horizontal / max(height, 1e-6);
    var p1 = vec3<f32>(0.0, height * max(1.0 - ratio, 0.05 * max(ratio, 1.0)), 0.0);
    p1.y += (height * 0.9 - p1.y) * min(droop * 2.0, 1.0);
    if arch > 0.0 {
        // Blend towards an arch whose tip rests on the ground. Its top is 0.25 of
        // its span for an undrooped stem, leaving the root at 45°, and flattens to
        // 0.12 as droop rises. The span is sized with the length estimate below,
        // so that the correction leaves a full arch's tip on the ground. Both ends
        // are rooted, so the arch faces along the lean alone and the bend only
        // sways its top, by half as much as it would move a free tip.
        let along = select(normalize(wanted), normalize(lean), dot(lean, lean) > 1e-12);
        let rise = 0.25 - 0.13 * droop;
        let span = height * 3.0 / (2.0 + sqrt(1.0 + 16.0 * rise * rise));
        let tip = vec3<f32>(along.x * span, 0.0, along.y * span);
        let sway = bend * height * 0.5;
        let top = vec3<f32>(along.x * span * 0.5 + sway.x, 2.0 * rise * span, along.y * span * 0.5 + sway.y);
        p1 = p1 + (top - p1) * arch;
        p2 = p2 + (tip - p2) * arch;
    }
    for (var i = 0; i < 2; i++) {
        let chord = distance(p2, p0);
        let polygon = distance(p1, p0) + distance(p2, p1);
        let scale = height / max((2.0 * chord + polygon) / 3.0, 1e-6);
        p1 = p0 + (p1 - p0) * scale;
        p2 = p1 + (p2 - p1) * scale;
    }
    return array<vec3<f32>, 3>(p0, p1, p2);
}

/// A random number in [0, 1) for a blade and `salt`, from the seed, facing
/// and clump its instance carries.
fn instance_random(instance: Instance, salt: u32) -> f32 {
    let bits = u32(round(instance.seed * 255.0)) | (u32(instance.facing * 256.0) << 8u)
        | (u32(round(instance.clump * 63.0)) << 16u);
    return unit(hash(bits ^ salt));
}

/// The colour of dried grass of `color`'s brightness.
fn straw_of(color: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(color, vec3<f32>(0.3, 0.59, 0.11))) * vec3<f32>(1.25, 1.1, 0.6);
}

/// Rows in a blade of level of detail `lod`.
fn lod_rows(frame: Frame, lod: u32) -> u32 {
    return frame.lod_rows[lod];
}

/// Twin of `mirror::trample_pack`. A push is packed with its strength in the
/// top 10 bits, so `atomicMax` on the packed word keeps the strongest push,
/// and its direction as two 11-bit components below, so unpacking needs no
/// trigonometry.
fn trample_pack(push: vec2<f32>) -> u32 {
    let strength = clamp(length(push), 0.0, 1.0);
    let direction = select(vec2<f32>(1.0, 0.0), push / max(length(push), 1e-6), strength > 1e-6);
    let d = vec2<u32>(round((direction * 0.5 + 0.5) * 2047.0));
    return (u32(round(strength * 1023.0)) << 22u) | (d.x << 11u) | d.y;
}

/// Twin of `mirror::trample_unpack`.
fn trample_unpack(word: u32) -> vec2<f32> {
    let strength = f32(word >> 22u) / 1023.0;
    let direction = vec2<f32>(f32((word >> 11u) & 0x7ffu), f32(word & 0x7ffu)) / 2047.0 * 2.0 - 1.0;
    return direction * strength;
}

/// Twin of `mirror::trample_slot`: where a world cell lives in the window.
/// The side is a power of two, so a mask is the positive modulo, negative
/// cells included; integer division made blade generation 30% slower.
fn trample_slot(cell: vec2<i32>) -> u32 {
    let wrapped = cell & vec2<i32>(TRAMPLE_TEXELS - 1);
    return u32(wrapped.y * TRAMPLE_TEXELS + wrapped.x);
}

/// Twin of `mirror::trample_cell`: the world cell slot `slot` holds while
/// the window's lowest cell is `origin`.
fn trample_cell(slot: vec2<i32>, origin: vec2<i32>) -> vec2<i32> {
    return origin + ((slot - origin) & vec2<i32>(TRAMPLE_TEXELS - 1));
}

/// Twin of `mirror::in_trample_window`.
fn in_trample_window(cell: vec2<i32>, origin: vec2<i32>) -> bool {
    return all(cell >= origin) && all(cell < origin + TRAMPLE_TEXELS);
}
