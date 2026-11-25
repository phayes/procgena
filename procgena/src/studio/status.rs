//! The build progress, mesh size or error shown over the 3D view.

use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::scene::Scene;
use procgena_plant_bevy::MAX_PLANTS;
use procgena_plant_bevy::prelude::PlantSettings;
use procgena_terrain::{FeatureTier, RegionStage};
use procgena_terrain_bevy::TerrainProgress;

use super::documents::{Document, Selection};
use super::kinds::Kinds;
use crate::ecotope::EcotopeStatus;
use crate::scene::{
    BuildStats, Detail, EnvironmentSettings, FieldSettings, GroveProgress, PlantsSubject,
    TextureDownloads, ViewerCommand, ViewerEvent,
};

/// Background of the overlay normally, and when a build failed.
const NORMAL_BG: Color = Color::srgba(0.1, 0.1, 0.12, 0.7);
const FAILED_BG: Color = Color::srgba(0.55, 0.12, 0.12, 0.85);

/// Where a build of the shown subject stands.
#[derive(Debug, Clone, Default)]
pub enum BuildStatus {
    /// Being built.
    #[default]
    Building,
    /// Built, with these mesh sizes and build time.
    Built(BuildStats),
    /// Rejected, with this reason.
    Failed(String),
}

impl BuildStatus {
    /// The status for a build result reported by the scene.
    fn from_result<E: std::fmt::Display>(result: &Result<BuildStats, E>) -> Self {
        match result {
            Ok(stats) => Self::Built(stats.clone()),
            Err(error) => Self::Failed(error.to_string()),
        }
    }
}

/// What the scene last reported about the subject it shows.
#[derive(Resource, Debug, Default)]
pub struct BuildState {
    /// The latest build's status.
    pub build: BuildStatus,
    /// The detail level on screen, for subjects that report one.
    pub level: usize,
    /// Whether the camera is too far away to draw the subject.
    pub too_far: bool,
    /// How many trees the last built tree Scatter has.
    pub grove_trees: usize,
    /// How far a tree Scatter has got generating its trees and baking their
    /// impostors.
    pub grove_progress: GroveProgress,
    /// Where placing an Ecotope stands.
    pub ecotope: EcotopeStatus,
    /// How far terrain has got, once it has reported.
    pub terrain: Option<TerrainProgress>,
    /// The plants shown, as last sent to the scene.
    pub plants: PlantsShown,
    /// How many texture files are downloading.
    pub downloading: usize,
}

/// The plants the scene was last asked to show, for the plant overlay.
#[derive(Debug, Clone)]
pub struct PlantsShown {
    /// The field shown, or `None` before one was sent.
    pub field: Option<FieldSettings>,
    /// The forced detail level, or `None` to switch by camera distance.
    pub lod: Option<u32>,
    /// Side length of the square field, in metres.
    pub field_size: f32,
    /// The plant drawing settings, for mesh sizes.
    pub settings: PlantSettings,
}

impl Default for PlantsShown {
    fn default() -> Self {
        Self {
            field: None,
            lod: None,
            field_size: EnvironmentSettings::default().field_size,
            settings: PlantSettings::default(),
        }
    }
}

/// Marks the overlay node.
#[derive(Component, Default, Clone)]
pub struct StatusOverlay;

/// Marks the overlay's text.
#[derive(Component, Default, Clone)]
pub struct StatusText;

/// The overlay, for the top-left corner of the 3D view.
pub fn status_overlay() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::axes(px(8), px(4)),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(NORMAL_BG)
        Pickable::IGNORE
        StatusOverlay
        Children [ @label_small("") StatusText ]
    }
}

