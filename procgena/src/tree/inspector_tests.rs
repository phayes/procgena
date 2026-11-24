//! The tree's Inspector settings, built from `tree.meta.yml`, against the
//! tree itself: every range end is valid and builds, revived settings are
//! valid, hidden settings don't change the tree, and settings come after
//! the ones that show or hide them.

use std::collections::HashSet;

use procgena_tree::procgena_fruit::{ClusterKind, FlowerPreset, FruitPreset};
use procgena_tree::{AttachedFruit, TreeOptions, TreePreset, TreeType};

use super::{META, custom};
use crate::params::checks::{self, variants};
use crate::params::{Control, Param, Section};
use crate::studio::inspector::{Settings, sections};

/// The tree's Inspector settings, in display order.
fn all_sections() -> Vec<Section<Settings<TreeOptions>>> {
    let value = procgena_core::preset::to_value(&TreeOptions::default());
    sections(&META, &value, &custom)
}

fn all_params() -> Vec<Param<Settings<TreeOptions>>> {
    all_sections()
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.params)
        .collect()
}

/// Whether any branch grows at `level`: the trunk always does, and a
/// deeper level grows from the one before it, through side branches or, on
/// deciduous trees, the continuation at each tip.
fn grows(o: &TreeOptions, level: usize) -> bool {
    (1..=level).all(|l| {
        l as u32 <= o.branch.levels
            && (o.tree_type == TreeType::Deciduous || o.branch.children[l - 1] > 0)
    })
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
fn every_range_end_is_valid() {
    for param in all_params() {
        let Control::Number { min, max, set, .. } = &param.control else {
            continue;
        };
        for end in [*min, *max] {
            let mut settings = Settings::new(TreeOptions::default());
            set(&mut settings, end);
            assert_eq!(
                settings.options().validate(),
                Ok(()),
                "{} at {end}",
                param.key
            );
        }
    }
}

#[test]
fn revived_settings_are_valid() {
    checks::revived_values_are_valid(&all_sections(), &varied_settings(), |settings| {
        settings
            .options()
            .validate()
            .map_err(|error| error.to_string())
    });
}

#[test]
fn a_new_branch_level_grows_and_can_be_seen() {
    // Raise Levels on every preset that has room, on both tree types.
    let sections = all_sections();
    let levels = all_params()
        .into_iter()
        .find(|p| p.key == "branch.levels")
        .unwrap();
    let Control::Number { set, .. } = &levels.control else {
        panic!("Levels is a number");
    };
    for preset in TreePreset::all() {
        let before = Settings::new(preset.tree.clone());
        let new_level = before.options().branch.levels as usize + 1;
        if new_level > procgena_tree::MAX_LEVELS as usize {
            continue;
        }
        let mut after = before.clone();
        set(&mut after, new_level as f32);
        crate::params::spec::revive(&sections, &before, &mut after);
        let name = &preset.name;
        let after = after.options();
        assert!(grows(after, new_level), "{name}: level {new_level} grows");
        assert!(
            after.branch.length[new_level] > 0.0,
            "{name}: level {new_level} has a length"
        );
    }
}

/// Default options with fruit and flowers growing in clusters of `kinds`,
/// and the trellis switched on.
fn flowering(kinds: (ClusterKind, ClusterKind)) -> TreeOptions {
    let mut options = TreeOptions::default();
    for (slot, cluster, kind) in [
        (&mut options.fruit, &FruitPreset::apple().fruit, kinds.0),
        (
            &mut options.flowers,
            &FlowerPreset::dogwood_bloom().flower,
            kinds.1,
        ),
    ] {
        let mut attached = AttachedFruit::new(cluster.clone(), Default::default());
        attached.placement.count = 2;
        attached.placement.tips = 0.5;
        attached.fruit.cluster.kind = kind;
        attached.fruit.cluster.items = 6;
        *slot = Some(attached);
    }
    options.trellis.enabled = true;
    options
}

/// Builds the most detailed mesh, as the viewer does, or describes the
/// error or panic.
fn build(options: &TreeOptions) -> Result<procgena_tree::TreeMesh, String> {
    let built = std::panic::catch_unwind(|| {
        let skeleton = procgena_tree::Skeleton::generate(options)?;
        Ok::<_, procgena_tree::OptionsError>(
            skeleton.mesh(options, &procgena_tree::default_lod().levels[0].detail),
        )
    });
    match built {
        Ok(Ok(mesh)) => Ok(mesh),
        Ok(Err(error)) => Err(format!("rejected: {error}")),
        Err(_) => Err("panicked".to_owned()),
    }
}

/// Builds the most detailed mesh and describes any error, panic, or
/// non-finite vertex position.
fn build_problem(options: &TreeOptions) -> Option<String> {
    let mesh = match build(options) {
        Ok(mesh) => mesh,
        Err(problem) => return Some(problem),
    };
    let parts = [
        ("branches", &mesh.branches),
        ("leaves", &mesh.leaves),
        ("fruit", &mesh.fruit),
        ("flowers", &mesh.flowers),
    ];
    let broken: Vec<&str> = parts
        .iter()
        .filter(|(_, buffer)| !buffer.positions.iter().flatten().all(|c| c.is_finite()))
        .map(|(name, _)| *name)
        .collect();
    (!broken.is_empty()).then(|| format!("non-finite positions in {broken:?}"))
}

#[test]
fn every_range_end_builds_a_tree() {
    let bases = [
        ("plain", TreeOptions::default()),
        (
            "panicle/umbel",
            flowering((ClusterKind::Panicle, ClusterKind::Umbel)),
        ),
        (
            "catkin/raceme",
            flowering((ClusterKind::Catkin, ClusterKind::Raceme)),
        ),
    ];
    // Each thread checks every `threads`-th setting, with its own copy of
    // the settings table.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut problems: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|first| {
                let bases = &bases;
                scope.spawn(move || {
                    let mut problems = Vec::new();
                    let params = all_params().into_iter().skip(first).step_by(threads);
                    for param in params {
                        let Control::Number { min, max, set, .. } = &param.control else {
                            continue;
                        };
                        // Fruit and flower settings only matter where they grow.
                        let clusters =
                            param.key.starts_with("fruit.") || param.key.starts_with("flowers.");
                        for (base_name, base) in bases {
                            if *base_name != "plain" && !clusters {
                                continue;
                            }
                            for end in [*min, *max] {
                                let mut settings = Settings::new(base.clone());
                                set(&mut settings, end);
                                if let Some(problem) = build_problem(settings.options()) {
                                    let key = &param.key;
                                    problems
                                        .push(format!("{base_name}: {key} at {end}: {problem}"));
                                }
                            }
                        }
                    }
                    problems
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    problems.sort();

    // Dragging one growth direction slider through zero can zero the whole vector.
    let mut options = flowering((ClusterKind::Single, ClusterKind::Single));
    options.branch.force.direction *= 0.0;
    if let Some(problem) = build_problem(&options) {
        problems.push(format!("zero growth direction: {problem}"));
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Settings that between them hide every kind of setting, named for
/// messages.
fn varied_settings() -> Vec<(String, Settings<TreeOptions>)> {
    use ClusterKind::{Catkin, Panicle, Raceme, Single, Umbel};
    let mut cases: Vec<(String, TreeOptions)> = TreePreset::all()
        .map(|preset| (preset.name.to_string(), preset.tree.clone()))
        .collect();
    cases.push(("panicle/umbel".into(), flowering((Panicle, Umbel))));
    cases.push(("catkin/catkin".into(), flowering((Catkin, Catkin))));

    // Bloom and ripeness at both ends.
    for maturity in [0.0, 1.0] {
        let mut options = flowering((Single, Raceme));
        for attached in [&mut options.fruit, &mut options.flowers]
            .into_iter()
            .flatten()
        {
            attached.fruit.maturity = maturity;
        }
        cases.push((format!("maturity {maturity}"), options));
    }

    // Levels that only grow by tip continuation, or not at all.
    let mut bare = TreeOptions::default();
    bare.branch.children[0] = 0;
    cases.push(("deciduous, no side branches".into(), bare.clone()));
    bare.tree_type = TreeType::Evergreen;
    cases.push(("evergreen trunk only".into(), bare));
    cases
        .into_iter()
        .map(|(name, options)| (name, Settings::new(options)))
        .collect()
}

/// The mesh parts that differ by more than rounding error.
fn mesh_differences(a: &procgena_tree::TreeMesh, b: &procgena_tree::TreeMesh) -> Vec<&'static str> {
    fn close<const N: usize>(a: &[[f32; N]], b: &[[f32; N]]) -> bool {
        a.len() == b.len()
            && a.iter()
                .flatten()
                .zip(b.iter().flatten())
                .all(|(x, y)| (x - y).abs() <= 1e-4)
    }
    fn parts(m: &procgena_tree::TreeMesh) -> [(&'static str, &procgena_tree::MeshBuffers); 4] {
        [
            ("branches", &m.branches),
            ("leaves", &m.leaves),
            ("fruit", &m.fruit),
            ("flowers", &m.flowers),
        ]
    }
    parts(a)
        .into_iter()
        .zip(parts(b))
        .filter(|((_, x), (_, y))| {
            !(x.indices == y.indices
                && close(&x.positions, &y.positions)
                && close(&x.normals, &y.normals)
                && close(&x.uvs, &y.uvs)
                && close(&x.tangents, &y.tangents)
                && close(&x.wind, &y.wind)
                && close(&x.colors, &y.colors))
        })
        .map(|((name, _), _)| name)
        .collect()
}

/// Material-only settings, such as bark texture and trellis wires, cannot
/// be checked this way and always pass.
#[test]
fn hidden_settings_do_not_change_the_tree() {
    let cases = varied_settings();
    // Each thread checks every `threads`-th case, with its own copy of the
    // settings table.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut problems: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|first| {
                let cases = &cases;
                scope.spawn(move || {
                    let params = all_params();
                    let mut problems = Vec::new();
                    for (name, settings) in cases.iter().skip(first).step_by(threads) {
                        let mesh = match build(settings.options()) {
                            Ok(mesh) => mesh,
                            Err(problem) => {
                                problems.push(format!("{name}: {problem}"));
                                continue;
                            }
                        };
                        let hidden = params.iter().filter(|p| !p.is_shown(settings));
                        for param in hidden {
                            for changed in variants(param, settings) {
                                let differences: Vec<String> = match build(changed.options()) {
                                    Ok(changed) => mesh_differences(&mesh, &changed)
                                        .into_iter()
                                        .map(str::to_owned)
                                        .collect(),
                                    Err(problem) => vec![problem],
                                };
                                if !differences.is_empty() {
                                    problems.push(format!(
                                        "{name}: hidden {} changes {differences:?}",
                                        param.key
                                    ));
                                }
                            }
                        }
                    }
                    problems
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn settings_come_after_the_settings_that_show_or_hide_them() {
    checks::settings_follow_what_shows_them(&all_params(), &varied_settings());
}

#[test]
fn no_setting_hides_itself() {
    checks::no_setting_hides_itself(&all_params(), &varied_settings());
}

#[test]
fn a_tree_without_fruit_still_has_its_fruit_and_flower_settings() {
    // The settings come from a tree with neither, so the range-end and
    // hidden-setting tests above reach these too.
    assert!(TreeOptions::default().fruit.is_none());
    let keys: Vec<String> = all_params().into_iter().map(|p| p.key).collect();
    for key in ["fruit.placement.count", "flowers.default_slots"] {
        assert!(keys.iter().any(|k| k == key), "no setting {key}");
    }
}

#[test]
fn every_tree_part_has_settings_and_every_setting_a_part() {
    let params = all_params();
    for (key, _) in &META.meta.parts {
        assert!(
            params.iter().any(|p| p.part == Some(key.as_str())),
            "part {key} has no setting in the Inspector"
        );
    }
    for param in &params {
        assert!(param.part.is_some(), "{} targets no part", param.key);
    }
}
