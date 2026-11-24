use std::collections::HashMap;

use glam::{DVec2, Vec2, Vec3};
use procgena_tree::{
    FlatGround, Ground, MAX_SCATTER_TREES, Mask, PresetFileError, ScatterArea, ScatterInput,
    ScatterTree, Tree, TreeCopy, TreePreset, TreeScatter, TreeScatterError, TreeScatterPlacement,
    TreeScatterPreset,
};

/// Places `scatter` with its own seed over a `size` rectangle centred on the
/// World origin, on `ground`, thinned by `mask` if given.
fn placed(
    scatter: &TreeScatter,
    size: [f32; 2],
    ground: &dyn Ground,
    mask: Option<&dyn Mask>,
) -> Result<TreeScatterPlacement, TreeScatterError> {
    let area = ScatterArea::new(DVec2::ZERO, Vec2::from(size));
    let mut input = ScatterInput::new(ground, area, scatter.default_seed);
    if let Some(mask) = mask {
        input = input.with_mask(mask);
    }
    TreeScatterPlacement::new(scatter, &input)
}

fn place(scatter: &TreeScatter, size: f32) -> Vec<TreeCopy> {
    placed(scatter, [size, size], &FlatGround, None)
        .unwrap()
        .instances()
        .to_vec()
}

fn by_id(instances: &[TreeCopy]) -> HashMap<u64, TreeCopy> {
    instances.iter().map(|i| (i.id, *i)).collect()
}

/// The Scatter of a built-in Scatter preset.
fn preset_scatter(preset: &TreeScatterPreset) -> TreeScatter {
    preset.tree_scatter.clone()
}

/// A tree preset's stand, which every built-in tree preset has.
fn stand(preset: &TreePreset) -> procgena_tree::TreeStand {
    preset
        .stand
        .expect("every built-in tree preset has a stand")
}

/// Two species of equal density, with no spacing, so placement follows
/// density alone.
fn even_pair() -> TreeScatter {
    TreeScatter::new(vec![
        ScatterTree::custom(TreePreset::oak_small().tree.clone(), 300.0, 0.0).with_variants(2),
        ScatterTree::custom(TreePreset::pine_small().tree.clone(), 300.0, 0.0),
    ])
}

#[test]
fn the_same_scatter_places_the_same_trees() {
    let scatter = preset_scatter(TreeScatterPreset::mixed_deciduous());
    let a = place(&scatter, 50.0);
    assert!(!a.is_empty());
    assert_eq!(a, place(&scatter, 50.0));
}

#[test]
fn lowering_density_under_a_density_function_only_removes_trees() {
    let scatter = preset_scatter(TreeScatterPreset::pine_stand());
    let fade = |p: DVec2| (p.x as f32 / 40.0 + 0.5).clamp(0.0, 1.0);
    let at = |scatter: &TreeScatter| {
        placed(scatter, [40.0, 40.0], &FlatGround, Some(&fade))
            .unwrap()
            .instances()
            .to_vec()
    };
    let full = by_id(&at(&scatter));
    let sparse = at(&scatter.clone().with_density(scatter.density() * 0.4));
    assert!(!sparse.is_empty() && sparse.len() < full.len());
    for tree in &sparse {
        assert_eq!(
            full.get(&tree.id),
            Some(tree),
            "a kept tree moved or changed"
        );
    }
}

#[test]
fn lowering_density_only_removes_trees() {
    let scatter = preset_scatter(TreeScatterPreset::pine_stand());
    let full = by_id(&place(&scatter, 40.0));
    let sparse = place(&scatter.clone().with_density(scatter.density() * 0.4), 40.0);
    assert!(sparse.len() < full.len());
    assert!(!sparse.is_empty());
    for tree in &sparse {
        assert_eq!(
            full.get(&tree.id),
            Some(tree),
            "a kept tree moved or changed"
        );
    }
}

