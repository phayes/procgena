//! Compose a grove from tree species and adjust how its trees vary.
//!
//! The Tree Grove tab starts from a grove preset. Each row controls one
//! species: its tree, how densely it grows and whether it gathers into stands.
//! Each species has a natural density in trees per hectare; the row's
//! multiplier and the grove's multiplier both scale it.

pub mod editor;
pub mod params;

use std::sync::LazyLock;

use procgena_tree::{
    FlatGround, GrovePlacement, GrovePreset, GroveTree, TreeGrove, TreeOptions, TreePreset,
};

/// The grove preset shown when the app starts.
pub const START_PRESET: GrovePreset = GrovePreset::OakWoodland;

/// Smallest and largest width of the grove's square of ground in the 3D
/// view, in metres.
pub const SIZE_RANGE: (f32, f32) = (10.0, 100.0);

/// Width of the square of flat ground the rows' trees per hectare are
/// counted on, in metres. It is fixed, so the counts don't change with the
/// size of the view.
const SAMPLE_SIZE: f32 = 100.0;

/// Picker label for a tree whose settings match no built-in preset.
pub const CUSTOM: &str = "Custom";

/// Square metres in a hectare.
const HECTARE: f32 = 10_000.0;

/// The species added by `+ Add Tree`: bushes at 10% of their natural density.
///
/// Its share of the grove depends on the densities of the other species.
pub fn added_tree() -> GroveTree {
    GroveTree::new(TreePreset::Bush2).scaled(0.1)
}

/// What a species row shows about its place in the whole grove.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RowStatus {
    /// Trees per hectare of this species. When `placed` is true this counts
    /// the trees actually placed, after spacing has removed some; otherwise
    /// it is the density the settings ask for.
    pub per_ha: f32,
    /// The species' share of the grove's trees, from 0 to 1.
    pub share: f32,
    /// Whether `per_ha` and `share` count placed trees.
    pub placed: bool,
    /// Whether this species fills the gaps the others leave, which makes its
    /// clustering have no effect. This is the last species with a density
    /// above 0.
    pub takes_rest: bool,
    /// Whether the row may be removed; the last remaining row may not.
    pub removable: bool,
}

/// Works out each species row's status for `grove`, in the grove's species
/// order.
///
/// Counts the trees a placement on 100 metres of flat ground actually
/// grows, after spacing and glades remove some. If the grove cannot be
/// placed, for example because it asks for too many trees, the rows show
/// the densities the settings ask for instead.
pub fn row_statuses(grove: &TreeGrove) -> Vec<RowStatus> {
    let count = grove.trees.len();
    let rest = grove.trees.iter().rposition(|tree| tree.density() > 0.0);
    let size = SAMPLE_SIZE;
    let placed = GrovePlacement::new(grove, [size, size], &FlatGround, None).ok();

    // Trees per species: placed ones when the placement worked, otherwise
    // the density the settings ask for.
    let per_ha: Vec<f32> = match &placed {
        Some(placement) => {
            let area_ha = size * size / HECTARE;
            let mut counts = vec![0usize; count];
            for instance in placement.instances() {
                counts[instance.tree] += 1;
            }
            counts.iter().map(|n| *n as f32 / area_ha).collect()
        }
        None => grove
            .trees
            .iter()
            .map(|tree| tree.density() * grove.density_scale.max(0.0))
            .collect(),
    };
    let total: f32 = per_ha.iter().sum();
    per_ha
        .iter()
        .enumerate()
        .map(|(index, per_ha)| RowStatus {
            per_ha: *per_ha,
            share: if total > 0.0 { per_ha / total } else { 0.0 },
            placed: placed.is_some(),
            takes_rest: rest == Some(index),
            removable: count > 1,
        })
        .collect()
}

/// Reads a grove from the text of a `.grove.json` file. Returns a readable
/// error if the text is not a grove or has a setting out of range.
pub fn read_grove(text: &str) -> Result<TreeGrove, String> {
    let grove = TreeGrove::from_json(text).map_err(|error| format!("Not a grove: {error}"))?;
    grove
        .validate()
        .map_err(|error| format!("Not a usable grove: {error}"))?;
    if grove.trees.is_empty() {
        return Err("Not a usable grove: it has no trees".to_string());
    }
    Ok(grove)
}

