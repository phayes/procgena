//! Smaller copies (mip levels) of an impostor's atlases, so far impostors
//! are drawn from a picture about their size on screen instead of
//! flickering between single texels.
//!
//! Each view of an atlas is shrunk on its own, so views never bleed into
//! each other. Colour is averaged in linear light and weighted by
//! coverage, so the see-through background doesn't darken edges. Each
//! view's alpha is then scaled so the same share of it survives the
//! impostor shader's cut-out, which keeps far impostors from thinning out
//! or growing.

/// The alpha below which the impostor shaders discard a texel.
const CUTOFF: f32 = 0.5;

/// The smallest view, in pixels along a side, that gets a mip level.
const MIN_VIEW: u32 = 8;

/// The colour and normal atlases of one impostor at every mip level, the
/// full-size level first. Each level is RGBA with 8 bits per channel.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MipChains {
    pub(crate) albedo: Vec<Vec<u8>>,
    pub(crate) normal: Vec<Vec<u8>>,
}

/// Builds the mip levels of an impostor's atlases. `size` is the pixels
/// along each side of the full-size atlases, which hold `frames × frames`
/// square views; `albedo` (sRGB colour, alpha as coverage) and `normal`
/// (normals packed into 0–1) must each be `size × size` RGBA pixels.
///
/// Levels stop before a view would be smaller than 8 pixels, or would have
/// an odd size. An atlas whose views can't be halved has the full-size
/// level only.
pub(crate) fn mip_chains(albedo: &[u8], normal: &[u8], size: u32, frames: u32) -> MipChains {
    let mut chains = MipChains {
        albedo: vec![albedo.to_vec()],
        normal: vec![normal.to_vec()],
    };
    if frames == 0 || !size.is_multiple_of(frames) {
        return chains;
    }

    // Each view's share of texels that survive the cut-out at full size.
    let view = size / frames;
    let coverage: Vec<f32> = (0..frames * frames)
        .map(|cell| view_coverage(albedo, size, view, cell % frames, cell / frames, 1.0))
        .collect();

    // Halve both atlases until the views are as small as they may get.
    // Each level is halved from the one before with its alpha unscaled, so
    // scaling never compounds from level to level.
    let mut level_size = size;
    let mut level_view = view;
    let mut unscaled = albedo.to_vec();
    while level_view.is_multiple_of(2) && level_view / 2 >= MIN_VIEW {
        let (albedo, normal) = halve(
            &unscaled,
            chains.normal.last().expect("level 0 is always present"),
            level_size,
        );
        level_size /= 2;
        level_view /= 2;
        unscaled = albedo.clone();
        let mut albedo = albedo;
        for (cell, &target) in coverage.iter().enumerate() {
            let (x, y) = (cell as u32 % frames, cell as u32 / frames);
            keep_coverage(&mut albedo, level_size, level_view, x, y, target);
        }
        chains.albedo.push(albedo);
        chains.normal.push(normal);
    }
    chains
}

/// Halves a `size × size` colour and normal atlas pair, averaging each
/// 2 × 2 block. Colour and normals are weighted by the colour's alpha.
fn halve(albedo: &[u8], normal: &[u8], size: u32) -> (Vec<u8>, Vec<u8>) {
    let half = size / 2;
    let mut small_albedo = Vec::with_capacity((half * half * 4) as usize);
    let mut small_normal = Vec::with_capacity((half * half * 4) as usize);
    for y in 0..half {
        for x in 0..half {
            let texels = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(dx, dy)| (((y * 2 + dy) * size + x * 2 + dx) * 4) as usize);

            // Coverage-weighted colour in linear light; plain averages where
            // nothing is covered, so the background stays as it was.
            let weights = texels.map(|t| f32::from(albedo[t + 3]) / 255.0);
            let total: f32 = weights.iter().sum();
            let weight = |i: usize| {
                if total > 0.0 {
                    weights[i] / total
                } else {
                    0.25
                }
            };
            for channel in 0..3 {
                let linear: f32 = (0..4)
                    .map(|i| srgb_to_linear(albedo[texels[i] + channel]) * weight(i))
                    .sum();
                small_albedo.push(linear_to_srgb(linear));
            }
            small_albedo.push(to_byte(total / 4.0));

            // Coverage-weighted normal, made unit length again.
            let mut sum = [0.0f32; 3];
            for (i, &t) in texels.iter().enumerate() {
                for (axis, value) in sum.iter_mut().enumerate() {
                    *value += (f32::from(normal[t + axis]) / 255.0 * 2.0 - 1.0) * weight(i);
                }
            }
            let length = sum.iter().map(|v| v * v).sum::<f32>().sqrt();
            for value in sum {
                let unit = if length > 0.0 { value / length } else { value };
                small_normal.push(to_byte(unit * 0.5 + 0.5));
            }
            let alpha: f32 = texels.iter().map(|&t| f32::from(normal[t + 3])).sum();
            small_normal.push((alpha / 4.0).round() as u8);
        }
    }
    (small_albedo, small_normal)
}

