//! Stable hashes of clump settings, and the pool id they name.
//!
//! A clump also depends on the surface it grows on, so its settings alone
//! don't fix its shape: a cache of built clumps needs the host's identity
//! as well as the clump's [`PoolId`] and seed.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId};
use procgena_specimen::{Tag, TagFilter, TagSet};

use crate::material::MaterialOptions;
use crate::options::{
    ClumpOptions, Direction, MaskOptions, MeshOptions, Placement, PlacementOptions, ShapeOptions,
};

/// The generator kind of every clump this crate builds.
pub const CLUMP_KIND: GeneratorKind = GeneratorKind::from_key("clump.generator.kind");

impl ClumpOptions {
    /// The pool these settings name: every clump built from them, whatever
    /// its seed. Clumps with equal pools and seeds grown on the same surface
    /// are the same.
    pub fn pool_id(&self) -> PoolId {
        PoolId::new(CLUMP_KIND, OptionsHash::of(self))
    }
}

/// Writes every setting except the seed.
impl ContentHash for ClumpOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            seed: _,
            directions,
            scale,
            mask,
            placement,
            shape,
            mesh,
            grows_on,
            tags,
            material,
        } = self;
        hasher.write(directions);
        hasher.write_f32(*scale);
        hasher.write(mask);
        hasher.write(placement);
        hasher.write(shape);
        hasher.write(mesh);
        write_filter(hasher, grows_on);
        write_tags(hasher, tags);
        hasher.write(material);
    }
}

/// Writes the vector and the strength.
impl ContentHash for Direction {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self { vector, strength } = self;
        hasher.write(vector);
        hasher.write_f32(*strength);
    }
}

/// Writes every setting.
impl ContentHash for MaskOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            any_facing,
            facing_min,
            facing_full,
            exposure_rays,
            exposure_power,
        } = self;
        hasher.write_bool(*any_facing);
        hasher.write_f32(*facing_min);
        hasher.write_f32(*facing_full);
        hasher.write_u32(*exposure_rays);
        hasher.write_f32(*exposure_power);
    }
}

/// Writes a fixed number per mode.
impl ContentHash for Placement {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(match self {
            Self::Everywhere => 0,
            Self::Scatter => 1,
        });
    }
}

/// Writes every setting.
impl ContentHash for PlacementOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            mode,
            count,
            radius,
            size_variation,
            min_spacing,
        } = self;
        hasher.write(mode);
        hasher.write_u32(*count);
        hasher.write_f32(*radius);
        hasher.write_f32(*size_variation);
        hasher.write_f32(*min_spacing);
    }
}

/// Writes every setting.
impl ContentHash for ShapeOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            thickness,
            sink,
            max_slope,
            min_support,
            lumpiness,
            lump_size,
            neck,
        } = self;
        hasher.write_f32(*thickness);
        hasher.write_f32(*sink);
        hasher.write_f32(*max_slope);
        hasher.write_f32(*min_support);
        hasher.write_f32(*lumpiness);
        hasher.write_f32(*lump_size);
        hasher.write_f32(*neck);
    }
}

/// Writes every setting.
impl ContentHash for MeshOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            edge_length,
            closed,
            strict,
        } = self;
        hasher.write_f32(*edge_length);
        hasher.write_bool(*closed);
        hasher.write_bool(*strict);
    }
}

/// Writes every setting.
impl ContentHash for MaterialOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            texture,
            color,
            roughness,
            texture_size,
        } = self;
        hasher.write(texture);
        hasher.write(color);
        hasher.write_f32(*roughness);
        hasher.write_f32(*texture_size);
    }
}

/// Writes a list of `count` tags: the count, then each name.
fn write_tag_list<'a>(
    hasher: &mut ContentHasher,
    count: usize,
    tags: impl Iterator<Item = &'a Tag>,
) {
    hasher.write_usize(count);
    for tag in tags {
        hasher.write_str(tag.as_str());
    }
}

/// Writes a tag set, in its sorted order.
fn write_tags(hasher: &mut ContentHasher, tags: &TagSet) {
    write_tag_list(hasher, tags.len(), tags.iter());
}

/// Writes a filter's wanted tags, then its excluded tags, each in the order
/// listed.
fn write_filter(hasher: &mut ContentHasher, filter: &TagFilter) {
    let TagFilter { any_of, none_of } = filter;
    write_tag_list(hasher, any_of.len(), any_of.iter());
    write_tag_list(hasher, none_of.len(), none_of.iter());
}
