//! Stable content hashes of this crate's settings.
//!
//! Each impl writes every field in declaration order. It destructures
//! `Self` in full, so adding a field fails to compile until the field is
//! hashed or deliberately left out.

use procgena_core::{ContentHash, ContentHasher, GeneratorKind, OptionsHash, PoolId};

use crate::detail::DeadwoodDetail;
use crate::options::{
    BarkOptions, BranchingOptions, DeadwoodOptions, EndKind, EndsOptions, HollowOptions,
    LeafOptions, MaterialOptions, PieceKind, RootOptions, SettleOptions, ShapeOptions, StubOptions,
};
use crate::scatter::ScatterOptions;
use crate::seed_keys::GENERATOR_KIND;

/// The generator kind of every pool of dead wood, standalone pieces and
/// scatter variants alike. It keeps deadwood pools apart from other
/// generators' pools.
pub const DEADWOOD_KIND: GeneratorKind = GeneratorKind::from_key(GENERATOR_KIND);

impl DeadwoodOptions {
    /// The pool of pieces built from these options: every seed gives one
    /// specimen of it. With the seed, it names one built piece, so caches of
    /// pieces can be keyed by `(pool_id(), seed)` and the detail drawn.
    pub fn pool_id(&self) -> PoolId {
        PoolId::new(DEADWOOD_KIND, OptionsHash::of(self))
    }
}

impl ScatterOptions {
    /// The pool a [`crate::StickScatter`]'s variants come from: the hash of
    /// [`piece`](Self::piece) and the decay, diameter and length ranges.
    /// Variant `v` is the specimen in slot `v` of this pool.
    ///
    /// The default seed, slope limit, density, spacing and number of
    /// variants are left out: they choose which specimens are drawn and
    /// where, not what the specimens are. So adding variants keeps the
    /// existing ones.
    pub fn variant_pool(&self) -> PoolId {
        let Self {
            default_seed: _,
            max_slope: _,
            density: _,
            min_spacing: _,
            variants: _,
            decay,
            diameter,
            length,
            piece,
        } = self;
        let mut h = ContentHasher::new();
        piece.content_hash(&mut h);
        decay.content_hash(&mut h);
        diameter.content_hash(&mut h);
        length.content_hash(&mut h);
        PoolId::new(DEADWOOD_KIND, OptionsHash(h.finish()))
    }
}

/// Writes every setting that shapes or colours the piece. Leaves out
/// [`DeadwoodOptions::default_seed`], which picks a specimen within the pool
/// these options name, and [`DeadwoodOptions::game_level`], which only changes
/// how finely the same piece is meshed.
impl ContentHash for DeadwoodOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            default_seed: _,
            kind,
            decay,
            length,
            diameter,
            taper,
            shape,
            ends,
            stubs,
            branching,
            bark,
            hollow,
            roots,
            leaves,
            material,
            settle,
            game_level: _,
        } = self;
        kind.content_hash(h);
        h.write_f32(*decay);
        h.write_f32(*length);
        h.write_f32(*diameter);
        h.write_f32(*taper);
        shape.content_hash(h);
        ends.content_hash(h);
        stubs.content_hash(h);
        branching.content_hash(h);
        bark.content_hash(h);
        hollow.content_hash(h);
        roots.content_hash(h);
        leaves.content_hash(h);
        material.content_hash(h);
        settle.content_hash(h);
    }
}

/// Writes 0 for a log, 1 for a branch and 2 for a stump.
impl ContentHash for PieceKind {
    fn content_hash(&self, h: &mut ContentHasher) {
        h.write_u32(match self {
            PieceKind::Log => 0,
            PieceKind::Branch => 1,
            PieceKind::Stump => 2,
        });
    }
}

impl ContentHash for ShapeOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            bend,
            wobble,
            lumps,
            lump_scale,
            ovality,
            lean,
        } = self;
        h.write_f32(*bend);
        h.write_f32(*wobble);
        h.write_f32(*lumps);
        h.write_f32(*lump_scale);
        ovality.content_hash(h);
        h.write_f32(*lean);
    }
}

impl ContentHash for EndsOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self { base, tip } = self;
        base.content_hash(h);
        tip.content_hash(h);
    }
}