#[test]
fn raising_patchiness_only_removes_trees() {
    let mut scatter = preset_scatter(TreeScatterPreset::oak_woodland());
    let mut previous = by_id(&place(&scatter, 100.0));
    for patchiness in [0.3, 0.6, 1.0] {
        scatter.patchiness = patchiness;
        let trees = place(&scatter, 100.0);
        assert!(
            trees.len() < previous.len(),
            "patchiness {patchiness} kept every tree"
        );
        for tree in &trees {
            assert_eq!(
                previous.get(&tree.id),
                Some(tree),
                "a kept tree moved or changed"
            );
        }
        previous = by_id(&trees);
    }
    assert!(!previous.is_empty(), "full patchiness left no woods at all");
}

#[test]
fn growing_the_size_keeps_the_trees_already_there() {
    let scatter = preset_scatter(TreeScatterPreset::oak_woodland());
    let small = place(&scatter, 30.0);
    let large = by_id(&place(&scatter, 60.0));
    // Trees well inside the smaller square have no new neighbours that
    // could crowd them out.
    for tree in small
        .iter()
        .filter(|t| t.position.x.abs() < 9.0 && t.position.z.abs() < 9.0)
    {
        assert_eq!(large.get(&tree.id), Some(tree));
    }
}

#[test]
fn trees_keep_their_spacing() {
    let scatter = preset_scatter(TreeScatterPreset::oak_woodland());
    let trees = place(&scatter, 60.0);
    for (n, a) in trees.iter().enumerate() {
        for b in &trees[n + 1..] {
            let apart = scatter.trees[a.tree]
                .spacing
                .max(scatter.trees[b.tree].spacing);
            let d = (a.position - b.position).with_y(0.0).length();
            assert!(d >= apart - 1e-4, "trees {d} m apart, need {apart} m");
        }
    }
}

#[test]
fn density_sets_the_number_of_trees_and_species_shares() {
    let scatter = even_pair();
    let trees = place(&scatter, 100.0);
    // 600 trees per hectare over one hectare.
    assert!(
        (trees.len() as f32 - 600.0).abs() < 60.0,
        "{} trees",
        trees.len()
    );
    let oaks = trees.iter().filter(|t| t.tree == 0).count() as f32;
    assert!((oaks / trees.len() as f32 - 0.5).abs() < 0.06);
    assert!(
        trees
            .iter()
            .all(|t| t.variant < scatter.trees[t.tree].variants)
    );
}

#[test]
fn the_last_species_fills_the_gaps() {
    let mut scatter = even_pair();
    scatter.trees[0].density_scale = 0.0;
    let trees = place(&scatter, 50.0);
    assert!(!trees.is_empty());
    assert!(trees.iter().all(|t| t.tree == 1));
}

#[test]
fn clustering_gathers_a_species_into_stands() {
    // The share of a tree's nearest neighbours that are its own species.
    let same_neighbours = |clustering: f32| {
        let mut scatter = even_pair();
        scatter.trees[0].clustering = clustering;
        let trees = place(&scatter, 100.0);
        let oaks: Vec<Vec3> = trees
            .iter()
            .filter(|t| t.tree == 0)
            .map(|t| t.position)
            .collect();
        let mut same = 0;
        for &oak in &oaks {
            let nearest = trees
                .iter()
                .filter(|t| t.position != oak)
                .min_by(|a, b| {
                    a.position
                        .distance(oak)
                        .total_cmp(&b.position.distance(oak))
                })
                .unwrap();
            same += usize::from(nearest.tree == 0);
        }
        same as f32 / oaks.len() as f32
    };
    assert!(same_neighbours(1.0) > same_neighbours(0.0) + 0.15);
}