/// Records build results and the shown detail level reported by the scene.
pub fn record_events(mut events: MessageReader<ViewerEvent>, mut state: ResMut<BuildState>) {
    for event in events.read() {
        match event {
            ViewerEvent::Building => {
                state.build = BuildStatus::Building;
                state.ecotope = EcotopeStatus::Placing;
            }
            ViewerEvent::TreeBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::StoneBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::ClumpBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::MushroomBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::FruitBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::DeadwoodBuilt(result) => state.build = BuildStatus::from_result(result),
            ViewerEvent::GroveBuilt(result) => {
                if let Ok(stats) = result {
                    state.grove_trees = stats.trees;
                }
                let built = result.as_ref().map(|stats| stats.build.clone());
                state.build = BuildStatus::from_result(&built);
            }
            ViewerEvent::GroveProgress(progress) => state.grove_progress = *progress,
            ViewerEvent::LodShown(level)
            | ViewerEvent::MushroomLodShown(level)
            | ViewerEvent::DeadwoodLodShown(level) => {
                state.too_far = level.is_none();
                if let Some(level) = level {
                    state.level = *level;
                }
            }
            ViewerEvent::EcotopeBuilt(result) => {
                state.ecotope = EcotopeStatus::from_result(result);
            }
            ViewerEvent::TerrainProgress(progress) => state.terrain = Some(*progress),
        }
    }
}

/// Starts the state over for each newly shown document, and records the
/// plants, detail level and field size sent to the scene.
pub fn record_commands(
    mut commands: MessageReader<ViewerCommand>,
    mut state: ResMut<BuildState>,
    mut shown: Local<Option<u64>>,
) {
    for command in commands.read() {
        match command {
            ViewerCommand::Show(show) => {
                if *shown != Some(show.document()) {
                    *shown = Some(show.document());
                    *state = BuildState {
                        plants: PlantsShown {
                            field: None,
                            ..state.plants.clone()
                        },
                        downloading: state.downloading,
                        ..default()
                    };
                }
                if let Some(field) = show.settings::<PlantsSubject>() {
                    state.plants.field = Some(field.clone());
                }
            }
            ViewerCommand::SetDetail(detail) => {
                state.plants.lod = match detail {
                    Detail::Level(level) => Some(*level as u32),
                    Detail::ByDistance { .. } => None,
                };
            }
            ViewerCommand::SetEnvironment(environment)
                if environment.field_size != state.plants.field_size =>
            {
                state.plants.field_size = environment.field_size;
            }
            _ => {}
        }
    }
}

/// Records how many texture files are downloading, and the plant drawing
/// settings, when they change.
pub fn record_resources(
    downloads: Option<Res<TextureDownloads>>,
    plant_settings: Option<Res<PlantSettings>>,
    mut state: ResMut<BuildState>,
) {
    let downloading = downloads.map_or(0, |downloads| downloads.in_progress());
    if state.downloading != downloading {
        state.downloading = downloading;
    }
    if let Some(settings) = plant_settings
        && settings.is_changed()
    {
        state.plants.settings = *settings;
    }
}

/// Shows the selected document's build status, with a line saying when
/// textures are downloading.
pub fn update_overlay(
    state: Res<BuildState>,
    selection: Res<Selection>,
    documents: Query<&Document>,
    kinds: Res<Kinds>,
    mut overlay: Single<&mut BackgroundColor, With<StatusOverlay>>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    if !state.is_changed() && !selection.is_changed() {
        return;
    }
    let kind = selection
        .document
        .and_then(|document| documents.get(document).ok())
        .and_then(|document| kinds.get(document.kind));
    let (line, failed) = match kind {
        Some(kind) => (kind.status_line)(&state),
        None => ("Open a preset from the Library".into(), false),
    };
    text.0 = with_downloads(line, state.downloading);
    overlay.0 = if failed { FAILED_BG } else { NORMAL_BG };
}

