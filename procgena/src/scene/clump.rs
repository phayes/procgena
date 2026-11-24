//! Clump subject: a clump, such as snow or moss, grown on a tree or rock.
//! Also grows the dead wood subject's cover on its piece.

use std::fmt;
use std::sync::Arc;

use bevy::prelude::*;
use procgena_clump::ClumpOptions;
use procgena_clump_bevy::{ClumpFinished, ClumpGenerating, ClumpMaterial, ClumpParams, ClumpPart};
use procgena_stone::{OptionsError as StoneOptionsError, StoneOptions};
use procgena_stone_bevy::{
    StoneEmbedDepth, StoneFinished, StoneGenerating, StoneParams, StonePart,
};
use procgena_tree::{OptionsError, TreeOptions};
use procgena_tree_bevy::{TreeFinished, TreeGenerating, TreeParams};

use super::deadwood::{DeadwoodSubject, DeadwoodView};
use super::orbit::OrbitCamera;
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::stone::{settle_rock, stone_view};
use super::subject::{Detail, SceneState, Subject};
use super::tree::tree_view;
use super::viewport::ViewportActivity;
use super::{AddSubject, Shown};

/// What the clump subject shows: a host, and the clump grown on it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpSettings {
    /// The tree or rock the clump grows on.
    pub subject: ClumpHost,
    /// The clump's settings, including how it looks. Lengths are in metres,
    /// like the host.
    pub clump: ClumpOptions,
}

impl Default for ClumpSettings {
    /// A fresh snow cap on a rock with default settings.
    fn default() -> Self {
        Self {
            subject: ClumpHost::Stone(Arc::default()),
            clump: ClumpOptions::default(),
        }
    }
}

/// The tree or rock a clump grows on.
///
/// A clump grows on a tree's trunk and branches at full detail, not on its
/// leaves, fruit or flowers, and on a rock's most detailed mesh.
#[derive(Debug, Clone, PartialEq)]
pub enum ClumpHost {
    /// A tree built from these options.
    Tree(Arc<TreeOptions>),
    /// A rock built from these settings.
    Stone(Arc<StoneOptions>),
}

/// Why a clump could not be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum ClumpBuildError {
    /// The host tree's options were rejected.
    Tree(OptionsError),
    /// The host rock's settings were rejected.
    Stone(StoneOptionsError),
    /// The clump's settings were rejected, or its host can't carry one.
    Clump(procgena_clump_bevy::ClumpBuildError),
}

impl fmt::Display for ClumpBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tree(error) => write!(f, "cannot build the tree: {error}"),
            Self::Stone(error) => write!(f, "cannot build the rock: {error}"),
            Self::Clump(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ClumpBuildError {}

/// A clump grown on a tree or rock, from [`ClumpSettings`].
///
/// A changed host is rebuilt and then grows a new clump; changed clump
/// settings grow a new clump on the host already shown. Each clump build
/// reports a [`ViewerEvent::ClumpBuilt`]. The host is drawn at full detail,
/// which the clump grows on, in still air, since the clump would not sway
/// with its tree.
pub struct ClumpSubject;

impl Subject for ClumpSubject {
    type Settings = ClumpSettings;
    const NAME: &'static str = "clump";
    const STILL_AIR: bool = true;

    fn spawn(world: &mut World, settings: &ClumpSettings) -> Entity {
        match &settings.subject {
            ClumpHost::Tree(options) => world.spawn(TreeParams::Options(options.clone())),
            ClumpHost::Stone(options) => world.spawn(StoneParams::Options(options.clone())),
        }
        .id()
    }

    fn update(world: &mut World, shown: &mut Shown, old: &ClumpSettings, new: &ClumpSettings) {
        // A new clump alone is grown by `show_clump`; a new host is rebuilt
        // here, and the clump follows once it is built.
        match (&old.subject, &new.subject) {
            (old, new) if old == new => {}
            // Keep the old tree or rock shown while a new one of the same kind builds.
            (ClumpHost::Tree(_), ClumpHost::Tree(options)) => {
                let target = shown.replacement::<TreeGenerating>(world);
                world
                    .entity_mut(target)
                    .insert(TreeParams::Options(options.clone()));
            }
            (ClumpHost::Stone(_), ClumpHost::Stone(options)) => {
                let target = shown.replacement::<StoneGenerating>(world);
                world
                    .entity_mut(target)
                    .insert(StoneParams::Options(options.clone()));
            }
            // A tree replaces a rock, or a rock a tree, at once.
            _ => {
                let entity = Self::spawn(world, new);
                shown.replace_with(world, entity);
            }
        }
    }

    fn view(_: &World, settings: &ClumpSettings) -> OrbitCamera {
        match &settings.subject {
            ClumpHost::Tree(_) => tree_view(),
            ClumpHost::Stone(rock) => stone_view(rock),
        }
    }

    fn detail(_: Detail) -> Detail {
        Detail::Level(0)
    }
}

/// Adds the clump subject, and the dead wood subject's cover.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<ClumpSubject>()
        .init_resource::<ShownClump>()
        .init_resource::<ShownCover>()
        .add_systems(Update, render_while_loading)
        .add_observer(report_clump)
        .add_observer(report_tree_host)
        .add_observer(report_stone_host);
}

/// The clump subject's clump: a [`ClumpParams`] child of the shown host, or
/// `None` when no clump subject is shown.
#[derive(Resource, Default)]
pub(crate) struct ShownClump(Option<Entity>);

