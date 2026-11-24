//! Ground materials: what the ground is made of at each point, such as
//! grass, rock, a gravel road or needle litter.
//!
//! - A [`MaterialCatalogue`] names a World's materials by key, each with a
//!   colour.
//! - A [`GroundLayer`] lays one material where its rule says: conditions
//!   over the ground's slope, height and water, road tags, patchy noise,
//!   and (in an Ecotope) the Ecotope's own Scatters.
//! - A [`GroundStack`] lays layers over each other by `order`, bottom to
//!   top, into a [`GroundMix`] of materials at each point.
//!   [`GroundStack::bare`] is the look of bare terrain.
//! - A [`GroundOverlay`] holds layers laid in among a stack's, such as an
//!   Ecotope's over its Biome's base ground.
//! - [`ChunkWeights`] paints one terrain chunk with any [`GroundPainter`]:
//!   a palette of up to [`PALETTE_SIZE`] materials and their weights at
//!   every texel, for drawing.
//!
//! The ground's look never feeds placement: Scatters may shape the ground,
//! but nothing is placed from ground layers.
//!
//! ```
//! use glam::{DVec2, Vec2};
//! use procgena_terrain::ground::{GroundSite, GroundStack};
//!
//! let bare = GroundStack::bare(7, 1.0);
//! let catalogue = bare.catalogue().clone();
//!
//! // Flat ground is grass; a 45° slope is rock.
//! let flat = bare.paint(DVec2::ZERO, &GroundSite::new(10.0, Vec2::ZERO));
//! assert_eq!(flat.weight(catalogue.id("grass").unwrap()), 1.0);
//! let steep = bare.paint(DVec2::ZERO, &GroundSite::new(10.0, Vec2::new(1.0, 0.0)));
//! assert_eq!(steep.weight(catalogue.id("cliff_rock").unwrap()), 1.0);
//! ```

mod bare;
mod layer;
mod material;
mod site;
mod stack;
mod weights;

use std::fmt;

use procgena_core::ChildKey;

pub use bare::{BARE_SEED_KIND, bare_layers};
pub use layer::{Condition, GroundLayer, GroundNoise, HeightRule, Ramp, Reach, ScatterInfluence};
pub use material::{GroundMaterial, MaterialCatalogue, MaterialId};
pub use site::{GroundSite, GroundSource, OnGround, Water};
pub use stack::{GroundMix, GroundOverlay, GroundPainter, GroundStack};
pub use weights::{ChunkWeights, GROUND_TEXELS, PALETTE_SIZE};

/// Why a catalogue or a stack of ground layers is invalid.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GroundError {
    /// A material's colour or roughness is outside 0 to 1.
    Material {
        /// The material's key.
        key: ChildKey,
    },
    /// The catalogue has more than 65,536 materials.
    TooManyMaterials,
    /// The World's vertical scale isn't more than 0.
    Vertical,
    /// A stack has no layers.
    Empty,
    /// A stack's lowest layer doesn't cover everything: it has conditions,
    /// noise or less than full opacity.
    BaseNotOpaque {
        /// The lowest layer's key.
        key: ChildKey,
    },
    /// A layer outside an Ecotope follows a Scatter.
    Influence {
        /// The layer's key.
        key: ChildKey,
    },
    /// A layer's opacity is outside 0 to 1, or a band, noise or reach in
    /// it is unusable.
    Layer {
        /// The layer's key.
        key: ChildKey,
    },
    /// A layer names a material the catalogue doesn't have.
    UnknownMaterial {
        /// The layer's key.
        key: ChildKey,
        /// The material it names.
        material: ChildKey,
    },
    /// Two layers share an order, so which lies on top isn't settled.
    SharedOrder {
        /// The shared order.
        order: u32,
        /// The two layers' keys.
        keys: [ChildKey; 2],
    },
    /// Two different layer keys hash to the same salt, so their noise
    /// would share a seed. Rename one of them.
    SharedSalt {
        /// The two keys.
        keys: [ChildKey; 2],
    },
}

impl fmt::Display for GroundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Material { key } => write!(
                f,
                "material {key}: colour and roughness must be from 0 to 1"
            ),
            Self::TooManyMaterials => write!(f, "a catalogue holds at most 65536 materials"),
            Self::Vertical => write!(f, "the vertical scale must be more than 0"),
            Self::Empty => write!(f, "a ground stack needs at least one layer"),
            Self::BaseNotOpaque { key } => write!(
                f,
                "ground layer {key} is the lowest of its stack, so it must cover everything: \
                 full opacity, no conditions, no noise"
            ),
            Self::Influence { key } => write!(
                f,
                "ground layer {key} follows a Scatter, which only an Ecotope's layers can"
            ),
            Self::Layer { key } => write!(
                f,
                "ground layer {key}: opacity must be from 0 to 1, bands must run from low to \
                 high, and noise wavelengths and reaches must be more than 0"
            ),
            Self::UnknownMaterial { key, material } => {
                write!(
                    f,
                    "ground layer {key}: no material {material} in the catalogue"
                )
            }
            Self::SharedOrder { order, keys } => write!(
                f,
                "ground layers {} and {} share order {order}",
                keys[0], keys[1]
            ),
            Self::SharedSalt { keys } => write!(
                f,
                "ground layer keys {} and {} hash to the same seed; rename one",
                keys[0], keys[1]
            ),
        }
    }
}

impl std::error::Error for GroundError {}
