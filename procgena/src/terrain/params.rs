//! Terrain controls and allowed input ranges for the Terrain tab.

use procgena_terrain::{
    MAX_EROSION, MAX_OCTAVES, MAX_PORT_WIDTH, MAX_RIVER_DENSITY, TerrainOptions,
};

use crate::params::{Group, Section, count, number, toggle};

/// Every terrain setting, in sections: the land's shape, rivers, roads and
/// borders. The seed is edited above the panel.
pub fn sections() -> Vec<Section<TerrainOptions>> {
    vec![
        Section {
            title: "Land",
            groups: vec![
                Group::plain(vec![
                    number!(
                        TerrainOptions,
                        "base.elevation",
                        "Elevation (m)",
                        (-500.0, 1500.0, 1.0),
                        |o| o.base.elevation
                    ),
                    number!(
                        TerrainOptions,
                        "base.erosion",
                        "Erosion",
                        (0.0, MAX_EROSION, 0.05),
                        |o| o.base.erosion
                    ),
                ]),
                Group::titled(
                    "Swells",
                    vec![
                        number!(
                            TerrainOptions,
                            "base.swells.height",
                            "Height (m)",
                            (0.0, 300.0, 0.5),
                            |o| o.base.swells.height
                        )
                        .exponential(),
                        number!(
                            TerrainOptions,
                            "base.swells.wavelength",
                            "Wavelength (m)",
                            (100.0, 20000.0, 10.0),
                            |o| o.base.swells.wavelength
                        )
                        .exponential(),
                        count!(
                            TerrainOptions,
                            "base.swells.octaves",
                            "Octaves",
                            (1, MAX_OCTAVES),
                            |o| o.base.swells.octaves
                        ),
                        number!(
                            TerrainOptions,
                            "base.swells.roughness",
                            "Roughness",
                            (0.0, 0.95, 0.01),
                            |o| o.base.swells.roughness
                        ),
                    ],
                ),
                Group::titled(
                    "Hills",
                    vec![
                        number!(
                            TerrainOptions,
                            "base.hills.height",
                            "Height (m)",
                            (0.0, 300.0, 0.5),
                            |o| o.base.hills.height
                        )
                        .exponential(),
                        number!(
                            TerrainOptions,
                            "base.hills.wavelength",
                            "Wavelength (m)",
                            (20.0, 5000.0, 5.0),
                            |o| o.base.hills.wavelength
                        )
                        .exponential(),
                        count!(
                            TerrainOptions,
                            "base.hills.octaves",
                            "Octaves",
                            (1, MAX_OCTAVES),
                            |o| o.base.hills.octaves
                        ),
                        number!(
                            TerrainOptions,
                            "base.hills.roughness",
                            "Roughness",
                            (0.0, 0.95, 0.01),
                            |o| o.base.hills.roughness
                        ),
                    ],
                ),
                Group::titled(
                    "Mountains",
                    vec![
                        number!(
                            TerrainOptions,
                            "base.mountains.height",
                            "Height (m)",
                            (0.0, 3000.0, 5.0),
                            |o| o.base.mountains.height
                        ),
                        number!(
                            TerrainOptions,
                            "base.mountains.wavelength",
                            "Wavelength (m)",
                            (200.0, 20000.0, 10.0),
                            |o| o.base.mountains.wavelength
                        )
                        .exponential(),
                        number!(
                            TerrainOptions,
                            "base.mountains.coverage",
                            "Coverage",
                            (0.0, 1.0, 0.01),
                            |o| o.base.mountains.coverage
                        ),
                        count!(
                            TerrainOptions,
                            "base.mountains.octaves",
                            "Octaves",
                            (1, MAX_OCTAVES),
                            |o| o.base.mountains.octaves
                        ),
                    ],
                ),
            ],
        },
        Section {
            title: "Rivers",
            groups: vec![Group::plain(vec![
                number!(
                    TerrainOptions,
                    "features.rivers.density",
                    "Sources per km²",
                    (0.0, MAX_RIVER_DENSITY, 0.05),
                    |o| o.features.rivers.density
                ),
                number!(
                    TerrainOptions,
                    "features.rivers.spacing",
                    "Spacing (m)",
                    (50.0, 5000.0, 10.0),
                    |o| o.features.rivers.spacing
                )
                .exponential(),
                number!(
                    TerrainOptions,
                    "features.rivers.width",
                    "Width (m)",
                    (1.0, MAX_PORT_WIDTH, 0.5),
                    |o| o.features.rivers.width
                ),
                number!(
                    TerrainOptions,
                    "features.rivers.depth",
                    "Depth (m)",
                    (0.1, 10.0, 0.1),
                    |o| o.features.rivers.depth
                ),
                number!(
                    TerrainOptions,
                    "features.rivers.min_length",
                    "Shortest (m)",
                    (0.0, 3000.0, 10.0),
                    |o| o.features.rivers.min_length
                ),
            ])],
        },
        Section {
            title: "Roads",
            groups: vec![Group::plain(vec![
                toggle!(TerrainOptions, "features.roads.enabled", "Roads", |o| o
                    .features
                    .roads
                    .enabled),
                number!(
                    TerrainOptions,
                    "features.roads.width",
                    "Width (m)",
                    (2.0, MAX_PORT_WIDTH, 0.5),
                    |o| o.features.roads.width
                ),
                number!(
                    TerrainOptions,
                    "features.roads.min_width",
                    "Narrowest (m)",
                    (1.0, MAX_PORT_WIDTH, 0.5),
                    |o| o.features.roads.min_width
                ),
                number!(
                    TerrainOptions,
                    "features.roads.grade",
                    "Comfortable Grade",
                    (0.01, 1.0, 0.01),
                    |o| o.features.roads.grade
                ),
                number!(
                    TerrainOptions,
                    "features.roads.max_grade",
                    "Steepest Grade",
                    (0.01, 1.0, 0.01),
                    |o| o.features.roads.max_grade
                ),
            ])],
        },
        Section {
            title: "Borders",
            groups: vec![Group::plain(vec![
                number!(
                    TerrainOptions,
                    "edges.stiffness",
                    "Stiffness",
                    (0.05, 20.0, 0.05),
                    |o| o.edges.stiffness
                )
                .exponential(),
                number!(
                    TerrainOptions,
                    "edges.margin",
                    "Margin (m)",
                    (10.0, 2000.0, 5.0),
                    |o| o.edges.margin
                )
                .exponential(),
                number!(
                    TerrainOptions,
                    "edges.roughness",
                    "Roughness (m)",
                    (0.0, 20.0, 0.1),
                    |o| o.edges.roughness
                ),
            ])],
        },
    ]
}

#[cfg(test)]
mod tests {
    use procgena_terrain::TerrainPreset;

    use super::*;
    use crate::params::checks;

    #[test]
    fn controls_cover_every_preset() {
        let params: Vec<_> = sections()
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.params)
            .collect();
        let cases: Vec<_> = TerrainPreset::all()
            .map(|p| (p.key.to_string(), p.terrain.clone()))
            .collect();
        checks::keys_are_unique(&params);
        checks::cases_fit_the_controls(&params, &cases);
        checks::settings_follow_what_shows_them(&params, &cases);
        checks::no_setting_hides_itself(&params, &cases);
    }
}
