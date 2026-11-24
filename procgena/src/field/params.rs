//! Controls for a Plant Scatter's shared appearance and each plant's role in
//! the mix.
//!
//! Plant shape and colour controls are shared with the Plant tab.

use procgena_plant::prelude::{GroundTint, PlantScatter, PlantVariation, ScatterPlant};

use crate::params::{Group, Section, count, number, srgb};

/// The Scatter's own settings, in display order. Its plants are edited
/// separately, one row each.
pub fn sections() -> Vec<Section<PlantScatter>> {
    vec![
        Section {
            title: "Variation",
            groups: vec![variation().lift(
                "variation",
                |f: &PlantScatter| &f.variation,
                |f| &mut f.variation,
            )],
        },
        Section {
            title: "Scatter",
            groups: vec![
                field(),
                ground_tint().lift(
                    "ground_tint",
                    |f: &PlantScatter| &f.ground_tint,
                    |f| &mut f.ground_tint,
                ),
            ],
        },
    ]
}

/// Controls how densely one plant grows and whether it forms patches.
///
/// The density scale multiplies the plant's natural plants per square metre.
/// Plant shape, colours and natural density are edited separately.
pub fn share_sections() -> Vec<Section<ScatterPlant>> {
    vec![Section {
        title: "Share",
        groups: vec![Group::plain(vec![
            number!(
                ScatterPlant,
                "density_scale",
                "Density ×",
                (0.001, 2.0, 0.001),
                |e| e.density_scale
            )
            .exponential(),
            number!(
                ScatterPlant,
                "clustering",
                "Clustering",
                (0.0, 1.0, 0.01),
                |e| e.clustering
            ),
        ])],
    }]
}

fn field() -> Group<PlantScatter> {
    Group::plain(vec![
        // Multiply all entry densities together without changing the plant proportions.
        number!(
            PlantScatter,
            "density_scale",
            "Scatter Density ×",
            (0.0, 2.0, 0.01),
            |f| f.density_scale
        ),
        number!(
            PlantScatter,
            "wind_scale",
            "Wind Scale",
            (0.0, 2.0, 0.01),
            |f| f.wind_scale
        ),
        count!(PlantScatter, "toon_bands", "Toon Bands", (0, 8), |f| f
            .toon_bands),
    ])
}

fn variation() -> Group<PlantVariation> {
    Group::plain(vec![
        number!(
            PlantVariation,
            "patch_amount",
            "Patches",
            (0.0, 1.0, 0.01),
            |v| v.patch_amount
        ),
        srgb!(PlantVariation, "patch_color", "Patch Colour", |v| v
            .patch_color)
        .shown_if(|v| v.patch_amount > 0.0),
        number!(
            PlantVariation,
            "patch_size",
            "Patch Size",
            (0.5, 20.0, 0.1),
            |v| v.patch_size
        )
        .shown_if(|v| v.patch_amount > 0.0)
        .exponential(),
        number!(
            PlantVariation,
            "dry_tips",
            "Dry Tips",
            (0.0, 1.0, 0.01),
            |v| v.dry_tips
        ),
        number!(
            PlantVariation,
            "dead_share",
            "Dead Share",
            (0.0, 1.0, 0.01),
            |v| v.dead_share
        ),
        number!(
            PlantVariation,
            "bare_share",
            "Bare Ground",
            (0.0, 1.0, 0.01),
            |v| v.bare_share
        ),
        number!(
            PlantVariation,
            "bare_patch_size",
            "Bare Patch Size",
            (0.5, 10.0, 0.1),
            |v| v.bare_patch_size
        )
        .shown_if(|v| v.bare_share > 0.0)
        .exponential(),
    ])
}

fn ground_tint() -> Group<GroundTint> {
    Group::plain(vec![
        number!(
            GroundTint,
            "amount",
            "Ground Tint Amount",
            (0.0, 1.0, 0.01),
            |g| g.amount
        ),
        srgb!(GroundTint, "color", "Ground Tint Colour", |g| g.color).shown_if(|g| g.amount > 0.0),
    ])
}

#[cfg(test)]
mod tests {
    use procgena_plant::prelude::PlantScatterPreset;

    use super::*;
    use crate::params::{Param, checks};

    fn field_params() -> Vec<Param<PlantScatter>> {
        params_of(sections())
    }

    fn share_params() -> Vec<Param<ScatterPlant>> {
        params_of(share_sections())
    }

    fn params_of<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
        sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Each Plant Scatter preset, named so test failures identify it.
    fn fields() -> Vec<(String, PlantScatter)> {
        PlantScatterPreset::all()
            .map(|preset| (preset.key.to_string(), preset.plant_scatter.clone()))
            .collect()
    }

    /// The presets, plus a Scatter that hides every setting that can be hidden.
    fn varied_fields() -> Vec<(String, PlantScatter)> {
        let mut cases = fields();
        let mut even = PlantScatterPreset::MEADOW.plant_scatter;
        even.variation.patch_amount = 0.0;
        even.variation.bare_share = 0.0;
        even.ground_tint.amount = 0.0;
        cases.push(("even field".into(), even));
        cases
    }

    /// Each plant of each Plant Scatter preset, named by its place.
    fn shares() -> Vec<(String, ScatterPlant)> {
        PlantScatterPreset::all()
            .flat_map(|preset| {
                preset
                    .plant_scatter
                    .plants
                    .iter()
                    .enumerate()
                    .map(|(i, entry)| (format!("{}.plants[{i}]", preset.key), *entry))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Checks `field` the way a `.plant_scatter.yml` file is checked.
    fn file_accepts(field: PlantScatter) -> Result<(), String> {
        PlantScatter::from_yaml(&field.to_yaml())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn keys_are_unique() {
        checks::keys_are_unique(&field_params());
        checks::keys_are_unique(&share_params());
    }

    #[test]
    fn every_setting_writes_the_field_named_by_its_key() {
        checks::settings_write_their_keys(&field_params(), &varied_fields());
        checks::settings_write_their_keys(&share_params(), &shares());
    }

    #[test]
    fn every_range_end_is_valid() {
        checks::range_ends_are_valid(&field_params(), &fields(), file_accepts);

        // A plant's share is checked in a Scatter holding only that plant.
        let alone =
            |entry: ScatterPlant| file_accepts(PlantScatter::new(Default::default(), vec![entry]));
        checks::range_ends_are_valid(&share_params(), &shares(), alone);
    }

    #[test]
    fn settings_come_after_the_settings_that_show_or_hide_them() {
        checks::settings_follow_what_shows_them(&field_params(), &varied_fields());
    }

    #[test]
    fn no_setting_hides_itself() {
        checks::no_setting_hides_itself(&field_params(), &varied_fields());
    }

    #[test]
    fn presets_fit_the_controls() {
        checks::cases_fit_the_controls(&field_params(), &fields());
        checks::cases_fit_the_controls(&share_params(), &shares());
    }
}
