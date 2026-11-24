//! Tree Scatters, such as groves and woodlands: several tree species spread
//! over a rectangle of ground.
//!
//! A [`TreeScatter`] says which trees grow (a list of [`ScatterTree`]s, each with
//! its own density, spacing and clustering) and how the placed trees vary.
//! [`TreeScatterPlacement::new`] lays it out over a [`ScatterInput`] (ground,
//! area, seed and an optional mask) and returns one [`TreeCopy`] per
//! tree: which species and which of its shape variants to draw, and where.
//! Ready-made Scatters are [`TreeScatterPreset`]s, and `.tree_scatter.yml`
//! files load with [`TreeScatterPreset::from_yaml`].
//!
//! Each species is drawn from a few shape variants (the same settings with
//! different seeds), so a renderer builds a handful of tree meshes and draws
//! each many times.

mod file;
mod presets;

use std::fmt;

use glam::{Affine3A, DVec2, Quat, Vec3};
use procgena_core::hash::{pcg2, seed_for};
use procgena_core::scatter::lattice::{self, Fixed2};
use procgena_core::scatter::{
    DEFAULT_SPACING_ROUNDS, Disc, Footprint, Lattice, Placement, Rank, Reach, Rest, Scatter,
    ScatterCopy, ScatterError, ScatterInput, ScatterSlot, Spaced, keep_spaced, place, steeper_than,
};
use procgena_core::{CellId, ChildKey, CopyAddress};
use serde::{Deserialize, Serialize};

use crate::options::{OptionsError, TreeOptions};
use crate::presets::{TreePreset, TreeStand};

pub use presets::TreeScatterPreset;

/// Most species one grove may hold.
pub const MAX_SCATTER_TREES: usize = 16;
/// Most shape variants one species may use.
pub const MAX_SCATTER_VARIANTS: u32 = 8;
/// Most trees one placement may ask for, before spacing removes any.
pub const MAX_SCATTER_COPIES: usize = 20_000;
/// Longest side of the ground a grove may be placed on, in metres.
pub const MAX_SCATTER_SIZE: f32 = 2048.0;

/// Shape variants a species uses unless told otherwise.
const DEFAULT_VARIANTS: u32 = 4;
/// Distance between candidate tree sites, in metres. Each square of this size
/// holds at most one tree, so a grove is never denser than 4 trees per m².
const SITE_CELL: f32 = 0.5;
/// Unit directions from a trunk to the points around it where the ground is
/// sampled: the axes and the diagonals.
const FOOTING_DIRECTIONS: [(f32, f32); 8] = {
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (D, D),
        (D, -D),
        (-D, D),
        (-D, -D),
    ]
};

/// Smallest radius around a trunk at which the ground is sampled, in metres.
const MIN_FOOTPRINT: f32 = 0.25;

impl TreeOptions {
    /// How a tree stands on uneven ground: upright, its trunk resting on the
    /// lowest ground around it, not sunk, on any bend. A grove sinks its
    /// trees further by its [`sink`](TreeScatterVariation::sink).
    ///
    /// It moves copies, not shapes, so it is not part of the options hash.
    pub fn ground_placement(&self) -> Placement {
        Placement {
            conform: 0.0,
            rest: Rest::Lowest,
            embed: 0.0,
            max_bend: None,
        }
    }
}
/// Square metres in a hectare.
const HECTARE: f32 = 10_000.0;
/// Width of the patches of ground that are either wooded or glade, in
/// metres. Neighbouring glade patches join into larger glades.
const GLADE_CELL: f32 = 12.0;
/// Share of the ground left open as glades at a patchiness of 1. Above a
/// half, glades join up across the grove and the woods break into islands.
const MAX_OPEN: f32 = 0.7;
/// How far ordinary trees spread around their normal size, as a share of
/// the size variation: they are scaled by `1 ± BODY_SPREAD * scale_jitter`.
const BODY_SPREAD: f32 = 0.5;
/// How much further than ordinary trees the largest giants grow, as a share
/// of the size variation: the largest tree is
/// `1 + (BODY_SPREAD + GIANT_GROWTH) * scale_jitter` times its normal size.
const GIANT_GROWTH: f32 = 2.5;
/// How rare giants are: the higher, the fewer trees approach the largest
/// size.
const GIANT_RARITY: i32 = 8;

/// How far away, in metres, a tree Scatter is drawn. Trees have no cull
/// distance of their own yet (their impostors draw on), so this is a fixed
/// stand-in for the largest trees.
const DRAW_RANGE: f32 = 2000.0;

// Seed keys for the Scatter's random streams, named
// `<crate>.<area>.<role>`. They are frozen: renaming one moves every tree.
// Add a new key to `skeleton::KEYS` too.
pub(crate) const LATTICE_SEED: &str = "tree.scatter.lattice";
pub(crate) const PATCHES_SEED: &str = "tree.scatter.patches";
pub(crate) const GLADES_SEED: &str = "tree.scatter.glades";

