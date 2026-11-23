//! Groves: several tree species spread over a rectangle of ground.
//!
//! A [`TreeGrove`] says which trees grow (a list of [`GroveTree`]s, each with
//! its own density, spacing and clustering) and how the placed trees vary.
//! [`GrovePlacement::new`] lays a grove out over ground of a given size and
//! returns one [`GroveInstance`] per tree: which species and which of its
//! shape variants to draw, and where.
//!
//! Each species is drawn from a few shape variants (the same settings with
//! different seeds), so a renderer builds a handful of tree meshes and draws
//! each many times.

mod file;
mod presets;

use std::collections::HashMap;
use std::fmt;

use glam::{Affine3A, Quat, Vec3};
use procgena_mesh::Ground;

use crate::options::{OptionsError, TreeOptions};
use crate::presets::TreePreset;

pub use file::GroveFileError;
pub use presets::GrovePreset;

/// Most species one grove may hold.
pub const MAX_GROVE_TREES: usize = 16;
/// Most shape variants one species may use.
pub const MAX_GROVE_VARIANTS: u32 = 8;
/// Most trees one placement may ask for, before spacing removes any.
pub const MAX_GROVE_INSTANCES: usize = 20_000;
/// Longest side of the ground a grove may be placed on, in metres.
pub const MAX_GROVE_SIZE: f32 = 2048.0;

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

// Fixed inputs that give each random choice its own sequence.
const SALT_X: u64 = 0x1;
const SALT_Z: u64 = 0x2;
const SALT_RANK: u64 = 0x3;
const SALT_VARIANT: u64 = 0x4;
const SALT_YAW: u64 = 0x5;
const SALT_LEAN_DIRECTION: u64 = 0x6;
const SALT_LEAN: u64 = 0x7;
const SALT_SCALE: u64 = 0x8;
const SALT_SPECIES: u64 = 0x100;
const SALT_PATCHES: u64 = 0x200;
const SALT_CLUSTER: u64 = 0x400;
const SALT_VARIANT_SEED: u64 = 0x300;
const SALT_GLADES: u64 = 0x500;

/// One species in a grove: what the tree looks like, and how many grow and
/// how close together.
///
/// [`GroveTree::new`] starts from a [`TreePreset`] with a density and spacing
/// that suit it. Every field is public, so a species can also be built or
/// changed directly.
#[derive(Debug, Clone, PartialEq)]
pub struct GroveTree {
    /// The tree's shape. Its [`seed`](TreeOptions::seed) is the seed of the
    /// species' first variant; the other variants use seeds derived from it.
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
    /// [`MAX_GROVE_VARIANTS`]. More variants look less repetitive but cost a
    /// mesh each.
    pub variants: u32,
    /// Steepest ground this species grows on, in degrees from level, from 0
    /// to 90. `None` uses the grove's
    /// [`max_slope`](GroveVariation::max_slope). Set it higher for shrubs and
    /// saplings, which cling to slopes large trees can't.
    pub max_slope: Option<f32>,
}

impl GroveTree {
    /// A species from a preset, at the preset's own grove density, spacing
    /// and slope tolerance, evenly mixed, with 4 shape variants.
    pub fn new(preset: TreePreset) -> Self {
        let mut tree = Self::custom(
            preset.options(),
            preset.grove_density(),
            preset.grove_spacing(),
        );
        tree.max_slope = preset.grove_max_slope();
        tree
    }

