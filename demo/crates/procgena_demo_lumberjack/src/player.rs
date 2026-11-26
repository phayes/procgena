//! The lumberjack: his body, walking, the input that drives him and the
//! camera that follows him.

use std::f32::consts::{FRAC_PI_2, PI};

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use bevy_enhanced_input::prelude::*;
use bevy_tnua::builtins::{
    TnuaBuiltinJump, TnuaBuiltinJumpConfig, TnuaBuiltinWalk, TnuaBuiltinWalkConfig,
};
use bevy_tnua::prelude::*;
use bevy_tnua_avian3d::prelude::*;
use procgena_plant_bevy::PlantInteractorFromCollider;

use crate::GameState;
use crate::animation::{AnimationPlugin, MODEL};
use crate::camera::MainCamera;
use crate::settings::DemoOptions;

/// The radius of the lumberjack's body, in metres.
pub const BODY_RADIUS: f32 = 0.3;

/// The length of the straight middle of his body's capsule, in metres: with
/// its rounded ends, he is 1.7 m tall.
const BODY_LENGTH: f32 = 1.1;

/// How high the middle of his body floats above the ground, in metres. A
/// little more than half his height, so the controller can step him over
/// roots and stones.
pub const FLOAT_HEIGHT: f32 = 1.0;

/// The nearest the third-person camera comes to the ground, in metres.
const CAMERA_CLEARANCE: f32 = 0.4;

/// The lumberjack's movements on top of walking.
#[derive(TnuaScheme)]
#[scheme(basis = TnuaBuiltinWalk)]
pub enum Moves {
    /// A jump.
    Jump(TnuaBuiltinJump),
}

/// Marks the lumberjack's body.
#[derive(Component, Debug, Default, Clone)]
pub struct Player;

/// Marks the entity holding his model, a child of his body.
#[derive(Component, Debug)]
pub struct PlayerModel;

/// Where the camera looks from, and which view it is in.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The view's turn about the vertical, in radians; 0 looks along −Z.
    pub yaw: f32,
    /// The view's tilt, in radians; positive looks up.
    pub pitch: f32,
    /// Seeing through his eyes instead of from behind him.
    pub first_person: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.15,
            first_person: false,
        }
    }
}

/// Where he should stand when play starts, from the land.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Spawn(pub Vec3);

/// What the player asks for this frame, from the input or from a script.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct Intent {
    /// Which way to move, relative to the view: +Y ahead, +X to the right.
    /// Its length, up to 1, is how hard.
    pub motion: Vec2,
    /// Run instead of walk.
    pub run: bool,
    /// Jump while held.
    pub jump: bool,
}

/// Moving about, on foot.
#[derive(Component, Debug, Default, Clone)]
pub struct OnFoot;

/// Which way to move.
#[derive(InputAction)]
#[action_output(Vec2)]
struct Move;

/// How the view turns.
#[derive(InputAction)]
#[action_output(Vec2)]
struct Look;

/// Run while held.
#[derive(InputAction)]
#[action_output(bool)]
struct Run;

/// Jump while held.
#[derive(InputAction)]
#[action_output(bool)]
struct Jump;

/// Switch between the third- and first-person view.
#[derive(InputAction)]
#[action_output(bool)]
struct ToggleView;

/// Pause and go on.
#[derive(InputAction)]
#[action_output(bool)]
struct Pause;