// Salts that give each draw of one spot its own value. Species salts are
// combined with the species' key.
const SALT_X: u32 = 0x1;
const SALT_Z: u32 = 0x2;
const SALT_RANK: u32 = 0x3;
const SALT_VARIANT: u32 = 0x4;
const SALT_YAW: u32 = 0x5;
const SALT_LEAN_DIRECTION: u32 = 0x6;
const SALT_LEAN: u32 = 0x7;
const SALT_SCALE: u32 = 0x8;
const SALT_SPECIES: u32 = 0x100;
const SALT_CLUSTER: u32 = 0x400;

/// One species in a [`TreeScatter`]: what the tree looks like, and how many
/// grow and how close together.
///
/// [`ScatterTree::new`] starts from a [`TreePreset`] with a density and spacing
/// that suit it. Every field is public, so a species can also be built or
/// changed directly.
///
/// In a file, `key` names the species within its Scatter, and `tree` is a
/// [`TreePreset`] key such as `oak_small`, or a map of tree settings with an
/// optional `base` tree preset key they are laid over. `key` may be left out
/// when the tree names a preset: it is then the preset's key. `density`,
/// `spacing` and `max_slope` may be left out when the tree names a preset:
/// they then come from its [`stand`](TreePreset::stand). A `max_slope` of
/// `null` uses the Scatter's own limit. The other settings default as in
/// [`ScatterTree::custom`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "file::ScatterTreeFile", into = "file::ScatterTreeFile")]
pub struct ScatterTree {
    /// Names the species within its Scatter. Each species' own random
    /// choices (its stands, and which candidate spots it takes) come from
    /// its key, not its place in the list. Keys must differ within a
    /// Scatter; changing one moves the species' trees.
    pub key: ChildKey,
    /// The tree's shape. Every variant, the first included, grows from a seed
    /// derived from this [`default_seed`](TreeOptions::default_seed); see
    /// [`variant`](Self::variant).
    pub tree: TreeOptions,
    /// Trees per hectare (10,000 m²) that suit this species on its own, before
    /// [`density_scale`](Self::density_scale) and the grove's own scale. Must
    /// be 0 or more.
    pub density: f32,
    /// Smallest distance between this species' trunks and any neighbour's,
    /// in metres. Between two species the larger spacing applies. Must be 0
    /// or more; 0 allows trees to touch.
    pub spacing: f32,
    /// Multiplies [`density`](Self::density) in this grove: 1 as it is, 0.5
    /// half as dense, 0 removes the species. Must be 0 or more.
    pub density_scale: f32,
    /// How much this species grows in stands of its own: 0 mixes it evenly
    /// among the others, 1 gathers it into patches several spacings across.
    /// Clustering changes where a species grows more than how much, and has
    /// no effect on the last species with a density above 0, which fills the
    /// gaps the others leave.
    pub clustering: f32,
    /// How many shape variants to draw this species from, from 1 to
    /// [`MAX_SCATTER_VARIANTS`]. More variants look less repetitive but cost a
    /// mesh each.
    pub variants: u32,
    /// Steepest ground this species grows on, in degrees from level, from 0
    /// to 90. `None` uses the grove's
    /// [`max_slope`](TreeScatterVariation::max_slope). Set it higher for shrubs and
    /// saplings, which cling to slopes large trees can't.
    pub max_slope: Option<f32>,
}

impl ScatterTree {
    /// A species from a preset, keyed by the preset's key, at the density,
    /// spacing and slope tolerance of the preset's
    /// [`stand`](TreePreset::stand), evenly mixed, with 4 shape variants. A
    /// preset without a stand gives a density and spacing of 0: set them
    /// yourself.
    ///
    /// # Panics
    ///
    /// Panics if the preset's key is not a well-formed [`ChildKey`]; every
    /// built-in preset's key is.
    pub fn new(preset: &TreePreset) -> Self {
        let stand = preset.stand.unwrap_or(TreeStand {
            density: 0.0,
            spacing: 0.0,
            max_slope: None,
        });
        let key = ChildKey::new(preset.key.as_ref()).expect("tree preset keys are child keys");
        let mut tree = Self::custom(key, preset.tree.clone(), stand.density, stand.spacing);
        tree.max_slope = stand.max_slope;
        tree
    }

    /// A species named `key`, with your own tree settings, `density` in trees
    /// per hectare and `spacing` in metres, evenly mixed, with 4 shape
    /// variants, on the Scatter's slope limit.
    pub fn custom(key: ChildKey, tree: TreeOptions, density: f32, spacing: f32) -> Self {
        Self {
            key,
            tree,
            density,
            spacing,
            density_scale: 1.0,
            clustering: 0.0,
            variants: DEFAULT_VARIANTS,
            max_slope: None,
        }
    }

    /// The same species, denser or sparser: sets
    /// [`density_scale`](Self::density_scale).
    pub fn scaled(mut self, density_scale: f32) -> Self {
        self.density_scale = density_scale;
        self
    }

    /// The same species, growing in stands: sets
    /// [`clustering`](Self::clustering).
    pub fn clustered(mut self, clustering: f32) -> Self {
        self.clustering = clustering;
        self
    }

    /// The same species, drawn from `variants` shapes.
    pub fn with_variants(mut self, variants: u32) -> Self {
        self.variants = variants;
        self
    }

    /// Trees per hectare of this species, after its own
    /// [`density_scale`](Self::density_scale) but before the grove's.
    pub fn density(&self) -> f32 {
        self.density * self.density_scale
    }