/// Keeps one clump on the clump subject's host, with the current settings,
/// and removes it when another subject is shown.
///
/// A new host gets a new clump, which grows once the host is built.
/// Changed settings replace the clump's settings: the previous clump stays
/// shown until the new one is ready, and a new colour alone only recolours
/// it.
pub(crate) fn show_clump(
    mut shown: ResMut<ShownClump>,
    state: Res<SceneState>,
    commands: Commands,
    clumps: Query<(&ClumpParams, &ChildOf, Has<ClumpGenerating>)>,
    activity: ResMut<ViewportActivity>,
) {
    let wanted = state
        .get::<ClumpSubject>()
        .map(|(host, settings)| (host.entity, settings.clump.clone()));
    keep_clump(&mut shown.0, wanted, commands, &clumps, activity);
}

/// The cover, such as moss, grown on the dead wood subject's piece: a
/// [`ClumpParams`] child of the piece, or `None` when there is none.
#[derive(Resource, Default)]
pub(crate) struct ShownCover(Option<Entity>);

/// Keeps the chosen cover growing on the dead wood subject's piece, and
/// removes it for other subjects, for scatters, or when no cover is chosen.
///
/// A replaced piece gets a new cover, which grows once the piece is built.
/// A changed cover replaces the old one's settings.
pub(crate) fn show_deadwood_cover(
    mut shown: ResMut<ShownCover>,
    state: Res<SceneState>,
    commands: Commands,
    covers: Query<(&ClumpParams, &ChildOf, Has<ClumpGenerating>)>,
    activity: ResMut<ViewportActivity>,
) {
    let wanted = state
        .get::<DeadwoodSubject>()
        .filter(|(_, settings)| settings.view == DeadwoodView::Piece)
        .and_then(|(piece, settings)| Some((piece.entity, settings.cover?.clump.clone())));
    keep_clump(&mut shown.0, wanted, commands, &covers, activity);
}

/// Keeps the clump `shown` growing with `wanted`'s settings on `wanted`'s
/// host, or removes it with no `wanted`.
///
/// A replaced host took its clump with it, so it gets a new one, which
/// grows once the host is built. Changed settings replace the clump's
/// settings.
fn keep_clump(
    shown: &mut Option<Entity>,
    wanted: Option<(Entity, ClumpOptions)>,
    mut commands: Commands,
    clumps: &Query<(&ClumpParams, &ChildOf, Has<ClumpGenerating>)>,
    mut activity: ResMut<ViewportActivity>,
) {
    let Some((host, wanted)) = wanted else {
        if let Some(clump) = shown.take()
            && let Ok(mut clump) = commands.get_entity(clump)
        {
            clump.despawn();
        }
        return;
    };
    let current = shown.and_then(|clump| Some((clump, clumps.get(clump).ok()?)));
    match current {
        Some((clump, (params, parent, generating))) if parent.parent() == host => {
            if *params.0 != wanted {
                commands.entity(clump).insert(ClumpParams::new(wanted));
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
            if generating {
                activity.request_frames(ViewportActivity::SETTLE_FRAMES);
            }
        }
        stale => {
            if let Some((clump, _)) = stale
                && let Ok(mut clump) = commands.get_entity(clump)
            {
                clump.despawn();
            }
            let clump = commands
                .spawn((ClumpParams::new(wanted), ChildOf(host)))
                .id();
            *shown = Some(clump);
            activity.request_frames(ViewportActivity::SETTLE_FRAMES);
        }
    }
}

/// Swaps in a finished replacement host tree, and reports a rejected one,
/// which can carry no clump.
fn report_tree_host(
    event: On<TreeFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some(shown) = state.get_mut::<ClumpSubject>() else {
        return;
    };
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }
    if let Err(error) = &event.result {
        let error = ClumpBuildError::Tree(error.clone());
        events.write(ViewerEvent::ClumpBuilt(Err(error)));
    }
}

/// Swaps in a finished replacement host rock and sinks it into the ground,
/// and reports a rejected one, which can carry no clump.
fn report_stone_host(
    event: On<StoneFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    rocks: Query<(&StoneParams, Option<&Children>, Option<&StoneEmbedDepth>)>,
    parts: Query<(&StonePart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    let Some(shown) = state.get_mut::<ClumpSubject>() else {
        return;
    };
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }
    match &event.result {
        Ok(()) => {
            settle_rock(&mut commands, event.entity, &rocks, &parts, &meshes);
        }
        Err(error) => {
            let error = ClumpBuildError::Stone(error.clone());
            events.write(ViewerEvent::ClumpBuilt(Err(error)));
        }
    }
}

/// Requests more frames while the clump's textures are loading, so an idle
/// viewport still shows the textured clump.
fn render_while_loading(
    parts: Query<&MeshMaterial3d<ClumpMaterial>, With<ClumpPart>>,
    materials: Res<Assets<ClumpMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| super::is_loading(&server, texture);
    let textures_loading = parts
        .iter()
        .filter_map(|part| materials.get(&part.0))
        .any(|material| {
            let textures = &material.extension;
            loading(&textures.color) || loading(&textures.normal) || loading(&textures.orh)
        });
    if textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Reports each finished build of the shown clump with
/// [`ViewerEvent::ClumpBuilt`]: its mesh size, as a single detail level, and
/// build time, or why it couldn't be built.
fn report_clump(
    finished: On<ClumpFinished>,
    shown: Res<ShownClump>,
    mut events: MessageWriter<ViewerEvent>,
    mut activity: ResMut<ViewportActivity>,
) {
    if shown.0 != Some(finished.entity) {
        return;
    }
    activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    let result = finished
        .result
        .clone()
        .map(|built| BuildStats {
            levels: vec![MeshSize {
                vertices: built.vertices,
                triangles: built.triangles,
            }],
            build_time: built.build_time,
        })
        .map_err(ClumpBuildError::Clump);
    events.write(ViewerEvent::ClumpBuilt(result));
}
