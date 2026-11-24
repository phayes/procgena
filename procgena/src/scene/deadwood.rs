//! Dead wood subject: a single fallen log, branch or stump, with any cover
//! grown on it, or a scatter of sticks.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use procgena_clump::ClumpPreset;
use procgena_deadwood::{DeadwoodOptions, PieceKind, ScatterOptions, default_lod, size_factor};
use procgena_deadwood_bevy::{
    DeadwoodFinished, DeadwoodGenerating, DeadwoodMaterial, DeadwoodParams, DeadwoodPart,
    DeadwoodScatterParams, ScatterGround,
};

use super::orbit::{OrbitCamera, OrbitLimits};
use super::protocol::{BuildStats, MeshSize, ViewerEvent};
use super::subject::{
    LevelPart, SavedRange, SceneState, Subject, SubjectLook, level_in_range, show_level,
};
use super::viewport::{ViewportActivity, ViewportCamera};
use super::{AddSubject, Detail, Shown};

/// The camera's nearest allowed distance, kept clear of the camera's 0.1 m
/// near clipping plane.
const DEADWOOD_NEAREST: f32 = 0.15;

/// What the dead wood subject shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeadwoodSettings {
    /// The log, branch or stump shown in [`DeadwoodView::Piece`].
    pub piece: DeadwoodOptions,
    /// A single piece, or a scatter of sticks.
    pub view: DeadwoodView,
    /// A clump, such as moss, grown on the piece, or `None` for bare wood.
    /// Scatters are always bare.
    pub cover: Option<&'static ClumpPreset>,
}

/// Whether the dead wood subject shows one piece or a scatter of sticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeadwoodView {
    /// The piece from [`DeadwoodSettings::piece`].
    #[default]
    Piece,
    /// [`ScatterOptions::kindling`](procgena_deadwood::ScatterOptions::kindling):
    /// sticks 1 to 7 cm thick on flat ground.
    Kindling,
    /// [`ScatterOptions::twig_litter`](procgena_deadwood::ScatterOptions::twig_litter):
    /// thin twigs on flat ground.
    TwigLitter,
}

/// A piece of dead wood or a scatter of sticks, from [`DeadwoodSettings`].
///
/// A changed piece is built in the background and replaces the old one once
/// ready, or stays if its settings are rejected; a changed view replaces it
/// at once. A changed cover alone regrows only the cover.
/// [`ViewerEvent::DeadwoodBuilt`] reports each piece built, and
/// [`ViewerEvent::DeadwoodLodShown`] its detail level on screen. The scene
/// uses its own detail levels, scaled to the piece's size, and ignores
/// `piece.detail`.
pub struct DeadwoodSubject;

impl Subject for DeadwoodSubject {
    type Settings = DeadwoodSettings;
    const NAME: &'static str = "dead wood";

    fn spawn(world: &mut World, settings: &DeadwoodSettings) -> Entity {
        match scatter_options(settings.view) {
            None => world.spawn(DeadwoodParams::new(settings.piece.clone())),
            Some((scatter, size)) => world.spawn(DeadwoodScatterParams {
                scatter,
                size: Vec2::splat(size),
                ground: ScatterGround::Flat,
            }),
        }
        .id()
    }

    fn update(
        world: &mut World,
        shown: &mut Shown,
        old: &DeadwoodSettings,
        new: &DeadwoodSettings,
    ) {
        if old.view != new.view {
            // A piece replaces a scatter, or one scatter another, at once.
            let entity = Self::spawn(world, new);
            shown.replace_with(world, entity);
        } else if new.view == DeadwoodView::Piece && old.piece != new.piece {
            // Keep the old piece shown while the new one builds. A changed
            // cover alone is regrown by `show_deadwood_cover`.
            let target = shown.replacement::<DeadwoodGenerating>(world);
            world
                .entity_mut(target)
                .insert(DeadwoodParams::new(new.piece.clone()));
        }
    }

    fn view(_: &World, settings: &DeadwoodSettings) -> OrbitCamera {
        let Some((_, size)) = scatter_options(settings.view) else {
            return piece_view(&settings.piece);
        };
        OrbitCamera {
            focus: Vec3::ZERO,
            yaw: 0.6,
            pitch: 0.5,
            distance: 1.2 * size,
            limits: OrbitLimits {
                distance: (DEADWOOD_NEAREST, 10.0 * size),
                pitch: (-0.05, 1.5),
            },
        }
    }
}

/// The scatter that `view` shows and the width of its square of ground in
/// metres, or `None` for a single piece.
fn scatter_options(view: DeadwoodView) -> Option<(ScatterOptions, f32)> {
    match view {
        DeadwoodView::Piece => None,
        DeadwoodView::Kindling => Some((ScatterOptions::kindling(), 6.0)),
        DeadwoodView::TwigLitter => Some((ScatterOptions::twig_litter(), 4.0)),
    }
}