    /// The tree settings of shape variant `variant`, counting from 0.
    ///
    /// Every variant differs from [`tree`](Self::tree) only in its seed,
    /// variant 0 included: the seed of specimen `variant` of the tree's pool
    /// ([`TreeOptions::specimen_id`]), with the tree's own
    /// [`default_seed`](TreeOptions::default_seed) standing in for the World
    /// seed. Changing the Scatter's seed therefore moves trees without
    /// changing their shapes.
    pub fn variant(&self, variant: u32) -> TreeOptions {
        let mut options = self.tree.clone();
        // The seed's bits, read as unsigned, so negative seeds stay distinct.
        let specimen = self
            .tree
            .specimen_id(self.tree.default_seed as u32, variant);
        options.default_seed = specimen.seed() as i32;
        options
    }

    fn validate(&self, index: usize) -> Result<(), TreeScatterError> {
        let field = |name: &str| format!("trees[{index}].{name}");
        non_negative(self.density, &field("density"))?;
        non_negative(self.spacing, &field("spacing"))?;
        non_negative(self.density_scale, &field("density_scale"))?;
        check(
            (0.0..=1.0).contains(&self.clustering),
            &field("clustering"),
            "between 0 and 1",
        )?;
        check(
            (1..=MAX_SCATTER_VARIANTS).contains(&self.variants),
            &field("variants"),
            "between 1 and 8",
        )?;
        check(
            self.max_slope.is_none_or(|s| (0.0..=90.0).contains(&s)),
            &field("max_slope"),
            "between 0 and 90",
        )?;
        self.tree
            .validate()
            .map_err(|error| TreeScatterError::Tree { index, error })
    }
}

/// How the trees of a [`TreeScatter`] differ from one another, beyond their
/// species and shape variant. These settings move, turn and resize trees;
/// they never change a tree's mesh.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeScatterVariation {
    /// How much tree sizes vary, from 0 to 0.9. Most trees are scaled by a
    /// factor between `1 - 0.5 * scale_jitter` and `1 + 0.5 * scale_jitter`,
    /// and a few grow into giants up to `1 + 3 * scale_jitter` times their
    /// normal size. At 0.9, about one tree in thirteen is more than twice its
    /// normal size.
    pub scale_jitter: f32,
    /// Largest lean of a trunk away from vertical, in degrees, each tree in
    /// a random direction. From 0 to 45.
    pub lean: f32,
    /// Steepest ground a tree grows on, in degrees from level, unless its
    /// species sets its own [`max_slope`](ScatterTree::max_slope). From 0 to
    /// 90; 90 allows any slope. The slope is measured across the trunk's
    /// footprint.
    pub max_slope: f32,
    /// How far each tree is sunk below the lowest ground around its trunk, in
    /// metres, so its roots never show above a slope. Must be 0 or more.
    pub sink: f32,
}

impl Default for TreeScatterVariation {
    fn default() -> Self {
        Self {
            scale_jitter: 0.15,
            lean: 3.0,
            max_slope: 35.0,
            sink: 0.1,
        }
    }
}

impl TreeScatterVariation {
    fn validate(&self) -> Result<(), TreeScatterError> {
        check(
            (0.0..=0.9).contains(&self.scale_jitter),
            "variation.scale_jitter",
            "between 0 and 0.9",
        )?;
        check(
            (0.0..=45.0).contains(&self.lean),
            "variation.lean",
            "between 0 and 45",
        )?;
        check(
            (0.0..=90.0).contains(&self.max_slope),
            "variation.max_slope",
            "between 0 and 90",
        )?;
        non_negative(self.sink, "variation.sink")
    }
}

/// A mix of tree species and how they spread over ground, such as a grove
/// or a woodland: the Scatter level for trees.
///
/// A Scatter holds its species itself, and says nothing about where it grows:
/// pass a [`ScatterInput`] (ground, area, seed and an optional mask) to
/// [`TreeScatterPlacement::new`]. Start from a [`TreeScatterPreset`]'s
/// [`tree_scatter`](TreeScatterPreset::tree_scatter), or list species with
/// [`TreeScatter::new`].
///
/// How the species share the grove:
///
/// - **Densities add up.** The grove grows the sum of its species'
///   densities ([`density`](Self::density)), and each species' share is its
///   density over that sum.
/// - **[`density_scale`](Self::density_scale)** multiplies every species'
///   density at once, keeping the mix.
/// - **Order matters for clustering.** Species are chosen in list order, and
///   the last species with a density above 0 fills whatever the others
///   leave. Put the species that fills the gaps last.
/// - **Spacing thins the scatter.** A tree too close to a neighbour is left
///   out, so a dense grove with wide spacing grows fewer trees than its
///   density asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeScatter {
    /// The seed to place this Scatter with when it is placed on its own:
    /// pass it as the [`ScatterInput`]'s seed. Inside a larger scene the
    /// input's seed comes from the scene instead. The placement seed picks
    /// where trees stand, which species each one is and how it is turned and
    /// sized. Changing it moves every tree but keeps the shape variants the
    /// same.
    pub default_seed: u32,
    /// The species that grow here, at most [`MAX_SCATTER_TREES`].
    pub trees: Vec<ScatterTree>,
    /// Multiplies every species' density, 1 by default, keeping the mix.
    /// Must be 0 or more.
    pub density_scale: f32,
    /// How much of the ground is left open as treeless glades, from 0 (none,
    /// the default) to 1. Around 0.5 the woods hold separate glades; from
    /// about 0.7 the glades join up and the trees stand in separate islands.
    /// Glades take trees away, so the grove grows fewer than
    /// [`density`](Self::density) asks for. Raising it only removes trees.
    pub patchiness: f32,
    /// How individual trees differ.
    pub variation: TreeScatterVariation,
}

