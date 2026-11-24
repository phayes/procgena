// Chooses which pooled rocks one view draws, and at which detail level.
//
// Run `reset_counts`, then `cull`, once per view and pool each frame. Before
// `reset_counts`, the view's draw arguments must hold a copy of the pool's: one
// indexed indirect draw per (variant, detail level), five words each. `cull`
// counts the rocks of each draw in its instance-count word and lists them in
// `visible`, where draw (v, l) reads entries from `l * instance_count + variant_start[v]`.

import package::shaders::pool_types::{PoolInstance, instance_center, instance_linear};
import procgena_core_bevy::shaders::dither::{dither_level, is_drawn, switch_distance};

/// Mirrors `CullUniform` in `src/pool/render/views.rs`.
struct CullView {
    /// Inward-facing frustum planes: left, right, bottom, top, near, far.
    planes: array<vec4<f32>, 6>,
    /// The position detail levels are chosen from, in `xyz`.
    lod_origin: vec4<f32>,
    /// Camera-distance limits of each detail level, packed as
    /// `LodRange::to_vec4` packs them: fade in over `x..y`, fully shown over
    /// `y..z`, fade out over `z..w`.
    lod_ranges: array<vec4<f32>, 8>,
    instance_count: u32,
    variant_count: u32,
    lod_count: u32,
    /// 1 for shadow views: one detail level per rock, no fading.
    shadow: u32,
    /// 1 to keep rocks in front of the near plane, for directional shadows.
    skip_near: u32,
    /// Radius around a rock's centre that contains its mesh, before the
    /// rock's own scale.
    radius: f32,
}

@group(0) @binding(0) var<uniform> cull_view: CullView;
@group(0) @binding(1) var<storage, read> instances: array<PoolInstance>;
@group(0) @binding(2) var<storage, read> variant_start: array<u32>;
@group(0) @binding(3) var<storage, read_write> draw_args: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> visible: array<vec2<u32>>;

/// Words per indexed indirect draw.
const DRAW_ARGS_WORDS: u32 = 5u;
/// Word of the instance count within an indexed indirect draw.
const INSTANCE_COUNT_WORD: u32 = 1u;

/// Sets every draw's instance count to zero.
@compute @workgroup_size(64)
fn reset_counts(@builtin(global_invocation_id) id: vec3<u32>) {
    let draw = id.x;
    if draw >= cull_view.variant_count * cull_view.lod_count {
        return;
    }
    atomicStore(&draw_args[draw * DRAW_ARGS_WORDS + INSTANCE_COUNT_WORD], 0u);
}

/// Whether a sphere is at least partly inside the view's frustum.
fn in_frustum(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        if i == 4u && cull_view.skip_near == 1u {
            continue;
        }
        let plane = cull_view.planes[i];
        if dot(plane.xyz, center) + plane.w < -radius {
            return false;
        }
    }
    return true;
}

/// The single detail level a shadow view draws at `distance`: the level whose
/// switching distance (the middle of its fade-in band) was passed last.
fn shadow_lod(distance: f32) -> u32 {
    var lod = 0u;
    for (var l = 1u; l < cull_view.lod_count; l++) {
        if distance >= switch_distance(cull_view.lod_ranges[l]) {
            lod = l;
        }
    }
    return lod;
}

/// Adds rock `index` to the draw of its variant at detail level `lod`.
fn emit(index: u32, variant: u32, lod: u32, dither: i32) {
    let draw = variant * cull_view.lod_count + lod;
    let slot = atomicAdd(&draw_args[draw * DRAW_ARGS_WORDS + INSTANCE_COUNT_WORD], 1u);
    let at = lod * cull_view.instance_count + variant_start[variant] + slot;
    visible[at] = vec2<u32>(index, bitcast<u32>(dither));
}

/// Tests one rock against the view and adds it to the draws of the detail
/// levels it shows at. A rock in a fade band is added to both levels.
@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= cull_view.instance_count {
        return;
    }
    let instance = instances[index];
    let center = instance_center(instance);
    let linear = instance_linear(instance);
    let scale = max(length(linear[0]), max(length(linear[1]), length(linear[2])));
    if !in_frustum(center, cull_view.radius * scale) {
        return;
    }

    let distance = length(center - cull_view.lod_origin.xyz);
    if cull_view.shadow == 1u {
        emit(index, instance.variant, shadow_lod(distance), 0);
        return;
    }
    for (var lod = 0u; lod < cull_view.lod_count; lod++) {
        let dither = dither_level(cull_view.lod_ranges[lod], distance);
        if is_drawn(dither) {
            emit(index, instance.variant, lod, dither);
        }
    }
}
