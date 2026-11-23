//! [`Piece`]: one generated piece of dead wood, resting on the ground.

use std::fmt;

use crate::build::{build, visible_span};
use crate::colour::Palette;
use crate::decay::Decay;
use crate::detail::{DeadwoodDetail, DeadwoodLod, LodLevel};
use crate::leaves::{Card, mesh_cards, place};
use crate::limb::Limb;
use crate::mesh::{DeadwoodMesh, Shade, build_mesh};
use crate::options::{DeadwoodOptions, OptionsError};
use crate::presets::DeadwoodPreset;
use crate::settle::{Placement, settle};

/// A generated piece of dead wood: a log, a fallen branch or a stump.
///
/// Creating a piece builds its shape and rests it on flat ground at
/// `y = 0`, with Y up. Meshes are made on request at any level of detail,
/// and every level has the same outline and resting position, so a
/// renderer can swap between them without the piece jumping.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    options: DeadwoodOptions,
    limbs: Vec<Limb>,
    placement: Placement,
    decay: Decay,
    /// Every leaf card, placed on the settled stem.
    cards: Vec<Card>,
}

/// Why a piece could not be created.
#[derive(Debug)]
#[non_exhaustive]
pub enum DeadwoodError {
    /// A setting is out of range.
    Options(OptionsError),
    /// JSON could not be read.
    Json(serde_json::Error),
}

impl fmt::Display for DeadwoodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeadwoodError::Options(e) => write!(f, "invalid deadwood options: {e}"),
            DeadwoodError::Json(e) => write!(f, "invalid deadwood JSON: {e}"),
        }
    }
}

impl std::error::Error for DeadwoodError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DeadwoodError::Options(e) => Some(e),
            DeadwoodError::Json(e) => Some(e),
        }
    }
}

impl From<OptionsError> for DeadwoodError {
    fn from(e: OptionsError) -> Self {
        DeadwoodError::Options(e)
    }
}

impl From<serde_json::Error> for DeadwoodError {
    fn from(e: serde_json::Error) -> Self {
        DeadwoodError::Json(e)
    }
}

impl Piece {
    /// Builds a piece from `options` and rests it on the ground.
    ///
    /// # Errors
    ///
    /// Returns the first invalid setting; see
    /// [`DeadwoodOptions::validate`].
    pub fn new(options: DeadwoodOptions) -> Result<Self, OptionsError> {
        options.validate()?;
        let decay = Decay::new(&options);
        let mut limbs = build(&options, &decay);
        let placement = settle(&options, &decay, &mut limbs);
        let span = visible_span(&options, &limbs[0]);
        let cards = place(&options, &limbs[0], span, placement.burial);
        Ok(Self {
            options,
            limbs,
            placement,
            decay,
            cards,
        })
    }

    /// Builds a piece from a preset's options. Presets are always valid, so
    /// this cannot fail.
    pub fn from_preset(preset: DeadwoodPreset) -> Self {
        Self::new(preset.options()).expect("presets are valid")
    }

    /// Builds a piece from JSON options; see [`DeadwoodOptions::from_json`].
    ///
    /// # Errors
    ///
    /// Returns [`DeadwoodError::Json`] if the JSON can't be read and
    /// [`DeadwoodError::Options`] if a setting is out of range.
    pub fn from_json(json: &str) -> Result<Self, DeadwoodError> {
        Ok(Self::new(DeadwoodOptions::from_json(json)?)?)
    }

    /// The options the piece was built from.
    pub fn options(&self) -> &DeadwoodOptions {
        &self.options
    }

    /// Where the piece came to rest, and its suggested burial depth.
    pub fn placement(&self) -> &Placement {
        &self.placement
    }

    /// Number of stems: the main stem plus every stub, side branch, twig and
    /// root.
    pub fn stem_count(&self) -> usize {
        self.limbs.len()
    }

    /// Builds the mesh at the options' own [`DeadwoodOptions::detail`].
    ///
    /// # Panics
    ///
    /// Panics if the mesh needs more vertices than fit in a 32-bit index.
    pub fn mesh(&self) -> DeadwoodMesh {
        self.mesh_with_detail(&self.options.detail)
    }

    /// Builds the mesh at `detail`.
    ///
    /// # Panics
    ///
    /// Panics if `detail` is invalid (see [`DeadwoodDetail::validate`]) or
    /// the mesh needs more vertices than fit in a 32-bit index.
    pub fn mesh_with_detail(&self, detail: &DeadwoodDetail) -> DeadwoodMesh {
        assert!(detail.validate().is_ok(), "invalid detail: {detail:?}");
        let shade = Shade {
            palette: Palette::new(&self.options.material),
            decay: self.decay,
        };
        let mut mesh = build_mesh(&self.limbs, detail, &shade);
        mesh.leaves = mesh_cards(
            &self.cards,
            &self.options.leaves,
            self.options.seed,
            detail.leaf_share,
        );
        mesh
    }

    /// Builds one mesh per level of `lod`, from nearest to furthest, each
    /// paired with its level.
    ///
    /// # Errors
    ///
    /// Returns the first problem with `lod`; see [`DeadwoodLod::validate`].
    ///
    /// # Panics
    ///
    /// Panics if a mesh needs more vertices than fit in a 32-bit index.
    pub fn lod_meshes(
        &self,
        lod: &DeadwoodLod,
    ) -> Result<Vec<(LodLevel, DeadwoodMesh)>, OptionsError> {
        lod.validate()?;
        Ok(lod
            .levels
            .iter()
            .map(|level| (*level, self.mesh_with_detail(&level.detail)))
            .collect())
    }

    /// Returns the stems, for other modules of this crate.
    pub(crate) fn limbs(&self) -> &[Limb] {
        &self.limbs
    }
}