/// Trees per hectare as a row shows them, rounded to a whole number:
/// `320/ha`.
pub fn per_ha_text(per_ha: f32) -> String {
    format!("{per_ha:.0}/ha")
}

/// Finds the picker option whose preset exactly matches `tree`'s settings.
/// Returns the trailing [`CUSTOM`] option when no preset matches.
pub fn choice_of(tree: &GroveTree) -> usize {
    preset_options()
        .iter()
        .position(|options| *options == tree.tree)
        .unwrap_or(TreePreset::ALL.len())
}

/// The settings of every tree preset, in [`TreePreset::ALL`] order, read
/// once on first use.
fn preset_options() -> &'static [TreeOptions] {
    static OPTIONS: LazyLock<Vec<TreeOptions>> =
        LazyLock::new(|| TreePreset::ALL.iter().map(|p| p.options()).collect());
    &OPTIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_planted_row_takes_the_rest_and_lone_rows_stay() {
        let mut grove = GrovePreset::OakWoodland.grove();
        grove
            .trees
            .push(GroveTree::new(TreePreset::Apple).scaled(0.0));
        let rows = row_statuses(&grove);
        let rest: Vec<bool> = rows.iter().map(|row| row.takes_rest).collect();
        assert_eq!(rest, [false, false, false, true, false]);
        assert!(rows.iter().all(|row| row.removable && row.placed));
        assert_eq!(rows[4].per_ha, 0.0);

        let alone = TreeGrove::new(vec![GroveTree::new(TreePreset::OakMedium)]);
        let rows = row_statuses(&alone);
        assert!(!rows[0].removable);
        assert_eq!(rows[0].share, 1.0);
    }

    #[test]
    fn rows_count_the_trees_spacing_and_glades_leave() {
        let mut grove = GrovePreset::PineStand.grove();
        let rows = row_statuses(&grove);
        let asked: f32 = grove.density();
        let placed: f32 = rows.iter().map(|row| row.per_ha).sum();
        assert!(rows.iter().all(|row| row.placed));
        assert!(
            placed < asked && placed > 0.6 * asked,
            "placed {placed}/ha of {asked}/ha"
        );
        let shares: f32 = rows.iter().map(|row| row.share).sum();
        assert!((shares - 1.0).abs() < 1e-4);

        // Glades take their trees away from the count.
        grove.patchiness = 1.0;
        let open: f32 = row_statuses(&grove).iter().map(|row| row.per_ha).sum();
        assert!(
            open < 0.6 * placed,
            "{open}/ha with glades, {placed}/ha without"
        );
    }

    #[test]
    fn rows_show_asked_densities_when_the_grove_cannot_be_placed() {
        let grove = GrovePreset::AspenStand.grove().with_density(20_000.0);
        let rows = row_statuses(&grove);
        assert!(rows.iter().all(|row| !row.placed));
        let asked: f32 = rows.iter().map(|row| row.per_ha).sum();
        assert!((asked - 20_000.0).abs() < 1.0, "{asked}");
    }

    #[test]
    fn trees_per_hectare_read_at_a_glance() {
        assert_eq!(per_ha_text(319.6), "320/ha");
        assert_eq!(per_ha_text(0.0), "0/ha");
    }

    #[test]
    fn a_tree_is_shown_as_its_preset_or_as_custom() {
        let pine = TreePreset::ALL
            .iter()
            .position(|p| *p == TreePreset::PineLarge)
            .unwrap();
        let mut tree = GroveTree::new(TreePreset::PineLarge);
        assert_eq!(choice_of(&tree), pine);
        tree.tree.seed += 1;
        assert_eq!(choice_of(&tree), TreePreset::ALL.len());
        assert!(TreePreset::ALL.iter().all(|p| p.name() != CUSTOM));
    }

    #[test]
    fn groves_are_read_from_files_and_checked() {
        let grove = GrovePreset::CedarGrove.grove();
        assert_eq!(read_grove(&grove.to_json()), Ok(grove));
        assert!(read_grove("not a grove").is_err());
        assert!(read_grove(r#"{ "densityScale": -1, "base": "orchard" }"#).is_err());
        assert!(read_grove("{}").is_err(), "a grove with no trees");
    }
}
