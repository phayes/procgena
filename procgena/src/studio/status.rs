//! The build progress, mesh size or error shown over the 3D view.

use crate::scene::{BuildStats, SceneMode, ViewerEvent};
use bevy::feathers::display::label_small;
use bevy::prelude::*;
use bevy::scene::Scene;

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
    /// The tree detail level on screen; the overlay shows that level's size.
    pub lod_shown: usize,
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
            ViewerEvent::LodShown(level) => state.lod_shown = *level,
        }
    }
}

/// Shows the active tab's build status, or hides the overlay on the plant tabs.
pub fn update_overlay(
    state: Res<BuildState>,
    tab: Res<ActiveTab>,
    mut overlay: Single<(&mut Node, &mut BackgroundColor), With<StatusOverlay>>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    if !state.is_changed() && !tab.is_changed() {
        return;
    }
    let (node, background) = &mut *overlay;
    if tab.0.mode() == SceneMode::Plants {
        node.display = Display::None;
        return;
    }
    node.display = Display::Flex;
    let (line, failed) = status_line(&state, tab.0);
    text.0 = line;
    background.0 = if failed { FAILED_BG } else { NORMAL_BG };
}

/// The overlay text for `tab`, and whether it reports a failure.
/// Tree sizes are for the detail level on screen; rock sizes are always for
/// the most detailed mesh.
fn status_line(state: &BuildState, tab: StudioTab) -> (String, bool) {
    let (status, noun, level) = match tab {
        StudioTab::Stones => (&state.stone, "rock", None),
        _ => (&state.tree, "tree", Some(state.lod_shown)),
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
            lod_shown: 1,
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
    }
}
