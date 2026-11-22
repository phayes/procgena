//! Plant controls shared by the Plant and Plant Field tabs.
//!
//! Defines editable ranges and which controls appear when branches, heads,
//! tufts or accents are enabled.

use procgena_plant::prelude::{
    PlantAccent, PlantArrangement, PlantBranches, PlantParams, PlantSegment,
};

use crate::params::{Group, Param, Section, choice, count, number, srgb};

/// Every plant settings section, in display order. The preset is chosen
/// separately.
pub fn sections() -> Vec<Section<PlantParams>> {
    vec![
        Section {
            title: "Plant",
            groups: plant(),
        },
        Section {
            title: "Accent",
            groups: vec![accent().lift("accent", |p: &PlantParams| &p.accent, |p| &mut p.accent)],
        },
    ]
}

/// Whether the plant's stems grow branches.
fn branched(p: &PlantParams) -> bool {
    p.levels[0].is_on()
}

/// Whether the main stems display a seed head or flower.
/// Branching hides the main head; accents have their own head settings.
fn has_head(p: &PlantParams) -> bool {
    !branched(p) && p.head_length > 0.0
}

fn plant() -> Vec<Group<PlantParams>> {
    let size = Group::plain(vec![
        // Plants per square metre that suit this plant when it grows alone.
        number!(PlantParams, "density", "Per m²", (0.1, 300.0, 0.1), |p| p
            .density)
        .exponential(),
        number!(PlantParams, "height.x", "Shortest", (0.0, 4.0, 0.01), |p| p
            .height
            .x)
        .exponential(),
        number!(PlantParams, "height.y", "Tallest", (0.01, 4.0, 0.01), |p| p
            .height
            .y)
        .exponential(),
        number!(PlantParams, "width", "Width", (0.001, 0.2, 0.001), |p| p
            .width)
        .exponential(),
        number!(PlantParams, "lean", "Lean", (0.0, 1.0, 0.01), |p| p.lean),
        count!(PlantParams, "stems_per_plant", "Stems", (1, 8), |p| p
            .stems_per_plant),
    ]);
    let light = Group::titled(
        "Light",
        vec![
            number!(
                PlantParams,
                "root_occlusion",
                "Root Shade",
                (0.0, 1.0, 0.01),
                |p| p.root_occlusion
            ),
            number!(
                PlantParams,
                "translucency",
                "Translucency",
                (0.0, 1.0, 0.01),
                |p| p.translucency
            ),
            number!(PlantParams, "specular", "Sheen", (0.0, 1.0, 0.01), |p| p
                .specular),
        ],
    );
    // The main stem may droop until it arches back to the ground.
    let stem = Group::titled("Stem", segment(2.0)).lift(
        "segment",
        |p: &PlantParams| &p.segment,
        |p| &mut p.segment,
    );

    // Show each branch level's settings only when that level can grow.
    // The second level needs an enabled first level to attach to.
    let first = branches("Branches").lift(
        "levels[0]",
        |p: &PlantParams| &p.levels[0],
        |p| &mut p.levels[0],
    );
    let second = branches("Leaves on Branches")
        .lift(
            "levels[1]",
            |p: &PlantParams| &p.levels[1],
            |p| &mut p.levels[1],
        )
        .shown_if(branched);

    // Heads grow only on unbranched plants, so they come after the branch counts.
    let head_length = Group::titled(
        "Head",
        vec![number!(
            PlantParams,
            "head_length",
            "Length",
            (0.0, 0.85, 0.01),
            |p| p.head_length
        )],
    )
    .shown_if(|p| !branched(p));
    let head_look = Group::plain(vec![
        number!(
            PlantParams,
            "head_width",
            "Width",
            (0.001, 0.2, 0.001),
            |p| p.head_width
        )
        .exponential()
        .revive(0.03),
        srgb!(PlantParams, "head_color", "Colour", |p| p.head_color),
    ])
    .shown_if(has_head);

    let tufts = Group::titled(
        "Tufts",
        vec![
            number!(PlantParams, "tufts", "Tufts", (0.0, 1.0, 0.01), |p| p.tufts),
            number!(
                PlantParams,
                "tuft_size",
                "Tuft Size",
                (0.05, 3.0, 0.01),
                |p| p.tuft_size
            )
            .shown_if(|p| p.tufts > 0.0)
            .exponential()
            .revive(0.6),
        ],
    );

    vec![
        size,
        light,
        stem,
        first,
        second,
        head_length,
        head_look,
        tufts,
    ]
}