/// The overlay text for plants, and whether it reports a failure.
///
/// The first line gives one plant's mesh at the detail level shown; with
/// detail chosen by distance, that is the nearest level. The second line
/// estimates the whole field at that level, before distance thinning and
/// culling remove plants. Accent stems are not counted.
pub fn plants_line(state: &BuildState) -> (String, bool) {
    let shown = &state.plants;
    let Some(field) = &shown.field else {
        return ("Building plants…".into(), false);
    };
    let field = &field.field;
    let level = shown.lod.unwrap_or(0) as usize;
    let lod = match shown.lod {
        Some(level) => format!("LOD {level}"),
        None => "LOD 0 (nearest, auto by distance)".into(),
    };

    // Sum each kind of plant's mesh over the plants of that kind in the field.
    let area = shown.field_size * shown.field_size;
    let scale = field.density_scale.max(0.0);
    let (mut plants, mut vertices, mut triangles) = (0.0, 0.0, 0.0);
    let mut per_plant = None;
    for entry in field.plants.iter().take(MAX_PLANTS) {
        let Some(size) = shown.settings.mesh_size(&entry.plant, level) else {
            return ("Cannot draw this plant: too many branches".into(), true);
        };
        per_plant.get_or_insert(size);
        let count = entry.density() * scale * area;
        plants += count;
        vertices += count * size.vertices as f32;
        triangles += count * size.triangles as f32;
    }

    // One plant alone shows its own size; a mix shows only the field total.
    let first = match (field.plants.len(), per_plant) {
        (1, Some(size)) => format!(
            "{lod} · {} vertices · {} triangles per plant",
            group_digits(size.vertices as usize),
            group_digits(size.triangles as usize)
        ),
        _ => lod,
    };
    let total = format!(
        "Scatter ≈ {} plants · {} vertices · {} triangles before thinning",
        group_digits(plants.round() as usize),
        group_digits(vertices.round() as usize),
        group_digits(triangles.round() as usize)
    );
    (format!("{first}\n{total}"), false)
}

/// The overlay text for a single object named `noun`, such as `"rock"`,
/// whose size is given for its most detailed level, and whether it reports
/// a failure.
pub fn object_line(state: &BuildState, noun: &str) -> (String, bool) {
    built_line(state, noun, None)
}

/// The overlay text for a single object named `noun` with detail levels,
/// whose size is given for the level on screen, and whether it reports a
/// failure.
pub fn leveled_line(state: &BuildState, noun: &str) -> (String, bool) {
    built_line(state, noun, Some(state.level))
}

/// The overlay text for a single object named `noun`, sized at `level` and
/// labelled with it, or at its most detailed level without one.
fn built_line(state: &BuildState, noun: &str, level: Option<usize>) -> (String, bool) {
    match &state.build {
        BuildStatus::Building => (format!("Building {noun}…"), false),
        BuildStatus::Built(_) if state.too_far => (
            format!("The camera is too far away to draw the {noun}"),
            false,
        ),
        BuildStatus::Built(stats) => {
            let size = stats
                .levels
                .get(level.unwrap_or(0))
                .copied()
                .unwrap_or_default();
            let lod = level.map(|l| format!("LOD {l} · ")).unwrap_or_default();
            let line = format!(
                "{lod}{} vertices · {} triangles · {} ms",
                group_digits(size.vertices),
                group_digits(size.triangles),
                stats.build_time.as_millis()
            );
            (line, false)
        }
        BuildStatus::Failed(error) => (format!("Cannot build this {noun}: {error}"), true),
    }
}

/// The overlay text for a tree Scatter, and whether it reports a failure.
/// Each tree switches detail by its own distance, so the size is given at
/// full detail.
pub fn grove_line(state: &BuildState) -> (String, bool) {
    match &state.build {
        BuildStatus::Building => (grove_building_line(state.grove_progress), false),
        BuildStatus::Built(stats) => {
            let size = stats.levels.first().copied().unwrap_or_default();
            let line = format!(
                "{} trees · {} vertices · {} triangles at full detail · {} ms",
                group_digits(state.grove_trees),
                group_digits(size.vertices),
                group_digits(size.triangles),
                stats.build_time.as_millis()
            );
            (with_bakes(line, state.grove_progress), false)
        }
        BuildStatus::Failed(error) => (format!("Cannot build this tree Scatter: {error}"), true),
    }
}

/// The overlay text for an Ecotope, and whether it reports a failure.
pub fn ecotope_line(state: &BuildState) -> (String, bool) {
    state.ecotope.line()
}

