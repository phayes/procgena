// The surface an impostor shows at one fragment of its quad: the three
// baked views nearest the viewer, each projected onto the viewer's ray, and
// blended, at the depth those views store. Shared by the forward and prepass
// shaders, so both find the same surface and write the same depth.
//
// The quad faces the nearest view, and each fragment's quad position says
// which view that is and where on its square the fragment lies. Its screen-space derivatives give
// the view's axes and size in the world, so the object's transform is never
// needed: materials that place impostors with their own transform (see
// `vertex.wesl`) draw the same surface.

import bevy_pbr::render::mesh_view_bindings::view;
import package::shaders::common::{albedo_atlas, atlas_sampler, impostor, normal_atlas};
import package::shaders::layout::{view_basis, view_direction, view_grid};
import package::shaders::place::{quad_across, quad_cell};

/// What an impostor shows at one fragment.
struct ImpostorSurface {
    /// Base colour, linear.
    color: vec3<f32>,
    /// How much of the fragment the object covers, 0 to 1; the shaders cut
    /// out below one half.
    coverage: f32,
    /// The surface's normal in the world.
    world_normal: vec3<f32>,
    /// Where the surface is in the world, in front of or behind the quad.
    world_position: vec3<f32>,
    /// The bounding sphere's radius in the world.
    world_radius: f32,
}

/// One baked view's contribution to a fragment, in the object's
/// coordinates.
struct ViewSample {
    color: vec3<f32>,
    coverage: f32,
    normal: vec3<f32>,
    position: vec3<f32>,
}

/// The surface at the fragment of an impostor quad at `world_position`
/// whose quad position is `uv`, as `impostor_corner` placed them.
///
/// Call it in uniform control flow, before any `discard`: it uses screen
/// space derivatives.
fn impostor_surface(world_position: vec3<f32>, uv: vec2<f32>) -> ImpostorSurface {
    let frames = impostor.frames;
    let f = f32(frames);
    let radius = impostor.radius;
    let center = impostor.center;
    let cell = quad_cell(uv);
    let across = quad_across(uv);

    // How the world position moves across the view's square, to the right
    // and up.
    let dp_x = dpdx(world_position);
    let dp_y = dpdy(world_position);
    let da_x = dpdx(across);
    let da_y = dpdy(across);
    let det = da_x.x * da_y.y - da_x.y * da_y.x;
    let safe_det = select(det, 1e-20, abs(det) < 1e-20);
    let along_x = (dp_x * da_y.y - dp_y * da_x.y) / safe_det;
    let along_y = (dp_y * da_x.x - dp_x * da_y.x) / safe_det;
    let right = normalize(along_x);
    let up = normalize(along_y);
    let world_basis = mat3x3<f32>(right, up, cross(right, up));

    // The drawn view's axes in the object's coordinates, and so the
    // object's turn and size in the world. The square spans the bounding
    // sphere's radius from its middle to each side.
    let object_basis = view_basis(view_direction(cell, frames, impostor.lowest_elevation));
    let world_from_object = world_basis * transpose(object_basis);
    let object_from_world = transpose(world_from_object);
    let scale = length(along_x) / radius;

    // Where the fragment lies on the quad, in the object's coordinates, and
    // where the object's centre is in the world.
    let on_quad = center + (object_basis[0] * across.x + object_basis[1] * across.y) * radius;
    let world_center = world_position - world_from_object * (on_quad - center) * scale;

    // Mip levels follow how fast the quad crosses the atlas: a whole view
    // of the atlas, 1 / frames of it, spans 2 across the square.
    let duv_x = vec2<f32>(da_x.x, -da_x.y) * (0.5 / f);
    let duv_y = vec2<f32>(da_y.x, -da_y.y) * (0.5 / f);

    // The viewer's ray through the fragment, and the direction from the
    // object towards the viewer, both in the object's coordinates.
    // Orthographic views, such as directional shadows, look along their own
    // axis.
    var ray: vec3<f32>;
    var to_view: vec3<f32>;
    if view.clip_from_view[3][3] == 1.0 {
        to_view = object_from_world * view.world_from_view[2].xyz;
        ray = -normalize(to_view);
    } else {
        ray = normalize(object_from_world * (world_position - view.world_position));
        to_view = object_from_world * (view.world_position - world_center);
    }

    // The three views around the viewer's direction, weighted by where it
    // falls in the triangle between them.
    let grid = view_grid(to_view, frames, impostor.lowest_elevation);
    let base = floor(grid);
    let t = grid - base;
    var corners: array<vec2<f32>, 3>;
    var weights: vec3<f32>;
    if t.x >= t.y {
        corners = array(base, base + vec2<f32>(1.0, 0.0), base + vec2<f32>(1.0, 1.0));
        weights = vec3<f32>(1.0 - t.x, t.x - t.y, t.y);
    } else {
        corners = array(base, base + vec2<f32>(0.0, 1.0), base + vec2<f32>(1.0, 1.0));
        weights = vec3<f32>(1.0 - t.y, t.y - t.x, t.x);
    }

    // Blend the views, each weighted by its coverage too, so the
    // background never darkens or bends the result.
    var color = vec3<f32>(0.0);
    var coverage = 0.0;
    var normal = vec3<f32>(0.0);
    var position = vec3<f32>(0.0);
    for (var i = 0u; i < 3u; i += 1u) {
        let corner = vec2<u32>(clamp(corners[i], vec2<f32>(0.0), vec2<f32>(f - 1.0)));
        let seen = view_sample(corner, on_quad, ray, duv_x, duv_y);
        let weight = weights[i] * seen.coverage;
        coverage += weight;
        color += weight * seen.color;
        normal += weight * seen.normal;
        position += weight * seen.position;
    }

    var out: ImpostorSurface;
    out.coverage = coverage;
    out.world_radius = radius * scale;
    if coverage > 0.0 {
        out.color = color / coverage;
        out.world_normal = normalize(world_from_object * normal);
        out.world_position = world_center + world_from_object * (position / coverage - center) * scale;
    } else {
        out.color = vec3<f32>(0.0);
        out.world_normal = world_basis[2];
        out.world_position = world_position;
    }
    return out;
}