#[test]
fn clustering_keeps_a_small_species_share() {
    // A species at 15% of the mix keeps about that share however clustered.
    for clustering in [0.0, 0.6, 1.0] {
        let scatter = TreeScatter::new(vec![
            ScatterTree::custom(TreePreset::ash_medium().tree.clone(), 150.0, 0.0)
                .clustered(clustering),
            ScatterTree::custom(TreePreset::oak_medium().tree.clone(), 850.0, 0.0),
        ]);
        let trees = place(&scatter, 60.0);
        let ash = trees.iter().filter(|t| t.tree == 0).count() as f32;
        let share = ash / trees.len() as f32;
        assert!(
            (share - 0.15).abs() < 0.04,
            "share {share} at clustering {clustering}"
        );
    }
}

#[test]
fn spacing_leaves_most_of_a_presets_density() {
    // Each preset alone keeps most of its stand's density after spacing.
    for preset in TreePreset::all().filter(|p| stand(p).density > 0.0) {
        let trees = place(&TreeScatter::new(vec![ScatterTree::new(preset)]), 60.0);
        let kept = trees.len() as f32 / (stand(preset).density * 0.36);
        assert!(kept > 0.75, "{} keeps {kept} of its density", preset.name);
    }
}

#[test]
fn missing_and_steep_ground_leave_gaps() {
    let scatter = even_pair();
    let half_missing = |p: DVec2| if p.x < 0.0 { f32::NAN } else { 0.0 };
    let placement = placed(&scatter, [40.0, 40.0], &half_missing, None).unwrap();
    assert!(!placement.instances().is_empty());
    assert!(placement.instances().iter().all(|t| t.position.x > 0.0));

    // A 45° slope is steeper than the default 35° limit.
    let steep = |p: DVec2| p.x as f32;
    let placement = placed(&scatter, [40.0, 40.0], &steep, None).unwrap();
    assert!(placement.instances().is_empty());
}

#[test]
fn trees_stand_on_the_ground_and_sink_below_it() {
    let mut scatter = even_pair();
    scatter.variation.lean = 0.0;
    let gentle = |x: f32, z: f32| 0.1 * x + 0.05 * z + 3.0;
    let ground = |p: DVec2| gentle(p.x as f32, p.y as f32);
    let placement = placed(&scatter, [30.0, 30.0], &ground, None).unwrap();
    for tree in placement.instances() {
        let p = tree.position;
        let below = gentle(p.x, p.z) - p.y;
        assert!(below >= scatter.variation.sink - 1e-4 && below < scatter.variation.sink + 0.2);
        assert_eq!(tree.transform.translation, p.into());
    }
}

#[test]
fn each_species_can_have_its_own_slope_limit() {
    // A 40° slope: steeper than the Scatter's 35° limit, gentler than the 45°
    // shrubs tolerate.
    let tan = 40f32.to_radians().tan();
    let slope = |p: DVec2| tan * p.x as f32;
    let alone = |tree: ScatterTree| {
        let scatter = TreeScatter::new(vec![tree]);
        placed(&scatter, [30.0, 30.0], &slope, None)
            .unwrap()
            .instances()
            .len()
    };
    assert_eq!(stand(TreePreset::bush_2()).max_slope, Some(45.0));
    assert!(
        alone(ScatterTree::new(TreePreset::bush_2())) > 0,
        "shrubs cling to the slope"
    );
    assert_eq!(alone(ScatterTree::new(TreePreset::oak_medium())), 0);

    // A species' own limit replaces the Scatter's either way.
    let mut oak = ScatterTree::new(TreePreset::oak_medium());
    oak.max_slope = Some(42.0);
    assert!(alone(oak) > 0);
    let mut bush = ScatterTree::new(TreePreset::bush_2());
    bush.max_slope = None;
    assert_eq!(alone(bush), 0);

    // Out of range is an error.
    let mut bad = ScatterTree::new(TreePreset::bush_2());
    bad.max_slope = Some(91.0);
    assert!(TreeScatter::new(vec![bad]).validate().is_err());
}

