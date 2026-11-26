//! A scripted walk across the play area, for checking walking without a
//! person at the keys: `--autowalk`.
//!
//! The lumberjack runs a loop around the start that crosses tile borders,
//! then out to the nearest road and along it, and back. Each frame checks
//! that he is above the ground, not stuck, and not inside a trunk. At the
//! end it prints what it found and exits, with an error if anything went
//! wrong.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::*;
use procgena_biome::ScatterCopies;
use procgena_biome_bevy::EcotopeCopies;
use procgena_terrain::{Flags, Landform};

use crate::GameState;
use crate::land::Land;
use crate::player::{BODY_RADIUS, FLOAT_HEIGHT, Intent, Player, Spawn, View};

/// How near a waypoint counts as reaching it, in metres.
const REACHED: f32 = 2.0;

/// How long without moving counts as stuck, in seconds.
const STUCK_AFTER: f32 = 3.0;

/// How far below the ground his feet may sink before it counts as falling
/// through, in metres.
const SINK_LIMIT: f32 = 0.35;

/// How far his body may overlap a trunk before it counts as passing
/// through, in metres.
const OVERLAP_LIMIT: f32 = 0.1;

/// The side of the loop around the start, in metres.
const LOOP_SIDE: f32 = 300.0;

/// How far out a road is looked for, in metres.
const ROAD_SEARCH: f32 = 700.0;

/// Drives the lumberjack along the route and checks each step.
pub struct AutowalkPlugin;

impl Plugin for AutowalkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), plan_route)
            .add_systems(
                Update,
                walk.run_if(in_state(GameState::Playing))
                    .run_if(resource_exists::<Route>),
            );
    }
}

/// The waypoints still to reach, and what the walk has found so far.
#[derive(Resource, Debug, Default)]
pub struct Route {
    /// Waypoints, World x and z, in metres; the next is first.
    points: Vec<Vec2>,
    /// What each waypoint is for, for the report.
    names: Vec<&'static str>,
    /// Seconds walked.
    time: f32,
    /// Metres walked.
    distance: f32,
    /// Where he was last frame.
    last: Option<Vec3>,
    /// Seconds he has not moved while asked to.
    still: f32,
    /// Times he sank through the ground, with where.
    falls: Vec<Vec3>,
    /// Waypoints given up on because he was stuck, with where.
    stuck: Vec<Vec3>,
    /// Times his body went into a trunk, with where.
    in_trunks: Vec<Vec3>,
    /// The deepest his feet went below the ground, in metres.
    deepest: f32,
    /// Metres walked on a road.
    on_road: f32,
    /// Trunks near the route: where each stands and its radius.
    trunks: Vec<(Vec2, f32)>,
}