/// What the view in atlas cell `cell` shows along the viewer's `ray`
/// through `on_quad`, both in the object's coordinates. `duv_x` and
/// `duv_y` are the quad's atlas derivatives, which pick the mip level.
fn view_sample(
    cell: vec2<u32>,
    on_quad: vec3<f32>,
    ray: vec3<f32>,
    duv_x: vec2<f32>,
    duv_y: vec2<f32>,
) -> ViewSample {
    var out: ViewSample;
    let radius = impostor.radius;
    let center = impostor.center;
    let basis = view_basis(view_direction(cell, impostor.frames, impostor.lowest_elevation));

    // A view seen from nearly edge-on, or from behind, shows nothing.
    let facing = dot(ray, basis[2]);
    if facing > -0.05 {
        return out;
    }

    // Where the ray meets the view's plane through the centre. A ray that
    // meets it outside the view's square misses the object.
    let hit = on_quad + ray * (dot(center - on_quad, basis[2]) / facing);
    let on_plane = square(hit, basis);
    if any(abs(on_plane) > vec2<f32>(1.0)) {
        return out;
    }

    // The view's texel there. It isn't moved by its depth (parallax
    // mapping): one step of that smears silhouettes outwards, since texels
    // just outside them hold the depth of the surface just inside. Blending
    // nearby views hides the small offsets between them instead.
    let at = cell_uv(cell, on_plane);
    let albedo = textureSampleGrad(albedo_atlas, atlas_sampler, at, duv_x, duv_y);
    let stored = textureSampleGrad(normal_atlas, atlas_sampler, at, duv_x, duv_y);
    out.color = albedo.rgb;
    out.coverage = albedo.a;
    out.normal = stored.xyz * 2.0 - 1.0;

    // The surface is where the ray reaches the texel's depth, so the depth
    // written matches what the fragment shows.
    let height = (stored.a * 2.0 - 1.0) * radius;
    out.position = hit + ray * (height / facing);
    return out;
}

/// Where `point`, in the object's coordinates, lies across the view with
/// axes `basis`: -1 to 1 over the bounding sphere, x to the right and y up.
fn square(point: vec3<f32>, basis: mat3x3<f32>) -> vec2<f32> {
    let offset = point - impostor.center;
    return vec2<f32>(dot(offset, basis[0]), dot(offset, basis[1])) / impostor.radius;
}

/// The atlas position in cell `cell` of `across`, a position across its
/// view as `square` gives it.
fn cell_uv(cell: vec2<u32>, across: vec2<f32>) -> vec2<f32> {
    let in_cell = vec2<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5);
    return (vec2<f32>(cell) + in_cell) / f32(impostor.frames);
}

/// The depth to write for `surface` in the current view, for
/// `@builtin(frag_depth)`.
///
/// Shadow views write it a little farther from the light. The surface
/// blended for the light's view and the one blended for the camera's differ
/// slightly, by more than ordinary shadow bias covers, which would speckle
/// impostors with their own shadow.
fn surface_depth(surface: ImpostorSurface) -> f32 {
    var position = surface.world_position;
    if any(view.lod_view_world_position != view.world_position) {
        var away: vec3<f32>;
        if view.clip_from_view[3][3] == 1.0 {
            away = -view.world_from_view[2].xyz;
        } else {
            away = normalize(position - view.world_position);
        }
        position += away * surface.world_radius * SHADOW_OFFSET;
    }
    return view_depth(position);
}

/// How far shadow views move impostor surfaces away from the light, as a
/// share of the bounding sphere's radius.
const SHADOW_OFFSET: f32 = 0.5;

/// The depth `world_position` has in the current view, for
/// `@builtin(frag_depth)`.
fn view_depth(world_position: vec3<f32>) -> f32 {
    let clip = view.clip_from_world * vec4<f32>(world_position, 1.0);
    return clip.z / clip.w;
}