    /// A species with your own tree settings, `density` in trees per hectare
    /// and `spacing` in metres, evenly mixed, with 4 shape variants, on the
    /// grove's slope limit.
    pub fn custom(tree: TreeOptions, density: f32, spacing: f32) -> Self {
        Self {
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
    /// Variant 0 is [`tree`](Self::tree) itself. The others differ only in
    /// their seed, which depends on the tree's own seed and the variant
    /// number, so changing the grove's seed moves trees without changing
    /// their shapes.
    pub fn variant(&self, variant: u32) -> TreeOptions {
        let mut options = self.tree.clone();
        if variant > 0 {
            let offset = mix(SALT_VARIANT_SEED ^ u64::from(variant)) as i32;
            options.seed = options.seed.wrapping_add(offset);
        }
        options
    }

    fn validate(&self, index: usize) -> Result<(), GroveError> {
        let field = |name: &str| format!("trees[{index}].{name}");
        non_negative(self.density, &field("density"))?;
        non_negative(self.spacing, &field("spacing"))?;
        non_negative(self.density_scale, &field("densityScale"))?;
        check(
            (0.0..=1.0).contains(&self.clustering),
            &field("clustering"),
            "between 0 and 1",
        )?;
        check(
            (1..=MAX_GROVE_VARIANTS).contains(&self.variants),
            &field("variants"),
            "between 1 and 8",
        )?;
        check(
            self.max_slope.is_none_or(|s| (0.0..=90.0).contains(&s)),
            &field("maxSlope"),
            "between 0 and 90",
        )?;
        self.tree
            .validate()
            .map_err(|error| GroveError::Tree { index, error })
    }
}

/// How the trees of a grove differ from one another, beyond their species
/// and shape variant. These settings move, turn and resize trees; they never
/// change a tree's mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroveVariation {
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
    /// species sets its own [`max_slope`](GroveTree::max_slope). From 0 to
    /// 90; 90 allows any slope. The slope is measured across the trunk's
    /// footprint.
    pub max_slope: f32,
    /// How far each tree is sunk below the lowest ground around its trunk, in
    /// metres, so its roots never show above a slope. Must be 0 or more.
    pub sink: f32,
}

impl Default for GroveVariation {
    fn default() -> Self {
        Self {
            scale_jitter: 0.15,
            lean: 3.0,
            max_slope: 35.0,
            sink: 0.1,
        }
    }
}

impl GroveVariation {
    fn validate(&self) -> Result<(), GroveError> {
        check(
            (0.0..=0.9).contains(&self.scale_jitter),
            "variation.scaleJitter",
            "between 0 and 0.9",
        )?;
        check(
            (0.0..=45.0).contains(&self.lean),
            "variation.lean",
            "between 0 and 45",
        )?;
        check(
            (0.0..=90.0).contains(&self.max_slope),
            "variation.maxSlope",
            "between 0 and 90",
        )?;
        non_negative(self.sink, "variation.sink")
    }
}

/// A mix of tree species and how they spread over ground: the tree
/// counterpart of a plant field.
///
/// A grove holds its species itself, and says nothing about where it grows:
/// pass the size and ground to [`GrovePlacement::new`]. Start from a
/// [`GrovePreset`] with [`TreeGrove::from_preset`], or list species with
/// [`TreeGrove::new`].
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
/// - **Spacing thins the grove.** A tree too close to a neighbour is left
///   out, so a dense grove with wide spacing grows fewer trees than its
///   density asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeGrove {
    /// Picks where trees stand, which species each one is and how it is
    /// turned and sized. Changing it moves every tree but keeps the shape
    /// variants the same.
    pub seed: u32,
    /// The species that grow here, at most [`MAX_GROVE_TREES`].
    pub trees: Vec<GroveTree>,
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
    pub variation: GroveVariation,
}

impl Default for TreeGrove {
    /// The [`GrovePreset::OakWoodland`] grove.
    fn default() -> Self {
        GrovePreset::OakWoodland.grove()
    }
}

impl TreeGrove {
    /// A grove of these species, with seed 0, density scale 1, no glades
    /// and the default [`GroveVariation`].
    pub fn new(trees: Vec<GroveTree>) -> Self {
        Self {
            seed: 0,
            trees,
            density_scale: 1.0,
            patchiness: 0.0,
            variation: GroveVariation::default(),
        }
    }

    /// A preset's grove. Same as [`GrovePreset::grove`].
    pub fn from_preset(preset: GrovePreset) -> Self {
        preset.grove()
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
    pub fn with_trees(mut self, trees: Vec<GroveTree>) -> Self {
        self.trees = trees;
        self
    }

    /// Checks every setting, including each species' tree settings.
    ///
    /// # Errors
    ///
    /// Returns a [`GroveError`] naming the first problem found.
    pub fn validate(&self) -> Result<(), GroveError> {
        if self.trees.len() > MAX_GROVE_TREES {
            return Err(GroveError::TooManyTrees {
                count: self.trees.len(),
            });
        }
        non_negative(self.density_scale, "densityScale")?;
        check(
            (0.0..=1.0).contains(&self.patchiness),
            "patchiness",
            "between 0 and 1",
        )?;
        self.variation.validate()?;
        for (index, tree) in self.trees.iter().enumerate() {
            tree.validate(index)?;
        }
        Ok(())
    }

    fn unscaled_density(&self) -> f32 {
        self.trees.iter().map(GroveTree::density).sum()
    }
}

/// Why a grove could not be placed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GroveError {
    /// A grove setting, or the ground size, is out of range.
    Options(OptionsError),
    /// The tree settings of species `index` are invalid.
    Tree {
        /// The species' position in [`TreeGrove::trees`].
        index: usize,
        /// What is wrong with its settings.
        error: OptionsError,
    },
    /// The grove lists more than [`MAX_GROVE_TREES`] species.
    TooManyTrees {
        /// The number of species listed.
        count: usize,
    },
    /// The density and size would place more than [`MAX_GROVE_INSTANCES`]
    /// trees.
    TooMany {
        /// The number of trees the settings ask for.
        count: usize,
    },
}

