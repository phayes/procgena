//! Controls for a tree Scatter's shared settings and each species' role in
//! the mix.
//!
//! Tree shape controls are shared with the Trees tab.

use procgena_tree::{MAX_SCATTER_VARIANTS, ScatterTree, TreeScatter, TreeScatterVariation};

use crate::params::{Group, Param, Section, count, number};

/// Largest Scatter seed the Seed control reaches.
const MAX_SEED: u32 = 999;

/// The Scatter's own settings, in display order. Its species are edited
/// separately, one row each.
pub fn sections() -> Vec<Section<TreeScatter>> {
    vec![
        Section {
            title: "Scatter",
            groups: vec![grove()],
        },
        Section {
            title: "Variation",
            groups: vec![variation().lift(
                "variation",
                |g: &TreeScatter| &g.variation,
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
pub fn share_sections() -> Vec<Section<ScatterTree>> {
    vec![Section {
        title: "Share",
        groups: vec![Group::plain(vec![
            number!(
                ScatterTree,
                "density_scale",
                "Density ×",
                (0.001, 2.0, 0.001),
                |t| t.density_scale
            )
            .exponential(),
            number!(
                ScatterTree,
                "clustering",
                "Clustering",
                (0.0, 1.0, 0.01),
                |t| t.clustering
            ),
            number!(ScatterTree, "spacing", "Spacing", (0.0, 10.0, 0.05), |t| t
                .spacing),
            count!(
                ScatterTree,
                "variants",
                "Variants",
                (1, MAX_SCATTER_VARIANTS),
                |t| t.variants
            ),
            // The species' own slope limit. A species without one shows the
            // default Scatter limit, and editing it gives the species its own.
            Param::number(
                "max_slope",
                "Max Slope",
                (0.0, 90.0, 0.5),
                |t: &ScatterTree| {
                    t.max_slope
                        .unwrap_or(TreeScatterVariation::default().max_slope)
                },
                |t: &mut ScatterTree, value| t.max_slope = Some(value),
            ),
        ])],
    }]
}

fn grove() -> Group<TreeScatter> {
    Group::plain(vec![
        count!(TreeScatter, "default_seed", "Seed", (0, MAX_SEED), |g| g
            .default_seed),
        // Multiply every species' density without changing the mix.
        number!(
            TreeScatter,
            "density_scale",
            "Scatter Density ×",
            (0.0, 2.0, 0.01),
            |g| g.density_scale
        ),
        // Open treeless glades: separate ones at medium values, joined ones
        // high up, leaving islands of trees.
        number!(
            TreeScatter,
            "patchiness",
            "Patchiness",
            (0.0, 1.0, 0.01),
            |g| g.patchiness
        ),
    ])
}

fn variation() -> Group<TreeScatterVariation> {
    Group::plain(vec![
        number!(
            TreeScatterVariation,
            "scale_jitter",
            "Size Variation",
            (0.0, 0.9, 0.01),
            |v| v.scale_jitter
        ),
        number!(
            TreeScatterVariation,
            "lean",
            "Lean",
            (0.0, 45.0, 0.5),
            |v| v.lean
        ),
        number!(
            TreeScatterVariation,
            "max_slope",
            "Max Slope",
            (0.0, 90.0, 1.0),
            |v| v.max_slope
        ),
        number!(
            TreeScatterVariation,
            "sink",
            "Sink",
            (0.0, 1.0, 0.01),
            |v| v.sink
        ),
    ])
}

#[cfg(test)]
mod tests {
    use procgena_tree::TreeScatterPreset;

    use super::*;
    use crate::params::{Param, checks};

    /// Checks a Scatter the way the Tree Scatter tab's viewer will.
    fn accepted(grove: TreeScatter) -> Result<(), String> {
        grove.validate().map_err(|error| error.to_string())
    }

    fn grove_params() -> Vec<Param<TreeScatter>> {
        params_of(sections())
    }

    fn share_params() -> Vec<Param<ScatterTree>> {
        params_of(share_sections())
    }

    fn params_of<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
        sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Each tree Scatter preset, named so failures identify it.
    fn groves() -> Vec<(String, TreeScatter)> {
        TreeScatterPreset::all()
            .map(|preset| (preset.key.to_string(), preset.tree_scatter.clone()))
            .collect()
    }

    /// Each species of each tree Scatter preset, named by its place.
    fn shares() -> Vec<(String, ScatterTree)> {
        TreeScatterPreset::all()
            .flat_map(|preset| {
                preset
                    .tree_scatter
                    .trees
                    .iter()
                    .enumerate()
                    .map(|(i, tree)| (format!("{}.trees[{i}]", preset.key), tree.clone()))
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

        // A species' share is checked in a Scatter holding only that species.
        let alone = |tree: ScatterTree| accepted(TreeScatter::new(vec![tree]));
        checks::range_ends_are_valid(&share_params(), &shares(), alone);
    }

    #[test]
    fn presets_fit_the_controls() {
        checks::cases_fit_the_controls(&grove_params(), &groves());
        checks::cases_fit_the_controls(&share_params(), &shares());
    }
}
