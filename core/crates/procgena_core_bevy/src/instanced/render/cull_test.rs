//! The frustum planes the culling shader tests copies against.

use bevy::camera::{CameraProjection, OrthographicProjection, PerspectiveProjection, ScalingMode};
use bevy::prelude::*;

use super::frustum_planes;

/// Whether a sphere is at least partly inside `planes`, as `in_frustum` in
/// `instanced_cull.wesl` works it out.
fn in_frustum(planes: &[Vec4; 6], center: Vec3, radius: f32, skip_near: bool) -> bool {
    planes.iter().enumerate().all(|(i, plane)| {
        (skip_near && i == 4) || plane.truncate().dot(center) + plane.w >= -radius
    })
}

#[test]
fn copies_are_culled_by_their_bounding_sphere() {
    // A camera at the origin looking down -Z, with an infinite far plane,
    // and a directional light's shadow view looking the same way.
    let perspective = PerspectiveProjection {
        fov: 1.0,
        aspect_ratio: 1.0,
        near: 0.1,
        ..default()
    };
    let camera = frustum_planes(&perspective.get_clip_from_view());
    let mut orthographic = OrthographicProjection {
        near: 0.1,
        far: 50.0,
        scaling_mode: ScalingMode::Fixed {
            width: 40.0,
            height: 40.0,
        },
        ..OrthographicProjection::default_3d()
    };
    orthographic.update(40.0, 40.0);
    let shadow = frustum_planes(&orthographic.get_clip_from_view());

    // Far ahead (beyond any far plane), beside the view but overlapping it,
    // and wholly beside or behind it.
    assert!(in_frustum(&camera, Vec3::new(0.0, 0.0, -1e5), 1.0, false));
    assert!(in_frustum(&camera, Vec3::new(6.0, 0.0, -10.0), 2.0, false));
    assert!(!in_frustum(
        &camera,
        Vec3::new(20.0, 0.0, -10.0),
        2.0,
        false
    ));
    assert!(!in_frustum(&camera, Vec3::new(0.0, 0.0, 10.0), 2.0, false));

    // A directional shadow takes casters behind its near plane.
    assert!(in_frustum(&shadow, Vec3::new(0.0, 0.0, 10.0), 2.0, true));
    assert!(!in_frustum(&shadow, Vec3::new(0.0, 0.0, 10.0), 2.0, false));
}