impl fmt::Display for GroveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options(error) => write!(f, "invalid grove settings: {error}"),
            Self::Tree { index, error } => write!(f, "invalid tree in trees[{index}]: {error}"),
            Self::TooManyTrees { count } => write!(
                f,
                "the grove lists {count} species, but at most {MAX_GROVE_TREES} are allowed"
            ),
            Self::TooMany { count } => write!(
                f,
                "the grove asks for {count} trees, but at most {MAX_GROVE_INSTANCES} are allowed"
            ),
        }
    }
}

impl std::error::Error for GroveError {}

impl From<OptionsError> for GroveError {
    fn from(error: OptionsError) -> Self {
        Self::Options(error)
    }
}

/// One placed tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroveInstance {
    /// The tree's species: its position in [`TreeGrove::trees`].
    pub tree: usize,
    /// Which of the species' shape variants to draw; see
    /// [`GroveTree::variant`].
    pub variant: u32,
    /// Names the spot the tree stands on. A tree at the same spot keeps its
    /// id when the grove changes, so a renderer can update trees in place
    /// instead of rebuilding them.
    pub id: u64,
    /// The foot of the trunk: X and Z, with the height it stands at in Y, in
    /// metres.
    pub position: Vec3,
    /// Moves the tree's meshes into place: sized, leaned, turned and stood on
    /// the ground. Apply it to every vertex of the variant's mesh, at any
    /// level of detail.
    pub transform: Affine3A,
}

/// Trees spread over a rectangle of ground by a [`TreeGrove`].
///
/// Create one with [`GrovePlacement::new`], then draw each of
/// [`instances`](Self::instances) with the mesh of its species' variant.
///
/// The same grove, size and ground always give the same trees. Lowering the
/// grove's [`density_scale`](TreeGrove::density_scale) only removes trees,
/// and raising it only adds them; the others stay exactly where they were.
/// Changing the size keeps the trees away from the edges where they were; a
/// tree near an old edge may make way for a new neighbour beyond it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrovePlacement {
    instances: Vec<GroveInstance>,
}

/// A tree site that passed the density and ground checks, waiting for the
/// spacing check.
struct Site {
    rank: f32,
    spacing: f32,
    instance: GroveInstance,
}

