//! [`Piece`]: one generated piece of dead wood, resting on the ground.

use procgena_core::chain::Step;

use crate::build::{build, visible_span};
use crate::colour::Palette;
use crate::decay::Decay;
use crate::detail::{DeadwoodDetail, DeadwoodLod};
use crate::leaves::{Card, mesh_cards, place};
use crate::limb::Limb;
use crate::mesh::{DeadwoodMesh, Shade, build_mesh};
use crate::options::{DeadwoodOptions, OptionsError};
use crate::presets::DeadwoodPreset;
use crate::settle::{RestingPose, settle};

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
    pose: RestingPose,
    decay: Decay,
    /// Every leaf card, placed on the settled stem.
    cards: Vec<Card>,
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
        let pose = settle(&options, &decay, &mut limbs);
        let span = visible_span(&options, &limbs[0]);
        let cards = place(&options, &limbs[0], span, pose.burial);
        Ok(Self {
            options,
            limbs,
            pose,
            decay,
            cards,
        })
    }

    /// Builds a piece from a preset's settings. To adjust settings first,
    /// clone [`DeadwoodPreset::deadwood`], edit it, and pass it to
    /// [`Self::new`] instead.
    ///
    /// # Errors
    ///
    /// Returns the first invalid setting; see [`DeadwoodOptions::validate`].
    /// Built-in presets always pass; a preset read from a file was checked
    /// when it was read.
    pub fn from_preset(preset: &DeadwoodPreset) -> Result<Self, OptionsError> {
        Self::new(preset.deadwood.clone())
    }

    /// The options the piece was built from.
    pub fn options(&self) -> &DeadwoodOptions {
        &self.options
    }

    /// Where the piece came to rest, and its suggested burial depth.
    pub fn pose(&self) -> &RestingPose {
        &self.pose
    }

    /// Number of stems: the main stem plus every stub, side branch, twig and
    /// root.
    pub fn stem_count(&self) -> usize {
        self.limbs.len()
    }

    /// Builds the game-level mesh: the options' own
    /// [`game_level`](DeadwoodOptions::game_level), sized for this piece (see
    /// [`DeadwoodDetail::sized_for`]). This is the piece's real shape.
    ///
    /// # Panics
    ///
    /// Panics if the mesh needs more vertices than fit in a 32-bit index.
    pub fn mesh(&self) -> DeadwoodMesh {
        self.mesh_with_detail(&self.options.game_level.sized_for(&self.options))
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
            self.options.default_seed,
            detail.leaf_share,
        );
        mesh
    }

    /// The details of the two meshes a renderer draws: the options'
    /// [`game_level`](DeadwoodOptions::game_level) and the coarser mesh
    /// after it, both sized for this piece (see
    /// [`DeadwoodDetail::sized_for`]).
    pub fn lod(&self) -> DeadwoodLod {
        self.options
            .game_level
            .lod()
            .map(|detail| detail.sized_for(&self.options))
    }

    /// Builds the two meshes a renderer draws, nearest first: the
    /// game-level mesh (the same as [`Self::mesh`]) and the coarser mesh
    /// drawn when the piece is small on screen. A renderer draws the
    /// piece's impostor after them.
    ///
    /// # Panics
    ///
    /// Panics if a mesh needs more vertices than fit in a 32-bit index.
    pub fn lod_meshes(&self) -> [(Step, DeadwoodMesh); 2] {
        self.lod()
            .meshes()
            .map(|(step, detail)| (step, self.mesh_with_detail(detail)))
    }

    /// Returns the stems, for other modules of this crate.
    pub(crate) fn limbs(&self) -> &[Limb] {
        &self.limbs
    }
}
