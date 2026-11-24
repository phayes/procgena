//! The build progress, mesh size or error shown over the 3D view.

use std::sync::Arc;

use crate::scene::{
    BuildStats, EnvironmentSettings, FieldSettings, GroveProgress, SceneMode, TextureDownloads,
    ViewerCommand, ViewerEvent,
};
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::scene::Scene;
use procgena_plant::MAX_PLANTS;
use procgena_plant::prelude::PlantSettings;
use procgena_terrain::RegionStage;
use procgena_terrain_bevy::TerrainProgress;

use super::ActiveTab;
use super::tabs::StudioTab;

/// Background of the overlay normally, and when a build failed.
const NORMAL_BG: Color = Color::srgba(0.1, 0.1, 0.12, 0.7);
const FAILED_BG: Color = Color::srgba(0.55, 0.12, 0.12, 0.85);

/// Where a tree or rock build stands.
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

/// The latest tree and rock build statuses, and the tree detail level on screen.
#[derive(Resource, Debug, Default)]
pub struct BuildState {
    /// The tree's build status.
    pub tree: BuildStatus,
    /// The rock's build status.
    pub stone: BuildStatus,
    /// The Clumps tab's build status, covering its subject and its clump.
    pub clump: BuildStatus,
    /// The mushroom's build status.
    pub mushroom: BuildStatus,
    /// The grove's build status, with every tree's meshes added together.
    pub grove: BuildStatus,
    /// How many trees the last built grove has.
    pub grove_trees: usize,
    /// How far the grove has got generating its trees and baking their
    /// impostors.
    pub grove_progress: GroveProgress,
    /// The tree detail level on screen; the overlay shows that level's size.
    pub lod_shown: usize,
    /// The mushroom detail level on screen, or the last one before the
    /// camera moved too far away to draw any.
    pub mushroom_lod_shown: usize,
    /// Whether the camera is too far from the mushroom to draw it.
    pub mushroom_too_far: bool,
    /// The dead wood piece's build status.
    pub deadwood: BuildStatus,
    /// The dead wood detail level on screen, or the last one before the
    /// camera moved too far away to draw any.
    pub deadwood_lod_shown: usize,
    /// Whether the camera is too far from the dead wood piece to draw it.
    pub deadwood_too_far: bool,
    /// How far the terrain has got, once terrain mode has reported.
    pub terrain: Option<TerrainProgress>,
    /// What the plant tabs show, as last sent to the scene.
    pub plants: PlantsShown,
    /// How many texture files are downloading.
    pub downloading: usize,
}

/// The plants the scene was last asked to show, for the plant tabs' overlay.
#[derive(Debug, Clone)]
pub struct PlantsShown {
    /// The field shown, or `None` before one was sent.
    pub field: Option<Arc<FieldSettings>>,
    /// The forced detail level, or `None` to switch by camera distance.
    pub lod: Option<u32>,
    /// Side length of the square field, in metres.
    pub field_size: f32,
}

impl Default for PlantsShown {
    fn default() -> Self {
        Self {
            field: None,
            lod: None,
            field_size: EnvironmentSettings::default().field_size,
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
            ViewerEvent::TreeBuilt(result) => state.tree = BuildStatus::from_result(result),
            ViewerEvent::StoneBuilt(result) => state.stone = BuildStatus::from_result(result),
            ViewerEvent::ClumpBuilt(result) => state.clump = BuildStatus::from_result(result),
            ViewerEvent::MushroomBuilt(result) => {
                state.mushroom = BuildStatus::from_result(result);
            }
            ViewerEvent::GroveBuilt(result) => {
                if let Ok(stats) = result {
                    state.grove_trees = stats.trees;
                }
                let built = result.as_ref().map(|stats| stats.build.clone());
                state.grove = BuildStatus::from_result(&built);
            }
            ViewerEvent::GroveProgress(progress) => state.grove_progress = *progress,
            ViewerEvent::LodShown(level) => state.lod_shown = *level,
            ViewerEvent::MushroomLodShown(level) => {
                state.mushroom_too_far = level.is_none();
                if let Some(level) = level {
                    state.mushroom_lod_shown = *level;
                }
            }
            ViewerEvent::DeadwoodBuilt(result) => {
                state.deadwood = BuildStatus::from_result(result);
            }
            ViewerEvent::DeadwoodLodShown(level) => {
                state.deadwood_too_far = level.is_none();
                if let Some(level) = level {
                    state.deadwood_lod_shown = *level;
                }
            }
            ViewerEvent::TerrainProgress(progress) => state.terrain = Some(*progress),
        }
    }
}