/// The overlay text for terrain, and whether it reports a failure: how
/// many chunks are drawn, or how many are still being built.
pub fn terrain_line(state: &BuildState) -> (String, bool) {
    let Some(progress) = state.terrain else {
        return ("Building terrain…".into(), false);
    };
    let failed = progress.failed > 0;
    let mut line = if progress.settled {
        format!("{} terrain chunks", group_digits(progress.drawn))
    } else {
        let doing = match progress.stage {
            Some(RegionStage::Plan) => "planning regions",
            Some(RegionStage::Tier(FeatureTier::Rough)) => "shaping ground",
            Some(RegionStage::Tier(FeatureTier::Fine)) => "adding roads",
            None if progress.refining > 0 => "adding roads to near ground",
            None => "reading chunks",
        };
        let percent = (progress.fraction * 100.0).floor();
        format!("Building terrain… {percent}% · {doing}")
    };
    if failed {
        line.push_str(&format!(
            "\n{} chunks could not be built",
            group_digits(progress.failed)
        ));
    }
    (line, failed)
}

/// The overlay text while the grove builds, with how many of its tree
/// shapes are generated once they are known.
fn grove_building_line(progress: GroveProgress) -> String {
    match progress {
        GroveProgress::Generating { done, total } if total > 0 => {
            format!("Building tree Scatter… {done} of {total} tree shapes generated")
        }
        _ => "Building tree Scatter…".into(),
    }
}

/// `line`, followed by a line saying how many impostors are baked while the
/// grove's far trees are still being baked into them.
fn with_bakes(line: String, progress: GroveProgress) -> String {
    match progress {
        GroveProgress::Baking { done, total } => {
            format!("{line}\nBaking far trees: {done} of {total} impostors…")
        }
        _ => line,
    }
}

/// `line`, followed by a line saying that textures are downloading when
/// `downloading` is above zero. What uses them appears once they arrive.
fn with_downloads(line: String, downloading: usize) -> String {
    match downloading {
        0 => line,
        1 => format!("{line}\nDownloading 1 texture…"),
        n => format!("{line}\nDownloading {n} textures…"),
    }
}