/// Writes a variant number (saw 0, chainsaw 1, axe 2, snapped 3, taper 4),
/// then the variant's fields in order.
impl ContentHash for EndKind {
    fn content_hash(&self, h: &mut ContentHasher) {
        match *self {
            EndKind::Saw { tilt } => {
                h.write_u32(0);
                h.write_f32(tilt);
            }
            EndKind::Chainsaw { tilt, step } => {
                h.write_u32(1);
                h.write_f32(tilt);
                h.write_f32(step);
            }
            EndKind::Axe { angle } => {
                h.write_u32(2);
                h.write_f32(angle);
            }
            EndKind::Snapped {
                jaggedness,
                splinters,
                one_sided,
            } => {
                h.write_u32(3);
                h.write_f32(jaggedness);
                h.write_u32(splinters);
                h.write_f32(one_sided);
            }
            EndKind::Taper {} => h.write_u32(4),
        }
    }
}

impl ContentHash for StubOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            per_metre,
            length,
            radius,
            angle,
            keep,
        } = self;
        h.write_f32(*per_metre);
        h.write_f32(*length);
        h.write_f32(*radius);
        h.write_f32(*angle);
        keep.content_hash(h);
    }
}

impl ContentHash for BranchingOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            levels,
            children,
            length,
            radius,
            angle,
            twigs,
            twig_length,
            keep,
            twig_keep,
        } = self;
        h.write_u32(*levels);
        h.write_u32(*children);
        h.write_f32(*length);
        h.write_f32(*radius);
        h.write_f32(*angle);
        h.write_u32(*twigs);
        h.write_f32(*twig_length);
        keep.content_hash(h);
        twig_keep.content_hash(h);
    }
}

impl ContentHash for BarkOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            thickness,
            stripped,
            patch_size,
            cracks,
            plate_size,
        } = self;
        h.write_f32(*thickness);
        stripped.content_hash(h);
        h.write_f32(*patch_size);
        h.write_f32(*cracks);
        h.write_f32(*plate_size);
    }
}

impl ContentHash for HollowOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            enabled,
            radius,
            open_base,
            open_tip,
            depth,
            roughness,
        } = self;
        h.write_bool(*enabled);
        h.write_f32(*radius);
        h.write_bool(*open_base);
        h.write_bool(*open_tip);
        h.write_f32(*depth);
        h.write_f32(*roughness);
    }
}

impl ContentHash for RootOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            flare,
            flare_height,
            lobes,
            lobe_depth,
            count,
            length,
            radius,
            pitch,
            depth,
        } = self;
        h.write_f32(*flare);
        h.write_f32(*flare_height);
        h.write_u32(*lobes);
        h.write_f32(*lobe_depth);
        h.write_u32(*count);
        h.write_f32(*length);
        h.write_f32(*radius);
        h.write_f32(*pitch);
        h.write_f32(*depth);
    }
}

impl ContentHash for LeafOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            per_metre,
            texture,
            cards,
            size,
            size_variance,
            spread,
            lean,
            up,
            flatten,
            top,
            ends,
            crossed,
            tint,
        } = self;
        h.write_f32(*per_metre);
        h.write_str(texture);
        h.write_u32(*cards);
        h.write_f32(*size);
        h.write_f32(*size_variance);
        h.write_f32(*spread);
        h.write_f32(*lean);
        h.write_f32(*up);
        h.write_f32(*flatten);
        h.write_f32(*top);
        h.write_f32(*ends);
        h.write_bool(*crossed);
        h.write_str(tint);
    }
}

impl ContentHash for MaterialOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            bark,
            bark_weathered,
            sapwood,
            heartwood,
            rot,
            bark_texture,
            bark_texture_size,
            end_texture,
            roughness,
        } = self;
        h.write_str(bark);
        h.write_str(bark_weathered);
        h.write_str(sapwood);
        h.write_str(heartwood);
        h.write_str(rot);
        bark_texture.content_hash(h);
        h.write_f32(*bark_texture_size);
        end_texture.content_hash(h);
        h.write_f32(*roughness);
    }
}

impl ContentHash for SettleOptions {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            settle,
            sag,
            burial,
        } = self;
        h.write_bool(*settle);
        sag.content_hash(h);
        burial.content_hash(h);
    }
}

/// Writes every setting of one detail level, for keys that must tell
/// meshes of different detail apart.
impl ContentHash for DeadwoodDetail {
    fn content_hash(&self, h: &mut ContentHasher) {
        let Self {
            sides,
            segments_per_diameter,
            cap_rings,
            min_radius,
            ribbon_radius,
            leaf_share,
        } = self;
        h.write_u32(*sides);
        h.write_f32(*segments_per_diameter);
        h.write_u32(*cap_rings);
        h.write_f32(*min_radius);
        h.write_f32(*ribbon_radius);
        h.write_f32(*leaf_share);
    }
}