/// Records the plants, detail level and field size sent to the scene.
pub fn record_plant_commands(
    mut commands: MessageReader<ViewerCommand>,
    mut state: ResMut<BuildState>,
) {
    for command in commands.read() {
        match command {
            ViewerCommand::SetField(field) => state.plants.field = Some(field.clone()),
            ViewerCommand::SetPlantLod(lod) => state.plants.lod = *lod,
            ViewerCommand::SetEnvironment(environment)
                if environment.field_size != state.plants.field_size =>
            {
                state.plants.field_size = environment.field_size;
            }
            _ => {}
        }
    }
}

/// Records how many texture files are downloading, when that changes.
pub fn record_downloads(downloads: Option<Res<TextureDownloads>>, mut state: ResMut<BuildState>) {
    let downloading = downloads.map_or(0, |downloads| downloads.in_progress());
    if state.downloading != downloading {
        state.downloading = downloading;
    }
}

/// Shows the active tab's build status or, on the plant tabs, mesh sizes,
/// with a line saying when textures are downloading.
pub fn update_overlay(
    state: Res<BuildState>,
    tab: Res<ActiveTab>,
    plant_settings: Option<Res<PlantSettings>>,
    mut overlay: Single<&mut BackgroundColor, With<StatusOverlay>>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    if !state.is_changed() && !tab.is_changed() {
        return;
    }
    let (line, failed) = if tab.0.mode() == SceneMode::Plants {
        let settings = plant_settings.map(|s| *s).unwrap_or_default();
        plants_line(&state.plants, &settings)
    } else {
        status_line(&state, tab.0)
    };
    text.0 = with_downloads(line, state.downloading);
    overlay.0 = if failed { FAILED_BG } else { NORMAL_BG };
}