/// Formats `n` with thousands separators, such as `12,345`.
fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::scene::MeshSize;

    use super::*;

    #[test]
    fn digits_are_grouped_in_thousands() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(1234567), "1,234,567");
    }

    #[test]
    fn the_terrain_line_shows_its_chunks_and_failures() {
        let line = |terrain| {
            terrain_line(&BuildState {
                terrain,
                ..default()
            })
        };
        assert_eq!(line(None), ("Building terrain…".into(), false));
        let mut progress = TerrainProgress {
            fraction: 0.427,
            stage: Some(RegionStage::Tier(FeatureTier::Fine)),
            drawn: 0,
            refining: 0,
            wanted: 1200,
            reading: 4,
            failed: 0,
            settled: false,
        };
        assert_eq!(
            line(Some(progress)),
            ("Building terrain… 42% · adding roads".into(), false)
        );
        progress.stage = None;
        assert_eq!(
            line(Some(progress)),
            ("Building terrain… 42% · reading chunks".into(), false)
        );
        progress.refining = 30;
        assert_eq!(
            line(Some(progress)),
            (
                "Building terrain… 42% · adding roads to near ground".into(),
                false
            )
        );
        progress = TerrainProgress {
            drawn: 1200,
            reading: 0,
            failed: 2,
            settled: true,
            ..progress
        };
        assert_eq!(
            line(Some(progress)),
            (
                "1,200 terrain chunks\n2 chunks could not be built".into(),
                true
            )
        );
    }

    #[test]
    fn the_line_shows_the_shown_level_and_failures() {
        let size = |n| MeshSize {
            vertices: n,
            triangles: n / 2,
        };
        let built = |levels, millis| {
            BuildStatus::Built(BuildStats {
                levels,
                build_time: Duration::from_millis(millis),
            })
        };
        let state = |build, level| BuildState {
            build,
            level,
            ..default()
        };

        let tree = state(built(vec![size(4000), size(1000)], 12), 1);
        assert_eq!(
            leveled_line(&tree, "tree"),
            (
                "LOD 1 · 1,000 vertices · 500 triangles · 12 ms".into(),
                false
            )
        );
        let flower = state(built(vec![size(600), size(200)], 1), 0);
        assert_eq!(
            object_line(&flower, "flower"),
            ("600 vertices · 300 triangles · 1 ms".into(), false)
        );
        let rock = state(BuildStatus::Failed("too small".into()), 0);
        assert_eq!(
            object_line(&rock, "rock"),
            ("Cannot build this rock: too small".into(), true)
        );
        let mushroom = state(built(vec![size(3000), size(800)], 5), 1);
        assert_eq!(
            leveled_line(&mushroom, "mushroom"),
            ("LOD 1 · 800 vertices · 400 triangles · 5 ms".into(), false)
        );
        let ecotope = BuildState {
            ecotope: EcotopeStatus::Failed("Scatters a and b share priority 5".into()),
            ..default()
        };
        assert_eq!(
            ecotope_line(&ecotope),
            (
                "Cannot place this Ecotope: Scatters a and b share priority 5".into(),
                true
            )
        );

        // Past the last level, no mushroom is drawn, so there is no size to give.
        let far = BuildState {
            too_far: true,
            ..mushroom
        };
        assert_eq!(
            leveled_line(&far, "mushroom"),
            (
                "The camera is too far away to draw the mushroom".into(),
                false
            )
        );
    }

    #[test]
    fn the_grove_line_shows_trees_generating_then_impostors_baking() {
        let line = |build, grove_progress| {
            let state = BuildState {
                build,
                grove_trees: 64,
                grove_progress,
                ..default()
            };
            grove_line(&state).0
        };
        let built = || {
            BuildStatus::Built(BuildStats {
                levels: vec![MeshSize {
                    vertices: 1000,
                    triangles: 500,
                }],
                build_time: Duration::from_millis(30),
            })
        };
        let built_line = "64 trees · 1,000 vertices · 500 triangles at full detail · 30 ms";

        // Before the trees are placed, the count of shapes is not known.
        let placing = GroveProgress::Generating { done: 0, total: 0 };
        assert_eq!(
            line(BuildStatus::Building, placing),
            "Building tree Scatter…"
        );
        let generating = GroveProgress::Generating { done: 3, total: 8 };
        assert_eq!(
            line(BuildStatus::Building, generating),
            "Building tree Scatter… 3 of 8 tree shapes generated"
        );

        // Once built, a second line counts the impostors baked, until all are.
        let baking = GroveProgress::Baking { done: 5, total: 8 };
        assert_eq!(
            line(built(), baking),
            format!("{built_line}\nBaking far trees: 5 of 8 impostors…")
        );
        assert_eq!(line(built(), GroveProgress::Idle), built_line);
    }

    #[test]
    fn downloading_textures_adds_a_line() {
        assert_eq!(with_downloads("Building tree…".into(), 0), "Building tree…");
        assert_eq!(
            with_downloads("Building tree…".into(), 1),
            "Building tree…\nDownloading 1 texture…"
        );
        assert_eq!(
            with_downloads("Building tree…".into(), 3),
            "Building tree…\nDownloading 3 textures…"
        );
    }

    #[test]
    fn the_plant_lines_show_one_plant_and_the_field_estimate() {
        use procgena_plant_bevy::prelude::{PlantParams, PlantScatterOptions, ScatterPlant};

        // A 10 m field of 2 unbranched blades per m², drawn at mid detail.
        let settings = PlantSettings::default();
        let blade = PlantParams {
            density: 2.0,
            stems_per_plant: 1,
            levels: [Default::default(), Default::default()],
            ..PlantParams::default()
        };
        let size = settings.mesh_size(&blade, 1).unwrap();
        let field = PlantScatterOptions {
            plants: vec![ScatterPlant::new(blade)].into(),
            density_scale: 1.0,
            ..PlantScatterOptions::default()
        };
        let state = BuildState {
            plants: PlantsShown {
                field: Some(FieldSettings { field }),
                lod: Some(1),
                field_size: 10.0,
                settings,
            },
            ..default()
        };
        let (line, failed) = plants_line(&state);
        assert!(!failed);
        assert_eq!(
            line,
            format!(
                "LOD 1 · {} vertices · {} triangles per plant\n\
                 Scatter ≈ 200 plants · {} vertices · {} triangles before thinning",
                size.vertices,
                size.triangles,
                group_digits(200 * size.vertices as usize),
                group_digits(200 * size.triangles as usize),
            )
        );

        // Before any field is sent, the overlay says the plants are coming.
        assert_eq!(
            plants_line(&BuildState::default()),
            ("Building plants…".into(), false)
        );
    }
}
