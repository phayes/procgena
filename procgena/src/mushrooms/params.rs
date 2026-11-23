//! Mushroom controls and allowed input ranges for the Mushrooms tab.

use procgena_mushroom::{
    CAP_TEXTURE_SETS, CapFamily, MAX_GILL_LINES, MAX_LOBES, MAX_WARTS, MaterialOptions,
    MushroomOptions, Pattern, STEM_TEXTURE_SETS,
};

use crate::params::{Group, Param, Section, choice, count, number, toggle};

/// The Cap Texture choices: "None", then each of [`CAP_TEXTURE_SETS`] in
/// order.
const CAP_TEXTURE_CHOICES: [&str; 1 + CAP_TEXTURE_SETS.len()] = ["None", "mushroom_top_001"];

/// The Stem Texture choices: "None", then each of [`STEM_TEXTURE_SETS`] in
/// order.
const STEM_TEXTURE_CHOICES: [&str; 1 + STEM_TEXTURE_SETS.len()] =
    ["None", "creamy_tan", "brown_net"];

/// A texture control for the setting at `key`: "None", or one of `sets`,
/// listed in `choices` after "None". `field` and `field_mut` pick the
/// setting from the material options. A set name the list doesn't know
/// shows as "None".
fn texture_choice(
    key: &'static str,
    choices: &'static [&'static str],
    sets: &'static [&'static str],
    field: fn(&MaterialOptions) -> &Option<String>,
    field_mut: fn(&mut MaterialOptions) -> &mut Option<String>,
) -> Param<MushroomOptions> {
    Param::choice(
        key,
        "Texture",
        choices,
        move |o: &MushroomOptions| {
            field(&o.material)
                .as_deref()
                .and_then(|set| sets.iter().position(|s| *s == set))
                .map_or(0, |i| i + 1)
        },
        move |o: &mut MushroomOptions, index| {
            *field_mut(&mut o.material) = index
                .checked_sub(1)
                .and_then(|i| sets.get(i))
                .map(|set| (*set).to_owned());
        },
    )
}

/// A colour picker for a `"#rrggbb"` text field of [`MushroomOptions`].
macro_rules! hex_colour {
    ($key:expr, $label:expr, |$o:ident| $place:expr) => {
        Param::color(
            $key,
            $label,
            |$o: &MushroomOptions| hex_to_picker(&$place),
            |$o: &mut MushroomOptions, value| $place = picker_to_hex(value),
        )
    };
}

