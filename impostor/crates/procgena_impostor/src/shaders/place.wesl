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
/// coordinates, its quad position (see `quad_uv`), and the view's axes.
struct Corner {
    position: vec3<f32>,
    uv: vec2<f32>,
    basis: mat3x3<f32>,
}

/// Places quad corner `corner` (each of x and y is -1 or 1) of the impostor
/// `params` describes, for the current view: facing the nearest baked view
/// of the viewer, across the bounding sphere and a margin (see
/// `quad_margin`), since the fragment shader blends nearby views too.
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
    let orthographic = view.clip_from_view[3][3] == 1.0;
    if orthographic {
        to_view = (local_from_world * vec4<f32>(view.world_from_view[2].xyz, 0.0)).xyz;
    } else {
        let eye = view.world_position - world_offset;
        to_view = (local_from_world * vec4<f32>(eye, 1.0)).xyz - params.center;
    }
    let cell = view_cell(to_view, params.frames, params.lowest_elevation);
    let basis = view_basis(view_direction(cell, params.frames, params.lowest_elevation));

    let across = corner * quad_margin(params.frames, length(to_view) / params.radius, orthographic);
    var out: Corner;
    out.position = params.center + (basis[0] * across.x + basis[1] * across.y) * params.radius;
    out.uv = quad_uv(cell, across);
    out.basis = basis;
    return out;
}

/// How far the quad reaches past the bounding sphere, as a share of its
/// radius, for a viewer `distance` radii from the sphere's centre (ignored
/// by `orthographic` views).
///
/// The fragment shader shows the object as seen along each ray, not as the
/// view the quad faces shows it, so the quad must cover the sphere as the
/// viewer sees it. Blended views lie up to about one atlas cell's angle from
/// the one the quad faces, which tilts the sphere's outline onto the quad
/// by about that angle; a near viewer's rays also spread the sphere's near
/// half outwards.
fn quad_margin(frames: u32, distance: f32, orthographic: bool) -> f32 {
    let tilt = 1.0 / cos(min(3.14159265 / f32(frames), 1.0));
    var spread = 1.0;
    if !orthographic {
        // The sphere seen from `distance` radii away, through a plane
        // through its centre; capped once the viewer is nearly inside it.
        let sine = min(1.0 / max(distance, 1.0), 0.6);
        spread = 1.0 / sqrt(1.0 - sine * sine);
    }
    return tilt * spread;
}

/// The quad position an impostor quad passes to its fragments: the atlas
/// cell it faces, and where on that view's square it is, with -1 to 1
/// spanning the bounding sphere, x to the right and y up. Both fit in one
/// `vec2` that interpolates exactly across the quad, since the cell is the
/// same at every corner; `quad_cell` and `quad_across` take it apart.
fn quad_uv(cell: vec2<u32>, across: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(cell) * QUAD_CELL_STRIDE + QUAD_CELL_STRIDE * 0.5 + across;
}

/// The atlas cell of a quad position; see `quad_uv`.
fn quad_cell(uv: vec2<f32>) -> vec2<u32> {
    return vec2<u32>(max(floor(uv / QUAD_CELL_STRIDE), vec2<f32>(0.0)));
}

/// Where on the view's square a quad position is; see `quad_uv`.
fn quad_across(uv: vec2<f32>) -> vec2<f32> {
    return uv - vec2<f32>(quad_cell(uv)) * QUAD_CELL_STRIDE - QUAD_CELL_STRIDE * 0.5;
}

/// The room each cell has in a quad position, wider than any quad reaches
/// (`quad_margin` is at most about 2.3).
const QUAD_CELL_STRIDE: f32 = 8.0;