/// Accent controls: show the share first, then appearance when the share is positive.
fn accent() -> Group<PlantAccent> {
    let share = number!(PlantAccent, "share", "Share", (0.0, 1.0, 0.01), |a| a.share);
    let size = vec![
        number!(PlantAccent, "height.x", "Shortest", (0.0, 4.0, 0.01), |a| a
            .height
            .x)
        .exponential(),
        number!(PlantAccent, "height.y", "Tallest", (0.01, 4.0, 0.01), |a| a
            .height
            .y)
        .exponential()
        .revive(0.6),
        number!(PlantAccent, "width", "Width", (0.001, 0.2, 0.001), |a| a
            .width)
        .exponential()
        .revive(0.02),
        number!(PlantAccent, "lean", "Lean", (0.0, 1.0, 0.01), |a| a.lean),
        number!(
            PlantAccent,
            "translucency",
            "Translucency",
            (0.0, 1.0, 0.01),
            |a| a.translucency
        ),
    ];
    let shape = segment(2.0)
        .into_iter()
        .map(|param| param.lift("segment", |a: &PlantAccent| &a.segment, |a| &mut a.segment));
    let head = vec![
        number!(
            PlantAccent,
            "head_length",
            "Head Length",
            (0.0, 0.85, 0.01),
            |a| a.head_length
        ),
        number!(
            PlantAccent,
            "head_width",
            "Head Width",
            (0.001, 0.2, 0.001),
            |a| a.head_width
        )
        .shown_if(|a| a.head_length > 0.0)
        .exponential()
        .revive(0.03),
        srgb!(PlantAccent, "head_color", "Head Colour", |a| a.head_color)
            .shown_if(|a| a.head_length > 0.0),
    ];
    let grows = size
        .into_iter()
        .chain(shape)
        .chain(head)
        .map(|param| param.shown_if(|a: &PlantAccent| a.share > 0.0));
    Group::plain(std::iter::once(share).chain(grows).collect())
}

/// Shape, colour and movement settings of a segment that droops by at most
/// `max_droop`.
fn segment(max_droop: f32) -> Vec<Param<PlantSegment>> {
    vec![
        number!(PlantSegment, "profile", "Profile", (0.0, 1.0, 0.01), |s| s
            .profile),
        number!(
            PlantSegment,
            "stiffness",
            "Stiffness",
            (0.0, 1.0, 0.01),
            |s| s.stiffness
        ),
        number!(
            PlantSegment,
            "droop",
            "Droop",
            (0.0, max_droop, 0.01),
            |s| s.droop
        ),
        number!(PlantSegment, "twist", "Twist", (0.0, 2.0, 0.01), |s| s
            .twist),
        number!(PlantSegment, "fold", "Fold", (0.0, 1.0, 0.01), |s| s.fold),
        srgb!(PlantSegment, "root_color", "Root Colour", |s| s.root_color),
        srgb!(PlantSegment, "tip_color", "Tip Colour", |s| s.tip_color),
    ]
}

