//! End-to-end checks of the scene that need a GPU. Run with `cargo test -- --ignored`.
//!
//! Wind and auto-rotation are off, so the scene renders on demand. These
//! tests check that it keeps rendering until the subject is fully drawn.

use std::sync::Arc;

use bevy::prelude::*;

use super::headless_scene::{Frame, HeadlessScene};
use super::protocol::ClumpBuildError;
use super::{
    ClumpSettings, ClumpSubject, EnvironmentSettings, FieldSettings, PointerButton, SceneMode,
    SceneStart, ViewerCommand, ViewerEvent, ViewportInput,
};

const SIZE: UVec2 = UVec2::new(480, 270);

/// Starts the scene in `mode` with wind off.
fn start(mode: SceneMode) -> HeadlessScene {
    HeadlessScene::new(
        SceneStart {
            mode,
            environment: EnvironmentSettings {
                wind: false,
                ..default()
            },
            ..default()
        },
        SIZE,
    )
}

/// The fraction of pixels in rows `rows` for which `test(r, g, b)` holds.
fn fraction(
    frame: &Frame,
    rows: std::ops::Range<u32>,
    test: impl Fn(i32, i32, i32) -> bool,
) -> f32 {
    let mut hits = 0;
    let mut total = 0;
    for y in rows {
        for x in 0..frame.width {
            let i = ((y * frame.width + x) * 4) as usize;
            let [b, g, r] = [0, 1, 2].map(|c| i32::from(frame.bgra[i + c]));
            total += 1;
            if test(r, g, b) {
                hits += 1;
            }
        }
    }
    hits as f32 / total as f32
}

#[test]
#[ignore = "needs a GPU"]
fn idle_tree_mode_draws_the_tree() {
    let mut scene = start(SceneMode::Tree);
    let frame = scene.settled_frame();

    // The tree was built and reported, with every detail level.
    let built = tree_event(&mut scene);
    let ViewerEvent::TreeBuilt(Ok(stats)) = built else {
        panic!("tree failed: {built:?}");
    };
    assert_eq!(stats.levels.len(), super::lod_levels().len());
    for level in &stats.levels {
        assert!(level.vertices > 0 && level.triangles > 0, "{stats:?}");
    }

    // Above the horizon, everything that is not sky (the top-left pixel) is the tree.
    let sky = [frame.bgra[2], frame.bgra[1], frame.bgra[0]].map(i32::from);
    let tree = fraction(&frame, 0..SIZE.y * 2 / 5, |r, g, b| {
        (r - sky[0])
            .abs()
            .max((g - sky[1]).abs())
            .max((b - sky[2]).abs())
            > 40
    });
    assert!(
        tree > 0.03,
        "only {:.1}% of the sky is covered by the tree",
        tree * 100.0
    );
}

#[test]
#[ignore = "needs a GPU"]
fn every_chosen_detail_level_draws_the_tree() {
    for lod in 0..super::lod_levels().len() {
        let mut scene = start(SceneMode::Tree);
        scene.send(ViewerCommand::SetLod(Some(lod)));
        let coverage = sky_coverage(&scene.settled_frame());
        assert!(
            coverage > 0.03,
            "level {lod} covers only {:.1}% of the sky",
            coverage * 100.0
        );

        // The chosen level is reported as shown.
        let shown: Vec<ViewerEvent> = scene
            .take_events()
            .into_iter()
            .filter(|event| matches!(event, ViewerEvent::LodShown(_)))
            .collect();
        assert_eq!(shown.last(), Some(&ViewerEvent::LodShown(lod)), "{shown:?}");
    }
}

/// Checks that choosing the detail level already on screen keeps the tree
/// drawn, and so does switching by distance again.
#[test]
#[ignore = "needs a GPU"]
fn choosing_the_shown_tree_level_keeps_the_tree() {
    let mut scene = start(SceneMode::Tree);
    assert!(matches!(
        tree_event(&mut scene),
        ViewerEvent::TreeBuilt(Ok(_))
    ));
    scene.settle();
    for choice in [Some(0), None] {
        scene.send(ViewerCommand::SetLod(choice));
        let coverage = sky_coverage(&scene.settled_frame());
        assert!(
            coverage > 0.03,
            "with {choice:?} the tree covers only {:.1}% of the sky",
            coverage * 100.0
        );
    }
}