#[test]
fn trees_on_a_diagonal_slope_stand_below_its_lowest_side() {
    // A slope falling towards -X and -Z, so the lowest ground around a trunk
    // lies between the X and Z axes.
    let mut scatter = TreeScatter::new(vec![ScatterTree::new(TreePreset::oak_large())]);
    scatter.variation.lean = 0.0;
    scatter.variation.sink = 0.0;
    let tan = 25f32.to_radians().tan();
    let diagonal = |x: f32, z: f32| tan * (x + z) * std::f32::consts::FRAC_1_SQRT_2;
    let ground = |p: DVec2| diagonal(p.x as f32, p.y as f32);
    let placement = placed(&scatter, [40.0, 40.0], &ground, None).unwrap();
    assert!(!placement.instances().is_empty());
    let trunk = TreePreset::oak_large().tree.branch.radius[0];
    for tree in placement.instances() {
        let p = tree.position;
        let radius = trunk * Vec3::from(tree.transform.matrix3.x_axis).length();
        let d = radius * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            p.y <= diagonal(p.x - d, p.z - d) + 1e-4,
            "the foot floats downhill"
        );
    }
}

#[test]
fn a_mask_thins_the_scatter() {
    let scatter = even_pair();
    let east_only = |p: DVec2| if p.x > 0.0 { 1.0 } else { 0.0 };
    let placement = placed(&scatter, [40.0, 40.0], &FlatGround, Some(&east_only)).unwrap();
    assert!(!placement.instances().is_empty());
    assert!(placement.instances().iter().all(|t| t.position.x > 0.0));
}

/// An area away from the origin places the same trees, relative to its
/// centre, as one at the origin, and asks the ground and the mask at World
/// positions.
#[test]
fn trees_are_placed_relative_to_the_area_centre() {
    let scatter = even_pair();
    let centre = DVec2::new(1000.0, -500.0);
    let at_origin = place(&scatter, 40.0);

    // Level ground 2 m up that exists only far from the origin, so trees
    // stand only if it is asked at World positions, and a mask keeping the
    // area's east half.
    let ground = |p: DVec2| if p.x > 500.0 { 2.0 } else { f32::NAN };
    let east = |p: DVec2| if p.x > centre.x { 1.0 } else { 0.0 };
    let area = ScatterArea::new(centre, Vec2::splat(40.0));
    let input = ScatterInput::new(&ground, area, scatter.default_seed).with_mask(&east);
    let moved = TreeScatterPlacement::new(&scatter, &input).unwrap();

    // The same sites, east half only, standing on the raised ground.
    let expected: Vec<_> = at_origin
        .iter()
        .filter(|t| t.position.x > 0.0)
        .map(|t| t.id)
        .collect();
    let ids: Vec<_> = moved.instances().iter().map(|t| t.id).collect();
    assert!(!ids.is_empty());
    assert_eq!(ids, expected);
    for tree in moved.instances() {
        assert!(tree.position.x > 0.0);
        assert!((tree.position.y - (2.0 - scatter.variation.sink)).abs() < 1e-5);
    }
}

#[test]
fn variants_differ_only_in_their_seed() {
    // Every variant, the first included, takes its specimen's seed.
    let oak = ScatterTree::new(TreePreset::oak_medium());
    let seeds: Vec<i32> = (0..3).map(|v| oak.variant(v).default_seed).collect();
    assert!(!seeds.contains(&oak.tree.default_seed), "{seeds:?}");
    assert!(seeds[0] != seeds[1] && seeds[1] != seeds[2] && seeds[0] != seeds[2]);
    for v in 0..3 {
        let mut other = oak.variant(v);
        assert_eq!(
            other.default_seed as u32,
            oak.tree.specimen_id(oak.tree.default_seed as u32, v).seed()
        );
        other.default_seed = oak.tree.default_seed;
        assert_eq!(other, oak.tree);
    }

    // Changing the Scatter's seed moves trees but keeps the variants.
    let mut scatter = preset_scatter(TreeScatterPreset::pine_stand());
    let before = scatter.trees[0].variant(2);
    scatter.default_seed += 1;
    assert_eq!(scatter.trees[0].variant(2), before);
}