/// The overlay text for the plant tabs, and whether it reports a failure.
///
/// The first line gives one plant's mesh at the detail level shown; with
/// detail chosen by distance, that is the nearest level. The second line
/// estimates the whole field at that level, before distance thinning and
/// culling remove plants. Accent stems are not counted.
fn plants_line(shown: &PlantsShown, settings: &PlantSettings) -> (String, bool) {
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
        let Some(size) = entry.plant.mesh_size(level, settings) else {
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

/// The overlay text for `tab`, and whether it reports a failure.
/// Tree and mushroom sizes are for the detail level on screen; rock sizes
/// are always for the most detailed mesh; clump sizes are for the clump
/// alone; grove sizes are for every tree at full detail.
fn status_line(state: &BuildState, tab: StudioTab) -> (String, bool) {
    let (status, noun, level) = match tab {
        StudioTab::Terrain => return terrain_line(state.terrain),
        StudioTab::Stones => (&state.stone, "rock", None),
        StudioTab::TreeScatter => (&state.grove, "tree Scatter", None),
        StudioTab::Clumps => (&state.clump, "clump", None),
        StudioTab::Mushrooms => (&state.mushroom, "mushroom", Some(state.mushroom_lod_shown)),
        StudioTab::Deadwood => (&state.deadwood, "dead wood", Some(state.deadwood_lod_shown)),
        StudioTab::Trees | StudioTab::Plant | StudioTab::PlantScatter => {
            (&state.tree, "tree", Some(state.lod_shown))
        }
    };
    match status {
        BuildStatus::Building if tab == StudioTab::TreeScatter => {
            (grove_building_line(state.grove_progress), false)
        }
        BuildStatus::Building => (format!("Building {noun}…"), false),
        BuildStatus::Built(_) if tab == StudioTab::Mushrooms && state.mushroom_too_far => (
            "The camera is too far away to draw the mushroom".into(),
            false,
        ),
        BuildStatus::Built(_) if tab == StudioTab::Deadwood && state.deadwood_too_far => (
            "The camera is too far away to draw the dead wood".into(),
            false,
        ),
        // Each tree of a grove switches detail by its own distance, so the
        // grove's size is given at full detail.
        BuildStatus::Built(stats) if tab == StudioTab::TreeScatter => {
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

/// The overlay text for the Terrain tab, and whether it reports a failure:
/// how many chunks are drawn, or how many are still being built.
fn terrain_line(progress: Option<TerrainProgress>) -> (String, bool) {
    let Some(progress) = progress else {
        return ("Building terrain…".into(), false);
    };
    let failed = progress.failed > 0;
    let mut line = if progress.settled {
        format!("{} terrain chunks", group_digits(progress.drawn))
    } else {
        let doing = match progress.stage {
            Some(RegionStage::Plan) => "planning regions",
            Some(RegionStage::Ground) => "shaping ground",
            Some(RegionStage::Roads) => "routing roads",
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
        assert_eq!(terrain_line(None), ("Building terrain…".into(), false));
        let mut progress = TerrainProgress {
            fraction: 0.427,
            stage: Some(RegionStage::Roads),
            drawn: 0,
            refining: 0,
            wanted: 1200,
            reading: 4,
            failed: 0,
            settled: false,
        };
        assert_eq!(
            terrain_line(Some(progress)),
            ("Building terrain… 42% · routing roads".into(), false)
        );
        progress.stage = None;
        assert_eq!(
            terrain_line(Some(progress)),
            ("Building terrain… 42% · reading chunks".into(), false)
        );
        progress.refining = 30;
        assert_eq!(
            terrain_line(Some(progress)),
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
            terrain_line(Some(progress)),
            (
                "1,200 terrain chunks\n2 chunks could not be built".into(),
                true
            )
        );
    }

    #[test]
    fn the_line_shows_the_shown_tree_level_and_rock_failures() {
        let size = |n| MeshSize {
            vertices: n,
            triangles: n / 2,
        };
        let state = BuildState {
            tree: BuildStatus::Built(BuildStats {
                levels: vec![size(4000), size(1000)],
                build_time: Duration::from_millis(12),
            }),
            stone: BuildStatus::Failed("too small".into()),
            clump: BuildStatus::Built(BuildStats {
                levels: vec![size(2000)],
                build_time: Duration::from_millis(40),
            }),
            mushroom: BuildStatus::Built(BuildStats {
                levels: vec![size(3000), size(800)],
                build_time: Duration::from_millis(5),
            }),
            grove: BuildStatus::Built(BuildStats {
                levels: vec![size(120_000), size(30_000)],
                build_time: Duration::from_millis(900),
            }),
            grove_trees: 64,
            grove_progress: GroveProgress::Idle,
            lod_shown: 1,
            mushroom_lod_shown: 1,
            mushroom_too_far: false,
            deadwood: BuildStatus::Building,
            deadwood_lod_shown: 0,
            deadwood_too_far: false,
            terrain: None,
            plants: PlantsShown::default(),
            downloading: 0,
        };
        assert_eq!(
            status_line(&state, StudioTab::Trees),
            (
                "LOD 1 · 1,000 vertices · 500 triangles · 12 ms".into(),
                false
            )
        );
        assert_eq!(
            status_line(&state, StudioTab::Stones),
            ("Cannot build this rock: too small".into(), true)
        );
        assert_eq!(
            status_line(&state, StudioTab::Clumps),
            ("2,000 vertices · 1,000 triangles · 40 ms".into(), false)
        );
        assert_eq!(
            status_line(&state, StudioTab::Mushrooms),
            ("LOD 1 · 800 vertices · 400 triangles · 5 ms".into(), false)
        );
        assert_eq!(
            status_line(&state, StudioTab::TreeScatter),
            (
                "64 trees · 120,000 vertices · 60,000 triangles at full detail · 900 ms".into(),
                false
            )
        );

        // Past the last level, no mushroom is drawn, so there is no size to give.
        let state = BuildState {
            mushroom_too_far: true,
            ..state
        };
        assert_eq!(
            status_line(&state, StudioTab::Mushrooms),
            (
                "The camera is too far away to draw the mushroom".into(),
                false
            )
        );
    }

    #[test]
    fn the_grove_line_shows_trees_generating_then_impostors_baking() {
        let line = |grove, grove_progress| {
            let state = BuildState {
                grove,
                grove_trees: 64,
                grove_progress,
                ..default()
            };
            status_line(&state, StudioTab::TreeScatter).0
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
        use procgena_plant::prelude::{PlantParams, PlantScatter, ScatterPlant};

        // A 10 m field of 2 unbranched blades per m², drawn at mid detail.
        let settings = PlantSettings::default();
        let blade = PlantParams {
            density: 2.0,
            stems_per_plant: 1,
            levels: [Default::default(), Default::default()],
            ..PlantParams::default()
        };
        let size = blade.mesh_size(1, &settings).unwrap();
        let field = PlantScatter {
            plants: vec![ScatterPlant::new(blade)].into(),
            density_scale: 1.0,
            ..PlantScatter::default()
        };
        let shown = PlantsShown {
            field: Some(Arc::new(FieldSettings { field })),
            lod: Some(1),
            field_size: 10.0,
        };
        let (line, failed) = plants_line(&shown, &settings);
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
            plants_line(&PlantsShown::default(), &settings),
            ("Building plants…".into(), false)
        );
    }
}