/// The fraction of the sky (the top 40% of rows) covered by something other
/// than sky, judged against the top-left pixel.
fn sky_coverage(frame: &Frame) -> f32 {
    let sky = [frame.bgra[2], frame.bgra[1], frame.bgra[0]].map(i32::from);
    fraction(frame, 0..SIZE.y * 2 / 5, |r, g, b| {
        (r - sky[0])
            .abs()
            .max((g - sky[1]).abs())
            .max((b - sky[2]).abs())
            > 40
    })
}

/// The next tree-building event, skipping detail level reports.
fn tree_event(scene: &mut HeadlessScene) -> ViewerEvent {
    scene.next_event(|event| matches!(event, ViewerEvent::TreeBuilt(_)))
}

/// Settles the scene in tree mode showing detail level `lod` (or switching by
/// distance), then sends `options`. Asserts that every frame until the scene
/// settles again shows a tree, and returns the event reported for `options`.
fn change_tree(options: procgena_tree::TreeOptions, lod: Option<usize>) -> ViewerEvent {
    let mut scene = start(SceneMode::Tree);
    scene.send(ViewerCommand::SetLod(lod));
    scene.settle();
    assert!(matches!(
        tree_event(&mut scene),
        ViewerEvent::TreeBuilt(Ok(_))
    ));

    scene.send(ViewerCommand::SetTree(Arc::new(options)));

    // Every frame until the scene settles shows a tree.
    let frames = scene.settle();
    assert!(!frames.is_empty(), "no frame was rendered for the new tree");
    for (index, frame) in frames.iter().enumerate() {
        let coverage = sky_coverage(frame);
        assert!(
            coverage > 0.03,
            "frame {} shows only {:.1}% tree",
            index + 1,
            coverage * 100.0
        );
    }
    tree_event(&mut scene)
}

/// Options for a differently shaped tree that is not cached yet.
fn new_tree() -> procgena_tree::TreeOptions {
    let mut options = procgena_tree::TreePreset::OakMedium.options();
    options.seed += 1;
    options
}

#[test]
#[ignore = "needs a GPU"]
fn changing_the_tree_never_shows_an_empty_frame() {
    let built = change_tree(new_tree(), None);
    assert!(matches!(built, ViewerEvent::TreeBuilt(Ok(_))), "{built:?}");
}

#[test]
#[ignore = "needs a GPU"]
fn changing_the_tree_at_a_chosen_detail_level_never_shows_an_empty_frame() {
    // The default camera distance selects level 0, so choose another.
    let built = change_tree(new_tree(), Some(2));
    assert!(matches!(built, ViewerEvent::TreeBuilt(Ok(_))), "{built:?}");
}

#[test]
#[ignore = "needs a GPU"]
fn rejected_tree_options_keep_the_shown_tree() {
    let mut options = procgena_tree::TreePreset::OakMedium.options();
    options.branch.sections[0] = 0;
    let built = change_tree(options, None);
    assert!(matches!(built, ViewerEvent::TreeBuilt(Err(_))), "{built:?}");
}

#[test]
#[ignore = "needs a GPU"]
fn idle_plants_mode_draws_the_grass() {
    let mut scene = start(SceneMode::Plants);
    let frame = scene.settled_frame();

    // The lower half is mostly green blades.
    let grass = fraction(&frame, SIZE.y / 2..SIZE.y, |r, g, b| {
        g > r + 10 && g > b + 10
    });
    assert!(
        grass > 0.3,
        "only {:.1}% of the ground is grass",
        grass * 100.0
    );
}

/// Estimates grass coverage by counting green pixels in the lower half of the image.
/// Sends `command` to the plants view and waits for the updated image before measuring.
fn grass_after(command: ViewerCommand) -> f32 {
    let mut scene = start(SceneMode::Plants);
    scene.settle();
    scene.send(command);
    let frame = scene.settled_frame();
    fraction(&frame, SIZE.y / 2..SIZE.y, |r, g, b| {
        g > r + 10 && g > b + 10
    })
}

#[test]
#[ignore = "needs a GPU"]
fn changed_plant_colours_are_drawn() {
    // Red blades, with no ground showing through or patches of other colours.
    let red = Color::srgb(0.6, 0.05, 0.05);
    let mut settings = FieldSettings::default();
    let field = &mut settings.field;
    for entry in field.plants.to_mut() {
        entry.plant.segment.root_color = red;
        entry.plant.segment.tip_color = red;
    }
    field.ground_tint.amount = 0.0;
    field.variation.patch_amount = 0.0;
    field.variation.dead_share = 0.0;
    field.variation.dry_tips = 0.0;

    let grass = grass_after(ViewerCommand::SetField(Arc::new(settings)));
    assert!(
        grass < 0.05,
        "{:.1}% of the ground is still green",
        grass * 100.0
    );
}