impl Default for TreeScatter {
    /// A Scatter with no trees: [`TreeScatter::new`] with an empty list.
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl TreeScatter {
    /// A Scatter of these species, with seed 0, density scale 1, no glades
    /// and the default [`TreeScatterVariation`].
    pub fn new(trees: Vec<ScatterTree>) -> Self {
        Self {
            default_seed: 0,
            trees,
            density_scale: 1.0,
            patchiness: 0.0,
            variation: TreeScatterVariation::default(),
        }
    }

    /// Trees per hectare of all species together, including the grove's
    /// [`density_scale`](Self::density_scale), before spacing removes any.
    pub fn density(&self) -> f32 {
        self.unscaled_density() * self.density_scale
    }

    /// The same grove and mix with a total of `per_hectare` trees per
    /// hectare: sets [`density_scale`](Self::density_scale). A grove whose
    /// species all have a density of 0 is returned unchanged.
    pub fn with_density(mut self, per_hectare: f32) -> Self {
        let unscaled = self.unscaled_density();
        if unscaled > 0.0 {
            self.density_scale = per_hectare / unscaled;
        }
        self
    }

    /// The same grove with these species.
    pub fn with_trees(mut self, trees: Vec<ScatterTree>) -> Self {
        self.trees = trees;
        self
    }

    /// Checks every setting, including each species' tree settings.
    ///
    /// # Errors
    ///
    /// Returns a [`TreeScatterError`] naming the first problem found.
    pub fn validate(&self) -> Result<(), TreeScatterError> {
        if self.trees.len() > MAX_SCATTER_TREES {
            return Err(TreeScatterError::TooManyTrees {
                count: self.trees.len(),
            });
        }
        non_negative(self.density_scale, "density_scale")?;
        check(
            (0.0..=1.0).contains(&self.patchiness),
            "patchiness",
            "between 0 and 1",
        )?;
        self.variation.validate()?;
        for (index, tree) in self.trees.iter().enumerate() {
            tree.validate(index)?;
            if let Some(first) = self.trees[..index]
                .iter()
                .position(|other| other.key.salt() == tree.key.salt())
            {
                return Err(TreeScatterError::KeyClash {
                    first,
                    second: index,
                });
            }
        }
        Ok(())
    }

    fn unscaled_density(&self) -> f32 {
        self.trees.iter().map(ScatterTree::density).sum()
    }
}

/// Why a grove could not be placed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TreeScatterError {
    /// A grove setting, or the ground size, is out of range.
    Options(OptionsError),
    /// The tree settings of species `index` are invalid.
    Tree {
        /// The species' position in [`TreeScatter::trees`].
        index: usize,
        /// What is wrong with its settings.
        error: OptionsError,
    },
    /// The grove lists more than [`MAX_SCATTER_TREES`] species.
    TooManyTrees {
        /// The number of species listed.
        count: usize,
    },
    /// The density and size would place more than [`MAX_SCATTER_COPIES`]
    /// trees.
    TooMany {
        /// The number of trees the settings ask for.
        count: usize,
    },
    /// Two species have the same key, or keys that share a salt.
    KeyClash {
        /// The first species' position in [`TreeScatter::trees`].
        first: usize,
        /// The second species' position.
        second: usize,
    },
}

impl fmt::Display for TreeScatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options(error) => write!(f, "invalid tree Scatter settings: {error}"),
            Self::Tree { index, error } => write!(f, "invalid tree in trees[{index}]: {error}"),
            Self::TooManyTrees { count } => write!(
                f,
                "the Scatter lists {count} species, but at most {MAX_SCATTER_TREES} are allowed"
            ),
            Self::TooMany { count } => write!(
                f,
                "the Scatter asks for {count} trees, but at most {MAX_SCATTER_COPIES} are allowed"
            ),
            Self::KeyClash { first, second } => write!(
                f,
                "trees[{first}] and trees[{second}] have the same key, or keys with the same salt"
            ),
        }
    }
}

impl std::error::Error for TreeScatterError {}

impl From<OptionsError> for TreeScatterError {
    fn from(error: OptionsError) -> Self {
        Self::Options(error)
    }
}

