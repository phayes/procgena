//! Check skeleton structure, repeatability, growth settings and option validation.

use glam::Vec3;
use procgena_tree::{OptionsError, PerLevel, Section, Skeleton, TreeOptions, TreePreset, TreeType};

/// Returns labelled settings to grow: every preset, plus the defaults with each
/// tree type, fewer branching levels, and several seeds.
fn cases() -> Vec<(String, TreeOptions)> {
    let mut cases: Vec<(String, TreeOptions)> = TreePreset::all()
        .map(|p| (p.name.to_string(), p.tree.clone()))
        .collect();
    for tree_type in [TreeType::Deciduous, TreeType::Evergreen] {
        for levels in 0..=3 {
            for seed in [-7, 1, 42] {
                let mut options = TreeOptions {
                    tree_type,
                    ..TreeOptions::default()
                };
                options.branch.levels = levels;
                options.default_seed = seed;
                cases.push((
                    format!("{tree_type:?} levels {levels} seed {seed}"),
                    options,
                ));
            }
        }
    }
    cases
}

#[test]
fn hierarchy_is_consistent() {
    for (case, options) in cases() {
        let skeleton = Skeleton::generate(&options).unwrap();
        let levels = options.branch.levels;

        // Require a root trunk at index 0. Every other branch is either an extra
        // trunk of a multi-stemmed tree or has a valid parent.
        let trunk = &skeleton.branches[0];
        assert_eq!((trunk.level, trunk.parent), (0, None), "{case}: trunk");

        for (i, branch) in skeleton.branches.iter().enumerate().skip(1) {
            let Some(at) = branch.parent else {
                assert_eq!(branch.level, 0, "{case}: branch {i} has no parent");
                continue;
            };
            assert!(at.branch < i, "{case}: branch {i} grows before its parent");
            let parent = &skeleton.branches[at.branch];
            assert_eq!(branch.level, parent.level + 1, "{case}: branch {i} level");
            assert!(branch.level <= levels, "{case}: branch {i} is too deep");

            // Reconstruct the attachment position and check the branch base within tolerance.
            let (origin, _) = attachment_point(&parent.sections, at.section, at.t);
            assert!(
                branch.sections[0].origin.abs_diff_eq(origin, 1e-5),
                "{case}: branch {i} starts away from its attachment"
            );
        }

        for (i, leaf) in skeleton.leaves.iter().enumerate() {
            let parent = &skeleton.branches[leaf.parent.branch];
            assert_eq!(
                parent.level, levels,
                "{case}: leaf {i} is not on the deepest level"
            );
            let (origin, _) =
                attachment_point(&parent.sections, leaf.parent.section, leaf.parent.t);
            assert!(
                leaf.origin.abs_diff_eq(origin, 1e-5),
                "{case}: leaf {i} sits away from its attachment"
            );
        }
    }
}

/// Reconstructs an attachment's position and radius from its parent sections.
/// Requires a valid `index` and `t` in 0..=1; the final section stays at the tip.
fn attachment_point(sections: &[Section], index: usize, t: f32) -> (Vec3, f32) {
    assert!((0.0..=1.0).contains(&t), "attachment t {t} out of range");
    let a = &sections[index];
    let b = sections.get(index + 1).unwrap_or(a);
    (
        a.origin.lerp(b.origin, t),
        (1.0 - t) * a.radius + t * b.radius,
    )
}

#[test]
fn generation_is_repeatable() {
    let options = TreeOptions::default();
    assert_eq!(Skeleton::generate(&options), Skeleton::generate(&options));
}

#[test]
fn deciduous_tips_continue_and_evergreen_tips_end() {
    // With one side leaf, a deciduous trunk at the deepest configured level also
    // gets one terminal leaf, recorded before its side leaf.
    let mut options = TreeOptions::default();
    options.branch.levels = 0;
    options.leaves.count = 1;

    let deciduous = Skeleton::generate(&options).unwrap();
    assert_eq!(deciduous.branches.len(), 1);
    assert_eq!(deciduous.leaves.len(), 2);
    let tip = deciduous.branches[0].sections.last().unwrap();
    assert_eq!(deciduous.leaves[0].origin, tip.origin);

    options.tree_type = TreeType::Evergreen;
    options.branch.levels = 1;
    options.branch.children[0] = 0;
    let evergreen = Skeleton::generate(&options).unwrap();
    assert_eq!(evergreen.branches.len(), 1);
    assert!(evergreen.leaves.is_empty());
    // This evergreen trunk is below the deepest configured level and has no children.
    // Its tip radius is exactly 0, unlike the 0.001 used at the deepest level.
    assert_eq!(evergreen.branches[0].sections.last().unwrap().radius, 0.0);
}