#[test]
#[ignore = "needs a GPU"]
fn a_smaller_field_is_drawn() {
    // Shrinking the field to 1 metre on each side should expose mostly bare ground.
    let environment = EnvironmentSettings {
        field_size: 1.0,
        wind: false,
        ..default()
    };
    let grass = grass_after(ViewerCommand::SetEnvironment(environment));
    assert!(
        grass < 0.15,
        "{:.1}% of the ground is still grass",
        grass * 100.0
    );
}

/// Estimates rock coverage in the middle half of the image by counting grey pixels.
fn rock_share(frame: &Frame) -> f32 {
    fraction(frame, SIZE.y / 4..SIZE.y * 3 / 4, |r, g, b| {
        (r - b).abs() < 25 && (r - g).abs() < 20
    })
}

/// Waits for a rock build result, ignoring other viewer events.
fn stone_event(scene: &mut HeadlessScene) -> ViewerEvent {
    scene.next_event(|event| matches!(event, ViewerEvent::StoneBuilt(_)))
}

/// Checks that the rock remains visible once the scene stops requesting frames.
#[test]
#[ignore = "needs a GPU"]
fn idle_stone_mode_draws_the_rock() {
    let mut scene = start(SceneMode::Stone);
    let ViewerEvent::StoneBuilt(Ok(stats)) = stone_event(&mut scene) else {
        panic!("the default rock was rejected");
    };
    assert!(
        stats.levels.iter().all(|level| level.triangles > 0),
        "{stats:?}"
    );
    let frame = scene.settled_frame();
    let rock = rock_share(&frame);
    assert!(rock > 0.05, "only {:.1}% of the view is rock", rock * 100.0);
}

/// Switches between a tree and a rock as procgena's tabs do, within one running
/// scene: each mode draws its subject, and the tree's camera view is kept
/// while the rock is shown.
#[test]
#[ignore = "needs a GPU"]
fn switched_modes_draw_their_subject_and_keep_the_camera_view() {
    let mut scene = start(SceneMode::Tree);
    let mut tree_view = None;
    for _ in 0..2 {
        // The tree is drawn, from the view it was left with.
        let coverage = sky_coverage(&scene.settled_frame());
        assert!(coverage > 0.03, "only {:.1}% tree", coverage * 100.0);
        let shown = scene.camera_view();
        if let Some(view) = tree_view {
            assert_eq!(shown, view);
        }

        // Turn the camera, then switch away and keep the view it ended with.
        scene.input(ViewportInput::Drag {
            button: PointerButton::Left,
            delta: Vec2::new(60.0, 0.0),
            shift: false,
        });
        scene.settle();
        let turned = scene.camera_view();
        assert_ne!(turned, shown);
        tree_view = Some(turned);

        // The rock is drawn.
        scene.send(ViewerCommand::SetMode(SceneMode::Stone));
        assert!(matches!(
            stone_event(&mut scene),
            ViewerEvent::StoneBuilt(Ok(_))
        ));
        let rock = rock_share(&scene.settled_frame());
        assert!(rock > 0.05, "only {:.1}% of the view is rock", rock * 100.0);

        scene.send(ViewerCommand::SetMode(SceneMode::Tree));
    }
}

/// Checks that invalid replacement settings report an error and leave the rock visible.
#[test]
#[ignore = "needs a GPU"]
fn rejected_stone_options_keep_the_shown_rock() {
    let mut scene = start(SceneMode::Stone);
    assert!(matches!(
        stone_event(&mut scene),
        ViewerEvent::StoneBuilt(Ok(_))
    ));
    let before = rock_share(&scene.settled_frame());

    let options = procgena_stone::StoneOptions {
        scale: -1.0,
        ..default()
    };
    scene.send(ViewerCommand::SetStone(Arc::new(options)));
    let built = stone_event(&mut scene);
    assert!(
        matches!(built, ViewerEvent::StoneBuilt(Err(_))),
        "{built:?}"
    );
    let after = rock_share(&scene.settled_frame());
    assert!(
        after > before * 0.9,
        "the rock vanished: {before} then {after}"
    );
}

