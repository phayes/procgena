//! Clump controls and allowed input ranges for the Clumps tab.

use procgena_clump::{MAX_NECK, Placement};

use crate::params::{
    Group, Param, Section, choice, count, linear_from_hex, linear_to_hex, number, toggle,
};
use crate::scene::ClumpSettings;

/// Returns the clump controls grouped in display order, with labels and input
/// limits. The subject, clump preset and seed controls are separate.
///
/// The direction controls edit the clump's first direction; every clump
/// preset has exactly one.
pub fn sections() -> Vec<Section<ClumpSettings>> {
    vec![
        Section {
            title: "Shape",
            groups: vec![Group::plain(vec![
                Param::color(
                    "clump.material.color",
                    "Tint",
                    |s: &ClumpSettings| linear_to_hex(s.clump.material.color),
                    |s: &mut ClumpSettings, hex| s.clump.material.color = linear_from_hex(hex),
                ),
                number!(
                    ClumpSettings,
                    "clump.scale",
                    "Scale",
                    (0.1, 10.0, 0.05),
                    |s| s.clump.scale
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.shape.thickness",
                    "Thickness (m)",
                    (0.005, 1.0, 0.005),
                    |s| s.clump.shape.thickness
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.shape.sink",
                    "Edge Sink (m)",
                    (0.0, 0.2, 0.005),
                    |s| s.clump.shape.sink
                ),
                number!(
                    ClumpSettings,
                    "clump.shape.max_slope",
                    "Steepest Side",
                    (0.1, 5.0, 0.05),
                    |s| s.clump.shape.max_slope
                ),
                number!(
                    ClumpSettings,
                    "clump.shape.min_support",
                    "Thinnest Support (m)",
                    (0.0, 0.5, 0.005),
                    |s| s.clump.shape.min_support
                ),
                number!(
                    ClumpSettings,
                    "clump.shape.lumpiness",
                    "Lumpiness",
                    (0.0, 1.0, 0.01),
                    |s| s.clump.shape.lumpiness
                ),
                number!(
                    ClumpSettings,
                    "clump.shape.lump_size",
                    "Lump Size (m)",
                    (0.01, 2.0, 0.01),
                    |s| s.clump.shape.lump_size
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.shape.neck",
                    "Neck",
                    (0.0, MAX_NECK, 0.01),
                    |s| s.clump.shape.neck
                ),
            ])],
        },
        Section {
            title: "Placement",
            groups: vec![Group::plain(vec![
                choice!(ClumpSettings, "clump.placement.mode", "Placement", [
                    "Everywhere" => Placement::Everywhere,
                    "Scatter" => Placement::Scatter,
                ], |s| s.clump.placement.mode),
                count!(
                    ClumpSettings,
                    "clump.placement.count",
                    "Count",
                    (1, 200),
                    |s| s.clump.placement.count
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.placement.radius",
                    "Radius (m)",
                    (0.01, 2.0, 0.01),
                    |s| s.clump.placement.radius
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.placement.size_variation",
                    "Size Variation",
                    (0.0, 1.0, 0.01),
                    |s| s.clump.placement.size_variation
                ),
                number!(
                    ClumpSettings,
                    "clump.placement.min_spacing",
                    "Min Spacing (m)",
                    (0.0, 5.0, 0.01),
                    |s| s.clump.placement.min_spacing
                )
                .exponential(),
            ])],
        },
        Section {
            title: "Coverage",
            groups: vec![Group::plain(vec![
                toggle!(ClumpSettings, "clump.mask.any_facing", "Any Facing", |s| s
                    .clump
                    .mask
                    .any_facing),
                number!(
                    ClumpSettings,
                    "clump.mask.facing_min",
                    "Facing Start",
                    (-1.0, 1.0, 0.01),
                    |s| s.clump.mask.facing_min
                ),
                number!(
                    ClumpSettings,
                    "clump.mask.facing_full",
                    "Facing Full",
                    (-1.0, 1.0, 0.01),
                    |s| s.clump.mask.facing_full
                ),
                count!(
                    ClumpSettings,
                    "clump.mask.exposure_rays",
                    "Exposure Rays",
                    (0, 256),
                    |s| s.clump.mask.exposure_rays
                )
                .exponential(),
                number!(
                    ClumpSettings,
                    "clump.mask.exposure_power",
                    "Shelter Avoidance",
                    (0.0, 4.0, 0.05),
                    |s| s.clump.mask.exposure_power
                ),
            ])],
        },
        Section {
            title: "Direction",
            groups: vec![Group::plain(vec![
                direction_axis(0, "Travel X"),
                direction_axis(1, "Travel Y"),
                direction_axis(2, "Travel Z"),
                number!(
                    ClumpSettings,
                    "clump.directions.0.strength",
                    "Strength",
                    (0.0, 2.0, 0.01),
                    |s| s.clump.directions[0].strength
                ),
            ])],
        },
        Section {
            title: "Mesh",
            groups: vec![Group::plain(vec![
                number!(
                    ClumpSettings,
                    "clump.mesh.edge_length",
                    "Triangle Size (m)",
                    (0.005, 0.5, 0.005),
                    |s| s.clump.mesh.edge_length
                )
                .exponential(),
                toggle!(ClumpSettings, "clump.mesh.closed", "Closed Solid", |s| s
                    .clump
                    .mesh
                    .closed),
                toggle!(ClumpSettings, "clump.mesh.strict", "No Crossings", |s| s
                    .clump
                    .mesh
                    .strict),
            ])],
        },
    ]
}

/// The control for axis `axis` (0 to 2) of the direction the material travels.
fn direction_axis(axis: usize, label: &'static str) -> Param<ClumpSettings> {
    Param::number(
        format!("clump.directions.0.vector.{axis}"),
        label,
        (-1.0, 1.0, 0.01),
        move |s: &ClumpSettings| s.clump.directions[0].vector[axis],
        move |s: &mut ClumpSettings, value| s.clump.directions[0].vector[axis] = value,
    )
}

#[cfg(test)]
mod tests {
    use procgena_clump::ClumpPreset;

    use super::*;
    use crate::params::checks::{keys_are_unique, variants};
    use crate::params::spec::Control;

    /// Every control.
    fn params() -> Vec<Param<ClumpSettings>> {
        sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    #[test]
    fn keys_are_unique_and_presets_sit_within_the_limits() {
        let params = params();
        keys_are_unique(&params);
        for preset in ClumpPreset::ALL {
            let settings = super::super::settings(super::super::START_SUBJECT.settings(), preset);
            for param in &params {
                if let Control::Number { min, max, get, .. } = &param.control {
                    let value = get(&settings);
                    assert!(
                        (*min..=*max).contains(&value),
                        "{preset:?} {} = {value}",
                        param.key
                    );
                }
            }
        }
    }

    #[test]
    fn every_control_changes_the_settings() {
        let settings = super::super::start_settings();
        for param in params() {
            assert!(
                variants(&param, &settings).iter().any(|v| *v != settings),
                "{} changes nothing",
                param.key
            );
        }
    }
}