/// One placed tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeCopy {
    /// The tree's species: its position in [`TreeScatter::trees`].
    pub tree: usize,
    /// Which of the species' shape variants to draw; see
    /// [`ScatterTree::variant`].
    pub variant: u32,
    /// Names the spot the tree stands on, anywhere in the World. A tree at
    /// the same spot keeps its address when the Scatter changes or is
    /// placed over another area, so a renderer can update trees in place
    /// instead of rebuilding them.
    pub address: CopyAddress,
    /// The foot of the trunk: X and Z relative to the area's centre, with
    /// the height it stands at in Y, in metres.
    pub position: Vec3,
    /// The ground the trunk takes up, in World fixed point. Other Scatters
    /// keep their copies out of it.
    pub footprint: Disc,
    /// Moves the tree's meshes into place: sized, leaned, turned and stood on
    /// the ground. Apply it to every vertex of the variant's mesh, at any
    /// level of detail.
    pub transform: Affine3A,
}

/// Trees spread over a rectangle of ground by a [`TreeScatter`].
///
/// Create one with [`TreeScatterPlacement::new`], then draw each of
/// [`instances`](Self::instances) with the mesh of its species' variant.
///
/// The same Scatter and input always give the same trees. Lowering the
/// Scatter's [`density_scale`](TreeScatter::density_scale) only removes
/// trees, and raising it only adds them; the others stay exactly where they
/// were. Trees stand on a lattice fixed to the World, so an area holds the
/// same trees, with the same addresses, as any larger area around it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeScatterPlacement {
    instances: Vec<TreeCopy>,
}

/// A tree spot that passed the density, exclusion and ground checks,
/// waiting for the spacing check.
struct Site {
    position: Fixed2,
    rank: Rank,
    spacing: i64,
    instance: TreeCopy,
}

impl Spaced for Site {
    fn position(&self) -> Fixed2 {
        self.position
    }

    fn rank(&self) -> Rank {
        self.rank
    }
}

impl TreeScatterPlacement {
    /// Places `scatter` over `input`'s area, standing each tree on its
    /// ground: the trees whose lattice cell's corner lies in the area, so a
    /// tree may stand up to 0.5 m outside it.
    ///
    /// Positions are in metres from the area's centre. The input's mask thins
    /// the Scatter in places (`None` is full density everywhere), its seed
    /// sets where trees stand (the Scatter's own
    /// [`default_seed`](TreeScatter::default_seed) is not used here), its
    /// prefix starts every tree's address, and no trunk overlaps its
    /// exclusions. Trees are left out where the ground at the trunk is
    /// steeper than their species' [`max_slope`](ScatterTree::max_slope), or
    /// the Scatter's [`max_slope`](TreeScatterVariation::max_slope) when the
    /// species sets none, or where any ground around the trunk is missing
    /// (NaN).
    ///
    /// # Errors
    ///
    /// Returns [`TreeScatterError::Options`] if either side of the area is not
    /// between 0 (exclusive) and [`MAX_SCATTER_SIZE`], any [`TreeScatterError`] from
    /// [`TreeScatter::validate`], and [`TreeScatterError::TooMany`] if the grove's
    /// density over the area asks for more than [`MAX_SCATTER_COPIES`]
    /// trees.
    pub fn new(scatter: &TreeScatter, input: &ScatterInput) -> Result<Self, TreeScatterError> {
        // Check the settings and the number of trees asked for.
        let size = input.area.size.to_array();
        for side in size {
            check(
                side > 0.0 && side <= MAX_SCATTER_SIZE,
                "size",
                "above 0 and at most 2048",
            )?;
        }
        scatter.validate()?;
        let per_m2: Vec<f32> = scatter
            .trees
            .iter()
            .map(|tree| tree.density() * scatter.density_scale / HECTARE)
            .collect();
        let total: f32 = per_m2.iter().sum();
        let count = (total * size[0] * size[1]).ceil() as usize;
        if count > MAX_SCATTER_COPIES {
            return Err(TreeScatterError::TooMany { count });
        }
        if total <= 0.0 {
            return Ok(Self::default());
        }

        // Look at every candidate spot near enough to decide the area's own.
        // Spots sit on a lattice fixed to the World, so a spot keeps its
        // place, rank and address however the area or density changes.
        let lattice = Lattice::new(SITE_CELL);
        let reach = scatter.reach();
        let own = input.area.bounds();
        let seeds = Seeds::new(input.seed);
        let chance = lattice::chance(f64::from(total) * f64::from(SITE_CELL * SITE_CELL));
        let mut sites = Vec::new();
        for cell in lattice.cells_in(reach.window(own)) {
            let h = lattice.hash(seeds.lattice, cell);
            let position = lattice.position(h, cell);

            // Keep the spot below a threshold that grows with the local
            // density, so a lower density keeps a subset of a higher one's
            // spots.
            let local = input.local_density(position);
            let rank = lattice::rank(h);
            if !lattice::dense_enough(rank, chance, local) {
                continue;
            }
            let world = position.to_world();
            let tree = pick_tree(scatter, &seeds, &per_m2, total, h, world);
            let Some(instance) = stand(scatter, tree, h, cell, position, input) else {
                continue;
            };
            sites.push(Site {
                position,
                rank: Rank {
                    tier: 0,
                    rank,
                    local,
                    cell,
                    index: 0,
                },
                spacing: lattice::to_fixed(f64::from(scatter.trees[tree].spacing)),
                instance,
            });
        }

        // Space the trees out, keep the area's own, and clear the glades
        // last, so a tree in a glade still keeps its neighbours at a
        // distance and raising the patchiness only removes trees.
        let keep = keep_spaced(&sites, reach.rounds, reach.spacing_fixed(), |a, b| {
            let apart = i128::from(a.spacing.max(b.spacing));
            a.position.distance_squared(b.position) < apart * apart
        });
        let mut kept: Vec<Site> = sites
            .into_iter()
            .zip(keep)
            .filter(|(site, keep)| {
                *keep
                    && own.contains(lattice.corner(site.rank.cell))
                    && !in_glade(scatter, &seeds, site.position.to_world())
            })
            .map(|(site, _)| site)
            .collect();
        // List them in rank order, so a lower density keeps a prefix's order.
        kept.sort_by_key(|site| site.rank);
        Ok(Self {
            instances: kept.into_iter().map(|site| site.instance).collect(),
        })
    }