/// The share of the view that is strongly red, like a fly agaric's cap.
fn red_share(frame: &Frame) -> f32 {
    fraction(frame, 0..frame.height, |r, g, b| r > g + 70 && r > b + 70)
}

/// Waits for a mushroom build result, ignoring other viewer events.
fn mushroom_event(scene: &mut HeadlessScene) -> ViewerEvent {
    scene.next_event(|event| matches!(event, ViewerEvent::MushroomBuilt(_)))
}

/// What a scene in mushroom mode showing `preset`, with wind off, starts with.
fn mushroom_start(preset: procgena_mushroom::MushroomPreset) -> SceneStart {
    SceneStart {
        mode: SceneMode::Mushroom,
        mushroom: Arc::new(preset.options()),
        environment: EnvironmentSettings {
            wind: false,
            ..default()
        },
        ..default()
    }
}

/// Starts mushroom mode showing `preset`, with wind off.
fn start_mushroom(preset: procgena_mushroom::MushroomPreset) -> HeadlessScene {
    HeadlessScene::new(mushroom_start(preset), SIZE)
}

/// Checks that a textured mushroom is drawn, texture included, once the
/// scene goes idle, with every detail level built.
#[test]
#[ignore = "needs a GPU"]
fn idle_mushroom_mode_draws_the_textured_mushroom() {
    let mut scene = start_mushroom(procgena_mushroom::MushroomPreset::FlyAgaric);
    assert_fly_agaric_is_textured(&mut scene);
}

/// Waits for a dead wood build result, ignoring other viewer events.
fn deadwood_event(scene: &mut HeadlessScene) -> ViewerEvent {
    scene.next_event(|event| matches!(event, ViewerEvent::DeadwoodBuilt(_)))
}

