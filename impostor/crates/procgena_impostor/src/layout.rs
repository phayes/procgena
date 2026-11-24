//! Where each view of an impostor sits in its atlas: an upper hemisphere of
//! directions laid out as an octahedron folded flat.
//!
//! The atlas is `frames` × `frames` cells. The cell in the middle looks
//! straight down on the object; the corners look at it from the horizon.
//! [`view_direction`] and [`view_cell`] convert between the two, and
//! [`view_basis`] gives the axes a view's image is drawn along.
//!
//! Atlases baked with the [`Elevations`](crate::BakeLayout::Elevations)
//! layout instead hold one strip of views around the object's +Y axis, from
//! below to above: see [`elevation_direction`], [`elevation_index`] and
//! [`elevation_basis`].

use bevy::math::{Mat3, UVec2, Vec3};

/// The direction from the object towards the viewer of atlas cell `cell`
/// (column, row from the top), in the object's own coordinates. `frames` is
/// the number of cells along each side of the atlas.
pub fn view_direction(cell: UVec2, frames: u32) -> Vec3 {
    let f = frames as f32;
    let u = (cell.x as f32 + 0.5) / f * 2.0 - 1.0;
    let v = (cell.y as f32 + 0.5) / f * 2.0 - 1.0;
    // Turn the square by 45° onto the diamond |x| + |z| <= 1, and lift it
    // onto the hemisphere.
    let (x, z) = ((u + v) / 2.0, (u - v) / 2.0);
    Vec3::new(x, 1.0 - x.abs() - z.abs(), z).normalize()
}

/// The atlas cell whose view is nearest to looking from `direction` (from
/// the object towards the viewer). Directions from below the horizon use
/// the horizon's views.
pub fn view_cell(direction: Vec3, frames: u32) -> UVec2 {
    let mut d = direction;
    d.y = d.y.max(0.0);
    let d = d.try_normalize().unwrap_or(Vec3::Y);
    let p = d / (d.x.abs() + d.y.abs() + d.z.abs());
    let (u, v) = (p.x + p.z, p.x - p.z);
    let cell = |t: f32| (((t + 1.0) / 2.0 * frames as f32).floor() as u32).min(frames - 1);
    UVec2::new(cell(u), cell(v))
}

/// The axes of a view's image, in the object's coordinates: right and up
/// across the image, and back towards the viewer (`direction` itself). The
/// same axes a Bevy camera at `direction` looking at the origin has.
pub fn view_basis(direction: Vec3) -> Mat3 {
    let back = direction.normalize();
    let up_hint = if back.y > 0.99 { Vec3::NEG_Z } else { Vec3::Y };
    let right = up_hint.cross(back).normalize();
    let up = back.cross(right);
    Mat3::from_cols(right, up, back)
}

/// The direction from the object towards the viewer of view `index` of
/// `count` in the [`Elevations`](crate::BakeLayout::Elevations) layout, in
/// the object's own coordinates.
///
/// The views look at the object's +Y axis from one side (from +Z), at
/// elevations evenly spaced from just above -Y (looking up from below) to
/// just below +Y (looking down from above), view 0 first. View `index` sits
/// in atlas cell `(index % frames, index / frames)`.
pub fn elevation_direction(index: u32, count: u32) -> Vec3 {
    let elevation = elevation_of(index, count);
    Vec3::new(0.0, elevation.sin(), elevation.cos())
}

/// The elevation view of `count` nearest to looking from `direction`, from
/// the object towards the viewer, whatever its azimuth about +Y.
pub fn elevation_index(direction: Vec3, count: u32) -> u32 {
    let d = direction.try_normalize().unwrap_or(Vec3::Z);
    let t = (d.y.clamp(-1.0, 1.0).asin() / std::f32::consts::PI + 0.5) * count as f32;
    (t.floor().max(0.0) as u32).min(count - 1)
}

/// The axes of an [`elevation_direction`] view's image, in the object's
/// coordinates: right (always +X), up (towards +Y on screen) and back
/// towards the viewer (`direction` itself). Unlike [`view_basis`], up never
/// flips, so +Y always points up the image.
pub fn elevation_basis(direction: Vec3) -> Mat3 {
    let back = direction.normalize();
    let right = Vec3::X;
    let up = back.cross(right);
    Mat3::from_cols(right, up, back)
}

/// The elevation of view `index` of `count`, in radians from -π/2 to π/2.
fn elevation_of(index: u32, count: u32) -> f32 {
    let pi = std::f32::consts::PI;
    -pi / 2.0 + pi * (index as f32 + 0.5) / count as f32
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Quat, Transform};

    use super::*;

    #[test]
    fn every_cell_is_found_again_from_its_own_direction() {
        for frames in [4, 8, 12] {
            for y in 0..frames {
                for x in 0..frames {
                    let cell = UVec2::new(x, y);
                    let direction = view_direction(cell, frames);
                    assert!(direction.y >= 0.0 && direction.is_normalized());
                    assert_eq!(view_cell(direction, frames), cell);
                }
            }
        }
    }

    #[test]
    fn the_middle_looks_down_and_below_the_horizon_folds_up() {
        let middle = view_cell(Vec3::Y, 8);
        assert!(view_direction(middle, 8).y > 0.9);
        let below = view_cell(Vec3::new(1.0, -0.5, 0.0), 8);
        assert_eq!(below, view_cell(Vec3::X, 8));
    }

    #[test]
    fn the_basis_matches_a_bevy_camera_looking_at_the_object() {
        for direction in [Vec3::new(0.3, 0.8, -0.5), Vec3::Y, Vec3::X] {
            let camera = Transform::from_translation(direction).looking_at(Vec3::ZERO, {
                if direction.normalize().y > 0.99 {
                    Vec3::NEG_Z
                } else {
                    Vec3::Y
                }
            });
            let basis = view_basis(direction);
            let expected = Mat3::from_quat(camera.rotation);
            assert!(basis.abs_diff_eq(expected, 1e-5), "{basis} vs {expected}");
        }
    }

    #[test]
    fn elevation_views_run_from_below_to_above_and_keep_their_up() {
        let count = 16;
        for index in 0..count {
            let direction = elevation_direction(index, count);
            assert!(direction.is_normalized());
            assert_eq!(elevation_index(direction, count), index);
            // Any azimuth about +Y finds the same view.
            let turned = Quat::from_rotation_y(1.0) * direction;
            assert_eq!(elevation_index(turned, count), index);
            // +Y points up the image, even nearly along +Y or -Y.
            let basis = elevation_basis(direction);
            assert!(basis.y_axis.dot(Vec3::Y) > 0.0, "view {index}");
            assert!(
                basis
                    .x_axis
                    .cross(basis.y_axis)
                    .abs_diff_eq(basis.z_axis, 1e-5)
            );
        }
        assert!(elevation_direction(0, count).y < -0.99);
        assert!(elevation_direction(count - 1, count).y > 0.99);
        assert_eq!(elevation_index(Vec3::NEG_Y, count), 0);
        assert_eq!(elevation_index(Vec3::Y, count), count - 1);
    }
}
