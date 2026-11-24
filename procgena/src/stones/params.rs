//! Rock controls and allowed input ranges for the Stones tab.

use std::sync::LazyLock;

use procgena_stone::{
    MAX_CARVE_STEPS, MAX_OCTAVES, Mesher, StoneDetail, StoneOptions, TEXTURE_SETS,
};

use crate::params::{
    Group, Param, Section, choice, count, linear_from_hex, linear_to_hex, name_choice, number,
    toggle,
};

/// Names of the detail presets, in [`StoneDetail::PRESETS`] order.
static DETAIL_NAMES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| StoneDetail::PRESETS.iter().map(|(name, _)| *name).collect());

/// Returns the rock controls grouped in display order, with labels and input
/// limits. Preset and seed controls are separate; Detail affects both the
/// preview and saved models.
pub fn sections() -> Vec<Section<StoneOptions>> {
    vec![
        Section {
            title: "Shape",
            groups: vec![Group::plain(vec![
                number!(StoneOptions, "scale", "Size (m)", (0.05, 10.0, 0.05), |o| o
                    .scale)
                .exponential(),
                stretch(0, "Stretch X"),
                stretch(1, "Stretch Y"),
                stretch(2, "Stretch Z"),
            ])],
        },
        Section {
            title: "Carving",
            groups: vec![Group::plain(vec![
                count!(
                    StoneOptions,
                    "carve.steps",
                    "Cuts",
                    (0, MAX_CARVE_STEPS),
                    |o| o.carve.steps
                ),
                number!(
                    StoneOptions,
                    "carve.smoothness",
                    "Edge Softness",
                    (0.005, 0.5, 0.005),
                    |o| o.carve.smoothness
                )
                .exponential(),
                number!(
                    StoneOptions,
                    "carve.base_radius",
                    "Starting Radius",
                    (0.3, 1.0, 0.01),
                    |o| o.carve.base_radius
                ),
                number!(
                    StoneOptions,
                    "carve.radius_min",
                    "Cut Distance",
                    (0.5, 10.0, 0.1),
                    |o| o.carve.radius_min
                ),
                number!(
                    StoneOptions,
                    "carve.radius_jitter",
                    "Cut Distance Variation",
                    (0.0, 5.0, 0.1),
                    |o| o.carve.radius_jitter
                ),
                number!(
                    StoneOptions,
                    "carve.depth",
                    "Cut Depth",
                    (0.1, 1.2, 0.01),
                    |o| o.carve.depth
                ),
                number!(
                    StoneOptions,
                    "carve.blockiness",
                    "Blockiness",
                    (0.0, 1.0, 0.01),
                    |o| o.carve.blockiness
                ),
            ])],
        },
        Section {
            title: "Placement",
            groups: vec![Group::plain(vec![
                toggle!(StoneOptions, "placement.settle", "Settle", |o| o
                    .placement
                    .settle),
                number!(
                    StoneOptions,
                    "placement.max_embed_fraction",
                    "Max Embed",
                    (0.05, 1.0, 0.01),
                    |o| o.placement.max_embed_fraction
                ),
                number!(
                    StoneOptions,
                    "placement.min_embed_fraction",
                    "Min Embed",
                    (0.0, 0.5, 0.01),
                    |o| o.placement.min_embed_fraction
                ),
            ])],
        },
        Section {
            title: "Surface",
            groups: vec![Group::plain(vec![
                number!(
                    StoneOptions,
                    "surface.displacement_scale",
                    "Roughness Strength",
                    (0.0, 0.5, 0.005),
                    |o| o.surface.displacement_scale
                ),
                number!(
                    StoneOptions,
                    "surface.displacement_spread",
                    "Roughness Frequency",
                    (1.0, 30.0, 0.5),
                    |o| o.surface.displacement_spread
                )
                .exponential(),
                count!(
                    StoneOptions,
                    "surface.octaves",
                    "Roughness Layers",
                    (1, MAX_OCTAVES),
                    |o| o.surface.octaves
                ),
            ])],
        },
        Section {
            title: "Material",
            groups: vec![Group::plain(vec![
                name_choice!(
                    StoneOptions,
                    "material.texture",
                    "Texture",
                    TEXTURE_SETS,
                    |o| o.material.texture
                ),
                Param::color(
                    "material.color",
                    "Tint",
                    |o: &StoneOptions| linear_to_hex(o.material.color),
                    |o: &mut StoneOptions, hex| o.material.color = linear_from_hex(hex),
                ),
                number!(
                    StoneOptions,
                    "material.roughness",
                    "Matte",
                    (0.0, 1.0, 0.01),
                    |o| o.material.roughness
                ),
                number!(
                    StoneOptions,
                    "material.texture_size",
                    "Texture Size (m)",
                    (0.05, 5.0, 0.05),
                    |o| o.material.texture_size
                )
                .exponential(),
            ])],
        },
        Section {
            title: "Mesh",
            groups: vec![Group::plain(vec![
                choice!(StoneOptions, "mesh.mesher", "Mesher", [
                    "Marching Cubes" => Mesher::MarchingCubes,
                    "Dual Contouring" => Mesher::DualContouring,
                ], |o| o.mesh.mesher),
                detail(),
            ])],
        },
    ]
}