    /// The placed trees, from the lowest rank (the trees a lower density
    /// keeps) to the highest, the same order every time for the same inputs.
    pub fn instances(&self) -> &[TreeCopy] {
        &self.instances
    }
}

impl TreeScatter {
    /// The position in [`Scatter::slots`] of species `tree`'s shape variant
    /// `variant`.
    ///
    /// # Panics
    ///
    /// Panics if `tree` is out of range.
    pub fn slot(&self, tree: usize, variant: u32) -> u32 {
        self.trees[..tree].iter().map(|t| t.variants).sum::<u32>() + variant
    }

    /// The largest size factor any tree can be scaled by.
    fn largest_scale(&self) -> f32 {
        tree_scale(1.0, self.variation.scale_jitter)
    }
}

/// A tree Scatter places its trees on a 0.5 m lattice fixed to the World.
/// Its slots are each species' shape variants, in species order, seeded by
/// each tree's own [`default_seed`](TreeOptions::default_seed) whatever the
/// World seed (see [`ScatterTree::variant`]).
impl Scatter for TreeScatter {
    fn lattice(&self) -> Lattice {
        Lattice::new(SITE_CELL)
    }

    fn reach(&self) -> Reach {
        let spacing = self.trees.iter().map(|t| t.spacing).fold(0.0, f32::max);
        let trunk = self
            .trees
            .iter()
            .map(|t| t.tree.branch.radius[0])
            .fold(0.0, f32::max);
        Reach {
            rounds: DEFAULT_SPACING_ROUNDS,
            spacing,
            displacement: SITE_CELL,
            footprint: (trunk * self.largest_scale()).max(MIN_FOOTPRINT),
        }
    }

    fn draw_range(&self) -> f32 {
        DRAW_RANGE
    }

    fn slots(&self, _world_seed: u32) -> Vec<ScatterSlot> {
        self.trees
            .iter()
            .enumerate()
            .flat_map(|(entry, tree)| {
                (0..tree.variants).map(move |v| ScatterSlot {
                    entry: entry as u16,
                    specimen: tree.tree.specimen_id(tree.tree.default_seed as u32, v),
                })
            })
            .collect()
    }

    fn place(&self, input: &ScatterInput<'_>) -> Result<Vec<ScatterCopy>, ScatterError> {
        let placed = TreeScatterPlacement::new(self, input)?;
        Ok(placed
            .instances
            .iter()
            .map(|tree| ScatterCopy {
                address: tree.address,
                slot: self.slot(tree.tree, tree.variant),
                transform: tree.transform,
                footprint: vec![tree.footprint],
            })
            .collect())
    }
}

/// The seeds of a tree Scatter's random choices, from its placement seed.
struct Seeds {
    /// Candidate spots: where each stands, its rank and its own draws.
    lattice: u32,
    /// Each species' stands, salted by the species' key.
    patches: u32,
    /// The glades.
    glades: u32,
}

impl Seeds {
    fn new(seed: u32) -> Self {
        Self {
            lattice: seed_for(seed, LATTICE_SEED),
            patches: seed_for(seed, PATCHES_SEED),
            glades: seed_for(seed, GLADES_SEED),
        }
    }
}

/// Picks the species for the spot hashed to `h` at World position `world`,
/// in list order: each species takes its share of what the species before
/// it left, biased toward its own patches by its clustering. Each species'
/// draws are salted by its key.
fn pick_tree(
    scatter: &TreeScatter,
    seeds: &Seeds,
    per_m2: &[f32],
    total: f32,
    h: u32,
    world: DVec2,
) -> usize {
    let mut left = total;
    let mut last = 0;
    for (index, (tree, &d)) in scatter.trees.iter().zip(per_m2).enumerate() {
        if d <= 0.0 {
            continue;
        }
        last = index;
        let chance = d / left;
        left -= d;
        // A clustered species decides some spots by its patch value instead
        // of an independent draw. Patch values are spread evenly from 0 to
        // 1, so either way the species keeps its share.
        let salt = tree.key.salt();
        let u = if lattice::unit(h, SALT_CLUSTER ^ salt) < tree.clustering {
            let patch = f64::from((tree.spacing * 6.0).max(4.0));
            patch_value(pcg2(seeds.patches, salt), world / patch)
        } else {
            lattice::unit(h, SALT_SPECIES ^ salt)
        };
        if u < chance {
            return index;
        }
    }
    last
}

