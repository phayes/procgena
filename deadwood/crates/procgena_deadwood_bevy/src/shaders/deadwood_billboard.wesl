// Turns thin stems drawn as flat strips toward whatever is viewing them: the camera, or
// a light while its shadows are drawn. Shared by the main and prepass vertex
// shaders.

import bevy_pbr::render::mesh_view_bindings::view;

/// A turned vertex position and its lighting normal, in mesh coordinates.
struct TurnedVertex {
    position: vec3<f32>,
    normal: vec3<f32>,
}

/// Returns true while drawing shadows. Bevy picks levels of detail from the
/// camera's position, so a view at any other position is a light's.
fn is_shadow_view() -> bool {
    return any(view.lod_view_world_position != view.world_position);
}

/// Returns one strip vertex turned around its axis to face the current
/// view, with a lighting normal that leans out toward the strip's edge, so
/// the strip is lit like the round stem it stands in for.
///
/// `pivot.xyz` is the point to turn around and `pivot.w` the sideways
/// distance from it; `axis.xyz` is the unit axis and `axis.w` the distance
/// along it. `local_from_world` converts world positions to the mesh's own
/// coordinates, which the result is in.
fn turn_billboard(
    pivot: vec4<f32>,
    axis: vec4<f32>,
    local_from_world: mat4x4<f32>,
) -> TurnedVertex {
    // Direction toward the viewer, in mesh coordinates. Orthographic views
    // (such as sunlight shadows) look along one direction everywhere.
    var to_view: vec3<f32>;
    if view.clip_from_view[3][3] == 1.0 {
        to_view = (local_from_world * vec4<f32>(view.world_from_view[2].xyz, 0.0)).xyz;
    } else {
        to_view = (local_from_world * vec4<f32>(view.world_position, 1.0)).xyz - pivot.xyz;
    }
    to_view = normalize(to_view);

    // The viewer's right, across the stem. When looking straight along the
    // stem, that is undefined, so use the view's own right instead.
    let up = axis.xyz;
    var right = cross(up, to_view);
    if dot(right, right) < 1e-8 {
        let view_right = (local_from_world * vec4<f32>(view.world_from_view[0].xyz, 0.0)).xyz;
        right = view_right - up * dot(view_right, up);
    }
    right = normalize(right);

    // While drawing shadows, push the strip away from the light, so the
    // copy turned toward the light doesn't cast a shadow onto the copy
    // turned toward the camera.
    var position = pivot.xyz + right * pivot.w + up * axis.w;
    if is_shadow_view() {
        position -= to_view * length(vec2<f32>(pivot.w, axis.w));
    }

    // Lean each edge's normal 60 degrees out from facing the viewer. Blended
    // across the strip, the normal then sweeps round like a cylinder's
    // surface: facing the viewer in the middle, turning up on one edge and
    // down on the other.
    let facing = cross(right, up);
    let edge = sign(pivot.w);

    var out: TurnedVertex;
    out.position = position;
    out.normal = normalize(facing * 0.5 + right * (edge * 0.866));
    return out;
}
