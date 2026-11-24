//! Ecotopes: preset files, keys and seeds, and placing Scatters in priority
//! order.

use glam::{DVec2, Vec2, Vec3};
use procgena_biome::{
    Ecotope, EcotopeError, EcotopePreset, EcotopeScatter, PlacedScatter, ScatterCopies,
    ScatterKind, scatter_seed,
};
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_core::{ChildKey, HeightGrid};
use procgena_deadwood::DeadwoodPreset;
use procgena_mushroom::MushroomPreset;

/// Side of the square test area, in metres.
const SIDE: f32 = 24.0;

/// Gently rolling ground a little wider than the test area.
fn ground() -> HeightGrid {
    let half = f64::from(SIDE) * 0.5 + 4.0;
    HeightGrid::from_fn(DVec2::splat(-half), DVec2::splat(half), 0.5, |p| {
        let (x, z) = (p.x as f32, p.y as f32);
        0.6 * (0.21 * x).sin() + 0.4 * (0.17 * z + 0.1 * x).cos()
    })
}

/// The test area, centred away from the World origin.
fn area() -> ScatterArea {
    ScatterArea::new(DVec2::new(0.0, 0.0), Vec2::splat(SIDE))
}

fn pine_floor() -> Ecotope {
    EcotopePreset::pine_forest_floor().ecotope.clone()
}

fn key(key: &'static str) -> ChildKey {
    ChildKey::from_static(key)
}

/// Every copy's position and turn, to compare placements.
fn copies(layer: &PlacedScatter) -> Vec<[f32; 6]> {
    let row = |p: Vec3, q: Vec3| [p.x, p.y, p.z, q.x, q.y, q.z];
    match layer.copies() {
        ScatterCopies::Tree(placed) => placed
            .instances()
            .iter()
            .map(|c| row(c.position, c.transform.matrix3.x_axis.into()))
            .collect(),
        ScatterCopies::Stone(placed) => placed
            .instances()
            .iter()
            .map(|c| row(c.position, c.transform.matrix3.x_axis.into()))
            .collect(),
        ScatterCopies::Deadwood(placed) => placed
            .instances()
            .iter()
            .map(|c| row(c.position, c.transform.matrix3.x_axis.into()))
            .collect(),
        ScatterCopies::Mushroom(placed) => placed
            .instances()
            .iter()
            .map(|c| row(c.translation, c.rotation * Vec3::X * c.scale))
            .collect(),
    }
}

fn place(ecotope: &Ecotope, seed: u32) -> procgena_biome::EcotopePlacement {
    let ground = ground();
    let input = ScatterInput::new(&ground, area(), seed);
    ecotope.place(&input).expect("the Ecotope places")
}

#[test]
fn every_built_in_preset_is_valid_and_reads_back_from_its_file() {
    for preset in EcotopePreset::all() {
        preset
            .ecotope
            .validate()
            .expect("built-in presets are valid");

        // Written in full, and as differences from itself.
        let full = preset.to_yaml(None);
        let read = EcotopePreset::from_yaml(&preset.key, &full).expect(&full);
        assert_eq!(read.ecotope, preset.ecotope, "{full}");
        assert_eq!(read.name, preset.name);
        let same = preset.to_yaml(Some(preset));
        let read = EcotopePreset::from_yaml(&preset.key, &same).expect(&same);
        assert_eq!(read.ecotope, preset.ecotope, "{same}");
    }
}

#[test]
fn the_pine_floor_holds_trees_boulders_logs_and_mushrooms() {
    let ecotope = pine_floor();
    let kinds: Vec<&str> = ecotope
        .order()
        .iter()
        .map(|(_, scatter)| scatter.scatter.name())
        .collect();
    assert_eq!(kinds, ["tree", "stone", "deadwood", "mushroom", "mushroom"]);

    let placed = place(&ecotope, ecotope.default_seed);
    for layer in placed.layers() {
        assert!(!layer.copies().is_empty(), "{} placed nothing", layer.key());
    }
}