/// Stands species `tree` on the spot of `cell`, hashed to `h`, at World
/// position `at`: picks its variant, size, lean and turn, and sets it on the
/// ground. Returns `None` where the trunk overlaps a footprint the input
/// keeps clear, where the ground at the trunk is too steep, or where any
/// ground around it is missing.
fn stand(
    scatter: &TreeScatter,
    tree: usize,
    h: u32,
    cell: CellId,
    at: Fixed2,
    input: &ScatterInput,
) -> Option<TreeCopy> {
    let species = &scatter.trees[tree];
    let variation = &scatter.variation;
    let world = at.to_world();

    // Keep the trunk off the footprints above it.
    let scale = tree_scale(lattice::unit(h, SALT_SCALE), variation.scale_jitter);
    let radius = (species.tree.branch.radius[0] * scale).max(MIN_FOOTPRINT);
    let trunk = Disc::new(world, radius);
    if input.excluded(&[trunk]) {
        return None;
    }

    // Reject steep ground next, from one sample at the trunk.
    let max_slope = species.max_slope.unwrap_or(variation.max_slope);
    if steeper_than(&input.sample_at(at), max_slope) {
        return None;
    }

    // Stand the trunk on the lowest ground under it and eight points round
    // its edge, sunk by the Scatter's sink.
    let variant = ((lattice::unit(h, SALT_VARIANT) * species.variants as f32) as u32)
        .min(species.variants - 1);
    let footprint = Footprint {
        contacts: FOOTING_DIRECTIONS
            .iter()
            .map(|&(dx, dz)| Vec3::new(dx * radius, 0.0, dz * radius))
            .collect(),
        extra: Vec::new(),
        size: 2.0 * radius,
    };
    let placement = Placement {
        embed: variation.sink / footprint.size,
        ..species.tree.ground_placement()
    };
    // The yaw turns the tree below; the footing is the same at any turn.
    let placed = place(input.ground, world, 0.0, &footprint, &placement)?;

    // Size, lean and turn the tree.
    let local = at.relative_to(input.area.centre);
    let position = Vec3::new(local.x, placed.height, local.y);
    let yaw = lattice::unit(h, SALT_YAW) * std::f32::consts::TAU;
    let lean_direction = lattice::unit(h, SALT_LEAN_DIRECTION) * std::f32::consts::TAU;
    let lean = (lattice::unit(h, SALT_LEAN) * variation.lean).to_radians();
    let axis = Vec3::new(lean_direction.cos(), 0.0, lean_direction.sin());
    let rotation = Quat::from_axis_angle(axis, lean) * Quat::from_rotation_y(yaw);
    Some(TreeCopy {
        tree,
        variant,
        address: input.prefix.copy(cell, 0),
        position,
        footprint: trunk,
        transform: Affine3A::from_scale_rotation_translation(
            Vec3::splat(scale),
            rotation,
            position,
        ),
    })
}

/// The size factor of a tree whose size draw is `u`, from 0 to 1, under a
/// size variation of `jitter`.
///
/// Draws below a half shrink the tree a little, and draws above it grow the
/// tree a little, until the last few percent, which grow it into a giant.
/// The factor rises with `u`, and at 0 jitter it is always 1.
fn tree_scale(u: f32, jitter: f32) -> f32 {
    let v = 2.0 * u - 1.0;
    let tail = v.max(0.0).powi(GIANT_RARITY);
    1.0 + jitter * (BODY_SPREAD * v + GIANT_GROWTH * tail)
}

/// Whether World position `world` lies in one of `scatter`'s glades, where no
/// tree grows.
///
/// The ground is split into patches about [`GLADE_CELL`] metres across,
/// each with its own value from 0 to 1; patches whose value falls below the
/// open share for the Scatter's patchiness are glades. Raising the
/// patchiness only turns more patches into glades.
fn in_glade(scatter: &TreeScatter, seeds: &Seeds, world: DVec2) -> bool {
    if scatter.patchiness <= 0.0 {
        return false;
    }
    patch_value(seeds.glades, world / f64::from(GLADE_CELL)) < scatter.patchiness * MAX_OPEN
}

/// The value, from 0 to 1, of the patch that `p` falls in, in units of the
/// patch size from the World origin. Patches are irregular areas about one
/// unit across, and every point of a patch has the same value.
///
/// Each patch has its own random value, independent of its size, so over a
/// large area the values are spread evenly from 0 to 1.
fn patch_value(seed: u32, p: DVec2) -> f32 {
    // Each unit cell holds one patch centre at a random spot; a point
    // belongs to the nearest centre.
    let (i, j) = (p.x.floor() as i32, p.y.floor() as i32);
    let mut nearest = (f64::MAX, 0);
    for ci in i - 1..=i + 1 {
        for cj in j - 1..=j + 1 {
            let h = pcg2(seed, pcg2(ci as u32, cj as u32));
            let cx = f64::from(ci) + f64::from(lattice::unit(h, SALT_X));
            let cz = f64::from(cj) + f64::from(lattice::unit(h, SALT_Z));
            let d = (cx - p.x).powi(2) + (cz - p.y).powi(2);
            if d < nearest.0 {
                nearest = (d, h);
            }
        }
    }
    lattice::unit(nearest.1, SALT_RANK)
}