#[test]
fn deciduous_branches_tapered_to_a_point_end_there() {
    // A trunk with no side branches, tapered to a point, has nothing growing from it.
    let mut options = TreeOptions::default();
    options.branch.children[0] = 0;
    options.branch.taper[0] = 1.0;
    let skeleton = Skeleton::generate(&options).unwrap();
    assert_eq!(skeleton.branches.len(), 1);
    assert!(skeleton.leaves.is_empty());

    // Full taper at any level still grows a finite tree.
    for level in 0..3 {
        let mut options = TreeOptions::default();
        options.branch.taper[level] = 1.0;
        let skeleton = Skeleton::generate(&options).unwrap();
        for section in skeleton.branches.iter().flat_map(|b| &b.sections) {
            assert!(
                section.origin.is_finite() && section.orientation.is_finite(),
                "level {level}: {section:?}"
            );
        }
        for leaf in &skeleton.leaves {
            assert!(leaf.origin.is_finite(), "level {level}: {leaf:?}");
        }
    }
}

/// A leaf count of zero removes every leaf, including tip leaves, for both
/// tree types, while branches and fruit still grow.
#[test]
fn zero_leaf_count_grows_no_leaves() {
    for tree_type in [TreeType::Deciduous, TreeType::Evergreen] {
        let mut options = TreeOptions {
            tree_type,
            ..TreeOptions::default()
        };
        options.fruit = Some(procgena_tree::AttachedFruit::default());
        options.leaves.count = 0;
        let bare = Skeleton::generate(&options).unwrap();
        assert!(bare.leaves.is_empty(), "{tree_type:?}");
        assert!(bare.branches.len() > 1, "{tree_type:?}");
        assert!(!bare.fruit.is_empty(), "{tree_type:?}");
    }
}

/// Flattening one level levels off its branches without moving other levels
/// or changing how many branches, sections and leaves grow.
#[test]
fn flatten_levels_off_only_its_own_level() {
    // Without the upward force or random bending, nothing competes with the pull
    // toward horizontal.
    let mut options = TreeOptions::default();
    options.branch.force.strength = 0.0;
    options.branch.gnarliness = PerLevel([0.0; 4]);
    let mut flat_options = options.clone();
    flat_options.branch.flatten[2] = 0.2;
    let base = Skeleton::generate(&options).unwrap();
    let flat = Skeleton::generate(&flat_options).unwrap();

    // Same structure: flattening draws no random numbers.
    assert_eq!(flat.branches.len(), base.branches.len());
    assert_eq!(flat.leaves.len(), base.leaves.len());
    for (a, b) in flat.branches.iter().zip(&base.branches) {
        assert_eq!(a.level, b.level);
        assert_eq!(a.sections.len(), b.sections.len());
        if a.level < 2 {
            assert_eq!(a, b);
        }
    }

    // Level-2 branches finish nearly level, well below their unflattened steepness.
    let mean_tip_steepness = |skeleton: &Skeleton| {
        let tips: Vec<f32> = skeleton
            .branches
            .iter()
            .filter(|b| b.level == 2)
            .map(|b| (b.sections.last().unwrap().orientation * Vec3::Y).y.abs())
            .collect();
        tips.iter().sum::<f32>() / tips.len() as f32
    };
    let (flat_steepness, base_steepness) = (mean_tip_steepness(&flat), mean_tip_steepness(&base));
    assert!(
        flat_steepness < 0.15 && flat_steepness < 0.5 * base_steepness,
        "flattened {flat_steepness} vs unflattened {base_steepness}"
    );
}