/// Spawns the lumberjack and moves him and the camera.
pub struct PlayerPlugin {
    /// Read [`Intent`] from the keyboard, mouse and gamepad. Off when a
    /// script drives him.
    pub input: bool,
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PhysicsPlugins::default(),
            TnuaControllerPlugin::<Moves>::new(FixedUpdate),
            TnuaAvian3dPlugin::new(FixedUpdate),
            AnimationPlugin,
        ))
        .init_resource::<View>()
        .init_resource::<Intent>()
        .add_systems(OnEnter(GameState::Playing), spawn_player)
        .add_systems(OnEnter(GameState::Paused), pause_physics)
        .add_systems(OnExit(GameState::Paused), resume_physics)
        .add_systems(
            Update,
            drive
                .in_set(TnuaUserControlsSystems)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            PostUpdate,
            follow
                .after(PhysicsSystems::Writeback)
                .before(TransformSystems::Propagate),
        );
        if self.input {
            app.add_plugins(EnhancedInputPlugin)
                .add_input_context::<OnFoot>()
                .add_observer(read_move)
                .add_observer(stop_moving)
                .add_observer(read_look)
                .add_observer(toggle_view)
                .add_observer(toggle_pause)
                .add_systems(PreUpdate, read_buttons.after(EnhancedInputSystems::Update));
        }
    }
}

/// Spawns the lumberjack at [`Spawn`], with his model and his input.
fn spawn_player(
    mut commands: Commands,
    mut configs: ResMut<Assets<MovesConfig>>,
    assets: Res<AssetServer>,
    spawn: Res<Spawn>,
    options: Res<DemoOptions>,
) {
    let walking = &options.walking;
    let config = configs.add(MovesConfig {
        basis: TnuaBuiltinWalkConfig {
            speed: walking.run_speed,
            float_height: FLOAT_HEIGHT,
            max_slope: walking.max_slope.to_radians(),
            ..default()
        },
        jump: TnuaBuiltinJumpConfig {
            height: walking.jump_height,
            ..default()
        },
    });
    let body = commands
        .spawn((
            Player,
            OnFoot,
            Transform::from_translation(spawn.0 + Vec3::Y * FLOAT_HEIGHT),
            RigidBody::Dynamic,
            Collider::capsule(BODY_RADIUS, BODY_LENGTH),
            TnuaController::<Moves>::default(),
            TnuaConfig::<Moves>(config),
            TnuaAvian3dSensorShape(Collider::cylinder(BODY_RADIUS - 0.01, 0.0)),
            LockedAxes::ROTATION_LOCKED.unlock_rotation_y(),
            PlantInteractorFromCollider::default(),
        ))
        .apply_scene(input_actions())
        .id();

    // The model's feet stand on the ground below the floating body; it
    // faces +Z, his body's back.
    commands.spawn((
        PlayerModel,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL))),
        Transform::from_xyz(0.0, -FLOAT_HEIGHT, 0.0).with_rotation(Quat::from_rotation_y(PI)),
        ChildOf(body),
    ));
}

/// The lumberjack's actions and their keys, mouse and gamepad bindings.
fn input_actions() -> impl Scene {
    bsn! {
        Actions<OnFoot> [
            Action::<Move>::new()
            DeadZone
            Bindings [
                {preset::cardinal::wasd_keys()}
                --
                {preset::axial::left_stick()}
            ]
            --
            Action::<Look>::new()
            Bindings [
                Binding::mouse_motion()
            ]
            --
            Action::<Run>::new()
            Bindings [
                Binding(KeyCode::ShiftLeft)
                --
                Binding(GamepadButton::LeftThumb)
            ]
            --
            Action::<Jump>::new()
            Bindings [
                Binding(KeyCode::Space)
                --
                Binding(GamepadButton::South)
            ]
            --
            Action::<ToggleView>::new()
            Press
            Bindings [
                Binding(KeyCode::KeyV)
                --
                Binding(GamepadButton::North)
            ]
            --
            Action::<Pause>::new()
            Press
            Bindings [
                Binding(KeyCode::Escape)
                --
                Binding(GamepadButton::Start)
            ]
        ]
    }
}

/// Reads which way to move.
fn read_move(moving: On<Fire<Move>>, mut intent: ResMut<Intent>) {
    intent.motion = moving.value.clamp_length_max(1.0);
}

/// Stops moving when the keys and stick are let go.
fn stop_moving(_: On<Complete<Move>>, mut intent: ResMut<Intent>) {
    intent.motion = Vec2::ZERO;
}