#[test]
fn a_file_changes_one_scatter_and_adds_another_without_restating_the_rest() {
    let text = "\
base: pine_forest_floor
ecotope:
  scatters:
    logs:
      density: 0.02
      piece: { base: rotten_log, length: 3.0 }
    chanterelles:
      priority: 160
      kind: mushroom
      mushroom: chanterelle
      density: 0.1
      min_spacing: 1.0
";
    let base = pine_floor();
    let read = EcotopePreset::from_yaml("mine", text).expect("the file reads");
    let ecotope = &read.ecotope;
    assert_eq!(read.group, "File");
    assert_eq!(ecotope.scatters.len(), base.scatters.len() + 1);

    // Untouched Scatters are the base's.
    for name in ["pines", "boulders", "porcini", "fly_agarics"] {
        assert_eq!(
            ecotope.scatters[&key(name)],
            base.scatters[&key(name)],
            "{name}"
        );
    }

    // The changed Scatter keeps everything else from the base.
    let (ScatterKind::Deadwood(logs), ScatterKind::Deadwood(base_logs)) = (
        &ecotope.scatters[&key("logs")].scatter,
        &base.scatters[&key("logs")].scatter,
    ) else {
        panic!("logs are dead wood");
    };
    assert_eq!(logs.density, 0.02);
    assert_eq!(logs.min_spacing, base_logs.min_spacing);
    let mut rotten = DeadwoodPreset::rotten_log().deadwood.clone();
    rotten.length = 3.0;
    assert_eq!(logs.piece, rotten);
    assert_eq!(ecotope.scatters[&key("logs")].priority, 800);

    // The new Scatter fills what it leaves out from its kind's defaults.
    let new = &ecotope.scatters[&key("chanterelles")];
    assert_eq!(new.priority, 160);
    assert_eq!(new.clearance, 0.0);
    let ScatterKind::Mushroom { mushroom, scatter } = &new.scatter else {
        panic!("chanterelles are mushrooms");
    };
    assert_eq!(mushroom, &MushroomPreset::chanterelle().mushroom);
    assert_eq!(scatter.density, 0.1);
    assert_eq!(
        scatter.specimens,
        procgena_mushroom::ScatterOptions::default().specimens
    );
    ecotope.validate().expect("the merged Ecotope is valid");

    // Written as differences from its base, it reads back the same.
    let again = EcotopePreset::from_yaml(
        "mine",
        &read.to_yaml(Some(EcotopePreset::pine_forest_floor())),
    )
    .expect("the written file reads");
    assert_eq!(&again.ecotope, ecotope);
}

#[test]
fn bad_files_say_what_is_wrong() {
    let error = |text: &str| {
        EcotopePreset::from_yaml("bad", text)
            .expect_err(text)
            .to_string()
    };
    let unknown_stone = "\
ecotope:
  scatters:
    rocks:
      priority: 850
      kind: stone
      classes:
        - stone: cobblestone
";
    let message = error(unknown_stone);
    assert!(
        message.contains("classes.0") && message.contains("cobblestone"),
        "{message}"
    );

    let bad_key = "ecotope:\n  scatters:\n    Rocks:\n      priority: 1\n      kind: tree\n";
    assert!(error(bad_key).contains("Rocks"));

    let no_kind = "ecotope:\n  scatters:\n    rocks:\n      priority: 1\n";
    assert!(error(no_kind).contains("kind"));

    let shared = "base: pine_forest_floor\necotope:\n  scatters:\n    logs:\n      priority: 900\n";
    assert!(error(shared).contains("share priority 900"));
}

#[test]
fn two_scatters_may_not_share_a_priority() {
    let ecotope = pine_floor();
    let mut logs = ecotope.scatters[&key("logs")].clone();
    logs.priority = 850;
    let ecotope = ecotope.with(key("logs"), logs);
    assert!(matches!(
        ecotope.validate(),
        Err(EcotopeError::SharedPriority { priority: 850, .. })
    ));
}

#[test]
fn each_scatter_is_seeded_from_the_ecotope_seed_and_its_key() {
    let ecotope = pine_floor();
    let placed = place(&ecotope, 42);
    let mut seeds = Vec::new();
    for layer in placed.layers() {
        assert_eq!(layer.seed(), scatter_seed(42, layer.key()));
        seeds.push(layer.seed());
    }
    seeds.sort_unstable();
    seeds.dedup();
    assert_eq!(
        seeds.len(),
        placed.layers().len(),
        "every key has its own seed"
    );

    // The Ecotope's own default seed is not used once a seed is given.
    let mut moved = ecotope.clone();
    moved.default_seed += 1;
    let again = place(&moved, 42);
    for (a, b) in placed.layers().iter().zip(again.layers()) {
        assert_eq!(copies(a), copies(b), "{}", a.key());
    }

    // A different Ecotope seed moves every Scatter.
    let other = place(&ecotope, 43);
    for (a, b) in placed.layers().iter().zip(other.layers()) {
        assert_ne!(copies(a), copies(b), "{}", a.key());
    }
}

#[test]
fn placing_twice_gives_the_same_copies() {
    let ecotope = pine_floor();
    let a = place(&ecotope, ecotope.default_seed);
    let b = place(&ecotope, ecotope.default_seed);
    for (a, b) in a.layers().iter().zip(b.layers()) {
        assert_eq!(a.key(), b.key());
        assert_eq!(copies(a), copies(b), "{}", a.key());
    }
}