#[test]
fn bad_settings_and_too_many_trees_are_errors() {
    let mut scatter = even_pair();
    scatter.trees[1].clustering = 2.0;
    assert!(matches!(
        placed(&scatter, [10.0, 10.0], &FlatGround, None),
        Err(TreeScatterError::Options(_))
    ));

    let mut scatter = even_pair();
    scatter.trees[0].tree.branch.levels = 9;
    assert!(matches!(
        scatter.validate(),
        Err(TreeScatterError::Tree { index: 0, .. })
    ));

    let scatter = even_pair().with_trees(vec![
        ScatterTree::new(TreePreset::apple());
        MAX_SCATTER_TREES + 1
    ]);
    assert!(matches!(
        scatter.validate(),
        Err(TreeScatterError::TooManyTrees { .. })
    ));

    assert!(matches!(
        placed(&even_pair(), [1000.0, 1000.0], &FlatGround, None),
        Err(TreeScatterError::TooMany { .. })
    ));
    assert!(placed(&even_pair(), [0.0, 10.0], &FlatGround, None).is_err());
}

#[test]
fn every_preset_places_trees_and_has_a_unique_key() {
    for preset in TreeScatterPreset::all() {
        assert_eq!(TreeScatterPreset::named(&preset.key), Some(preset));
        let scatter = preset_scatter(preset);
        scatter.validate().unwrap();
        assert!(
            !place(&scatter, 40.0).is_empty(),
            "{} placed no trees",
            preset.name
        );
    }
    for preset in TreePreset::all() {
        assert_eq!(TreePreset::named(&preset.key), Some(preset));
    }
}

#[test]
fn preset_spacing_suits_the_crown() {
    // Spacing should keep trunks apart without leaving the crowns far from
    // touching: between about half and the whole crown radius.
    for preset in TreePreset::all() {
        let tree = Tree::from_preset(preset).unwrap();
        let crown = tree
            .mesh()
            .branches
            .positions
            .iter()
            .chain(&tree.mesh().leaves.positions)
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0, f32::max);
        let spacing = stand(preset).spacing;
        assert!(
            spacing >= 0.5 * crown && spacing <= 1.2 * crown,
            "{}: spacing {spacing} m for a crown radius of {crown} m",
            preset.name
        );
    }
}

/// The guide in `docs/scatters.md`, which these tests keep in step with the code.
const GUIDE: &str = include_str!("../../../docs/scatters.md");

#[test]
fn the_guides_scatter_file_is_valid() {
    let start = GUIDE.find("```yaml\n").expect("a YAML example") + "```yaml\n".len();
    let end = start + GUIDE[start..].find("```").unwrap();
    let preset = TreeScatterPreset::from_yaml("guide", &GUIDE[start..end]).unwrap();
    let scatter = preset.tree_scatter;
    scatter.validate().unwrap();
    assert_eq!(scatter.trees.len(), 3);
    assert_eq!(scatter.trees[2].density, 300.0);
}

#[test]
fn the_guides_preset_table_matches_the_presets() {
    for preset in TreePreset::all() {
        let row = format!(
            "| {} | `{}` | {} | {} |",
            preset.name,
            preset.key,
            stand(preset).density,
            stand(preset).spacing
        );
        assert!(GUIDE.contains(&row), "the guide lacks the row {row}");
    }
    for preset in TreeScatterPreset::all() {
        let row = format!("| {} | `{}` |", preset.name, preset.key);
        assert!(GUIDE.contains(&row), "the guide lacks the row {row}");
    }
}