/// Turns the view with the mouse or stick.
fn read_look(
    looking: On<Fire<Look>>,
    mut view: ResMut<View>,
    options: Res<DemoOptions>,
    state: Res<State<GameState>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let turn = looking.value * options.camera.mouse_sensitivity;
    view.yaw -= turn.x;
    view.pitch = (view.pitch - turn.y).clamp(-FRAC_PI_2 + 0.05, FRAC_PI_2 - 0.05);
}

/// Reads the held buttons: run and jump.
fn read_buttons(
    run: Single<&Action<Run>>,
    jump: Single<&Action<Jump>>,
    mut intent: ResMut<Intent>,
) {
    intent.run = ***run;
    intent.jump = ***jump;
}

/// Switches between the third- and first-person view.
fn toggle_view(_: On<Start<ToggleView>>, mut view: ResMut<View>) {
    view.first_person = !view.first_person;
}

/// Pauses or goes on.
fn toggle_pause(
    _: On<Start<Pause>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    match state.get() {
        GameState::Playing => next.set(GameState::Paused),
        GameState::Paused => next.set(GameState::Playing),
        GameState::Loading => {}
    }
}

/// Stops the physics while paused.
fn pause_physics(mut time: ResMut<Time<Physics>>) {
    time.pause();
}

/// Starts the physics again.
fn resume_physics(mut time: ResMut<Time<Physics>>) {
    time.unpause();
}

/// Feeds [`Intent`] to the controller, turned to the view: walk or run
/// towards it, facing where he goes, and jump while asked.
fn drive(
    intent: Res<Intent>,
    view: Res<View>,
    options: Res<DemoOptions>,
    mut controller: Single<&mut TnuaController<Moves>>,
) {
    controller.initiate_action_feeding();
    let walking = &options.walking;
    let pace = if intent.run {
        1.0
    } else {
        walking.walk_speed / walking.run_speed
    };
    let turn = Quat::from_rotation_y(view.yaw);
    let ahead = turn * Vec3::NEG_Z;
    let right = turn * Vec3::X;
    let motion = (ahead * intent.motion.y + right * intent.motion.x) * pace;
    controller.basis = TnuaBuiltinWalk {
        desired_motion: motion,
        desired_forward: Dir3::new(motion).ok(),
    };
    if intent.jump {
        controller.action(Moves::Jump(Default::default()));
    }
}

/// The lumberjack's body, never the camera.
type OnlyPlayer = (With<Player>, Without<MainCamera>);

/// Places the camera: behind and above him looking past his shoulder, kept
/// out of the ground, or in his eyes, with his model hidden.
fn follow(
    view: Res<View>,
    options: Res<DemoOptions>,
    player: Option<Single<(Entity, &Transform), OnlyPlayer>>,
    mut model: Query<&mut Visibility, With<PlayerModel>>,
    mut camera: Single<&mut Transform, With<MainCamera>>,
    spatial: SpatialQuery,
) {
    let Some(player) = player else { return };
    let (body, transform) = *player;
    let feet = transform.translation - Vec3::Y * FLOAT_HEIGHT;
    let look = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch, 0.0);
    let rig = &options.camera;
    let (eye, visible) = if view.first_person {
        (feet + Vec3::Y * rig.eye_height, Visibility::Hidden)
    } else {
        // Back off along the view until the ground or a trunk is in the way.
        let pivot = feet + Vec3::Y * rig.look_height;
        let back = Dir3::new(look * Vec3::Z).expect("a rotation keeps length");
        let filter = SpatialQueryFilter::from_excluded_entities([body]);
        let distance = spatial
            .cast_ray(pivot, back, rig.distance, true, &filter)
            .map_or(rig.distance, |hit| {
                (hit.distance - CAMERA_CLEARANCE).max(0.3)
            });
        (pivot + back * distance, Visibility::Inherited)
    };
    camera.translation = eye;
    camera.rotation = look;
    for mut model in &mut model {
        model.set_if_neq(visible);
    }
}
