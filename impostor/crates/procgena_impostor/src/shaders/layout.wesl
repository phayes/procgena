// The impostor atlas layout, matching `layout.rs`: the view directions an
// impostor is baked for, folded flat as an octahedron, `frames` × `frames`
// cells. `lowest` is the lowest viewer elevation baked, in degrees: -90
// covers the whole sphere, and any other value spreads the views from
// straight above down to `lowest` over the whole atlas.

const HALF_PI: f32 = 1.5707963;

/// The atlas cell whose view is nearest to looking from `direction` (from
/// the object towards the viewer). Directions below `lowest` use the views
/// at `lowest`.
fn view_cell(direction: vec3<f32>, frames: u32, lowest: f32) -> vec2<u32> {
    var d = direction;
    if dot(d, d) < 1e-12 {
        d = vec3<f32>(0.0, 1.0, 0.0);
    }
    d = normalize(d);
    var uv: vec2<f32>;
    if lowest <= -90.0 {
        uv = octahedron_uv(d);
    } else {
        uv = hemi_octahedron_uv(warp(d, lowest));
    }
    let f = f32(frames);
    let cell = min(floor((uv + 1.0) * 0.5 * f), vec2<f32>(f - 1.0));
    return vec2<u32>(cell);
}

/// The direction from the object towards the viewer of atlas cell `cell`.
fn view_direction(cell: vec2<u32>, frames: u32, lowest: f32) -> vec3<f32> {
    let uv = (vec2<f32>(cell) + 0.5) / f32(frames) * 2.0 - 1.0;
    if lowest <= -90.0 {
        return octahedron_direction(uv);
    }
    return unwarp(hemi_octahedron_direction(uv), lowest);
}

/// The axes of a view's image: right and up across it, and back towards the
/// viewer, as the columns of a matrix.
fn view_basis(direction: vec3<f32>) -> mat3x3<f32> {
    let back = normalize(direction);
    var up_hint = vec3<f32>(0.0, 1.0, 0.0);
    if back.y > 0.99 {
        up_hint = vec3<f32>(0.0, 0.0, -1.0);
    } else if back.y < -0.99 {
        up_hint = vec3<f32>(0.0, 0.0, 1.0);
    }
    let right = normalize(cross(up_hint, back));
    let up = cross(back, right);
    return mat3x3<f32>(right, up, back);
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

/// 1 for zero and above, -1 below, for each component.
fn sign_not_zero(t: vec2<f32>) -> vec2<f32> {
    return select(vec2<f32>(-1.0), vec2<f32>(1.0), t >= vec2<f32>(0.0));
}

/// The unit direction at `uv` of an octahedron folded flat: the upper half
/// in the middle diamond, the lower half in the corners.
fn octahedron_direction(uv: vec2<f32>) -> vec3<f32> {
    let y = 1.0 - abs(uv.x) - abs(uv.y);
    var xz = uv;
    if y < 0.0 {
        xz = (1.0 - abs(uv.yx)) * sign_not_zero(uv);
    }
    return normalize(vec3<f32>(xz.x, y, xz.y));
}

/// Where unit direction `d` lies on `octahedron_direction`'s square.
fn octahedron_uv(d: vec3<f32>) -> vec2<f32> {
    let p = d / (abs(d.x) + abs(d.y) + abs(d.z));
    if p.y >= 0.0 {
        return p.xz;
    }
    return (1.0 - abs(p.zx)) * sign_not_zero(p.xz);
}

/// The unit direction at `uv` of the upper half of an octahedron, turned by
/// 45° so that it fills the whole square.
fn hemi_octahedron_direction(uv: vec2<f32>) -> vec3<f32> {
    let x = (uv.x + uv.y) * 0.5;
    let z = (uv.x - uv.y) * 0.5;
    return normalize(vec3<f32>(x, 1.0 - abs(x) - abs(z), z));
}

/// Where unit direction `d`, at or above the horizon, lies on
/// `hemi_octahedron_direction`'s square.
fn hemi_octahedron_uv(d: vec3<f32>) -> vec2<f32> {
    let p = d / (abs(d.x) + abs(d.y) + abs(d.z));
    return vec2<f32>(p.x + p.z, p.x - p.z);
}

/// The largest angle from +Y covered down to elevation `lowest`, in radians.
fn polar_max(lowest: f32) -> f32 {
    return radians(90.0 - lowest);
}

/// Moves unit direction `d` towards +Y so that elevation `lowest` lands on
/// the horizon; directions below `lowest` land on the horizon too.
fn warp(d: vec3<f32>, lowest: f32) -> vec3<f32> {
    if lowest == 0.0 {
        let folded = vec3<f32>(d.x, max(d.y, 0.0), d.z);
        if dot(folded, folded) < 1e-12 {
            return vec3<f32>(0.0, 1.0, 0.0);
        }
        return normalize(folded);
    }
    let top = polar_max(lowest);
    let polar = min(acos(clamp(d.y, -1.0, 1.0)), top);
    return with_polar(d, polar * HALF_PI / top);
}

/// The inverse of `warp` for directions at or above the horizon.
fn unwarp(d: vec3<f32>, lowest: f32) -> vec3<f32> {
    if lowest == 0.0 {
        return d;
    }
    let polar = acos(clamp(d.y, -1.0, 1.0));
    return with_polar(d, polar * polar_max(lowest) / HALF_PI);
}

/// The unit direction with `d`'s azimuth about +Y and angle `polar` from +Y.
fn with_polar(d: vec3<f32>, polar: f32) -> vec3<f32> {
    var around = vec2<f32>(1.0, 0.0);
    if dot(d.xz, d.xz) > 1e-12 {
        around = normalize(d.xz);
    }
    return vec3<f32>(around.x * sin(polar), cos(polar), around.y * sin(polar));
}