#[test]
fn changing_one_scatter_never_moves_the_scatters_above_it() {
    let ecotope = pine_floor();
    let before = place(&ecotope, 5);

    // Thin the logs: every Scatter above them stays where it was.
    let mut logs = ecotope.scatters[&key("logs")].clone();
    let ScatterKind::Deadwood(options) = &mut logs.scatter else {
        panic!("logs are dead wood");
    };
    options.density *= 0.5;
    options.variants = 2;
    let after = place(&ecotope.clone().with(key("logs"), logs), 5);
    for name in ["pines", "boulders"] {
        assert_eq!(
            copies(before.get(name).unwrap()),
            copies(after.get(name).unwrap()),
            "{name}"
        );
    }
    assert_ne!(
        copies(before.get("logs").unwrap()),
        copies(after.get("logs").unwrap())
    );

    // Change the last Scatter, and add a new one at the bottom: nothing
    // above either moves.
    let mut fly = ecotope.scatters[&key("fly_agarics")].clone();
    let ScatterKind::Mushroom { scatter, .. } = &mut fly.scatter else {
        panic!("fly agarics are mushrooms");
    };
    scatter.density = 0.06;
    let extra = EcotopeScatter::new(
        120,
        ScatterKind::Mushroom {
            mushroom: MushroomPreset::puffball().mushroom.clone(),
            scatter: procgena_mushroom::ScatterOptions::default(),
        },
    );
    let after = place(
        &ecotope
            .clone()
            .with(key("fly_agarics"), fly)
            .with(key("puffballs"), extra),
        5,
    );
    for name in ["pines", "boulders", "logs", "porcini"] {
        assert_eq!(
            copies(before.get(name).unwrap()),
            copies(after.get(name).unwrap()),
            "{name}"
        );
    }
}

#[test]
fn a_scatter_keeps_its_copies_under_a_new_key_only_if_its_seed_is_kept() {
    // Renaming a key gives the Scatter a new seed, and moves its copies.
    let ecotope = pine_floor();
    let before = place(&ecotope, 5);
    let mut renamed = ecotope.clone();
    let porcini = renamed.scatters.remove(&key("porcini")).unwrap();
    renamed.scatters.insert(key("ceps"), porcini);
    let after = place(&renamed, 5);
    assert_ne!(
        copies(before.get("porcini").unwrap()),
        copies(after.get("ceps").unwrap())
    );
    for name in ["pines", "boulders", "logs"] {
        assert_eq!(
            copies(before.get(name).unwrap()),
            copies(after.get(name).unwrap()),
            "{name}"
        );
    }
}

#[test]
fn lower_scatters_keep_their_centres_out_of_higher_footprints() {
    let ecotope = pine_floor();
    let placed = place(&ecotope, ecotope.default_seed);
    let centre = area().centre;
    let world = |p: Vec3| centre + DVec2::new(f64::from(p.x), f64::from(p.z));

    // Logs and mushrooms are masked at their centres.
    let logs = placed.get("logs").unwrap();
    let ScatterCopies::Deadwood(sticks) = logs.copies() else {
        panic!("logs are dead wood");
    };
    let mask = placed.mask_above("logs").unwrap();
    for log in sticks.instances() {
        assert!(!mask.blocks(world(log.position)));
    }
    for name in ["porcini", "fly_agarics"] {
        let ScatterCopies::Mushroom(mushrooms) = placed.get(name).unwrap().copies() else {
            panic!("{name} are mushrooms");
        };
        let mask = placed.mask_above(name).unwrap();
        for copy in mushrooms.copies() {
            assert!(!mask.blocks(world(copy.translation)), "{name}");
        }
    }

    // Masks hold every footprint above, and only those.
    let pines = placed.get("pines").unwrap();
    let ScatterCopies::Tree(trees) = pines.copies() else {
        panic!("pines are trees");
    };
    assert!(!trees.instances().is_empty());
    for tree in trees.instances() {
        assert!(
            !placed
                .mask_above("pines")
                .unwrap()
                .blocks(world(tree.position))
        );
        assert!(pines.footprints().contains(world(tree.position)));
        assert!(
            placed
                .mask_above("boulders")
                .unwrap()
                .blocks(world(tree.position))
        );
    }
}

#[test]
fn without_the_scatters_above_more_mushrooms_grow() {
    let ecotope = pine_floor();
    let placed = place(&ecotope, 9);
    let alone = Ecotope::default().with(key("porcini"), ecotope.scatters[&key("porcini")].clone());
    let alone = place(&alone, 9);
    let n = |p: &procgena_biome::EcotopePlacement| p.get("porcini").unwrap().copies().len();
    assert!(n(&alone) > n(&placed), "{} vs {}", n(&alone), n(&placed));
}