/// The share of one view's texels whose alpha, multiplied by `scale`, is
/// at least the cut-out. `x` and `y` pick the view; `view` is its size.
fn view_coverage(albedo: &[u8], size: u32, view: u32, x: u32, y: u32, scale: f32) -> f32 {
    let mut covered = 0u32;
    for row in y * view..(y + 1) * view {
        for column in x * view..(x + 1) * view {
            let alpha = f32::from(albedo[((row * size + column) * 4 + 3) as usize]) / 255.0;
            if alpha * scale >= CUTOFF {
                covered += 1;
            }
        }
    }
    covered as f32 / (view * view) as f32
}

/// Scales one view's alpha so that `target` of its texels survive the
/// cut-out, as near as a scale can get it.
fn keep_coverage(albedo: &mut [u8], size: u32, view: u32, x: u32, y: u32, target: f32) {
    if target <= 0.0 {
        return;
    }

    // Coverage only grows with the scale, and changes in steps, so find
    // the step that crosses the target (a scale of 256 lifts any alpha
    // above 0 to 1), then keep whichever side of it is nearer.
    let (mut low, mut high) = (0.0f32, 256.0f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if view_coverage(albedo, size, view, x, y, middle) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    let below = view_coverage(albedo, size, view, x, y, low);
    let above = view_coverage(albedo, size, view, x, y, high);
    let scale = if target - below < above - target {
        low
    } else {
        high
    };
    for row in y * view..(y + 1) * view {
        for column in x * view..(x + 1) * view {
            let index = ((row * size + column) * 4 + 3) as usize;
            albedo[index] = to_byte(f32::from(albedo[index]) / 255.0 * scale);
        }
    }
}

fn srgb_to_linear(value: u8) -> f32 {
    let v = f32::from(value) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let v = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    to_byte(v)
}

/// A 0–1 value as a byte, clamped.
fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `size` atlas of `frames²` views, each covered (opaque red, normal
    /// facing +Z) inside a centred square `inset` pixels in from each edge
    /// of the view, and clear elsewhere.
    fn atlas(size: u32, frames: u32, inset: u32) -> (Vec<u8>, Vec<u8>) {
        let view = size / frames;
        let mut albedo = vec![0u8; (size * size * 4) as usize];
        let mut normal = vec![0u8; (size * size * 4) as usize];
        for row in 0..size {
            for column in 0..size {
                let (vx, vy) = (column % view, row % view);
                let inside =
                    (inset..view - inset).contains(&vx) && (inset..view - inset).contains(&vy);
                if inside {
                    let i = ((row * size + column) * 4) as usize;
                    albedo[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
                    normal[i..i + 4].copy_from_slice(&[128, 128, 255, 255]);
                }
            }
        }
        (albedo, normal)
    }

    #[test]
    fn levels_stop_at_the_smallest_view() {
        let (albedo, normal) = atlas(64, 2, 4);
        let chains = mip_chains(&albedo, &normal, 64, 2);
        // Views of 32, 16 and 8 pixels.
        let sizes: Vec<usize> = chains.albedo.iter().map(Vec::len).collect();
        assert_eq!(sizes, [64 * 64 * 4, 32 * 32 * 4, 16 * 16 * 4]);
        assert_eq!(chains.normal.len(), 3);

        // Views that can't be halved keep only the full-size level.
        let (albedo, normal) = atlas(24, 2, 2);
        assert_eq!(mip_chains(&albedo, &normal, 24, 2).albedo.len(), 1);
    }

    #[test]
    fn edges_keep_their_colour_and_normal() {
        // An odd inset puts covered and clear texels in the same 2 × 2 block.
        let (albedo, normal) = atlas(64, 2, 3);
        let chains = mip_chains(&albedo, &normal, 64, 2);
        for (small_albedo, small_normal) in chains.albedo.iter().zip(&chains.normal).skip(1) {
            for (colour, normal) in small_albedo.chunks(4).zip(small_normal.chunks(4)) {
                if colour[3] > 0 {
                    assert_eq!(&colour[..3], &[255, 0, 0], "no dark fringe");
                    assert_eq!(&normal[..3], &[128, 128, 255]);
                }
            }
        }
    }

    #[test]
    fn each_view_keeps_its_cut_out_coverage() {
        let (albedo, normal) = atlas(64, 2, 3);
        let chains = mip_chains(&albedo, &normal, 64, 2);
        let full = view_coverage(&albedo, 64, 32, 1, 0, 1.0);
        for (level, small) in chains.albedo.iter().enumerate().skip(1) {
            let size = 64 >> level;
            let covered = view_coverage(small, size, size / 2, 1, 0, 1.0);
            // One texel row or column of a view, at most, either way.
            let texel = 2.0 / (size / 2) as f32;
            assert!(
                (covered - full).abs() <= texel,
                "level {level}: {covered} against {full}"
            );
        }
    }

    #[test]
    fn views_never_bleed_into_each_other() {
        // Only the first view is covered.
        let (mut albedo, normal) = atlas(64, 2, 0);
        for row in 0..64u32 {
            for column in 0..64u32 {
                if row >= 32 || column >= 32 {
                    albedo[((row * 64 + column) * 4 + 3) as usize] = 0;
                }
            }
        }
        let chains = mip_chains(&albedo, &normal, 64, 2);
        for (level, small) in chains.albedo.iter().enumerate() {
            let size = 64 >> level;
            assert_eq!(view_coverage(small, size, size / 2, 1, 0, 1.0), 0.0);
            assert_eq!(view_coverage(small, size, size / 2, 0, 0, 1.0), 1.0);
        }
    }
}
