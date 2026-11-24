//! Stable hashes of rock settings, and the ids that name rocks built from
//! them.
//!
//! A rock is named by its [`PoolId`] (the generator and the hash of every
//! shape and look setting) plus the seed it is built from. Two rocks with
//! equal names have the same shape. The detail levels are not part of the
//! name; hash them with [`content_hash`](procgena_core::content_hash) when
//! you also key meshes by them.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId};

use crate::detail::{DetailLevel, StoneDetail};
use crate::material::TextureSize;
use crate::options::{
    CarveOptions, MaterialOptions, MeshOptions, Mesher, PlacementOptions, StoneOptions,
    SurfaceOptions,
};

/// The generator kind of every rock this crate builds.
pub const STONE_KIND: GeneratorKind = GeneratorKind::from_key("stone.generator.kind");

impl StoneOptions {
    /// The pool these settings name: every rock built from them, whatever
    /// its seed or detail levels.
    pub fn pool_id(&self) -> PoolId {
        PoolId::new(STONE_KIND, OptionsHash::of(self))
    }
}

/// Writes every setting that changes the rock's shape or look, leaving out
/// the seed and the detail levels.
impl ContentHash for StoneOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            seed: _,
            scale,
            stretch,
            placement,
            carve,
            surface,
            mesh,
            material,
        } = self;
        hasher.write_f32(*scale);
        hasher.write(stretch);
        hasher.write(placement);
        hasher.write(carve);
        hasher.write(surface);
        hasher.write(mesh);
        hasher.write(material);
    }
}

/// Writes every setting.
impl ContentHash for PlacementOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            settle,
            max_embed_fraction,
            min_embed_fraction,
        } = self;
        hasher.write_bool(*settle);
        hasher.write_f32(*max_embed_fraction);
        hasher.write_f32(*min_embed_fraction);
    }
}

/// Writes every setting.
impl ContentHash for CarveOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            steps,
            smoothness,
            base_radius,
            radius_min,
            radius_jitter,
            depth,
            blockiness,
        } = self;
        hasher.write_u32(*steps);
        hasher.write_f32(*smoothness);
        hasher.write_f32(*base_radius);
        hasher.write_f32(*radius_min);
        hasher.write_f32(*radius_jitter);
        hasher.write_f32(*depth);
        hasher.write_f32(*blockiness);
    }
}

/// Writes every setting.
impl ContentHash for SurfaceOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            displacement_scale,
            displacement_spread,
            octaves,
        } = self;
        hasher.write_f32(*displacement_scale);
        hasher.write_f32(*displacement_spread);
        hasher.write_u32(*octaves);
    }
}

/// Writes the mesher only: the detail levels are not part of a rock's name.
impl ContentHash for MeshOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self { mesher, detail: _ } = self;
        hasher.write(mesher);
    }
}

/// Writes a fixed number per method.
impl ContentHash for Mesher {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(match self {
            Self::MarchingCubes => 0,
            Self::DualContouring => 1,
        });
    }
}

/// Writes every setting.
impl ContentHash for MaterialOptions {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            texture,
            color,
            roughness,
            metallic,
            texture_size,
        } = self;
        hasher.write_str(texture);
        hasher.write(color);
        hasher.write_f32(*roughness);
        hasher.write_f32(*metallic);
        hasher.write_f32(*texture_size);
    }
}

/// Writes every level.
impl ContentHash for StoneDetail {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write(self.levels());
    }
}

/// Writes every setting.
impl ContentHash for DetailLevel {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        let Self {
            resolution,
            distance,
            hysteresis,
            texture_size,
        } = self;
        hasher.write_u32(*resolution);
        hasher.write_f32(*distance);
        hasher.write_f32(*hysteresis);
        hasher.write(texture_size);
    }
}

/// Writes a fixed number per size.
impl ContentHash for TextureSize {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_u32(match self {
            Self::Size512 => 0,
            Self::Size1k => 1,
            Self::Size2k => 2,
        });
    }
}