/// Succeeds when `ok` is true; otherwise names the grove setting `field` and
/// what it must be.
fn check(ok: bool, field: &str, requirement: &'static str) -> Result<(), TreeScatterError> {
    if ok {
        Ok(())
    } else {
        Err(TreeScatterError::Options(OptionsError::InvalidSetting {
            field: field.to_string(),
            requirement,
        }))
    }
}

/// Accepts zero and positive finite values.
fn non_negative(value: f32, field: &str) -> Result<(), TreeScatterError> {
    check(
        (0.0..f32::INFINITY).contains(&value),
        field,
        "a finite value of 0 or more",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_values_are_even_and_shared_by_neighbours() {
        // Sample one point per patch-sized step: the values spread evenly.
        let values: Vec<f32> = (0..2000)
            .map(|n| patch_value(7, DVec2::new((n % 50) as f64 * 1.7, (n / 50) as f64 * 1.7)))
            .collect();
        let below = values.iter().filter(|v| **v < 0.2).count() as f32;
        assert!((below / values.len() as f32 - 0.2).abs() < 0.04);
        // Close points almost always share a patch.
        let same = (0..1000)
            .filter(|n| {
                let x = *n as f64 * 0.37;
                patch_value(7, DVec2::new(x, 0.5)) == patch_value(7, DVec2::new(x + 0.01, 0.5))
            })
            .count();
        assert!(same > 950);
    }

    #[test]
    fn size_variation_keeps_most_trees_near_normal_with_rare_giants() {
        let draws: Vec<f32> = (0..10_000).map(|n| n as f32 / 10_000.0).collect();
        assert!(draws.iter().all(|u| tree_scale(*u, 0.0) == 1.0));

        // Sizes rise with the draw and stay between 0.55 and 3.7 times normal.
        let sizes: Vec<f32> = draws.iter().map(|u| tree_scale(*u, 0.9)).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(sizes[0] >= 0.55 - 1e-4 && sizes[sizes.len() - 1] <= 3.7 + 1e-4);

        // Half the trees stay within 25% of normal, and about one in
        // thirteen grows to more than twice its normal size.
        let share = |test: &dyn Fn(f32) -> bool| {
            sizes.iter().filter(|s| test(**s)).count() as f32 / sizes.len() as f32
        };
        assert!(share(&|s| (s - 1.0).abs() < 0.25) > 0.5);
        let giants = share(&|s| s > 2.0);
        assert!(
            (0.06..0.09).contains(&giants),
            "{giants} of the trees are giants"
        );
    }

    /// The sizes of the connected areas of a 400 m square, sampled every 2 m,
    /// that are glade (when `open`) or wooded, largest first.
    fn areas(scatter: &TreeScatter, open: bool) -> Vec<usize> {
        const N: usize = 200;
        let seeds = Seeds::new(scatter.default_seed);
        let at = |i: usize, j: usize| {
            let p = DVec2::new(i as f64 * 2.0 - 200.0, j as f64 * 2.0 - 200.0);
            in_glade(scatter, &seeds, p) == open
        };
        let mut seen = vec![false; N * N];
        let mut sizes = Vec::new();
        for start in 0..N * N {
            if seen[start] || !at(start % N, start / N) {
                continue;
            }
            // Flood out from `start` through matching neighbours.
            seen[start] = true;
            let mut stack = vec![start];
            let mut size = 0;
            while let Some(cell) = stack.pop() {
                size += 1;
                let (i, j) = (cell % N, cell / N);
                let neighbours = [
                    (i.wrapping_sub(1), j),
                    (i + 1, j),
                    (i, j.wrapping_sub(1)),
                    (i, j + 1),
                ];
                for (ni, nj) in neighbours {
                    if ni < N && nj < N && !seen[nj * N + ni] && at(ni, nj) {
                        seen[nj * N + ni] = true;
                        stack.push(nj * N + ni);
                    }
                }
            }
            sizes.push(size);
        }
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        sizes
    }

    #[test]
    fn medium_patchiness_opens_separate_glades_and_high_joins_them() {
        let mut scatter = TreeScatterPreset::oak_woodland().tree_scatter.clone();
        let total = |sizes: &[usize]| sizes.iter().sum::<usize>() as f32;

        // No patchiness, no glades.
        assert!(areas(&scatter, true).is_empty());

        // Medium: about a third of the ground is open, in many glades, and
        // the woods around them stay joined.
        scatter.patchiness = 0.5;
        let (glades, woods) = (areas(&scatter, true), areas(&scatter, false));
        let open = total(&glades) / (total(&glades) + total(&woods));
        assert!((0.27..0.43).contains(&open), "{open} of the ground is open");
        assert!(glades.len() > 20, "{} glades", glades.len());
        assert!(glades[0] as f32 / total(&glades) < 0.3);
        assert!(woods[0] as f32 / total(&woods) > 0.9);

        // High: the glades join into one, leaving islands of woods.
        scatter.patchiness = 1.0;
        let (glades, woods) = (areas(&scatter, true), areas(&scatter, false));
        assert!(glades[0] as f32 / total(&glades) > 0.85);
        assert!(woods.len() > 10, "{} islands", woods.len());
        assert!(woods[0] as f32 / total(&woods) < 0.5);
    }
}
