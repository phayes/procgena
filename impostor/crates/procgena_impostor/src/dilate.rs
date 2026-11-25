//! Fills the texels around a baked object with the colour, normal and depth
//! of the nearest texel it covers.
//!
//! Filtering and block compression mix neighbouring texels, so texels just
//! outside the silhouette leak into its edge. Left as the clear colour, they
//! darken the edge and bend its normals towards nothing. Filled from the
//! object instead, they blend in unseen; their alpha stays 0, so the cut-out
//! doesn't change.

use std::collections::VecDeque;

use crate::mips::covers;

/// Fills every uncovered texel of every view of a `size × size` albedo and
/// normal atlas pair from the nearest covered texel of the same view, so
/// views never borrow from each other. Albedo alpha is left as it is; the
/// normal atlas's alpha (depth) is filled along with its normal.
///
/// Atlases whose views don't divide them evenly are left alone, as are
/// views the object doesn't cover at all.
pub(crate) fn dilate(albedo: &mut [u8], normal: &mut [u8], size: u32, frames: u32) {
    if frames == 0 || !size.is_multiple_of(frames) {
        return;
    }
    let view = size / frames;
    for cell_y in 0..frames {
        for cell_x in 0..frames {
            dilate_view(albedo, normal, size, view, cell_x * view, cell_y * view);
        }
    }
}

/// Fills one `view × view` view whose top-left texel is at `(left, top)`.
///
/// A breadth-first search from every covered texel at once reaches each
/// uncovered texel first from a nearest covered one, counting diagonal steps
/// as one.
fn dilate_view(albedo: &mut [u8], normal: &mut [u8], size: u32, view: u32, left: u32, top: u32) {
    let index = |x: u32, y: u32| ((top + y) * size + left + x) as usize * 4;

    let mut filled = vec![false; (view * view) as usize];
    let mut queue = VecDeque::new();
    for y in 0..view {
        for x in 0..view {
            if covers(albedo[index(x, y) + 3]) {
                filled[(y * view + x) as usize] = true;
                queue.push_back((x, y));
            }
        }
    }

    while let Some((x, y)) = queue.pop_front() {
        let from = index(x, y);
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                continue;
            };
            if nx >= view || ny >= view || filled[(ny * view + nx) as usize] {
                continue;
            }
            filled[(ny * view + nx) as usize] = true;
            let to = index(nx, ny);
            albedo.copy_within(from..from + 3, to);
            normal.copy_within(from..from + 4, to);
            queue.push_back((nx, ny));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `size × size` atlas pair, uncovered and black, with the texels in
    /// `covered` given `colour`, normal `(n, n, n)` and depth `depth`.
    fn atlases(size: u32, covered: &[(u32, u32, u8, u8, u8)]) -> (Vec<u8>, Vec<u8>) {
        let mut albedo = vec![0; (size * size * 4) as usize];
        let mut normal = vec![0; (size * size * 4) as usize];
        for &(x, y, colour, n, depth) in covered {
            let t = ((y * size + x) * 4) as usize;
            albedo[t..t + 4].copy_from_slice(&[colour, colour, colour, 255]);
            normal[t..t + 4].copy_from_slice(&[n, n, n, depth]);
        }
        (albedo, normal)
    }

    fn texel(atlas: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let t = ((y * size + x) * 4) as usize;
        [atlas[t], atlas[t + 1], atlas[t + 2], atlas[t + 3]]
    }

    #[test]
    fn uncovered_texels_take_the_nearest_covered_texel() {
        let (mut albedo, mut normal) = atlases(8, &[(1, 1, 200, 10, 90), (6, 6, 50, 240, 30)]);
        dilate(&mut albedo, &mut normal, 8, 1);

        // Near the first texel, and near the second.
        assert_eq!(texel(&albedo, 8, 0, 3), [200, 200, 200, 0]);
        assert_eq!(texel(&normal, 8, 0, 3), [10, 10, 10, 90]);
        assert_eq!(texel(&albedo, 8, 7, 4), [50, 50, 50, 0]);
        assert_eq!(texel(&normal, 8, 7, 4), [240, 240, 240, 30]);

        // Covered texels are untouched.
        assert_eq!(texel(&albedo, 8, 1, 1), [200, 200, 200, 255]);
    }

    #[test]
    fn views_fill_only_from_themselves() {
        // Two views across, the left one covered at one texel, the right
        // one not at all.
        let (mut albedo, mut normal) = atlases(8, &[(3, 0, 120, 60, 200)]);
        dilate(&mut albedo, &mut normal, 8, 2);

        assert_eq!(texel(&albedo, 8, 0, 3), [120, 120, 120, 0]);
        assert_eq!(texel(&albedo, 8, 4, 0), [0, 0, 0, 0]);
        assert_eq!(texel(&normal, 8, 4, 0), [0, 0, 0, 0]);
        assert_eq!(texel(&albedo, 8, 4, 4), [0, 0, 0, 0]);
    }
}
