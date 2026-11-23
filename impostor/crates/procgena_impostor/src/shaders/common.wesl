// The impostor material's bindings, and where an impostor quad's corners go
// for the current view. Shared by the forward and prepass shaders.

import bevy_pbr::render::mesh_view_bindings::view;
import package::shaders::layout::{view_basis, view_cell, view_direction};

/// The baked object's bounding sphere and the atlas size; see `material.rs`.
struct ImpostorParams {
    center: vec3<f32>,
    radius: f32,
    frames: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> impostor: ImpostorParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var albedo_atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var atlas_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_atlas: texture_2d<f32>;

/// The per-instance input of instanced pipelines: the instance's mesh
/// record index, and its dither level as the bits of an `i32`.
struct Instance {
    @location(12) instance: vec2<u32>,
}

/// One corner of the quad for this view: where it is in the object's
/// coordinates, where it samples the atlas, and the view's axes.
struct Corner {
    position: vec3<f32>,
    uv: vec2<f32>,
    basis: mat3x3<f32>,
}

/// Places quad corner `corner` (each of x and y is -1 or 1) for the current
/// view: facing the nearest baked view of the viewer, across the bounding
/// sphere. Orthographic views, such as directional shadows, look along
/// their own axis.
fn impostor_corner(corner: vec2<f32>, local_from_world: mat4x4<f32>) -> Corner {
    var to_view: vec3<f32>;
    if view.clip_from_view[3][3] == 1.0 {
        to_view = (local_from_world * vec4<f32>(view.world_from_view[2].xyz, 0.0)).xyz;
    } else {
        to_view = (local_from_world * vec4<f32>(view.world_position, 1.0)).xyz - impostor.center;
    }
    let cell = view_cell(to_view, impostor.frames);
    let basis = view_basis(view_direction(cell, impostor.frames));

    var out: Corner;
    out.position = impostor.center + (basis[0] * corner.x + basis[1] * corner.y) * impostor.radius;

    // In a shadow view, move the quad to the far side of the object from
    // the light. Its shadow on other things lands in the same place, since
    // it moves along the light's rays, but it no longer shadows the
    // camera-facing quad, which stands through the object's middle.
    if any(view.lod_view_world_position != view.world_position) {
        out.position -= basis[2] * impostor.radius;
    }
    let in_cell = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    out.uv = (vec2<f32>(cell) + in_cell) / f32(impostor.frames);
    out.basis = basis;
    return out;
}

/// The axes of the baked view an atlas `uv` lies in.
fn atlas_basis(uv: vec2<f32>) -> mat3x3<f32> {
    let frames = impostor.frames;
    let cell = min(vec2<u32>(floor(uv * f32(frames))), vec2<u32>(frames - 1u));
    return view_basis(view_direction(cell, frames));
}
