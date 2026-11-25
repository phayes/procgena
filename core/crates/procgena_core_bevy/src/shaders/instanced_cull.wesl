// Chooses which copies of one Scatter one view draws, and at which detail
// level, for the shared instanced renderer.
//
// Each frame, per view and Scatter: copy the Scatter's template draw
// arguments into the view's, then run `reset_counts`, `cull` and
// `write_args`. Copies are sorted by slot. List `slot * levels + level` holds
// the copies drawn at that level, from entry
// `level * copy_count + slots[slot].start` of `visible`. Every part draws one
// list, through its indexed indirect draw (five words) in `draw_args`.

import procgena_core_bevy::shaders::dither::{dither_level, is_drawn, switch_distance};

/// Mirrors `CullUniform` in `src/instanced/render.rs`.
struct CullView {
    /// Inward-facing frustum planes: left, right, bottom, top, near, far.
    planes: array<vec4<f32>, 6>,
    /// Where detail levels are measured from, in `xyz`: the camera, also
    /// for its shadow views.
    lod_origin: vec4<f32>,
    /// What every slot's bands are multiplied by, besides each copy's size:
    /// the quality setting's distance scale times how much larger things
    /// look through this view's camera than through the default lens.
    band_scale: f32,
    copy_count: u32,
    list_count: u32,
    part_count: u32,
    /// The most detail levels of any slot.
    levels: u32,
    /// 1 for shadow views: one level per copy, without dithering.
    shadow: u32,
    /// 1 to keep copies in front of the near plane, for directional shadows.
    skip_near: u32,
    /// Attached copies' counts, read by `instanced_attach.wesl` only.
    host_count: u32,
    row_threads: u32,
    attached_count: u32,
}

/// Mirrors `GpuCopy`.
struct Copy {
    /// World-space bounding sphere: centre in `xyz`, radius in `w`.
    sphere: vec4<f32>,
    /// World-space origin in `xyz`, where levels are measured from; the
    /// copy's size over its Lod's reference size in `w`.
    origin: vec4<f32>,
    slot: u32,
    /// Index in the Scatter's mesh records.
    record: u32,
}

/// Mirrors `GpuSlot`.
struct Slot {
    /// Index of the slot's first copy.
    start: u32,
    count: u32,
    level_count: u32,
    /// Bit `l` set when level `l` casts shadows.
    shadow_mask: u32,
    /// Index of level 0's range in `ranges`.
    first_range: u32,
}

@group(0) @binding(0) var<uniform> view: CullView;
@group(0) @binding(1) var<storage, read> copies: array<Copy>;
@group(0) @binding(2) var<storage, read> slots: array<Slot>;
/// Every slot's level ranges for a copy of its reference size at High
/// quality, packed as `LodRange::to_vec4` packs them.
@group(0) @binding(3) var<storage, read> ranges: array<vec4<f32>>;
/// The list each part draws.
@group(0) @binding(4) var<storage, read> part_lists: array<u32>;
@group(0) @binding(5) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read_write> draw_args: array<u32>;
@group(0) @binding(7) var<storage, read_write> visible: array<vec2<u32>>;

/// Words per indexed indirect draw.
const DRAW_ARGS_WORDS: u32 = 5u;
/// Word of the instance count within an indexed indirect draw.
const INSTANCE_COUNT_WORD: u32 = 1u;

/// Sets every list's count to zero.
@compute @workgroup_size(64)
fn reset_counts(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= view.list_count {
        return;
    }
    atomicStore(&counts[id.x], 0u);
}

/// Whether a sphere is at least partly inside the view's frustum.
fn in_frustum(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        if i == 4u && view.skip_near == 1u {
            continue;
        }
        let plane = view.planes[i];
        if dot(plane.xyz, center) + plane.w < -radius {
            return false;
        }
    }
    return true;
}

/// Adds copy `index` to its slot's list at `level`.
fn emit(index: u32, copy: Copy, level: u32, dither: i32) {
    let list = copy.slot * view.levels + level;
    let at = atomicAdd(&counts[list], 1u);
    visible[level * view.copy_count + slots[copy.slot].start + at] =
        vec2<u32>(copy.record, bitcast<u32>(dither));
}

/// Tests one copy against the view and adds it to the lists of the levels
/// it is drawn at. In a fade band a camera draws both levels, dithered; a
/// shadow view draws the level whose switch distance was passed last, if
/// that level casts shadows.
@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= view.copy_count {
        return;
    }
    let copy = copies[id.x];
    if !in_frustum(copy.sphere.xyz, copy.sphere.w) {
        return;
    }
    let slot = slots[copy.slot];
    if slot.level_count == 0u {
        return;
    }

    // Distance in the slot's reference metres.
    let distance = length(copy.origin.xyz - view.lod_origin.xyz)
        / (copy.origin.w * view.band_scale);
    if view.shadow == 1u {
        let last = ranges[slot.first_range + slot.level_count - 1u];
        if distance >= 0.5 * last.z + 0.5 * last.w {
            return;
        }
        var level = 0u;
        for (var l = 1u; l < slot.level_count; l++) {
            if distance >= switch_distance(ranges[slot.first_range + l]) {
                level = l;
            }
        }
        if (slot.shadow_mask & (1u << level)) != 0u {
            emit(id.x, copy, level, 0);
        }
        return;
    }
    for (var level = 0u; level < slot.level_count; level++) {
        let dither = dither_level(ranges[slot.first_range + level], distance);
        if is_drawn(dither) {
            emit(id.x, copy, level, dither);
        }
    }
}

/// Writes each part's instance count from its list's.
@compute @workgroup_size(64)
fn write_args(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= view.part_count {
        return;
    }
    draw_args[id.x * DRAW_ARGS_WORDS + INSTANCE_COUNT_WORD] = atomicLoad(&counts[part_lists[id.x]]);
}
