//! End-to-end checks of the scene that need a GPU. Run with `cargo test -- --ignored`.
//!
//! Wind and auto-rotation are off, so the scene renders on demand. These
//! tests check that it keeps rendering until the subject is fully drawn.

use std::sync::Arc;

use bevy::prelude::*;

use super::headless_scene::{Frame, HeadlessScene};
use super::{
    EnvironmentSettings, FieldSettings, PointerButton, SceneMode, SceneStart, ViewerCommand,
    ViewerEvent, ViewportInput,
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
