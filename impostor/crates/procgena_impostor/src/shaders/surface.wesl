// The surface an impostor shows at one fragment of its quad: the three
// baked views nearest the viewer's ray, each marched along that ray to the
// depth it stores, and blended there. Shared by the forward and prepass
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

    // The viewer's ray through the fragment, in the object's coordinates.
    // Orthographic views, such as directional shadows, look along their own
    // axis.
    var ray: vec3<f32>;
    if view.clip_from_view[3][3] == 1.0 {
        ray = -normalize(object_from_world * view.world_from_view[2].xyz);
    } else {
        ray = normalize(object_from_world * (world_position - view.world_position));
    }

    var out: ImpostorSurface;
    out.coverage = 0.0;
    out.world_radius = radius * scale;
    out.color = vec3<f32>(0.0);
    out.world_normal = world_basis[2];
    out.world_position = world_position;

    // Where the ray is inside the bounding sphere, as distances along it
    // from `on_quad`. A ray that misses the sphere misses the object.
    let from_center = on_quad - center;
    let middle = -dot(from_center, ray);
    let reach = middle * middle - dot(from_center, from_center) + radius * radius;
    if reach <= 0.0 {
        return out;
    }
    let span = vec2<f32>(middle - sqrt(reach), middle + sqrt(reach));

    // The atlas mip level the views are marched at, from how fast the quad
    // crosses the atlas.
    let atlas_size = f32(textureDimensions(normal_atlas).x);
    let footprint = max(length(duv_x), length(duv_y)) * atlas_size;
    let level = max(log2(max(footprint, 1e-8)), 0.0);

    // The three views around the direction from the object towards the
    // viewer, weighted by where it falls in the triangle between them. The
    // whole object uses the same three, so their silhouettes agree across
    // it; the march below still follows each fragment's own ray.
    var to_view = -ray;
    if view.clip_from_view[3][3] != 1.0 {
        to_view = object_from_world * (view.world_position - world_center);
    }
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
        let start = on_quad + ray * span.x;
        let end = on_quad + ray * span.y;
        let seen = view_sample(corner, start, end, level, duv_x, duv_y);
        let weight = weights[i] * seen.coverage;
        coverage += weight;
        color += weight * seen.color;
        normal += weight * seen.normal;
        position += weight * seen.position;
    }

    out.coverage = coverage;
    if coverage > 0.0 {
        out.color = color / coverage;
        out.world_normal = normalize(world_from_object * normal);
        out.world_position = world_center + world_from_object * (position / coverage - center) * scale;
    }
    return out;
}

/// What the view in atlas cell `cell` shows along the viewer's ray from
/// `start` to `end`, where it enters and leaves the bounding sphere, in the
/// object's coordinates. The view's depth is marched at mip `level`;
/// `duv_x` and `duv_y` are the quad's atlas derivatives, which pick the mip
/// level of the colour and normal at the surface found.
///
/// Each view stores only the surface nearest its own camera, as a height
/// towards it. Finding where the ray first falls below that height lines
/// the three blended views up on the same surface, which is what keeps
/// their outlines from doubling.
fn view_sample(
    cell: vec2<u32>,
    start: vec3<f32>,
    end: vec3<f32>,
    level: f32,
    duv_x: vec2<f32>,
    duv_y: vec2<f32>,
) -> ViewSample {
    var out: ViewSample;
    let basis = view_basis(view_direction(cell, impostor.frames, impostor.lowest_elevation));

    // A view seen from nearly edge-on, or from behind, shows nothing.
    let facing = dot(normalize(end - start), basis[2]);
    if facing > -0.05 {
        return out;
    }

    // The ray across the view: where it is on the view's square, and its
    // height towards the view's camera, -1 to 1 over the bounding sphere.
    // Both change linearly along the ray, since the view is orthographic.
    let entry = vec3<f32>(square(start, basis), height(start, basis));
    let exit = vec3<f32>(square(end, basis), height(end, basis));

    // Step about one texel at a time across the view, so thin parts aren't
    // stepped over; far impostors march at a coarse mip in a few steps.
    let view_texels = f32(textureDimensions(normal_atlas).x) / f32(impostor.frames) * exp2(-level);
    let texels = length(exit.xy - entry.xy) * 0.5 * view_texels;
    let steps = u32(clamp(ceil(texels), f32(MIN_STEPS), f32(MAX_STEPS)));

    // Where the ray first comes down onto the stored surface inside the
    // view's silhouette. Uncovered texels hold the depth of the nearest
    // covered one, which is no surface at all, so the ray passes through
    // them once it is below that depth.
    //
    // A ray that passes under that depth beside the silhouette and then
    // crosses into it meets the side of the depth the view stores, not
    // anything the view saw, so this view shows nothing there; the views on
    // that side of the object show it instead.
    var above = 0.0;
    var hit = -1.0;
    var beside = false;
    for (var i = 0u; i <= steps; i += 1u) {
        let s = f32(i) / f32(steps);
        let point = mix(entry, exit, s);
        if gap(cell, point, level) > 0.0 {
            beside = false;
            above = s;
            continue;
        }
        if beside {
            if covers(cell, point.xy, level) {
                break;
            }
            continue;
        }

        // Coming down from above: find where the ray crosses the depth
        // between the two steps, since a ray grazing the silhouette may
        // cross it only briefly, then see whether the view saw the object
        // there.
        var crossing = s;
        if i > 0u {
            crossing = cross_depth(cell, entry, exit, above, s, level);
        }
        if covers(cell, mix(entry.xy, exit.xy, crossing), level) {
            hit = crossing;
            break;
        }
        beside = true;
    }
    if hit < 0.0 {
        return out;
    }
    let s = hit;

    let at = cell_uv(cell, mix(entry.xy, exit.xy, s), level);
    let albedo = textureSampleGrad(albedo_atlas, atlas_sampler, at, duv_x, duv_y);
    let stored = textureSampleGrad(normal_atlas, atlas_sampler, at, duv_x, duv_y);
    out.color = albedo.rgb;
    out.coverage = albedo.a;
    out.normal = stored.xyz * 2.0 - 1.0;
    out.position = mix(start, end, s);
    return out;
}