impl GrovePlacement {
    /// Places `grove` on a `size` rectangle (width in X, depth in Z, in
    /// metres) centred on the origin, standing each tree on `ground`.
    ///
    /// `density` thins the grove in places: it takes X and Z and returns from
    /// 0 (no trees) to 1 (the grove's full density); `None` is full
    /// everywhere. Trees are left out where any ground around their trunk is
    /// missing (NaN) or steeper than their species'
    /// [`max_slope`](GroveTree::max_slope), or the grove's
    /// [`max_slope`](GroveVariation::max_slope) when the species sets none.
    ///
    /// # Errors
    ///
    /// Returns [`GroveError::Options`] if `size` is not between 0 (exclusive)
    /// and [`MAX_GROVE_SIZE`], any [`GroveError`] from
    /// [`TreeGrove::validate`], and [`GroveError::TooMany`] if the grove's
    /// density over this size asks for more than [`MAX_GROVE_INSTANCES`]
    /// trees.
    pub fn new(
        grove: &TreeGrove,
        size: [f32; 2],
        ground: &dyn Ground,
        density: Option<&dyn Fn(f32, f32) -> f32>,
    ) -> Result<Self, GroveError> {
        // Check the settings and the number of trees asked for.
        for side in size {
            check(
                side > 0.0 && side <= MAX_GROVE_SIZE,
                "size",
                "above 0 and at most 2048",
            )?;
        }
        grove.validate()?;
        let per_m2: Vec<f32> = grove
            .trees
            .iter()
            .map(|tree| tree.density() * grove.density_scale / HECTARE)
            .collect();
        let total: f32 = per_m2.iter().sum();
        let count = (total * size[0] * size[1]).ceil() as usize;
        if count > MAX_GROVE_INSTANCES {
            return Err(GroveError::TooMany { count });
        }
        if total <= 0.0 {
            return Ok(Self::default());
        }

        // Visit every candidate site. Sites sit on a lattice fixed to the
        // origin, so a site keeps its place, rank and id however the size or
        // density changes.
        let half = [size[0] * 0.5, size[1] * 0.5];
        let cells = |h: f32| {
            (
                (-h / SITE_CELL).floor() as i32,
                (h / SITE_CELL).ceil() as i32,
            )
        };
        let (x0, x1) = cells(half[0]);
        let (z0, z1) = cells(half[1]);
        let mut sites = Vec::new();
        for i in x0..x1 {
            for j in z0..z1 {
                let id = site_id(i, j);
                let h = mix(u64::from(grove.seed) ^ mix(id));
                let x = (i as f32 + draw(h, SALT_X)) * SITE_CELL;
                let z = (j as f32 + draw(h, SALT_Z)) * SITE_CELL;
                if x.abs() > half[0] || z.abs() > half[1] {
                    continue;
                }

                // Keep the site with a chance equal to the local density per
                // site. Each site has a fixed rank, scaled up where the
                // density function thins the grove, and is kept below a
                // threshold that grows with density, so a lower density keeps
                // a subset of a higher one's sites.
                let local = density.map_or(1.0, |f| f(x, z).clamp(0.0, 1.0));
                if local <= 0.0 {
                    continue;
                }
                let rank = draw(h, SALT_RANK) / local;
                if rank >= total * SITE_CELL * SITE_CELL {
                    continue;
                }
                let tree = pick_tree(grove, &per_m2, total, h, x, z);
                if let Some(instance) = stand(grove, tree, id, h, x, z, ground) {
                    sites.push(Site {
                        rank,
                        spacing: grove.trees[tree].spacing,
                        instance,
                    });
                }
            }
        }

        // Keep trees in rank order, leaving out any too close to a tree
        // already kept. Rank order means the sites a lower density keeps
        // are visited first and decided the same way.
        sites.sort_by(|a, b| {
            a.rank
                .total_cmp(&b.rank)
                .then(a.instance.id.cmp(&b.instance.id))
        });
        let reach = grove
            .trees
            .iter()
            .map(|tree| tree.spacing)
            .fold(0.0, f32::max);
        let mut kept: Vec<GroveInstance> = Vec::new();
        let mut spacings: Vec<f32> = Vec::new();
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for site in sites {
            let p = site.instance.position;
            if reach > 0.0 {
                let key = |v: f32| (v / reach).floor() as i32;
                let (bx, bz) = (key(p.x), key(p.z));
                let crowded = (bx - 1..=bx + 1)
                    .flat_map(|cx| (bz - 1..=bz + 1).map(move |cz| (cx, cz)))
                    .filter_map(|cell| buckets.get(&cell))
                    .flatten()
                    .any(|&k| {
                        let q = kept[k].position;
                        let apart = site.spacing.max(spacings[k]);
                        (p.x - q.x).powi(2) + (p.z - q.z).powi(2) < apart * apart
                    });
                if crowded {
                    continue;
                }
                buckets.entry((bx, bz)).or_default().push(kept.len());
            }
            kept.push(site.instance);
            spacings.push(site.spacing);
        }

        // Clear the glades last, so a tree in a glade still keeps its
        // neighbours at a distance and raising the patchiness only removes
        // trees.
        kept.retain(|tree| !in_glade(grove, tree.position.x, tree.position.z));
        Ok(Self { instances: kept })
    }

    /// The placed trees, in no particular order, but the same order every
    /// time for the same inputs.
    pub fn instances(&self) -> &[GroveInstance] {
        &self.instances
    }
}

