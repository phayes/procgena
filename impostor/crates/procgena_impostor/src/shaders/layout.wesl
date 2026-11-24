// The impostor atlas layout, matching `layout.rs`: an upper hemisphere of
// view directions folded flat as an octahedron, `frames` × `frames` cells.

/// The atlas cell whose view is nearest to looking from `direction` (from
/// the object towards the viewer). Directions from below the horizon use
/// the horizon's views.
fn view_cell(direction: vec3<f32>, frames: u32) -> vec2<u32> {
    var d = direction;
    d.y = max(d.y, 0.0);
    if dot(d, d) < 1e-12 {
        d = vec3<f32>(0.0, 1.0, 0.0);
    }
    let p = d / (abs(d.x) + abs(d.y) + abs(d.z));
    let uv = vec2<f32>(p.x + p.z, p.x - p.z);
    let f = f32(frames);
    let cell = min(floor((uv + 1.0) * 0.5 * f), vec2<f32>(f - 1.0));
    return vec2<u32>(cell);
}

/// The direction from the object towards the viewer of atlas cell `cell`.
fn view_direction(cell: vec2<u32>, frames: u32) -> vec3<f32> {
    let uv = (vec2<f32>(cell) + 0.5) / f32(frames) * 2.0 - 1.0;
    let x = (uv.x + uv.y) * 0.5;
    let z = (uv.x - uv.y) * 0.5;
    return normalize(vec3<f32>(x, 1.0 - abs(x) - abs(z), z));
}

/// The elevation view of `count`, in the `Elevations` layout, nearest to
/// looking from `direction` (from the object towards the viewer), whatever
/// its azimuth about +Y. View `index` sits in cell `(index % frames, index / frames)`.
fn elevation_index(direction: vec3<f32>, count: u32) -> u32 {
    var d = direction;
    if dot(d, d) < 1e-12 {
        d = vec3<f32>(0.0, 0.0, 1.0);
    }
    let y = clamp(normalize(d).y, -1.0, 1.0);
    let t = (asin(y) / 3.14159265 + 0.5) * f32(count);
    return min(u32(max(floor(t), 0.0)), count - 1u);
}

/// The axes of a view's image: right and up across it, and back towards the
/// viewer, as the columns of a matrix.
fn view_basis(direction: vec3<f32>) -> mat3x3<f32> {
    let back = normalize(direction);
    var up_hint = vec3<f32>(0.0, 1.0, 0.0);
    if back.y > 0.99 {
        up_hint = vec3<f32>(0.0, 0.0, -1.0);
    }
    let right = normalize(cross(up_hint, back));
    let up = cross(back, right);
    return mat3x3<f32>(right, up, back);
}