/// Checks that dead wood mode builds every detail level of a log and draws
/// its bark and end-grain textures, and that a chosen detail level is
/// reported.
#[test]
#[ignore = "needs a GPU"]
fn idle_deadwood_mode_draws_the_textured_log() {
    use procgena_deadwood::{DeadwoodLod, DeadwoodPreset};

    let start = SceneStart {
        mode: SceneMode::Deadwood,
        deadwood: Arc::new(DeadwoodPreset::FreshCutLog.options()),
        environment: EnvironmentSettings {
            wind: false,
            ..default()
        },
        ..default()
    };
    let mut scene = HeadlessScene::new(start, SIZE);
    let ViewerEvent::DeadwoodBuilt(Ok(stats)) = deadwood_event(&mut scene) else {
        panic!("the log was not built");
    };
    assert_eq!(stats.levels.len(), DeadwoodLod::default().levels.len());
    let textured = scene.settled_frame();

    // The same log without textures looks different.
    let mut plain = DeadwoodPreset::FreshCutLog.options();
    plain.material.bark_texture = None;
    plain.material.end_texture = None;
    scene.send(ViewerCommand::SetDeadwood(Arc::new(plain)));
    assert!(matches!(
        deadwood_event(&mut scene),
        ViewerEvent::DeadwoodBuilt(Ok(_))
    ));
    let untextured = scene.settled_frame();
    let differing = textured
        .bgra
        .chunks(4)
        .zip(untextured.bgra.chunks(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    assert!(
        differing > (SIZE.x * SIZE.y / 100) as usize,
        "only {differing} pixels changed without the textures"
    );

    // Choosing the strips-only level shows and reports it.
    scene.send(ViewerCommand::SetDeadwoodLod(Some(3)));
    let shown = scene.next_event(|event| matches!(event, ViewerEvent::DeadwoodLodShown(_)));
    assert!(
        matches!(shown, ViewerEvent::DeadwoodLodShown(Some(3))),
        "{shown:?}"
    );
}

/// Checks that with no textures on disk, the scene downloads the rock's
/// textures into its cache and draws the rock looking as it does with the
/// workspace's copies.
#[test]
#[ignore = "needs a GPU and the network"]
fn missing_textures_are_downloaded_and_drawn() {
    let root = std::env::temp_dir()
        .join("procgena-tests")
        .join(format!("texture-download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (assets, cache) = (root.join("assets"), root.join("cache"));

    // The rock as drawn from the workspace's textures.
    let mut local = start(SceneMode::Stone);
    assert!(matches!(
        stone_event(&mut local),
        ViewerEvent::StoneBuilt(Ok(_))
    ));
    let expected = local.settled_frame();

    // The same rock, starting with no textures at all.
    let empty = SceneStart {
        mode: SceneMode::Stone,
        environment: EnvironmentSettings {
            wind: false,
            ..default()
        },
        ..default()
    };
    let mut downloaded = HeadlessScene::with_sources(empty, SIZE, |app| {
        super::assets::add_asset_sources_in(app, &assets, &cache);
    });
    assert!(matches!(
        stone_event(&mut downloaded),
        ViewerEvent::StoneBuilt(Ok(_))
    ));
    let frame = downloaded.settled_frame();
    let differing = expected
        .bgra
        .chunks(4)
        .zip(frame.bgra.chunks(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    assert!(
        differing < (SIZE.x * SIZE.y / 200) as usize,
        "{differing} pixels differ from the rock drawn with local textures"
    );
    let saved = std::fs::read_dir(cache.join("stone/rock")).map_or(0, Iterator::count);
    assert!(saved > 0, "no rock textures were saved to the cache");
}

/// Checks that `scene`, started showing the fly agaric, builds every detail
/// level and draws its cap texture, then shows it without the texture.
fn assert_fly_agaric_is_textured(scene: &mut HeadlessScene) {
    use procgena_mushroom::{MushroomLod, MushroomPreset};

    let ViewerEvent::MushroomBuilt(Ok(stats)) = mushroom_event(scene) else {
        panic!("the fly agaric was rejected");
    };
    assert_eq!(stats.levels.len(), MushroomLod::default().levels.len());
    assert!(
        stats.levels.iter().all(|level| level.triangles > 0),
        "{stats:?}"
    );
    let textured = scene.settled_frame();
    let red = red_share(&textured);
    assert!(
        red > 0.01,
        "only {:.1}% of the view is red cap",
        red * 100.0
    );

    // The same mushroom without its texture looks different on the cap.
    let mut plain = MushroomPreset::FlyAgaric.options();
    plain.material.cap_texture = None;
    scene.send(ViewerCommand::SetMushroom(Arc::new(plain)));
    assert!(matches!(
        mushroom_event(scene),
        ViewerEvent::MushroomBuilt(Ok(_))
    ));
    let untextured = scene.settled_frame();
    let changed = changed_share(&textured, &untextured);
    assert!(
        changed > 0.002,
        "the texture changed only {:.2}% of the view",
        changed * 100.0
    );
}

/// The fraction of pixels in which some colour channel differs by more
/// than 30 between frames `a` and `b`.
fn changed_share(a: &Frame, b: &Frame) -> f32 {
    let changed = a
        .bgra
        .chunks(4)
        .zip(b.bgra.chunks(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 30))
        .count();
    changed as f32 / (SIZE.x * SIZE.y) as f32
}

/// The mushroom detail levels reported as shown since the last check.
fn mushroom_levels_shown(scene: &mut HeadlessScene) -> Vec<Option<usize>> {
    scene
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            ViewerEvent::MushroomLodShown(level) => Some(level),
            _ => None,
        })
        .collect()
}

/// Checks that each chosen mushroom detail level is drawn and reported,
/// and that the flat level looks different from full detail.
#[test]
#[ignore = "needs a GPU"]
fn every_chosen_mushroom_level_is_drawn_and_reported() {
    let mut scene = start_mushroom(procgena_mushroom::MushroomPreset::FlyAgaric);
    assert!(matches!(
        mushroom_event(&mut scene),
        ViewerEvent::MushroomBuilt(Ok(_))
    ));
    let full = scene.settled_frame();

    let levels = procgena_mushroom::MushroomLod::default().levels.len();
    let mut frames = Vec::new();
    for lod in 0..levels {
        scene.send(ViewerCommand::SetMushroomLod(Some(lod)));
        let frame = scene.settled_frame();
        let red = red_share(&frame);
        assert!(
            red > 0.005,
            "level {lod} shows only {:.1}% red cap",
            red * 100.0
        );
        let shown = mushroom_levels_shown(&mut scene);
        assert_eq!(shown.last(), Some(&Some(lod)), "{shown:?}");
        frames.push(frame);
    }
    let flat = changed_share(&full, &frames[levels - 1]);
    assert!(
        flat > 0.002,
        "the flat level changed only {:.2}% of the view",
        flat * 100.0
    );

    // Switching by distance again shows full detail, as the starting view
    // is close enough for it.
    scene.send(ViewerCommand::SetMushroomLod(None));
    let auto = scene.settled_frame();
    assert_eq!(mushroom_levels_shown(&mut scene).last(), Some(&Some(0)));
    assert!(changed_share(&full, &auto) < 0.002);
}

/// Checks that invalid replacement settings report an error and leave the
/// mushroom visible.
#[test]
#[ignore = "needs a GPU"]
fn rejected_mushroom_options_keep_the_shown_mushroom() {
    let mut scene = start_mushroom(procgena_mushroom::MushroomPreset::FlyAgaric);
    assert!(matches!(
        mushroom_event(&mut scene),
        ViewerEvent::MushroomBuilt(Ok(_))
    ));
    let before = red_share(&scene.settled_frame());

    let options = procgena_mushroom::MushroomOptions {
        size: -1.0,
        ..procgena_mushroom::MushroomPreset::FlyAgaric.options()
    };
    scene.send(ViewerCommand::SetMushroom(Arc::new(options)));
    let built = mushroom_event(&mut scene);
    assert!(
        matches!(built, ViewerEvent::MushroomBuilt(Err(_))),
        "{built:?}"
    );
    let after = red_share(&scene.settled_frame());
    assert!(
        after > before * 0.9,
        "the mushroom vanished: {before} then {after}"
    );
}

/// The share of the view that is bright, nearly grey pixels, such as snow.
fn snow_share(frame: &Frame) -> f32 {
    fraction(frame, 0..frame.height, |r, g, b| {
        r.min(g).min(b) > 190 && r.max(g).max(b) - r.min(g).min(b) < 30
    })
}

/// The next clump build result.
fn clump_event(scene: &mut HeadlessScene) -> ViewerEvent {
    scene.next_event(|event| matches!(event, ViewerEvent::ClumpBuilt(_)))
}

/// Grows a white snow cap on the default rock, then on a tree.
#[test]
#[ignore = "needs a GPU"]
fn clump_mode_grows_a_clump_on_a_rock_and_on_a_tree() {
    let mut scene = start(SceneMode::Clump);
    let ViewerEvent::ClumpBuilt(Ok(stats)) = clump_event(&mut scene) else {
        panic!("the default clump was rejected");
    };
    assert!(stats.levels[0].triangles > 0, "{stats:?}");
    let snow = snow_share(&scene.settled_frame());
    assert!(snow > 0.01, "only {:.2}% of the view is snow", snow * 100.0);

    // A tree subject is built, then grows its own clump.
    let mut settings = ClumpSettings {
        subject: ClumpSubject::Tree(Arc::new(procgena_tree::TreePreset::OakMedium.options())),
        ..default()
    };
    settings.clump.mesh.edge_length = 0.1;
    settings.clump.mask.exposure_rays = 16;
    scene.send(ViewerCommand::SetClump(Arc::new(settings)));
    scene.send(ViewerCommand::ResetCamera);
    let ViewerEvent::ClumpBuilt(Ok(stats)) = clump_event(&mut scene) else {
        panic!("the tree's clump was rejected");
    };
    assert!(stats.levels[0].triangles > 0, "{stats:?}");
}

/// Settings changed while a clump is building replace it: only the latest
/// settings' clump is reported, and rejected settings are reported as such.
#[test]
#[ignore = "needs a GPU"]
fn only_the_latest_clump_settings_are_shown() {
    let mut scene = start(SceneMode::Clump);
    assert!(matches!(
        clump_event(&mut scene),
        ViewerEvent::ClumpBuilt(Ok(_))
    ));

    // Two quick edits: the first build's result is dropped.
    let mut settings = ClumpSettings::default();
    for seed in [1, 2] {
        settings.clump.seed = seed;
        scene.send(ViewerCommand::SetClump(Arc::new(settings.clone())));
        scene.update();
    }
    scene.settle();
    let built: Vec<_> = scene
        .take_events()
        .into_iter()
        .filter(|event| matches!(event, ViewerEvent::ClumpBuilt(_)))
        .collect();
    assert!(
        matches!(built.as_slice(), [ViewerEvent::ClumpBuilt(Ok(_))]),
        "{built:?}"
    );

    // Out-of-range settings are reported.
    settings.clump.shape.thickness = 0.0;
    scene.send(ViewerCommand::SetClump(Arc::new(settings)));
    assert!(matches!(
        clump_event(&mut scene),
        ViewerEvent::ClumpBuilt(Err(ClumpBuildError::Clump(_)))
    ));
}