/// Picks the species for the site at `x`, `z`, in list order: each species
/// takes its share of what the species before it left, biased toward its own
/// patches by its clustering.
fn pick_tree(grove: &TreeGrove, per_m2: &[f32], total: f32, h: u64, x: f32, z: f32) -> usize {
    let mut left = total;
    let mut last = 0;
    for (index, (tree, &d)) in grove.trees.iter().zip(per_m2).enumerate() {
        if d <= 0.0 {
            continue;
        }
        last = index;
        let chance = d / left;
        left -= d;
        // A clustered species decides some sites by its patch value instead
        // of an independent draw. Patch values are spread evenly from 0 to
        // 1, so either way the species keeps its share.
        let salt = index as u64;
        let u = if draw(h, SALT_CLUSTER + salt) < tree.clustering {
            let patch = (tree.spacing * 6.0).max(4.0);
            let seed = mix(u64::from(grove.seed) ^ (SALT_PATCHES + salt));
            patch_value(seed, x / patch, z / patch)
        } else {
            draw(h, SALT_SPECIES + salt)
        };
        if u < chance {
            return index;
        }
    }
    last
}

/// Stands species `tree` at `x`, `z`: picks its variant, size, lean and turn,
/// and sets it on the ground. Returns `None` where the ground around the
/// trunk is missing or too steep.
fn stand(
    grove: &TreeGrove,
    tree: usize,
    id: u64,
    h: u64,
    x: f32,
    z: f32,
    ground: &dyn Ground,
) -> Option<GroveInstance> {
    let species = &grove.trees[tree];
    let variation = &grove.variation;
    let variant = (draw(h, SALT_VARIANT) * species.variants as f32) as u32;
    let scale = tree_scale(draw(h, SALT_SCALE), variation.scale_jitter);

    // Sample the ground at the trunk and at eight points around its edge.
    let footprint = (species.tree.branch.radius[0] * scale).max(MIN_FOOTPRINT);
    let centre = ground.height(x, z);
    let mut lowest = centre;
    let mut steepest: f32 = 0.0;
    for (dx, dz) in FOOTING_DIRECTIONS {
        let y = ground.height(x + dx * footprint, z + dz * footprint);
        if y.is_nan() {
            return None;
        }
        lowest = lowest.min(y);
        steepest = steepest.max((y - centre).abs());
    }
    let max_slope = species.max_slope.unwrap_or(variation.max_slope);
    if centre.is_nan() || (steepest / footprint).atan().to_degrees() > max_slope {
        return None;
    }

    // Size, lean and turn the tree, and stand it on the lowest sample.
    let position = Vec3::new(x, lowest - variation.sink, z);
    let yaw = draw(h, SALT_YAW) * std::f32::consts::TAU;
    let lean_direction = draw(h, SALT_LEAN_DIRECTION) * std::f32::consts::TAU;
    let lean = (draw(h, SALT_LEAN) * variation.lean).to_radians();
    let axis = Vec3::new(lean_direction.cos(), 0.0, lean_direction.sin());
    let rotation = Quat::from_axis_angle(axis, lean) * Quat::from_rotation_y(yaw);
    Some(GroveInstance {
        tree,
        variant,
        id,
        position,
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

/// Whether `x`, `z` lies in one of `grove`'s glades, where no tree grows.
///
/// The ground is split into patches about [`GLADE_CELL`] metres across,
/// each with its own value from 0 to 1; patches whose value falls below the
/// open share for the grove's patchiness are glades. Raising the patchiness
/// only turns more patches into glades.
fn in_glade(grove: &TreeGrove, x: f32, z: f32) -> bool {
    if grove.patchiness <= 0.0 {
        return false;
    }
    let seed = mix(u64::from(grove.seed) ^ SALT_GLADES);
    patch_value(seed, x / GLADE_CELL, z / GLADE_CELL) < grove.patchiness * MAX_OPEN
}

/// A site's lasting id, from its lattice cell.
fn site_id(i: i32, j: i32) -> u64 {
    (u64::from(i as u32) << 32) | u64::from(j as u32)
}

/// Scrambles the bits of `x` (the SplitMix64 finaliser).
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A number from 0 (inclusive) to 1 (exclusive) for the choice `salt` of the
/// site hashed to `h`.
fn draw(h: u64, salt: u64) -> f32 {
    (mix(h ^ mix(salt)) >> 40) as f32 / (1u64 << 24) as f32
}

/// The value, from 0 to 1, of the patch that `x`, `z` falls in. Patches are
/// irregular areas about one unit across, and every point of a patch has the
/// same value.
///
/// Each patch has its own random value, independent of its size, so over a
/// large area the values are spread evenly from 0 to 1.
fn patch_value(seed: u64, x: f32, z: f32) -> f32 {
    // Each unit cell holds one patch centre at a random spot; a point
    // belongs to the nearest centre.
    let (i, j) = (x.floor() as i32, z.floor() as i32);
    let mut nearest = (f32::MAX, 0);
    for ci in i - 1..=i + 1 {
        for cj in j - 1..=j + 1 {
            let h = mix(seed ^ mix(site_id(ci, cj)));
            let cx = ci as f32 + draw(h, SALT_X);
            let cz = cj as f32 + draw(h, SALT_Z);
            let d = (cx - x).powi(2) + (cz - z).powi(2);
            if d < nearest.0 {
                nearest = (d, h);
            }
        }
    }
    draw(nearest.1, SALT_RANK)
}

/// Succeeds when `ok` is true; otherwise names the grove setting `field` and
/// what it must be.
fn check(ok: bool, field: &str, requirement: &'static str) -> Result<(), GroveError> {
    if ok {
        Ok(())
    } else {
        Err(GroveError::Options(OptionsError::InvalidSetting {
            field: field.to_string(),
            requirement,
        }))
    }
}

/// Accepts zero and positive finite values.
fn non_negative(value: f32, field: &str) -> Result<(), GroveError> {
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
    fn draws_cover_the_unit_interval() {
        let draws: Vec<f32> = (0..1000).map(|i| draw(mix(i), SALT_RANK)).collect();
        assert!(draws.iter().all(|d| (0.0..1.0).contains(d)));
        let mean = draws.iter().sum::<f32>() / draws.len() as f32;
        assert!((mean - 0.5).abs() < 0.05);
    }

    #[test]
    fn patch_values_are_even_and_shared_by_neighbours() {
        // Sample one point per patch-sized step: the values spread evenly.
        let values: Vec<f32> = (0..2000)
            .map(|n| patch_value(7, (n % 50) as f32 * 1.7, (n / 50) as f32 * 1.7))
            .collect();
        let below = values.iter().filter(|v| **v < 0.2).count() as f32;
        assert!((below / values.len() as f32 - 0.2).abs() < 0.04);
        // Close points almost always share a patch.
        let same = (0..1000)
            .filter(|n| {
                let x = *n as f32 * 0.37;
                patch_value(7, x, 0.5) == patch_value(7, x + 0.01, 0.5)
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
    fn areas(grove: &TreeGrove, open: bool) -> Vec<usize> {
        const N: usize = 200;
        let at = |i: usize, j: usize| {
            in_glade(grove, i as f32 * 2.0 - 200.0, j as f32 * 2.0 - 200.0) == open
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
        let mut grove = GrovePreset::OakWoodland.grove();
        let total = |sizes: &[usize]| sizes.iter().sum::<usize>() as f32;

        // No patchiness, no glades.
        assert!(areas(&grove, true).is_empty());

        // Medium: about a third of the ground is open, in many glades, and
        // the woods around them stay joined.
        grove.patchiness = 0.5;
        let (glades, woods) = (areas(&grove, true), areas(&grove, false));
        let open = total(&glades) / (total(&glades) + total(&woods));
        assert!((0.27..0.43).contains(&open), "{open} of the ground is open");
        assert!(glades.len() > 20, "{} glades", glades.len());
        assert!(glades[0] as f32 / total(&glades) < 0.3);
        assert!(woods[0] as f32 / total(&woods) > 0.9);

        // High: the glades join into one, leaving islands of woods.
        grove.patchiness = 1.0;
        let (glades, woods) = (areas(&grove, true), areas(&grove, false));
        assert!(glades[0] as f32 / total(&glades) > 0.85);
        assert!(woods.len() > 10, "{} islands", woods.len());
        assert!(woods[0] as f32 / total(&woods) < 0.5);
    }

    #[test]
    fn site_ids_differ_for_negative_cells() {
        assert_ne!(site_id(-1, 0), site_id(0, -1));
        assert_ne!(site_id(-1, -1), site_id(1, 1));
    }
}