/// Returns the mushroom controls grouped in display order, with labels and
/// input limits. Preset and seed controls are separate.
pub fn sections() -> Vec<Section<MushroomOptions>> {
    vec![
        Section {
            title: "Cap",
            groups: vec![Group::plain(vec![
                number!(
                    MushroomOptions,
                    "size",
                    "Cap Width (m)",
                    (0.005, 0.5, 0.001),
                    |o| o.size
                )
                .exponential(),
                choice!(MushroomOptions, "cap.family", "Shape", [
                    "Bell" => CapFamily::Campanulate,
                    "Cone" => CapFamily::Conical,
                    "Dome" => CapFamily::Convex,
                    "Dipped" => CapFamily::Depressed,
                    "Flat" => CapFamily::Flat,
                    "Funnel" => CapFamily::Infundibuliform,
                    "Egg" => CapFamily::Ovate,
                    "Navel" => CapFamily::Umbilicate,
                    "Knob" => CapFamily::Umbonate,
                    "Globe" => CapFamily::Globe,
                ], |o| o.cap.family),
                number!(
                    MushroomOptions,
                    "cap.height",
                    "Height",
                    (0.2, 3.0, 0.01),
                    |o| o.cap.height
                ),
                count!(
                    MushroomOptions,
                    "cap.lobes",
                    "Rim Waves",
                    (0, MAX_LOBES),
                    |o| o.cap.lobes
                ),
                number!(
                    MushroomOptions,
                    "cap.lobe_depth",
                    "Wave Depth",
                    (0.0, 0.5, 0.01),
                    |o| o.cap.lobe_depth
                ),
                number!(
                    MushroomOptions,
                    "cap.asymmetry",
                    "Lopsidedness",
                    (0.0, 1.0, 0.01),
                    |o| o.cap.asymmetry
                ),
                number!(
                    MushroomOptions,
                    "cap.tilt",
                    "Tilt (°)",
                    (0.0, 45.0, 0.5),
                    |o| o.cap.tilt
                ),
                number!(
                    MushroomOptions,
                    "cap.roughness",
                    "Bumps",
                    (0.0, 0.2, 0.001),
                    |o| o.cap.roughness
                ),
                number!(
                    MushroomOptions,
                    "cap.roughness_scale",
                    "Bump Count",
                    (0.5, 40.0, 0.5),
                    |o| o.cap.roughness_scale
                )
                .exponential(),
            ])],
        },
        Section {
            title: "Underside",
            groups: vec![Group::plain(vec![
                number!(
                    MushroomOptions,
                    "underside.rim_thickness",
                    "Rim Thickness",
                    (0.0, 0.5, 0.01),
                    |o| o.underside.rim_thickness
                ),
                number!(
                    MushroomOptions,
                    "underside.sag",
                    "Sag",
                    (-0.5, 0.5, 0.01),
                    |o| o.underside.sag
                ),
                number!(
                    MushroomOptions,
                    "underside.decurrent",
                    "Runs Down Stem",
                    (0.0, 1.0, 0.01),
                    |o| o.underside.decurrent
                ),
            ])],
        },
        Section {
            title: "Stem",
            groups: vec![Group::plain(vec![
                number!(
                    MushroomOptions,
                    "stem.height",
                    "Height",
                    (0.05, 10.0, 0.05),
                    |o| o.stem.height
                )
                .exponential(),
                number!(
                    MushroomOptions,
                    "stem.radius",
                    "Thickness",
                    (0.01, 0.4, 0.005),
                    |o| o.stem.radius
                ),
                number!(
                    MushroomOptions,
                    "stem.base",
                    "Base Width",
                    (0.3, 3.0, 0.05),
                    |o| o.stem.base
                ),
                number!(
                    MushroomOptions,
                    "stem.bulb",
                    "Bulb",
                    (0.0, 3.0, 0.05),
                    |o| o.stem.bulb
                ),
                number!(
                    MushroomOptions,
                    "stem.bulb_height",
                    "Bulb Height",
                    (0.01, 1.0, 0.01),
                    |o| o.stem.bulb_height
                ),
                number!(
                    MushroomOptions,
                    "stem.collar",
                    "Collar",
                    (0.0, 1.0, 0.01),
                    |o| o.stem.collar
                ),
                number!(
                    MushroomOptions,
                    "stem.lean",
                    "Lean",
                    (0.0, 1.0, 0.01),
                    |o| o.stem.lean
                ),
                number!(
                    MushroomOptions,
                    "stem.curve",
                    "Bend Near Top",
                    (1.0, 4.0, 0.05),
                    |o| o.stem.curve
                ),
                number!(
                    MushroomOptions,
                    "stem.upturn",
                    "Upturn",
                    (0.0, 1.0, 0.01),
                    |o| o.stem.upturn
                ),
                number!(
                    MushroomOptions,
                    "stem.lean_direction",
                    "Lean Direction (°)",
                    (0.0, 360.0, 1.0),
                    |o| o.stem.lean_direction
                ),
                number!(
                    MushroomOptions,
                    "stem.roughness",
                    "Bumps",
                    (0.0, 0.3, 0.005),
                    |o| o.stem.roughness
                ),
            ])],
        },
        Section {
            title: "Ring",
            groups: vec![
                Group::plain(vec![toggle!(
                    MushroomOptions,
                    "ring.enabled",
                    "Ring",
                    |o| o.ring.enabled
                )]),
                Group::plain(vec![
                    number!(
                        MushroomOptions,
                        "ring.position",
                        "Height on Stem",
                        (0.1, 1.0, 0.01),
                        |o| o.ring.position
                    ),
                    number!(
                        MushroomOptions,
                        "ring.length",
                        "Length",
                        (0.01, 0.5, 0.01),
                        |o| o.ring.length
                    ),
                    number!(
                        MushroomOptions,
                        "ring.flare",
                        "Flare",
                        (1.0, 4.0, 0.05),
                        |o| o.ring.flare
                    ),
                    number!(
                        MushroomOptions,
                        "ring.raggedness",
                        "Raggedness",
                        (0.0, 1.0, 0.01),
                        |o| o.ring.raggedness
                    ),
                ])
                .shown_if(|o: &MushroomOptions| o.ring.enabled),
            ],
        },
        Section {
            title: "Warts",
            groups: vec![
                Group::plain(vec![count!(
                    MushroomOptions,
                    "warts.count",
                    "Count",
                    (0, MAX_WARTS),
                    |o| o.warts.count
                )]),
                Group::plain(vec![
                    number!(
                        MushroomOptions,
                        "warts.size",
                        "Size",
                        (0.005, 0.3, 0.005),
                        |o| o.warts.size
                    ),
                    number!(
                        MushroomOptions,
                        "warts.size_jitter",
                        "Size Variation",
                        (0.0, 1.0, 0.01),
                        |o| o.warts.size_jitter
                    ),
                    number!(
                        MushroomOptions,
                        "warts.height",
                        "Height",
                        (0.05, 2.0, 0.01),
                        |o| o.warts.height
                    ),
                    number!(
                        MushroomOptions,
                        "warts.coverage",
                        "Coverage",
                        (0.05, 1.0, 0.01),
                        |o| o.warts.coverage
                    ),
                ])
                .shown_if(|o: &MushroomOptions| o.warts.count > 0),
            ],
        },
        Section {
            title: "Colours",
            groups: vec![Group::plain(vec![
                hex_colour!("material.cap_centre", "Cap Centre", |o| o
                    .material
                    .cap_centre),
                hex_colour!("material.cap_rim", "Cap Rim", |o| o.material.cap_rim),
                hex_colour!("material.underside", "Underside", |o| o.material.underside),
                hex_colour!("material.stem_top", "Stem Top", |o| o.material.stem_top),
                hex_colour!("material.stem_base", "Stem Base", |o| o.material.stem_base),
                hex_colour!("material.ring", "Ring", |o| o.material.ring),
                hex_colour!("material.wart", "Warts", |o| o.material.wart),
                number!(
                    MushroomOptions,
                    "material.roughness",
                    "Matte",
                    (0.0, 1.0, 0.01),
                    |o| o.material.roughness
                ),
                count!(
                    MushroomOptions,
                    "material.gill_lines",
                    "Gill Lines",
                    (0, MAX_GILL_LINES),
                    |o| o.material.gill_lines
                ),
            ])],
        },
        Section {
            title: "Pattern",
            groups: vec![
                Group::plain(vec![
                    choice!(MushroomOptions, "material.pattern", "Pattern", [
                    "None" => Pattern::None,
                    "Speckle" => Pattern::Speckle,
                    "Cracks" => Pattern::Cracks,
                    "Spots" => Pattern::Spots,
                ], |o| o.material.pattern),
                ]),
                Group::plain(vec![
                    hex_colour!("material.pattern_colour", "Colour", |o| o
                        .material
                        .pattern_colour),
                    number!(
                        MushroomOptions,
                        "material.pattern_scale",
                        "Features Across Cap",
                        (0.5, 60.0, 0.5),
                        |o| o.material.pattern_scale
                    )
                    .exponential(),
                    number!(
                        MushroomOptions,
                        "material.pattern_strength",
                        "Strength",
                        (0.0, 1.0, 0.01),
                        |o| o.material.pattern_strength
                    ),
                ])
                .shown_if(|o: &MushroomOptions| o.material.pattern != Pattern::None),
            ],
        },
        Section {
            title: "Cap Texture",
            groups: vec![
                Group::plain(vec![texture_choice(
                    "material.cap_texture",
                    &CAP_TEXTURE_CHOICES,
                    &CAP_TEXTURE_SETS,
                    |m| &m.cap_texture,
                    |m| &mut m.cap_texture,
                )]),
                Group::plain(vec![
                    number!(
                        MushroomOptions,
                        "material.cap_texture_size",
                        "Repeat Width (m)",
                        (0.01, 1.0, 0.005),
                        |o| o.material.cap_texture_size
                    )
                    .exponential(),
                ])
                .shown_if(|o: &MushroomOptions| o.material.cap_texture.is_some()),
            ],
        },
        Section {
            title: "Stem Texture",
            groups: vec![
                Group::plain(vec![texture_choice(
                    "material.stem_texture",
                    &STEM_TEXTURE_CHOICES,
                    &STEM_TEXTURE_SETS,
                    |m| &m.stem_texture,
                    |m| &mut m.stem_texture,
                )]),
                Group::plain(vec![
                    number!(
                        MushroomOptions,
                        "material.stem_texture_size",
                        "Repeat Length (m)",
                        (0.005, 0.5, 0.005),
                        |o| o.material.stem_texture_size
                    )
                    .exponential(),
                ])
                .shown_if(|o: &MushroomOptions| o.material.stem_texture.is_some()),
            ],
        },
    ]
}

