// Where an impostor quad's corners go for the current view, without any
// material bindings, so other materials can place impostors too.

import bevy_pbr::render::mesh_view_bindings::view;
import package::shaders::layout::{view_basis, view_cell, view_direction};

/// The baked object's bounding sphere and atlas layout, as
/// `ImpostorUniform` in `material.rs` holds them.
struct ImpostorParams {
    /// Centre of the bounding sphere, in the object's own coordinates.
    center: vec3<f32>,
    /// Radius of the bounding sphere.
    radius: f32,
    /// Views along each side of the atlas.
    frames: u32,
    /// The lowest viewer elevation baked, in degrees; -90 is the whole sphere.
    lowest_elevation: f32,
}

/// One corner of the quad for this view: where it is in the object's
/// coordinates, where it samples the atlas, and the view's axes.
struct Corner {
    position: vec3<f32>,
    uv: vec2<f32>,
    basis: mat3x3<f32>,
}

/// Places quad corner `corner` (each of x and y is -1 or 1) of the impostor
/// `params` describes, for the current view: facing the nearest baked view
/// of the viewer, across the bounding sphere.
///
/// `local_from_world` takes world positions into the object's coordinates.
/// `world_offset` is how far the object is drawn from where its transform
/// puts it, in world units, such as a sway in the wind; pass zero for none.
/// The offset itself is not added to the corner. Orthographic views, such as
/// directional shadows, look along their own axis.
fn impostor_corner(
    params: ImpostorParams,
    corner: vec2<f32>,
    local_from_world: mat4x4<f32>,
    world_offset: vec3<f32>,
) -> Corner {
    var to_view: vec3<f32>;
    if view.clip_from_view[3][3] == 1.0 {
        to_view = (local_from_world * vec4<f32>(view.world_from_view[2].xyz, 0.0)).xyz;
    } else {
        let eye = view.world_position - world_offset;
        to_view = (local_from_world * vec4<f32>(eye, 1.0)).xyz - params.center;
    }
    let cell = view_cell(to_view, params.frames, params.lowest_elevation);
    let basis = view_basis(view_direction(cell, params.frames, params.lowest_elevation));

    var out: Corner;
    out.position = params.center + (basis[0] * corner.x + basis[1] * corner.y) * params.radius;

    // In a shadow view, move the quad to the far side of the object from
    // the light. Its shadow on other things lands in the same place, since
    // it moves along the light's rays, but it no longer shadows the
    // camera-facing quad, which stands through the object's middle.
    if any(view.lod_view_world_position != view.world_position) {
        out.position -= basis[2] * params.radius;
    }
    let in_cell = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    out.uv = (vec2<f32>(cell) + in_cell) / f32(params.frames);
    out.basis = basis;
    return out;
}

/// The axes of the baked view drawn at this fragment, in world space: right
/// and up across the view's image, and back towards its viewer, as the
/// columns of a matrix. `world_position` and `uv` are the fragment's
/// interpolated world position and atlas position on an impostor quad.
///
/// Call it in uniform control flow, before any `discard`: it uses screen
/// space derivatives.
fn world_view_basis(world_position: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {
    // Solve for how the world position moves along the atlas's u and v.
    let dp_x = dpdx(world_position);
    let dp_y = dpdy(world_position);
    let duv_x = dpdx(uv);
    let duv_y = dpdy(uv);
    let along_u = dp_x * duv_y.y - dp_y * duv_x.y;
    let along_v = dp_y * duv_x.x - dp_x * duv_y.x;
    let det = duv_x.x * duv_y.y - duv_x.y * duv_y.x;

    // The atlas's u runs right across each view, and v runs down.
    let right = normalize(along_u * sign(det));
    let up = normalize(-along_v * sign(det));
    return mat3x3<f32>(right, up, cross(right, up));
}