/// Creates the Detail picker, choosing one of the [`StoneDetail`] presets.
/// A custom detail set loaded from a file shows as the first preset, and is
/// kept until another preset is picked.
fn detail() -> Param<StoneOptions> {
    Param::choice(
        "mesh.detail",
        "Detail",
        DETAIL_NAMES.as_slice(),
        |o: &StoneOptions| {
            StoneDetail::PRESETS
                .iter()
                .position(|(_, preset)| *preset == o.mesh.detail)
                .unwrap_or(0)
        },
        |o: &mut StoneOptions, index| {
            if let Some((_, preset)) = StoneDetail::PRESETS.get(index) {
                o.mesh.detail = preset.clone();
            }
        },
    )
}

/// Creates a stretch control for axis 0 (X), 1 (Y) or 2 (Z).
/// Values are relative: the largest axis sets the reference size, and the
/// others are shortened in proportion.
fn stretch(axis: usize, label: &'static str) -> Param<StoneOptions> {
    Param::number(
        format!("stretch.{axis}"),
        label,
        (0.1, 1.0, 0.01),
        move |o: &StoneOptions| o.stretch[axis],
        move |o: &mut StoneOptions, value| o.stretch[axis] = value,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use procgena_stone::StonePreset;
    use serde_json::Value;

    use super::*;
    use crate::params::Control;

    fn all_params() -> Vec<Param<StoneOptions>> {
        sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Looks up a JSON field using a dotted path, such as `stretch.0`.
    /// Numeric path segments select array elements.
    fn json_at<'a>(json: &'a Value, key: &str) -> &'a Value {
        key.split('.')
            .fold(json, |value, part| match part.parse::<usize>() {
                Ok(index) if value.is_array() => &value[index],
                _ => &value[part],
            })
    }

    fn json(options: &StoneOptions) -> Value {
        serde_json::to_value(options).unwrap()
    }

    #[test]
    fn keys_are_unique() {
        let mut seen = HashSet::new();
        for param in all_params() {
            assert!(
                seen.insert(param.key.clone()),
                "duplicate key {}",
                param.key
            );
        }
    }

    #[test]
    fn every_setting_writes_the_json_field_named_by_its_key() {
        for param in all_params() {
            let mut options = StoneOptions::default();
            let key = &param.key;
            match &param.control {
                Control::Number {
                    min, max, get, set, ..
                } => {
                    // Change the value to an allowed endpoint so an ineffective setter fails the test.
                    let target = if get(&options) == *max { *min } else { *max };
                    set(&mut options, target);
                    assert_eq!(get(&options), target, "{key} reads back");
                    let written = json_at(&json(&options), key).as_f64();
                    let written = written.unwrap_or_else(|| panic!("{key} is not a JSON number"));
                    assert!(
                        (written as f32 - target).abs() < 1e-4,
                        "{key} wrote {written}"
                    );
                }
                Control::Color { get, set } => {
                    set(&mut options, 0x123456);
                    assert_eq!(get(&options), 0x123456, "{key} reads back");
                    assert!(json_at(&json(&options), key).is_array(), "{key}");
                }
                Control::Choice {
                    options: names,
                    get,
                    set,
                } => {
                    // Check that every choice can be read back and saves a distinct JSON value.
                    let mut written = HashSet::new();
                    for index in 0..names.len() {
                        set(&mut options, index);
                        assert_eq!(get(&options), index, "{key} option {index}");
                        written.insert(json_at(&json(&options), key).to_string());
                    }
                    assert_eq!(written.len(), names.len(), "{key} writes each choice");
                }
                Control::Toggle { get, set } => {
                    let flipped = !get(&options);
                    set(&mut options, flipped);
                    assert_eq!(get(&options), flipped, "{key} reads back");
                    assert_eq!(
                        json_at(&json(&options), key),
                        &Value::Bool(flipped),
                        "{key}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_range_end_is_valid() {
        for param in all_params() {
            let Control::Number { min, max, set, .. } = &param.control else {
                continue;
            };
            for end in [*min, *max] {
                let mut options = StoneOptions::default();
                set(&mut options, end);
                assert_eq!(options.validate(), Ok(()), "{} at {end}", param.key);
            }
        }
    }

    #[test]
    fn presets_fit_the_controls() {
        let params = all_params();
        for preset in StonePreset::all() {
            let options = preset.stone.clone();
            let name = &preset.name;
            for param in &params {
                let key = &param.key;
                match &param.control {
                    Control::Number { min, max, get, .. } => {
                        let value = get(&options);
                        assert!(
                            (*min..=*max).contains(&value),
                            "{name}: {key} is {value}, outside {min}..={max}"
                        );
                    }
                    // Detect preset values that the picker would silently replace with its first choice.
                    Control::Choice { get, set, .. } => {
                        let mut shown = options.clone();
                        set(&mut shown, get(&options));
                        assert_eq!(
                            json_at(&json(&shown), key),
                            json_at(&json(&options), key),
                            "{name}: {key} is not one of the choices"
                        );
                    }
                    // Check that writing a preset's colour through the picker preserves
                    // the displayed colour. The stored tint can round to 8-bit precision.
                    Control::Color { get, set } => {
                        let mut shown = options.clone();
                        set(&mut shown, get(&options));
                        assert_eq!(get(&shown), get(&options), "{name}");
                    }
                    Control::Toggle { .. } => {}
                }
            }
        }
    }
}
