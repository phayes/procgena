//! The lumberjack's clips, chosen and timed by how his body moves.

use std::time::Duration;

use avian3d::prelude::LinearVelocity;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy_tnua::prelude::*;

use crate::player::{Moves, Player, PlayerModel};
use crate::settings::DemoOptions;

/// The lumberjack's model and clips, from the repository root. Made by
/// `tools/prepare_lumberjack.py`.
pub const MODEL: &str = "demo/assets/lumberjack/lumberjack.glb";

/// How long one clip takes to blend into the next.
const BLEND: Duration = Duration::from_millis(200);

/// How long the landing clip plays before walking takes over, in seconds.
const LANDING: f32 = 0.35;

/// Plays the lumberjack's clips.
pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_model)
            .add_systems(Update, (attach_graph, choose_clip).chain());
    }
}

/// The model file, kept loaded so its clips can be found by name.
#[derive(Resource)]
struct ModelFile(Handle<Gltf>);

/// Which clip plays now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gait {
    /// Standing still.
    Idle,
    /// Walking.
    Walk,
    /// Running.
    Run,
    /// Leaving the ground.
    TakeOff,
    /// In the air.
    Air,
    /// Just come down.
    Land,
}

/// The clips in the lumberjack's animation graph, and which plays now.
#[derive(Component)]
struct Clips {
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    run: AnimationNodeIndex,
    take_off: AnimationNodeIndex,
    air: AnimationNodeIndex,
    land: AnimationNodeIndex,
    gait: Gait,
    /// Seconds since the gait last changed.
    since: f32,
}

impl Clips {
    /// The graph node playing `gait`.
    fn node(&self, gait: Gait) -> AnimationNodeIndex {
        match gait {
            Gait::Idle => self.idle,
            Gait::Walk => self.walk,
            Gait::Run => self.run,
            Gait::TakeOff => self.take_off,
            Gait::Air => self.air,
            Gait::Land => self.land,
        }
    }
}

/// Starts loading the model file.
fn load_model(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(ModelFile(assets.load(MODEL)));
}

/// Gives the model's animation player the lumberjack's graph, once his
/// scene has spawned and his file has loaded.
fn attach_graph(
    mut commands: Commands,
    file: Res<ModelFile>,
    files: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    players: Query<Entity, (With<AnimationPlayer>, Without<Clips>)>,
    parents: Query<&ChildOf>,
    models: Query<(), With<PlayerModel>>,
) {
    for entity in &players {
        // Only the lumberjack's own player.
        if !parents
            .iter_ancestors(entity)
            .any(|ancestor| models.contains(ancestor))
        {
            continue;
        }
        let Some(file) = files.get(&file.0) else {
            return;
        };
        let clip = |name: &str| {
            file.named_animations
                .get(name)
                .unwrap_or_else(|| panic!("{MODEL} has no clip {name}: make it again"))
                .clone()
        };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let mut add = |name: &str| graph.add_clip(clip(name), 1.0, root);
        let clips = Clips {
            idle: add("Idle_Loop"),
            walk: add("Walk_Loop"),
            run: add("Jog_Fwd_Loop"),
            take_off: add("Jump_Start"),
            air: add("Jump_Loop"),
            land: add("Jump_Land"),
            gait: Gait::Idle,
            since: 0.0,
        };
        let mut transitions = AnimationTransitions::new();
        let mut player = AnimationPlayer::default();
        transitions
            .play(&mut player, clips.idle, Duration::ZERO)
            .repeat();
        commands.entity(entity).insert((
            AnimationGraphHandle(graphs.add(graph)),
            player,
            transitions,
            clips,
        ));
    }
}

/// The lumberjack's controller and how fast his body moves.
type Body<'a> = (&'a TnuaController<Moves>, &'a LinearVelocity);

/// Picks the clip from how the body moves, and plays walking and running
/// at the rate that keeps the feet still on the ground.
fn choose_clip(
    time: Res<Time>,
    options: Res<DemoOptions>,
    body: Option<Single<Body, With<Player>>>,
    mut animated: Query<(&mut Clips, &mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(body) = body else { return };
    let (controller, velocity) = *body;
    let Ok((mut clips, mut player, mut transitions)) = animated.single_mut() else {
        return;
    };
    let walking = &options.walking;
    clips.since += time.delta_secs();

    // Choose the gait.
    let airborne = controller.is_airborne().unwrap_or(false);
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let gait = if airborne {
        if velocity.y > 0.5 {
            Gait::TakeOff
        } else {
            Gait::Air
        }
    } else if matches!(clips.gait, Gait::TakeOff | Gait::Air)
        || (clips.gait == Gait::Land && clips.since < LANDING)
    {
        Gait::Land
    } else if speed < walking.idle_below {
        Gait::Idle
    } else if speed < walking.run_above {
        Gait::Walk
    } else {
        Gait::Run
    };

    // Blend into it when it changes.
    if gait != clips.gait {
        let node = clips.node(gait);
        let playing = transitions.play(&mut player, node, BLEND);
        if matches!(gait, Gait::Idle | Gait::Walk | Gait::Run | Gait::Air) {
            playing.repeat();
        }
        clips.gait = gait;
        clips.since = 0.0;
    }

    // Time the feet to the ground.
    let rate = match gait {
        Gait::Walk => speed / walking.walk_clip_speed,
        Gait::Run => speed / walking.run_clip_speed,
        Gait::Land => 2.0,
        Gait::Idle | Gait::TakeOff | Gait::Air => 1.0,
    };
    let node = clips.node(gait);
    if let Some(active) = player.animation_mut(node) {
        active.set_speed(rate);
    }
}