/// Plans the route: a square loop around the start, then to the nearest
/// road, along it and back.
fn plan_route(
    mut commands: Commands,
    spawn: Res<Spawn>,
    land: Res<Land>,
    tiles: Query<&EcotopeCopies>,
) {
    let start = Vec2::new(spawn.0.x, spawn.0.z);
    let half = LOOP_SIDE * 0.5;
    let mut points = vec![
        start + Vec2::new(half, 0.0),
        start + Vec2::new(half, half),
        start + Vec2::new(-half, half),
        start + Vec2::new(-half, -half),
        start + Vec2::new(half, -half),
        start + Vec2::new(half, 0.0),
        start,
    ];
    let mut names = vec!["loop"; points.len()];

    // The nearest road, by rings of samples, then along it.
    if let Some((road, along)) = nearest_road(&land, start) {
        points.extend([road, road + along * 60.0, road - along * 60.0, start]);
        names.extend([
            "to the road",
            "along the road",
            "back along the road",
            "home",
        ]);
    } else {
        warn!("autowalk: no road within {ROAD_SEARCH} m of the start");
    }

    // Every trunk, for the overlap check.
    let trunks = tiles
        .iter()
        .flat_map(|copies| {
            let centre = copies.area().centre.as_vec2();
            copies
                .placement()
                .layers()
                .iter()
                .filter_map(|layer| match layer.copies() {
                    ScatterCopies::Tree(trees) => Some(trees.instances()),
                    _ => None,
                })
                .flatten()
                .map(move |tree| {
                    (
                        centre + tree.position.xz(),
                        tree.footprint.radius as f32 / 256.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    info!("autowalk: {} waypoints", points.len());
    commands.insert_resource(Route {
        points,
        names,
        trunks,
        ..default()
    });
}

/// The nearest point on a road within [`ROAD_SEARCH`] of `start`, and the
/// road's direction there.
fn nearest_road(land: &Land, start: Vec2) -> Option<(Vec2, Vec2)> {
    let is_road = |p: Vec2| {
        land.terrain
            .annotate(p.as_dvec2())
            .is_some_and(|l| l.flags.contains(Flags::ROAD) || l.landform == Landform::Road)
    };
    let mut radius = 20.0;
    while radius <= ROAD_SEARCH {
        let steps = (radius * std::f32::consts::TAU / 10.0) as usize;
        for k in 0..steps {
            let angle = k as f32 / steps as f32 * std::f32::consts::TAU;
            let p = start + Vec2::from_angle(angle) * radius;
            if !is_road(p) {
                continue;
            }
            // The road's direction: the way that stays on it longest.
            let along = (0..36)
                .map(|a| Vec2::from_angle(a as f32 * std::f32::consts::PI / 36.0))
                .max_by_key(|d| {
                    (1..=10)
                        .filter(|s| is_road(p + *d * (*s as f32 * 3.0)))
                        .count()
                })
                .expect("36 directions");
            return Some((p, along));
        }
        radius += 20.0;
    }
    None
}

/// Points him at the next waypoint, checks the step he took, and reports
/// and exits once the route is walked.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn walk(
    mut commands: Commands,
    time: Res<Time>,
    land: Res<Land>,
    mut route: ResMut<Route>,
    mut intent: ResMut<Intent>,
    mut view: ResMut<View>,
    player: Option<Single<(&Transform, &LinearVelocity), With<Player>>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(player) = player else { return };
    let (transform, velocity) = *player;
    let dt = time.delta_secs();
    let body = transform.translation;
    let feet = body - Vec3::Y * FLOAT_HEIGHT;
    let here = Vec2::new(body.x, body.z);
    route.time += dt;
    if let Some(last) = route.last {
        let step = Vec2::new(body.x - last.x, body.z - last.z).length();
        route.distance += step;
        let on_road = land
            .terrain
            .annotate(here.as_dvec2())
            .is_some_and(|l| l.flags.contains(Flags::ROAD));
        if on_road {
            route.on_road += step;
        }
    }
    route.last = Some(body);

    // Above the ground?
    let ground = land.height(here);
    let sink = ground - feet.y;
    route.deepest = route.deepest.max(sink);
    if sink > SINK_LIMIT && route.falls.last().is_none_or(|p| p.distance(body) > 5.0) {
        error!("autowalk: sank {sink:.2} m below the ground at {body}");
        route.falls.push(body);
    }

    // Inside a trunk?
    let overlap = route
        .trunks
        .iter()
        .map(|(at, radius)| radius + BODY_RADIUS - at.distance(here))
        .fold(f32::MIN, f32::max);
    if overlap > OVERLAP_LIMIT
        && route
            .in_trunks
            .last()
            .is_none_or(|p| p.distance(body) > 2.0)
    {
        error!("autowalk: {overlap:.2} m inside a trunk at {body}");
        route.in_trunks.push(body);
    }

    // On to the next waypoint, or report.
    let Some(&target) = route.points.first() else {
        report(&route, &mut exit);
        intent.motion = Vec2::ZERO;
        commands.remove_resource::<Route>();
        return;
    };
    let to = target - here;
    let horizontal = Vec2::new(velocity.x, velocity.z).length();
    route.still = if horizontal < 0.3 {
        route.still + dt
    } else {
        0.0
    };
    let reached = to.length() < REACHED;
    if reached || route.still > STUCK_AFTER {
        if !reached {
            error!(
                "autowalk: stuck at {body} on the way to {target} ({})",
                route.names[0]
            );
            route.stuck.push(body);
        }
        route.points.remove(0);
        route.names.remove(0);
        route.still = 0.0;
        return;
    }
    // Face the waypoint and run straight at it.
    view.yaw = (-to.x).atan2(-to.y);
    intent.motion = Vec2::Y;
    intent.run = true;
    intent.jump = false;
}

/// Prints the walk's findings and exits, with an error if he fell, got
/// stuck or went into a trunk.
fn report(route: &Route, exit: &mut MessageWriter<AppExit>) {
    println!(
        "autowalk: {:.0} m in {:.0} s ({:.0} m on roads) | fell through: {} | stuck: {} | in \
         trunks: {} | deepest below ground: {:.2} m",
        route.distance,
        route.time,
        route.on_road,
        route.falls.len(),
        route.stuck.len(),
        route.in_trunks.len(),
        route.deepest,
    );
    let ok = route.falls.is_empty() && route.stuck.is_empty() && route.in_trunks.is_empty();
    exit.write(if ok {
        AppExit::Success
    } else {
        AppExit::error()
    });
}
