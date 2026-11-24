//! Controls for an Ecotope's view and its Scatters' placement settings.
//!
//! The assets a Scatter spreads (a tree, a stone, a piece of dead wood or a
//! mushroom) are edited with the controls of their own tabs.

use procgena_biome::EcotopeScatter;
use procgena_deadwood::{MAX_DECAY, MAX_VARIANTS, ScatterOptions as DeadwoodScatterOptions};
use procgena_mushroom::{MAX_SPECIMENS, ScatterOptions as MushroomScatterOptions};
use procgena_plant_bevy::PlantScatterOptions;
use procgena_stone::{MAX_CLUSTER_MEMBERS, MAX_STONE_VARIANTS, StoneClass};

use super::EcotopeView;
use crate::params::{Group, Section, choice, count, number};
use crate::scene::EcotopeGround;

/// Highest priority the Priority control reaches.
pub const MAX_PRIORITY: u32 = 1000;

/// The ground the Ecotope is shown on, and how much of it.
pub fn view_sections() -> Vec<Section<EcotopeView>> {
    vec![Section {
        title: "Ground",
        groups: vec![Group::plain(vec![
            choice!(
                EcotopeView,
                "ground",
                "Ground",
                [
                    "Rolling Hills" => EcotopeGround::RollingHills,
                    "Flat" => EcotopeGround::Flat,
                ],
                |v| v.ground
            ),
            number!(EcotopeView, "size", "Patch Size", (10.0, 200.0, 1.0), |v| v
                .size)
            .exponential(),
        ])],
    }]
}

/// A Scatter's place in the Ecotope's order, and the room it keeps around
/// its copies.
pub fn scatter_sections() -> Vec<Section<EcotopeScatter>> {
    vec![Section {
        title: "Order",
        groups: vec![Group::plain(vec![
            count!(
                EcotopeScatter,
                "priority",
                "Priority",
                (0, MAX_PRIORITY),
                |s| s.priority
            ),
            number!(
                EcotopeScatter,
                "clearance",
                "Clearance",
                (0.0, 5.0, 0.05),
                |s| s.clearance
            ),
        ])],
    }]
}

/// Where and how densely one class of a stone Scatter lies. Its stone's
/// shape is edited separately.
pub fn stone_class_sections() -> Vec<Section<StoneClass>> {
    vec![Section {
        title: "Placement",
        groups: vec![
            Group::plain(vec![
                number!(
                    StoneClass,
                    "density",
                    "Density /m²",
                    (0.0, 0.2, 0.0005),
                    |c| c.density
                )
                .exponential(),
                number!(StoneClass, "spacing", "Spacing", (0.1, 20.0, 0.05), |c| c
                    .spacing)
                .exponential(),
                count!(
                    StoneClass,
                    "variants",
                    "Variants",
                    (1, MAX_STONE_VARIANTS),
                    |c| c.variants
                ),
                number!(StoneClass, "size.0", "Size Min", (0.05, 5.0, 0.01), |c| c
                    .size[0])
                .exponential(),
                number!(StoneClass, "size.1", "Size Max", (0.05, 5.0, 0.01), |c| c
                    .size[1])
                .exponential(),
                number!(
                    StoneClass,
                    "max_slope",
                    "Max Slope",
                    (0.0, 90.0, 0.5),
                    |c| c.max_slope
                ),
            ]),
            Group::titled(
                "Clusters",
                vec![
                    count!(
                        StoneClass,
                        "members.0",
                        "Members Min",
                        (1, MAX_CLUSTER_MEMBERS),
                        |c| c.members[0]
                    ),
                    count!(
                        StoneClass,
                        "members.1",
                        "Members Max",
                        (1, MAX_CLUSTER_MEMBERS),
                        |c| c.members[1]
                    ),
                    number!(
                        StoneClass,
                        "cluster_radius",
                        "Cluster Radius",
                        (0.0, 5.0, 0.05),
                        |c| c.cluster_radius
                    ),
                ],
            ),
            Group::titled(
                "Patches",
                vec![
                    number!(
                        StoneClass,
                        "patchiness",
                        "Patchiness",
                        (0.0, 0.95, 0.01),
                        |c| c.patchiness
                    ),
                    number!(
                        StoneClass,
                        "patch_size",
                        "Patch Size",
                        (0.5, 100.0, 0.5),
                        |c| c.patch_size
                    )
                    .exponential(),
                ],
            ),
        ],
    }]
}

/// Where and how densely a dead wood Scatter's pieces lie, and how they
/// vary. The piece they start from is edited separately.
pub fn deadwood_sections() -> Vec<Section<DeadwoodScatterOptions>> {
    vec![Section {
        title: "Placement",
        groups: vec![
            Group::plain(vec![
                number!(
                    DeadwoodScatterOptions,
                    "density",
                    "Density /m²",
                    (0.001, 2.0, 0.001),
                    |o| o.density
                )
                .exponential(),
                number!(
                    DeadwoodScatterOptions,
                    "min_spacing",
                    "Spacing",
                    (0.0, 20.0, 0.05),
                    |o| o.min_spacing
                ),
                number!(
                    DeadwoodScatterOptions,
                    "max_slope",
                    "Max Slope",
                    (0.0, 90.0, 0.5),
                    |o| o.max_slope
                ),
                count!(
                    DeadwoodScatterOptions,
                    "variants",
                    "Variants",
                    (1, MAX_VARIANTS),
                    |o| o.variants
                ),
            ]),
            Group::titled(
                "Variation",
                vec![
                    number!(
                        DeadwoodScatterOptions,
                        "diameter.0",
                        "Diameter Min",
                        (0.005, 1.5, 0.005),
                        |o| o.diameter[0]
                    )
                    .exponential(),
                    number!(
                        DeadwoodScatterOptions,
                        "diameter.1",
                        "Diameter Max",
                        (0.005, 1.5, 0.005),
                        |o| o.diameter[1]
                    )
                    .exponential(),
                    number!(
                        DeadwoodScatterOptions,
                        "length.0",
                        "Length Min",
                        (0.05, 15.0, 0.05),
                        |o| o.length[0]
                    )
                    .exponential(),
                    number!(
                        DeadwoodScatterOptions,
                        "length.1",
                        "Length Max",
                        (0.05, 15.0, 0.05),
                        |o| o.length[1]
                    )
                    .exponential(),
                    number!(
                        DeadwoodScatterOptions,
                        "decay.0",
                        "Decay Min",
                        (0.0, MAX_DECAY, 0.05),
                        |o| o.decay[0]
                    ),
                    number!(
                        DeadwoodScatterOptions,
                        "decay.1",
                        "Decay Max",
                        (0.0, MAX_DECAY, 0.05),
                        |o| o.decay[1]
                    ),
                ],
            ),
        ],
    }]
}

