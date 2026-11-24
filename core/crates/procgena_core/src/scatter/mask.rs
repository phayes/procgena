//! Masks as 8-bit textures on texels fixed to the World: what a Scatter
//! leaves for the Scatters below it.

use glam::DVec2;

use super::Mask;
use super::footprint::Disc;
use super::lattice::{Fixed2, FixedRect};

/// Texel width of footprint masks, as a power of two in fixed-point units:
/// `1 << 7` units, 0.5 m.
pub const FOOTPRINT_TEXEL_SHIFT: u32 = 7;

/// An 8-bit mask over a rectangle of the World: 0 allows none of a
/// Scatter's density, 255 all of it.
///
/// Texels are squares `1 << shift` fixed-point units wide, fixed to the
/// World: texel `(x, z)` spans from `x << shift` up to `(x + 1) << shift`
/// along X, and likewise along Z. Reading between texel centres blends the
/// four nearest texels in integer maths ([`sample`](Self::sample)), so the
/// edge of a stamped disc is about one texel soft: a mask thins density, and
/// exact exclusion uses [`Footprints`](super::Footprints) instead.
///
/// A layer is also a [`Mask`], so it can be the mask of the next Scatter's
/// [`ScatterInput`](super::ScatterInput). Outside its texels it allows
/// full density.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskLayer {
    shift: u32,
    x0: i64,
    z0: i64,
    width: usize,
    height: usize,
    texels: Vec<u8>,
}

impl MaskLayer {
    /// A layer allowing full density everywhere, with texels `1 << shift`
    /// units wide, holding every texel that [`sample`](Self::sample) reads
    /// anywhere in `rect`.
    ///
    /// # Panics
    ///
    /// Panics if `shift` is 0 or more than 30.
    pub fn full(rect: FixedRect, shift: u32) -> Self {
        assert!((1..=30).contains(&shift), "texel shift must be 1 to 30");
        let half = 1i64 << (shift - 1);
        let first = |v: i64| (v - half) >> shift;
        let (x0, z0) = (first(rect.min.x), first(rect.min.z));
        let (x1, z1) = (first(rect.max.x - 1) + 2, first(rect.max.z - 1) + 2);
        let width = (x1 - x0).max(0) as usize;
        let height = (z1 - z0).max(0) as usize;
        Self {
            shift,
            x0,
            z0,
            width,
            height,
            texels: vec![255; width * height],
        }
    }

    /// The texel width as a power of two in fixed-point units.
    pub fn shift(&self) -> u32 {
        self.shift
    }

    /// The value of texel `(x, z)`, 255 outside the layer.
    pub fn texel(&self, x: i64, z: i64) -> u8 {
        self.index(x, z).map_or(255, |i| self.texels[i])
    }

    fn index(&self, x: i64, z: i64) -> Option<usize> {
        let (dx, dz) = (x - self.x0, z - self.z0);
        (dx >= 0 && dz >= 0 && (dx as usize) < self.width && (dz as usize) < self.height)
            .then(|| dz as usize * self.width + dx as usize)
    }

    /// Sets to 0 every texel whose centre lies inside one of `discs`.
    pub fn stamp<'a>(&mut self, discs: impl IntoIterator<Item = &'a Disc>) {
        let size = 1i64 << self.shift;
        let half = size / 2;
        for disc in discs {
            let lo = (disc.centre.x - disc.radius - half) >> self.shift;
            let hi = (disc.centre.x + disc.radius - half) >> self.shift;
            let lo_z = (disc.centre.z - disc.radius - half) >> self.shift;
            let hi_z = (disc.centre.z + disc.radius - half) >> self.shift;
            for z in lo_z..=hi_z + 1 {
                for x in lo..=hi + 1 {
                    let centre = Fixed2::new(x * size + half, z * size + half);
                    if disc.contains(centre)
                        && let Some(i) = self.index(x, z)
                    {
                        self.texels[i] = 0;
                    }
                }
            }
        }
    }

    /// Multiplies every texel by `other`'s texel at the same place, out of
    /// 255. Both layers must have the same texel width.
    ///
    /// # Panics
    ///
    /// Panics if the texel widths differ.
    pub fn multiply(&mut self, other: &MaskLayer) {
        assert_eq!(self.shift, other.shift, "mask layers differ in texel width");
        for z in 0..self.height {
            for x in 0..self.width {
                let (wx, wz) = (self.x0 + x as i64, self.z0 + z as i64);
                let i = z * self.width + x;
                let v = u32::from(self.texels[i]) * u32::from(other.texel(wx, wz)) / 255;
                self.texels[i] = v as u8;
            }
        }
    }

    /// The mask at World position `p`, from 0 to 255, blended from the four
    /// nearest texel centres in fixed point.
    pub fn sample(&self, p: Fixed2) -> u8 {
        let shift = self.shift;
        let half = 1i64 << (shift - 1);
        let (ux, uz) = (p.x - half, p.z - half);
        let (tx, tz) = (ux >> shift, uz >> shift);
        let wx = (((ux - (tx << shift)) << 8) >> shift) as u32;
        let wz = (((uz - (tz << shift)) << 8) >> shift) as u32;
        let at = |x, z| u32::from(self.texel(x, z));
        let top = at(tx, tz) * (256 - wx) + at(tx + 1, tz) * wx;
        let bottom = at(tx, tz + 1) * (256 - wx) + at(tx + 1, tz + 1) * wx;
        ((top * (256 - wz) + bottom * wz) >> 16) as u8
    }
}

impl Mask for MaskLayer {
    fn density(&self, p: DVec2) -> f32 {
        f32::from(self.sample(Fixed2::from_world(p))) / 255.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scatter::lattice::quantise;

    fn rect(side: i64) -> FixedRect {
        FixedRect::new(Fixed2::new(-side, -side), Fixed2::new(side, side))
    }

    #[test]
    fn a_stamped_disc_clears_its_texels_and_blends_at_its_edge() {
        let mut layer = MaskLayer::full(rect(10 * 256), FOOTPRINT_TEXEL_SHIFT);
        let disc = Disc::new(DVec2::ZERO, 1.0);
        layer.stamp([&disc]);
        assert_eq!(layer.sample(Fixed2::new(0, 0)), 0);
        assert_eq!(layer.sample(Fixed2::new(5 * 256, 0)), 255);
        // Between texel centres just outside the disc, the edge is soft.
        let edge = layer.sample(Fixed2::new(256, 64));
        assert!(edge > 0 && edge < 255, "{edge}");
        // On a texel centre the read is exact.
        assert_eq!(layer.sample(Fixed2::new(3 * 128 + 64, 64)), 255);
    }

    #[test]
    fn layers_multiply_and_read_as_masks() {
        let mut a = MaskLayer::full(rect(4 * 256), FOOTPRINT_TEXEL_SHIFT);
        let mut b = a.clone();
        b.stamp([&Disc::new(DVec2::new(2.0, 2.0), 0.5)]);
        a.multiply(&b);
        assert_eq!(a, b);
        assert_eq!(a.density(DVec2::new(2.0, 2.0)), 0.0);
        assert_eq!(a.density(DVec2::new(-2.0, -2.0)), 1.0);
        // Far outside the layer, full density.
        assert_eq!(a.density(DVec2::new(1.0e6, 0.0)), 1.0);
        // A read as a mask quantises back to the same byte.
        let p = DVec2::new(2.3, 2.1);
        assert_eq!(quantise(a.density(p)), a.sample(Fixed2::from_world(p)));
    }
}
