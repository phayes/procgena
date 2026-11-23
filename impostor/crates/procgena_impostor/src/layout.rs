//! Where each view of an impostor sits in its atlas: an upper hemisphere of
//! directions laid out as an octahedron folded flat.
//!
//! The atlas is `frames` × `frames` cells. The cell in the middle looks
//! straight down on the object; the corners look at it from the horizon.
//! [`view_direction`] and [`view_cell`] convert between the two, and
//! [`view_basis`] gives the axes a view's image is drawn along.

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

#[cfg(test)]
mod tests {
    use bevy::prelude::Transform;

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
}
