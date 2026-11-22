//! The build progress, mesh size or error shown over the 3D view.

use std::sync::Arc;

use crate::scene::{
    BuildStats, EnvironmentSettings, FieldSettings, SceneMode, ViewerCommand, ViewerEvent,
};
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::scene::Scene;
use procgena_plant::MAX_PLANTS;
use procgena_plant::prelude::PlantSettings;

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
    /// The tree detail level on screen; the overlay shows that level's size.
    pub lod_shown: usize,
    /// What the plant tabs show, as last sent to the scene.
    pub plants: PlantsShown,
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
            ViewerEvent::LodShown(level) => state.lod_shown = *level,
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

/// Shows the active tab's build status or, on the plant tabs, mesh sizes.
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
    text.0 = line;
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
        "Field ≈ {} plants · {} vertices · {} triangles before thinning",
        group_digits(plants.round() as usize),
        group_digits(vertices.round() as usize),
        group_digits(triangles.round() as usize)
    );
    (format!("{first}\n{total}"), false)
}

/// The overlay text for `tab`, and whether it reports a failure.
/// Tree sizes are for the detail level on screen; rock sizes are always for
/// the most detailed mesh; clump sizes are for the clump alone.
fn status_line(state: &BuildState, tab: StudioTab) -> (String, bool) {
    let (status, noun, level) = match tab {
        StudioTab::Stones => (&state.stone, "rock", None),
        StudioTab::Clumps => (&state.clump, "clump", None),
        StudioTab::Mushrooms => (&state.mushroom, "mushroom", None),
        StudioTab::Trees | StudioTab::Plant | StudioTab::PlantField => {
            (&state.tree, "tree", Some(state.lod_shown))
        }
    };
    match status {
        BuildStatus::Building => (format!("Building {noun}…"), false),
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
            lod_shown: 1,
            plants: PlantsShown::default(),
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
        // Mushrooms report their most detailed level, whatever the tree shows.
        assert_eq!(
            status_line(&state, StudioTab::Mushrooms),
            ("3,000 vertices · 1,500 triangles · 5 ms".into(), false)
        );
    }

    #[test]
    fn the_plant_lines_show_one_plant_and_the_field_estimate() {
        use procgena_plant::prelude::{FieldPlant, PlantField, PlantParams};

        // A 10 m field of 2 unbranched blades per m², drawn at mid detail.
        let settings = PlantSettings::default();
        let blade = PlantParams {
            density: 2.0,
            stems_per_plant: 1,
            levels: [Default::default(), Default::default()],
            ..PlantParams::default()
        };
        let size = blade.mesh_size(1, &settings).unwrap();
        let field = PlantField {
            plants: vec![FieldPlant::new(blade)].into(),
            density_scale: 1.0,
            ..PlantField::default()
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
                 Field ≈ 200 plants · {} vertices · {} triangles before thinning",
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
