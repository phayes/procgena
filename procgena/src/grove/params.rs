//! Controls for a grove's shared settings and each species' role in the mix.
//!
//! Tree shape controls are shared with the Trees tab.

use procgena_tree::{GroveTree, GroveVariation, MAX_GROVE_VARIANTS, TreeGrove};

use crate::params::{Group, Param, Section, count, number};

/// Largest grove seed the Seed control reaches.
const MAX_SEED: u32 = 999;

/// The grove's own settings, in display order. Its species are edited
/// separately, one row each.
pub fn sections() -> Vec<Section<TreeGrove>> {
    vec![
        Section {
            title: "Grove",
            groups: vec![grove()],
        },
        Section {
            title: "Variation",
            groups: vec![variation().lift(
                "variation",
                |g: &TreeGrove| &g.variation,
                |g| &mut g.variation,
            )],
        },
    ]
}

/// Controls how densely one species grows, how far apart its trunks stand,
/// whether it gathers into stands, and how many shapes it is drawn from.
///
/// The density scale multiplies the species' natural trees per hectare.
/// Tree shape and natural density are edited separately.
pub fn share_sections() -> Vec<Section<GroveTree>> {
    vec![Section {
        title: "Share",
        groups: vec![Group::plain(vec![
            number!(
                GroveTree,
                "densityScale",
                "Density ×",
                (0.001, 2.0, 0.001),
                |t| t.density_scale
            )
            .exponential(),
            number!(
                GroveTree,
                "clustering",
                "Clustering",
                (0.0, 1.0, 0.01),
                |t| t.clustering
            ),
            number!(GroveTree, "spacing", "Spacing", (0.0, 10.0, 0.05), |t| t
                .spacing),
            count!(
                GroveTree,
                "variants",
                "Variants",
                (1, MAX_GROVE_VARIANTS),
                |t| t.variants
            ),
            // The species' own slope limit. A species without one shows the
            // default grove limit, and editing it gives the species its own.
            Param::number(
                "maxSlope",
                "Max Slope",
                (0.0, 90.0, 0.5),
                |t: &GroveTree| t.max_slope.unwrap_or(GroveVariation::default().max_slope),
                |t: &mut GroveTree, value| t.max_slope = Some(value),
            ),
        ])],
    }]
}

fn grove() -> Group<TreeGrove> {
    Group::plain(vec![
        count!(TreeGrove, "seed", "Seed", (0, MAX_SEED), |g| g.seed),
        // Multiply every species' density without changing the mix.
        number!(
            TreeGrove,
            "densityScale",
            "Grove Density ×",
            (0.0, 2.0, 0.01),
            |g| g.density_scale
        ),
        // Open treeless glades: separate ones at medium values, joined ones
        // high up, leaving islands of trees.
        number!(
            TreeGrove,
            "patchiness",
            "Patchiness",
            (0.0, 1.0, 0.01),
            |g| g.patchiness
        ),
    ])
}

fn variation() -> Group<GroveVariation> {
    Group::plain(vec![
        number!(
            GroveVariation,
            "scaleJitter",
            "Size Variation",
            (0.0, 0.9, 0.01),
            |v| v.scale_jitter
        ),
        number!(GroveVariation, "lean", "Lean", (0.0, 45.0, 0.5), |v| v.lean),
        number!(
            GroveVariation,
            "maxSlope",
            "Max Slope",
            (0.0, 90.0, 1.0),
            |v| v.max_slope
        ),
        number!(GroveVariation, "sink", "Sink", (0.0, 1.0, 0.01), |v| v.sink),
    ])
}

#[cfg(test)]
mod tests {
    use procgena_tree::GrovePreset;

    use super::*;
    use crate::params::{Param, checks};

    /// Checks a grove the way the grove tab's viewer will.
    fn accepted(grove: TreeGrove) -> Result<(), String> {
        grove.validate().map_err(|error| error.to_string())
    }

    fn grove_params() -> Vec<Param<TreeGrove>> {
        params_of(sections())
    }

    fn share_params() -> Vec<Param<GroveTree>> {
        params_of(share_sections())
    }

    fn params_of<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
        sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Each grove preset, named so failures identify it.
    fn groves() -> Vec<(String, TreeGrove)> {
        GrovePreset::ALL
            .iter()
            .map(|preset| (preset.key().to_string(), preset.grove()))
            .collect()
    }

    /// Each species of each grove preset, named by its place.
    fn shares() -> Vec<(String, GroveTree)> {
        GrovePreset::ALL
            .iter()
            .flat_map(|preset| {
                preset
                    .grove()
                    .trees
                    .into_iter()
                    .enumerate()
                    .map(|(i, tree)| (format!("{}.trees[{i}]", preset.key()), tree))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn keys_are_unique() {
        checks::keys_are_unique(&grove_params());
        checks::keys_are_unique(&share_params());
    }

    #[test]
    fn every_range_end_is_valid() {
        checks::range_ends_are_valid(&grove_params(), &groves(), accepted);

        // A species' share is checked in a grove holding only that species.
        let alone = |tree: GroveTree| accepted(TreeGrove::new(vec![tree]));
        checks::range_ends_are_valid(&share_params(), &shares(), alone);
    }

    #[test]
    fn presets_fit_the_controls() {
        checks::cases_fit_the_controls(&grove_params(), &groves());
        checks::cases_fit_the_controls(&share_params(), &shares());
    }
}