/// Flattening leaves turns them level and face up without moving them or
/// changing how many branches and leaves grow; half the amount goes partway.
#[test]
fn leaf_flatten_lays_leaves_level_and_face_up() {
    let options = TreePreset::oak_medium().tree.clone();
    let base = Skeleton::generate(&options).unwrap();
    let with_flatten = |amount: f32| {
        let mut flat_options = options.clone();
        flat_options.leaves.flatten = amount;
        Skeleton::generate(&flat_options).unwrap()
    };
    let (flat, half) = (with_flatten(1.0), with_flatten(0.5));

    // Same structure and leaf positions: flattening draws no random numbers.
    assert_eq!(flat.branches, base.branches);
    assert_eq!(flat.leaves.len(), base.leaves.len());
    for (a, b) in flat.leaves.iter().zip(&base.leaves) {
        assert_eq!((a.parent, a.origin, a.size), (b.parent, b.origin, b.size));
    }

    // Fully flattened leaves lie level, face straight up, and keep the
    // horizontal heading of their original tip.
    for (a, b) in flat.leaves.iter().zip(&base.leaves) {
        let tip = a.orientation * Vec3::Y;
        assert!(tip.y.abs() < 1e-4, "tip {tip}");
        assert!((a.orientation * Vec3::Z).y > 0.9999);
        let old_tip = b.orientation * Vec3::Y;
        let old_heading = Vec3::new(old_tip.x, 0.0, old_tip.z);
        if old_heading.length() > 0.1 {
            assert!(
                tip.dot(old_heading.normalize()) > 0.9999,
                "{tip} vs {old_tip}"
            );
        }
    }

    // Half flattening faces leaves further upward than the original on average.
    let mean_face_up = |skeleton: &Skeleton| {
        let sum: f32 = skeleton
            .leaves
            .iter()
            .map(|l| (l.orientation * Vec3::Z).y)
            .sum();
        sum / skeleton.leaves.len() as f32
    };
    let (base_up, half_up) = (mean_face_up(&base), mean_face_up(&half));
    assert!(
        half_up > base_up + 0.2 && half_up < 0.99,
        "{base_up} -> {half_up}"
    );
}

/// A multi-stemmed tree grows the requested number of parentless trunks, all
/// starting near the origin and tilted no more than the stem angle.
#[test]
fn stems_grow_separate_trunks_from_the_base() {
    let mut options = TreeOptions::default();
    options.branch.stems = 6;
    options.branch.stem_angle = 30.0;
    let skeleton = Skeleton::generate(&options).unwrap();

    let trunks: Vec<_> = skeleton
        .branches
        .iter()
        .filter(|b| b.parent.is_none())
        .collect();
    assert_eq!(trunks.len(), 6);
    // Trunks come first, all at level 0.
    assert!(
        skeleton.branches[..6]
            .iter()
            .all(|b| b.parent.is_none() && b.level == 0)
    );

    for trunk in trunks {
        let base = trunk.sections[0];
        assert!(base.origin.y.abs() < 1e-6, "{base:?}");
        assert!(
            base.origin.length() <= options.branch.radius[0] + 1e-4,
            "{base:?}"
        );
        let tilt = (base.orientation * Vec3::Y)
            .angle_between(Vec3::Y)
            .to_degrees();
        assert!(tilt <= 30.0 + 1e-3, "tilt {tilt}");
    }

    // Different trunks lean in different directions.
    let first = skeleton.branches[0].sections[0].orientation * Vec3::Y;
    let second = skeleton.branches[1].sections[0].orientation * Vec3::Y;
    assert!(first.angle_between(second) > 1e-3);
}

/// One stem with any stem angle grows the same tree as the defaults.
#[test]
fn single_stem_ignores_stem_angle() {
    let reference = Skeleton::generate(&TreeOptions::default()).unwrap();
    let mut options = TreeOptions::default();
    options.branch.stem_angle = 45.0;
    assert_eq!(Skeleton::generate(&options).unwrap(), reference);
}

/// Rejects zero stems and stem angles outside 0–90 degrees.
#[test]
fn invalid_stem_settings_are_rejected() {
    let mut options = TreeOptions::default();
    options.branch.stems = 0;
    assert!(options.validate().is_err());
    for bad in [-1.0, 91.0, f32::NAN] {
        let mut options = TreeOptions::default();
        options.branch.stem_angle = bad;
        assert!(options.validate().is_err(), "{bad} accepted");
    }
}

#[test]
fn invalid_options_are_rejected() {
    let mut options = TreeOptions::default();
    options.branch.sections[0] = 0;
    assert_eq!(
        Skeleton::generate(&options),
        Err(OptionsError::ZeroSections { level: 0 })
    );
}
