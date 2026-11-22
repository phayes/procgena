//! Controls for a field's shared appearance and each plant's role in the mix.
//!
//! Plant shape and colour controls are shared with the Plant tab.

use procgena_plant::prelude::{FieldPlant, GroundTint, PlantField, PlantVariation};

use crate::params::{Group, Section, count, number, srgb};

/// The field's own settings, in display order. Its plants are edited
/// separately, one row each.
pub fn sections() -> Vec<Section<PlantField>> {
    vec![
        Section {
            title: "Variation",
            groups: vec![variation().lift(
                "variation",
                |f: &PlantField| &f.variation,
                |f| &mut f.variation,
            )],
        },
        Section {
            title: "Field",
            groups: vec![
                field(),
                ground_tint().lift(
                    "ground_tint",
                    |f: &PlantField| &f.ground_tint,
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
pub fn share_sections() -> Vec<Section<FieldPlant>> {
    vec![Section {
        title: "Share",
        groups: vec![Group::plain(vec![
            number!(
                FieldPlant,
                "density_scale",
                "Density ×",
                (0.001, 2.0, 0.001),
                |e| e.density_scale
            ),
            number!(
                FieldPlant,
                "clustering",
                "Clustering",
                (0.0, 1.0, 0.01),
                |e| e.clustering
            ),
        ])],
    }]
}

fn field() -> Group<PlantField> {
    Group::plain(vec![
        // Multiply all entry densities together without changing the plant proportions.
        number!(
            PlantField,
            "density_scale",
            "Field Density ×",
            (0.0, 2.0, 0.01),
            |f| f.density_scale
        ),
        number!(
            PlantField,
            "wind_scale",
            "Wind Scale",
            (0.0, 2.0, 0.01),
            |f| f.wind_scale
        ),
        count!(PlantField, "toon_bands", "Toon Bands", (0, 8), |f| f
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
        .shown_if(|v| v.patch_amount > 0.0),
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
        .shown_if(|v| v.bare_share > 0.0),
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
    use procgena_plant::prelude::FieldPreset;

    use super::*;
    use crate::params::{Param, checks};

    fn field_params() -> Vec<Param<PlantField>> {
        params_of(sections())
    }

    fn share_params() -> Vec<Param<FieldPlant>> {
        params_of(share_sections())
    }

    fn params_of<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
        sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Each field preset, named so test failures identify it.
    fn fields() -> Vec<(String, PlantField)> {
        FieldPreset::all()
            .map(|preset| (preset.key.to_string(), preset.field.clone()))
            .collect()
    }

    /// The presets, plus a field that hides every setting that can be hidden.
    fn varied_fields() -> Vec<(String, PlantField)> {
        let mut cases = fields();
        let mut even = FieldPreset::MEADOW.field;
        even.variation.patch_amount = 0.0;
        even.variation.bare_share = 0.0;
        even.ground_tint.amount = 0.0;
        cases.push(("even field".into(), even));
        cases
    }

    /// Each plant of each field preset, named by its place.
    fn shares() -> Vec<(String, FieldPlant)> {
        FieldPreset::all()
            .flat_map(|preset| {
                preset
                    .field
                    .plants
                    .iter()
                    .enumerate()
                    .map(|(i, entry)| (format!("{}.plants[{i}]", preset.key), *entry))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Checks `field` the way a `.field.ron` file is checked.
    fn file_accepts(field: PlantField) -> Result<(), String> {
        PlantField::from_ron(&field.to_ron())
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

        // A plant's share is checked in a field holding only that plant.
        let alone =
            |entry: FieldPlant| file_accepts(PlantField::new(Default::default(), vec![entry]));
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