/// Reads a `"#rrggbb"` colour as the colour picker's `0xRRGGBB`. Text that
/// can't be read shows as black.
fn hex_to_picker(text: &str) -> u32 {
    u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(0) & 0x00ff_ffff
}

/// Writes the colour picker's `0xRRGGBB` as `"#rrggbb"`.
fn picker_to_hex(value: u32) -> String {
    format!("#{:06x}", value & 0x00ff_ffff)
}

#[cfg(test)]
mod tests {
    use procgena_mushroom::MushroomPreset;

    use super::*;
    use crate::params::checks;

    #[test]
    fn the_texture_choices_follow_the_texture_sets() {
        assert_eq!(CAP_TEXTURE_CHOICES[1..], CAP_TEXTURE_SETS);
        assert_eq!(STEM_TEXTURE_CHOICES[1..], STEM_TEXTURE_SETS);
    }

    #[test]
    fn colours_survive_the_picker() {
        for text in ["#000000", "#c8102e", "#ffffff"] {
            assert_eq!(picker_to_hex(hex_to_picker(text)), text);
        }
        assert_eq!(hex_to_picker("#C8102E"), 0xc8102e);
    }

    fn all_params() -> Vec<Param<MushroomOptions>> {
        sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Every preset, named for failure messages.
    fn presets() -> Vec<(String, MushroomOptions)> {
        MushroomPreset::ALL
            .iter()
            .map(|preset| (preset.name().to_owned(), preset.options()))
            .collect()
    }

    /// Whether the mushroom generator accepts `options`.
    fn accepted(options: MushroomOptions) -> Result<(), String> {
        options.validate().map_err(|error| error.to_string())
    }

    #[test]
    fn keys_are_unique() {
        checks::keys_are_unique(&all_params());
    }

    #[test]
    fn every_preset_fits_the_controls() {
        checks::cases_fit_the_controls(&all_params(), &presets());
    }

    #[test]
    fn every_control_value_is_accepted() {
        checks::range_ends_are_valid(&all_params(), &presets(), accepted);
    }

    #[test]
    fn controls_show_and_hide_sensibly() {
        checks::settings_follow_what_shows_them(&all_params(), &presets());
        checks::no_setting_hides_itself(&all_params(), &presets());
    }
}