#[test]
fn scatters_round_trip_through_yaml() {
    let mut preset = TreeScatterPreset::oak_woodland().clone();
    let scatter = &mut preset.tree_scatter;
    scatter.trees[1].tree.default_seed = 77; // no longer a preset, so written out in full
    scatter.variation.lean = 7.5;
    scatter.patchiness = 0.4;
    // A shrub preset on the Scatter's slope limit, and a tree with its own.
    assert_eq!(scatter.trees[0].max_slope, Some(45.0));
    scatter.trees[0].max_slope = None;
    scatter.trees[2].max_slope = Some(20.0);
    for base in [None, Some(TreeScatterPreset::oak_woodland())] {
        let yaml = preset.to_yaml(base);
        assert_eq!(
            TreeScatterPreset::from_yaml(&preset.key, &yaml).unwrap(),
            preset,
            "{yaml}"
        );
    }
    // Numbers are written as typed, not widened to many digits.
    let yaml = preset.to_yaml(None);
    assert!(yaml.contains("density_scale: 0.6"), "{yaml}");
    assert!(!yaml.contains("00000"), "{yaml}");
}

#[test]
fn scatter_files_list_only_what_differs() {
    let preset = TreeScatterPreset::from_yaml(
        "mine",
        "base: pine_stand\n\
         tree_scatter:\n  \
           patchiness: 0.6\n  \
           variation: { lean: 10 }\n  \
           trees:\n    \
             - { tree: { base: cedar, default_seed: 5 }, clustering: 0.5 }\n    \
             - { tree: pine_medium, density: 123 }\n",
    )
    .unwrap();
    let scatter = preset.tree_scatter;
    let base = preset_scatter(TreeScatterPreset::pine_stand());
    assert_eq!(scatter.default_seed, base.default_seed);
    assert_eq!(scatter.patchiness, 0.6);
    assert_eq!(scatter.density_scale, base.density_scale);
    assert_eq!(scatter.variation.lean, 10.0);
    assert_eq!(scatter.variation.sink, base.variation.sink);
    assert_eq!(scatter.trees[0].tree.default_seed, 5);
    assert_eq!(scatter.trees[0].density, stand(TreePreset::cedar()).density);
    assert_eq!(scatter.trees[0].clustering, 0.5);
    assert_eq!(scatter.trees[1].density, 123.0);
    assert_eq!(
        scatter.trees[1].spacing,
        stand(TreePreset::pine_medium()).spacing
    );
}

#[test]
fn bad_scatter_files_say_what_is_wrong() {
    let read = |text: &str| TreeScatterPreset::from_yaml("bad", text).unwrap_err();

    let unknown = read("base: jungle\n");
    assert!(matches!(unknown, PresetFileError::UnknownBase { .. }));
    assert!(unknown.to_string().contains("oak_woodland"), "{unknown}");

    let unknown = read("tree_scatter:\n  trees: [{ tree: baobab }]\n");
    assert!(
        matches!(&unknown, PresetFileError::Setting { path, .. } if path == "tree_scatter.trees[0]"),
        "{unknown:?}"
    );
    let message = unknown.to_string();
    assert!(message.contains("baobab"), "{message}");
    assert!(message.contains("oak_small"), "{message}");

    let missing = read("tree_scatter:\n  trees: [{ tree: { default_seed: 1 }, density: 5 }]\n");
    assert!(
        matches!(
            &missing,
            PresetFileError::Setting { path, message }
                if path == "tree_scatter.trees[0]" && message.ends_with("must set spacing")
        ),
        "{missing:?}"
    );

    let bad_tree = read("tree_scatter:\n  trees: [{ tree: oak_small }, { tree: 3 }]\n");
    assert!(
        matches!(&bad_tree, PresetFileError::Setting { path, .. } if path == "tree_scatter.trees[1].tree"),
        "{bad_tree:?}"
    );

    assert!(matches!(
        read("tree_scatter:\n  sed: 3\n"),
        PresetFileError::Setting { .. }
    ));
    assert!(matches!(
        read("sed: 3\n"),
        PresetFileError::UnknownKey { .. }
    ));
}
