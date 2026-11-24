//! Stable ids for mushroom options and the specimens grown from them.
//!
//! Every options type here has a [`ContentHash`]: a hash of its settings
//! that is the same in every run and on every machine. The hash of a
//! [`MushroomOptions`] leaves out the seed and the mesh detail, and names a
//! pool of look-alike specimens; a [`procgena_core::SpecimenId`] names one
//! specimen in that pool.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId};

use crate::cluster::{ClusterLayout, ClusterOptions, FairyRingOptions};
use crate::options::{
    AnnulusOptions, BulbOptions, CapOptions, MaterialOptions, MushroomOptions, Pattern,
    StemOptions, UndersideOptions, WartOptions,
};
use crate::profile::CapFamily;

/// The seed key that [`GENERATOR_KIND`] is made from.
pub(crate) const GENERATOR_KIND_KEY: &str = "mushroom.generator.kind";

/// The generator kind of mushrooms, part of every mushroom [`PoolId`].
pub const GENERATOR_KIND: GeneratorKind = GeneratorKind::from_key(GENERATOR_KIND_KEY);

impl MushroomOptions {
    /// Returns the pool these options grow specimens in: mushrooms whose
    /// options differ only in [`seed`](Self::seed) or
    /// [`detail`](Self::detail) share a pool.
    pub fn pool(&self) -> PoolId {
        PoolId::new(GENERATOR_KIND, OptionsHash::of(self))
    }
}

// --------------------------------------------------------------------------
// Content hashes
//
// Each impl writes every field in declaration order. Enums write variant
// numbers chosen here, which must never change.

impl ContentHash for MushroomOptions {
    /// Leaves out `seed` and `detail`.
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            seed: _,
            size,
            cap,
            underside,
            stem,
            bulb,
            annulus,
            warts,
            material,
            cluster,
            detail: _,
        } = self;
        h.write(size);
        h.write(cap);
        h.write(underside);
        h.write(stem);
        h.write(bulb);
        h.write(annulus);
        h.write(warts);
        h.write(material);
        h.write(cluster);
    }
}

impl ContentHash for CapOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            family,
            height,
            lobes,
            lobe_depth,
            asymmetry,
            tilt,
            roughness,
            roughness_scale,
        } = self;
        h.write(family);
        h.write(height);
        h.write(lobes);
        h.write(lobe_depth);
        h.write(asymmetry);
        h.write(tilt);
        h.write(roughness);
        h.write(roughness_scale);
    }
}

impl ContentHash for CapFamily {
    fn content_hash(&self, h: &mut ContentHasher) {
        h.write_u32(match self {
            CapFamily::Campanulate => 0,
            CapFamily::Conical => 1,
            CapFamily::Convex => 2,
            CapFamily::Depressed => 3,
            CapFamily::Flat => 4,
            CapFamily::Infundibuliform => 5,
            CapFamily::Ovate => 6,
            CapFamily::Umbilicate => 7,
            CapFamily::Umbonate => 8,
            CapFamily::Globe => 9,
        });
    }
}

impl ContentHash for UndersideOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            rim_thickness,
            sag,
            decurrent,
        } = self;
        h.write(rim_thickness);
        h.write(sag);
        h.write(decurrent);
    }
}

impl ContentHash for StemOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            height,
            radius,
            base,
            collar,
            lean,
            curve,
            upturn,
            lean_direction,
            roughness,
        } = self;
        h.write(height);
        h.write(radius);
        h.write(base);
        h.write(collar);
        h.write(lean);
        h.write(curve);
        h.write(upturn);
        h.write(lean_direction);
        h.write(roughness);
    }
}

impl ContentHash for BulbOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            closed,
            size,
            height,
            depth,
        } = self;
        h.write(closed);
        h.write(size);
        h.write(height);
        h.write(depth);
    }
}

impl ContentHash for AnnulusOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            enabled,
            position,
            length,
            flare,
            raggedness,
        } = self;
        h.write(enabled);
        h.write(position);
        h.write(length);
        h.write(flare);
        h.write(raggedness);
    }
}

impl ContentHash for WartOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            count,
            size,
            size_jitter,
            height,
            coverage,
        } = self;
        h.write(count);
        h.write(size);
        h.write(size_jitter);
        h.write(height);
        h.write(coverage);
    }
}

impl ContentHash for MaterialOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            cap_centre,
            cap_rim,
            underside,
            stem_top,
            stem_base,
            annulus,
            wart,
            gills,
            gill_lines,
            gill_strength,
            pattern,
            pattern_colour,
            pattern_scale,
            pattern_strength,
            roughness,
            cap_texture,
            cap_texture_size,
            stem_texture,
            stem_texture_size,
        } = self;
        h.write(cap_centre);
        h.write(cap_rim);
        h.write(underside);
        h.write(stem_top);
        h.write(stem_base);
        h.write(annulus);
        h.write(wart);
        h.write(gills);
        h.write(gill_lines);
        h.write(gill_strength);
        h.write(pattern);
        h.write(pattern_colour);
        h.write(pattern_scale);
        h.write(pattern_strength);
        h.write(roughness);
        h.write(cap_texture);
        h.write(cap_texture_size);
        h.write(stem_texture);
        h.write(stem_texture_size);
    }
}

impl ContentHash for Pattern {
    fn content_hash(&self, h: &mut ContentHasher) {
        h.write_u32(match self {
            Pattern::None => 0,
            Pattern::Speckle => 1,
            Pattern::Cracks => 2,
            Pattern::Spots => 3,
        });
    }
}

impl ContentHash for ClusterOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            stems,
            layout,
            size_variation,
            lean,
            sweep,
            spacing,
            variants,
            fairy_ring,
        } = self;
        h.write(stems);
        h.write(layout);
        h.write(size_variation);
        h.write(lean);
        h.write(sweep);
        h.write(spacing);
        h.write(variants);
        h.write(fairy_ring);
    }
}

impl ContentHash for ClusterLayout {
    fn content_hash(&self, h: &mut ContentHasher) {
        h.write_u32(match self {
            ClusterLayout::Tuft => 0,
            ClusterLayout::Line => 1,
            ClusterLayout::FairyRing => 2,
        });
    }
}

impl ContentHash for FairyRingOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self { radius, width, arc } = self;
        h.write(radius);
        h.write(width);
        h.write(arc);
    }
}