/// Returns the starting view for the dead wood piece built from `options`:
/// aimed at the middle of the piece and far enough out to show all of it,
/// and allowed out to where it stops being drawn.
fn piece_view(options: &DeadwoodOptions) -> OrbitCamera {
    // A stump stands its length tall on a base widened by its flare; logs
    // and branches lie on the ground.
    let stump = options.kind == PieceKind::Stump;
    let (height, width) = if stump {
        (
            options.length,
            options.diameter * (1.0 + options.roots.flare),
        )
    } else {
        (options.diameter, options.diameter)
    };
    let extent = options.length.max(width);
    let lod = default_lod();
    let cull = lod.cull_distance.expect("the default levels cull") * size_factor(&lod, options);
    OrbitCamera {
        focus: Vec3::new(0.0, 0.5 * height, 0.0),
        yaw: 0.6,
        pitch: 0.35,
        distance: (1.6 * extent).max(DEADWOOD_NEAREST),
        limits: OrbitLimits {
            distance: (DEADWOOD_NEAREST, (1.2 * cull).max(2.0)),
            pitch: (-0.05, 1.5),
        },
    }
}

/// Adds the dead wood subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<DeadwoodSubject>()
        .add_systems(Update, (render_while_loading, report_lod))
        .add_observer(report_deadwood);
}

/// Shows each dead wood part as the shown subject's detail asks.
pub(crate) fn apply_lod(
    mut commands: Commands,
    look: Res<SubjectLook>,
    mut parts: Query<LevelPart<DeadwoodPart>>,
) {
    for (entity, part, visibility, range, saved) in &mut parts {
        let ranges = (range, saved);
        show_level(
            &mut commands,
            entity,
            part.lod,
            look.detail,
            visibility,
            ranges,
        );
    }
}

/// Reports the dead wood detail level on screen whenever it changes, and
/// when a piece is first shown.
fn report_lod(
    state: Res<SceneState>,
    look: Res<SubjectLook>,
    camera: Query<&Transform, With<ViewportCamera>>,
    children: Query<&Children>,
    parts: Query<(&DeadwoodPart, Option<&VisibilityRange>, Option<&SavedRange>)>,
    mut reported: Local<Option<(u64, Option<usize>)>>,
    mut events: MessageWriter<ViewerEvent>,
) {
    let Some((shown, _)) = state.get::<DeadwoodSubject>() else {
        return;
    };
    let Ok(camera) = camera.single() else {
        return;
    };

    // Each level's own distance range, from the shown piece's parts. Their
    // ranges grow with the piece's size.
    let kids = children.get(shown.entity).into_iter().flatten();
    let ranges: Vec<(usize, &VisibilityRange)> = parts
        .iter_many(kids)
        .flatten()
        .filter_map(|(part, range, saved)| Some((part.lod, saved.map(|s| &s.0).or(range)?)))
        .collect();
    if ranges.is_empty() {
        return;
    }

    // The piece lies at the origin.
    let level = level_in_range(camera.translation.length(), &ranges);
    let level = match look.detail {
        Detail::Level(n) => Some(n),
        detail => level.map(|level| detail.level_for(level)),
    };
    let now = (state.generation(), level);
    if *reported != Some(now) {
        *reported = Some(now);
        events.write(ViewerEvent::DeadwoodLodShown(level));
    }
}

/// Displays a successfully built dead wood piece in place of the previous
/// one, and reports its mesh counts per detail level and build time.
/// Reports rejected settings without replacing the old piece.
fn report_deadwood(
    event: On<DeadwoodFinished>,
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut events: MessageWriter<ViewerEvent>,
    children: Query<&Children>,
    parts: Query<(&DeadwoodPart, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    // Remove the old piece only after its replacement has built successfully.
    let Some(shown) = state.get_mut::<DeadwoodSubject>() else {
        return;
    };
    if !shown.finish(&mut commands, event.entity, event.result.is_ok()) {
        return;
    }

    // Add up each level's parts: a level can have round stems and strips.
    let result = event.result.clone().map(|()| {
        let mut levels: Vec<MeshSize> = Vec::new();
        let kids = children.get(event.entity).into_iter().flatten();
        for (part, mesh) in parts.iter_many(kids).flatten() {
            let Some(mesh) = meshes.get(&mesh.0) else {
                continue;
            };
            if levels.len() <= part.lod {
                levels.resize(part.lod + 1, MeshSize::default());
            }
            levels[part.lod].vertices += mesh.count_vertices();
            levels[part.lod].triangles += mesh.indices().map_or(0, |i| i.len() / 3);
        }
        BuildStats {
            levels,
            build_time: shown.requested.elapsed(),
        }
    });
    events.write(ViewerEvent::DeadwoodBuilt(result));
}

/// Requests more frames while dead wood is generating or its textures are
/// loading, so an idle viewport still shows the finished piece.
fn render_while_loading(
    generating: Query<(), With<DeadwoodGenerating>>,
    parts: Query<&MeshMaterial3d<DeadwoodMaterial>, With<DeadwoodPart>>,
    materials: Res<Assets<DeadwoodMaterial>>,
    server: Res<AssetServer>,
    mut activity: ResMut<ViewportActivity>,
) {
    let loading = |texture: &Option<Handle<Image>>| super::is_loading(&server, texture);
    let textures_loading = parts
        .iter()
        .filter_map(|part| materials.get(&part.0))
        .any(|material| {
            let wood = &material.extension;
            loading(&wood.bark_color)
                || loading(&wood.bark_normal)
                || loading(&wood.bark_roughness)
                || loading(&wood.end_color)
                || loading(&wood.end_normal)
        });
    if !generating.is_empty() || textures_loading {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}