/// Where and how densely a mushroom Scatter grows. Its species is edited
/// separately.
/// A plant Scatter's settings: the Plant Scatter tab's, and the steepest
/// ground its plants grow on.
pub fn plant_sections() -> Vec<Section<PlantScatterOptions>> {
    let mut sections = crate::field::params::sections();
    sections.push(Section {
        title: "Placement",
        groups: vec![Group::plain(vec![number!(
            PlantScatterOptions,
            "max_slope",
            "Max Slope",
            (0.0, 90.0, 0.5),
            |o| o.max_slope
        )])],
    });
    sections
}

pub fn mushroom_sections() -> Vec<Section<MushroomScatterOptions>> {
    vec![Section {
        title: "Placement",
        groups: vec![Group::plain(vec![
            number!(
                MushroomScatterOptions,
                "density",
                "Density /m²",
                (0.0, 5.0, 0.001),
                |o| o.density
            )
            .exponential(),
            number!(
                MushroomScatterOptions,
                "min_spacing",
                "Spacing",
                (0.0, 10.0, 0.05),
                |o| o.min_spacing
            ),
            number!(
                MushroomScatterOptions,
                "max_slope",
                "Max Slope",
                (0.0, 90.0, 0.5),
                |o| o.max_slope
            ),
            number!(
                MushroomScatterOptions,
                "scale_min",
                "Size Min",
                (0.1, 3.0, 0.01),
                |o| o.scale_min
            ),
            number!(
                MushroomScatterOptions,
                "scale_max",
                "Size Max",
                (0.1, 3.0, 0.01),
                |o| o.scale_max
            ),
            count!(
                MushroomScatterOptions,
                "specimens",
                "Specimens",
                (1, MAX_SPECIMENS.min(16)),
                |o| o.specimens
            ),
        ])],
    }]
}

#[cfg(test)]
mod tests {
    use procgena_biome::{EcotopePreset, ScatterKind};

    use super::*;
    use crate::params::Param;
    use crate::params::checks;

    /// Every setting of `sections`, in order.
    fn params<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
        sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Each built-in Ecotope's Scatters, named by preset and key.
    fn scatters() -> Vec<(String, EcotopeScatter)> {
        EcotopePreset::all()
            .flat_map(|preset| {
                preset
                    .ecotope
                    .scatters
                    .iter()
                    .map(move |(key, scatter)| (format!("{}.{key}", preset.key), scatter.clone()))
            })
            .collect()
    }

    /// The settings of each built-in Scatter of the kind `pick` takes.
    fn cases<T>(pick: impl Fn(&ScatterKind) -> Vec<T>) -> Vec<(String, T)> {
        scatters()
            .into_iter()
            .flat_map(|(name, scatter)| {
                pick(&scatter.scatter)
                    .into_iter()
                    .enumerate()
                    .map(move |(i, value)| (format!("{name}.{i}"), value))
            })
            .collect()
    }

    fn stone_classes() -> Vec<(String, StoneClass)> {
        cases(|kind| match kind {
            ScatterKind::Stone(options) => options.classes.clone(),
            _ => Vec::new(),
        })
    }

    fn deadwood() -> Vec<(String, DeadwoodScatterOptions)> {
        cases(|kind| match kind {
            ScatterKind::Deadwood(options) => vec![options.clone()],
            _ => Vec::new(),
        })
    }

    fn mushrooms() -> Vec<(String, MushroomScatterOptions)> {
        cases(|kind| match kind {
            ScatterKind::Mushroom { scatter, .. } => vec![scatter.clone()],
            _ => Vec::new(),
        })
    }

    #[test]
    fn every_setting_has_its_own_key() {
        checks::keys_are_unique(&params(view_sections()));
        checks::keys_are_unique(&params(scatter_sections()));
        checks::keys_are_unique(&params(stone_class_sections()));
        checks::keys_are_unique(&params(deadwood_sections()));
        checks::keys_are_unique(&params(mushroom_sections()));
    }

    #[test]
    fn the_built_in_scatters_fit_the_controls() {
        assert!(!stone_classes().is_empty() && !deadwood().is_empty() && !mushrooms().is_empty());
        checks::cases_fit_the_controls(&params(scatter_sections()), &scatters());
        checks::cases_fit_the_controls(&params(stone_class_sections()), &stone_classes());
        checks::cases_fit_the_controls(&params(deadwood_sections()), &deadwood());
        checks::cases_fit_the_controls(&params(mushroom_sections()), &mushrooms());
    }
}
