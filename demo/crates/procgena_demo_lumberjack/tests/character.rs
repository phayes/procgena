//! Loads the prepared lumberjack in Bevy and plays every clip on him.
//!
//! The model isn't in the repository, so this test is ignored by default.
//! Make it with `tools/prepare_lumberjack.py`, then run:
//!
//! ```bash
//! cargo test -p procgena_demo_lumberjack --test character -- --include-ignored
//! ```

use std::time::Duration;

use bevy::animation::AnimationTargetId;
use bevy::asset::{LoadState, RecursiveDependencyLoadState};
use bevy::gltf::{Gltf, GltfMaterial};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::time::TimeUpdateStrategy;
use bevy::winit::WinitPlugin;

/// The model, from the repository root.
const MODEL: &str = "demo/assets/lumberjack/lumberjack.glb";

/// Every clip the demo plays, by name.
const CLIPS: [&str; 8] = [
    "Idle_Loop",
    "Walk_Loop",
    "Jog_Fwd_Loop",
    "Sprint_Loop",
    "Jump_Start",
    "Jump_Loop",
    "Jump_Land",
    "TreeChopping_Loop",
];

/// An app with Bevy's asset, glTF, scene and animation plugins, no window
/// and no GPU, stepping 1/60 s per update.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../..").to_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: None,
                    ..default()
                })),
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app
}

/// Updates `app` until `done` holds, panicking after ten thousand updates.
fn until(app: &mut App, what: &str, mut done: impl FnMut(&mut App) -> bool) {
    for _ in 0..10_000 {
        app.update();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("{what} never happened");
}

/// Each joint's local transform, by its animation target id.
fn pose(app: &mut App) -> Vec<(AnimationTargetId, Transform)> {
    let mut query = app.world_mut().query::<(&AnimationTargetId, &Transform)>();
    let mut pose: Vec<_> = query
        .iter(app.world())
        .map(|(id, transform)| (*id, *transform))
        .collect();
    pose.sort_by_key(|(id, _)| id.0);
    pose
}

/// The largest angle, in degrees, any joint turned between `a` and `b`.
fn largest_turn(a: &[(AnimationTargetId, Transform)], b: &[(AnimationTargetId, Transform)]) -> f32 {
    a.iter()
        .zip(b)
        .map(|((_, a), (_, b))| a.rotation.angle_between(b.rotation).to_degrees())
        .fold(0.0, f32::max)
}

#[test]
#[ignore = "needs demo/assets/lumberjack/lumberjack.glb, made by tools/prepare_lumberjack.py"]
fn every_clip_moves_the_lumberjack() {
    let mut app = app();
    let path = format!("{}/../../../{MODEL}", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::path::Path::new(&path).exists(),
        "{MODEL} is missing: make it with tools/prepare_lumberjack.py"
    );

    // The file and everything it holds load, textures included.
    let gltf: Handle<Gltf> = app.world().resource::<AssetServer>().load(MODEL);
    until(&mut app, "loading the model", |app| {
        let server = app.world().resource::<AssetServer>();
        if let Some(LoadState::Failed(error)) = server.get_load_state(&gltf) {
            panic!("{MODEL} failed to load: {error}");
        }
        match server.get_recursive_dependency_load_state(&gltf) {
            Some(RecursiveDependencyLoadState::Failed(error)) => {
                panic!("a part of {MODEL} failed to load: {error}")
            }
            Some(RecursiveDependencyLoadState::Loaded) => true,
            _ => false,
        }
    });
    let world = app.world();
    let model = world.resource::<Assets<Gltf>>().get(&gltf).unwrap();
    for clip in CLIPS {
        assert!(
            model.named_animations.contains_key(clip),
            "{clip} is in the file"
        );
    }
    let images = world.resource::<Assets<Image>>();
    let materials = world.resource::<Assets<GltfMaterial>>();
    for (name, material) in &model.named_materials {
        let material = materials.get(material).expect("the material loaded");
        for texture in [
            &material.base_color_texture,
            &material.normal_map_texture,
            &material.metallic_roughness_texture,
        ]
        .into_iter()
        .flatten()
        {
            assert!(images.get(texture).is_some(), "{name}'s textures loaded");
        }
    }

    // His scene spawns with one animation player over the 65 joints.
    let scene = model.scenes[0].clone();
    let clips: Vec<Handle<AnimationClip>> = CLIPS
        .iter()
        .map(|clip| model.named_animations[*clip].clone())
        .collect();
    let (graph, nodes) = AnimationGraph::from_clips(clips);
    let graph = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    app.world_mut()
        .spawn(bevy::world_serialization::WorldAssetRoot(scene));
    until(&mut app, "spawning the scene", |app| {
        let mut players = app.world_mut().query::<&AnimationPlayer>();
        players.iter(app.world()).count() == 1
    });
    let mut players = app
        .world_mut()
        .query_filtered::<Entity, With<AnimationPlayer>>();
    let player = players.single(app.world()).unwrap();
    app.world_mut()
        .entity_mut(player)
        .insert(AnimationGraphHandle(graph));
    app.update();
    let rest = pose(&mut app);
    assert_eq!(rest.len(), 65, "every joint is animated");

    // Each clip, played alone for a third of a second, turns his joints.
    for (clip, node) in CLIPS.iter().zip(&nodes) {
        let mut animation = app.world_mut().get_mut::<AnimationPlayer>(player).unwrap();
        animation.stop_all();
        animation.play(*node).repeat();
        for _ in 0..20 {
            app.update();
        }
        let posed = pose(&mut app);
        let turn = largest_turn(&rest, &posed);
        assert!(
            turn > 5.0,
            "{clip} turns a joint by more than 5° (it turned {turn:.1}°)"
        );
    }
}