/// Where, between `above` and `below` along the ray from `entry` to `exit`
/// (see `view_sample`), the ray crosses the depth view `cell` stores, given
/// that it is above it at `above` and not at `below`: the step is halved a
/// few times, then the crossing taken on the straight line between its
/// ends.
fn cross_depth(
    cell: vec2<u32>,
    entry: vec3<f32>,
    exit: vec3<f32>,
    above: f32,
    below: f32,
    level: f32,
) -> f32 {
    var high = above;
    var low = below;
    for (var i = 0u; i < REFINE_STEPS; i += 1u) {
        let middle = (high + low) * 0.5;
        if gap(cell, mix(entry, exit, middle), level) <= 0.0 {
            low = middle;
        } else {
            high = middle;
        }
    }
    let gap_high = gap(cell, mix(entry, exit, high), level);
    let gap_low = gap(cell, mix(entry, exit, low), level);
    return mix(high, low, clamp(gap_high / max(gap_high - gap_low, 1e-6), 0.0, 1.0));
}

/// How far a point on the ray, given as its place on the view's square and
/// its height (see `view_sample`), is above the surface view `cell` stores
/// there. Off the view's square nothing is stored, so the point is above it.
fn gap(cell: vec2<u32>, point: vec3<f32>, level: f32) -> f32 {
    if any(abs(point.xy) > vec2<f32>(1.0)) {
        return 1.0;
    }
    let stored = textureSampleLevel(normal_atlas, atlas_sampler, cell_uv(cell, point.xy, level), level).a;
    return point.z - (stored * 2.0 - 1.0);
}

/// Whether view `cell` saw the object at `across` on its square, at mip
/// `level`: whether the texel there survives the impostor's cut-out.
fn covers(cell: vec2<u32>, across: vec2<f32>, level: f32) -> bool {
    let alpha = textureSampleLevel(albedo_atlas, atlas_sampler, cell_uv(cell, across, level), level).a;
    return alpha >= 0.5;
}

/// The fewest and most steps `view_sample` marches each view in, and how
/// many times it then halves the step that reached the surface.
const MIN_STEPS: u32 = 6u;
const MAX_STEPS: u32 = 16u;
const REFINE_STEPS: u32 = 3u;

/// The height of `point`, in the object's coordinates, towards the camera
/// of the view with axes `basis`: -1 to 1 over the bounding sphere, as the
/// normal atlas stores depth.
fn height(point: vec3<f32>, basis: mat3x3<f32>) -> f32 {
    return dot(point - impostor.center, basis[2]) / impostor.radius;
}

/// Where `point`, in the object's coordinates, lies across the view with
/// axes `basis`: -1 to 1 over the bounding sphere, x to the right and y up.
fn square(point: vec3<f32>, basis: mat3x3<f32>) -> vec2<f32> {
    let offset = point - impostor.center;
    return vec2<f32>(dot(offset, basis[0]), dot(offset, basis[1])) / impostor.radius;
}

/// The atlas position in cell `cell` of `across`, a position across its
/// view as `square` gives it, for sampling at mip `level`.
///
/// Positions are kept half a texel inside the cell at that level and the
/// one above it, so filtering never reaches into the neighbouring view.
fn cell_uv(cell: vec2<u32>, across: vec2<f32>, level: f32) -> vec2<f32> {
    let frames = f32(impostor.frames);
    let view_size = f32(textureDimensions(normal_atlas).x) / frames;
    let inset = 0.5 * exp2(floor(level) + 1.0) / view_size;
    let in_cell = vec2<f32>(across.x * 0.5 + 0.5, 0.5 - across.y * 0.5);
    return (vec2<f32>(cell) + clamp(in_cell, vec2<f32>(inset), vec2<f32>(1.0 - inset))) / frames;
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