/// Controls for one branch level, grouped under `title`.
/// Only the count is visible while the level is disabled.
fn branches(title: &'static str) -> Group<PlantBranches> {
    let placement = vec![
        choice!(PlantBranches, "arrangement", "Arrangement", [
            "Opposite" => PlantArrangement::Opposite,
            "Alternate" => PlantArrangement::Alternate,
        ], |b| b.arrangement),
        number!(
            PlantBranches,
            "attachment_start",
            "Bare Base",
            (0.0, 0.95, 0.01),
            |b| b.attachment_start
        ),
        number!(PlantBranches, "length", "Length", (0.0, 3.0, 0.01), |b| b
            .length)
        .exponential()
        .revive(0.5),
        number!(
            PlantBranches,
            "length_envelope",
            "Longest At",
            (0.0, 1.0, 0.01),
            |b| b.length_envelope
        ),
        number!(PlantBranches, "width", "Width", (0.0, 1.0, 0.01), |b| b
            .width)
        .revive(0.25),
        number!(PlantBranches, "angle", "Angle", (0.0, 1.0, 0.01), |b| b
            .angle)
        .revive(0.5),
        number!(PlantBranches, "spread", "Spread", (0.0, 1.0, 0.01), |b| b
            .spread),
    ];
    // Branches droop at most to a hanging tip.
    let look = segment(1.0).into_iter().map(|param| {
        param.lift(
            "segment",
            |b: &PlantBranches| &b.segment,
            |b| &mut b.segment,
        )
    });

    let count = count!(PlantBranches, "count", "Count", (0, 32), |b| b.count);
    let grows = placement
        .into_iter()
        .chain(look)
        .map(|param| param.shown_if(PlantBranches::is_on));
    Group::titled(title, std::iter::once(count).chain(grows).collect())
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2Swizzles;
    use procgena_plant::prelude::{FieldPreset, PlantPreset};

    use super::*;
    use crate::params::checks;

    fn all_params() -> Vec<Param<PlantParams>> {
        sections()
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.params)
            .collect()
    }

    /// Built-in plants and plants from every field preset.
    /// Names identify the source preset when a test fails.
    fn presets() -> Vec<(String, PlantParams)> {
        let plants = PlantPreset::all().map(|preset| (preset.key.to_string(), preset.plant));
        let in_fields = FieldPreset::all().flat_map(|preset| {
            preset
                .field
                .plants
                .iter()
                .enumerate()
                .map(|(i, entry)| (format!("{}.plants[{i}]", preset.key), entry.plant))
                .collect::<Vec<_>>()
        });
        plants.chain(in_fields).collect()
    }

    /// Presets and extra plants covering every conditional control's visibility.
    fn varied_plants() -> Vec<(String, PlantParams)> {
        let mut cases = presets();

        // A plant with no head, tufts or accent.
        let mut plain = PlantParams {
            head_length: 0.0,
            tufts: 0.0,
            ..PlantParams::default()
        };
        plain.accent.share = 0.0;
        cases.push(("plain plant".into(), plain));

        // Plants branched once, and twice.
        let mut branched = plain;
        branched.levels[0].count = 6;
        cases.push(("one branch level".into(), branched));
        branched.levels[1].count = 4;
        cases.push(("two branch levels".into(), branched));

        // A plant with tufts and an accent with a head.
        let mut accented = plain;
        accented.tufts = 0.5;
        accented.accent.share = 0.2;
        accented.accent.head_length = 0.3;
        cases.push(("tufted and accented".into(), accented));
        cases
    }

    /// Checks file-loader acceptance after normalising hidden settings.
    fn file_accepts(mut plant: PlantParams) -> Result<(), String> {
        // Sort the height limits and clear hidden heads and branch counts
        // before this check. The loader also accepts these settings unchanged;
        // this helper therefore tests only the normalised form.
        if plant.height.x > plant.height.y {
            plant.height = plant.height.yx();
        }
        if plant.levels[0].is_on() {
            plant.head_length = 0.0;
        } else {
            plant.levels[1].count = 0;
        }
        PlantParams::from_ron(&plant.to_ron())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn keys_are_unique() {
        checks::keys_are_unique(&all_params());
    }

    #[test]
    fn every_setting_writes_the_field_named_by_its_key() {
        checks::settings_write_their_keys(&all_params(), &varied_plants());
    }

    #[test]
    fn every_range_end_is_valid() {
        checks::range_ends_are_valid(&all_params(), &presets(), file_accepts);
    }

    #[test]
    fn revived_settings_are_valid() {
        checks::revived_values_are_valid(&sections(), &varied_plants(), file_accepts);
    }

    #[test]
    fn adding_a_branch_gives_it_a_usable_shape() {
        // Unbranched presets keep zeros in their hidden branch settings.
        let before = PlantPreset::all()
            .map(|preset| preset.plant)
            .find(|plant| !plant.levels[0].is_on() && plant.levels[0].length == 0.0)
            .expect("an unbranched preset with no branch length");
        let mut after = before;
        after.levels[0].count = 1;
        assert!(crate::params::spec::revive(
            &sections(),
            &before,
            &mut after
        ));
        let branch = after.levels[0];
        assert_eq!(
            (branch.length, branch.width, branch.angle),
            (0.5, 0.25, 0.5)
        );

        // The second level appears under the first and is revived the same way.
        let before = after;
        after.levels[1].count = 1;
        crate::params::spec::revive(&sections(), &before, &mut after);
        assert!(after.levels[1].length > 0.0 && after.levels[1].width > 0.0);
    }

    #[test]
    fn settings_come_after_the_settings_that_show_or_hide_them() {
        checks::settings_follow_what_shows_them(&all_params(), &varied_plants());
    }

    #[test]
    fn no_setting_hides_itself() {
        checks::no_setting_hides_itself(&all_params(), &varied_plants());
    }

    #[test]
    fn presets_fit_the_controls() {
        checks::cases_fit_the_controls(&all_params(), &presets());
    }
}
