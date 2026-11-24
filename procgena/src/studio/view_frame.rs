//! The view frame: the controls around the 3D view that are never saved
//! into a preset. The bar above the view holds the Detail control and the
//! camera controls; the Environment settings fold out over the view.

use bevy::feathers::controls::{FeathersSelect, OptionIndex, list_rows_from_strings};
use bevy::feathers::display::label_dim;
use bevy::picking::events::PointerDrag;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::ValueChange;

use super::documents::{DetailChoice, Document, Selection};
use super::kinds::Kinds;
use crate::params::{ParamPanel, ParamsChanged, spawn_param_panel};
use crate::scene::{EnvironmentSettings, ViewerCommand};

/// Background behind the Environment settings, matching the status overlay.
const POPOVER_BG: Color = Color::srgba(0.1, 0.1, 0.12, 0.85);

/// The first two choices of the Detail control; the selected kind's levels
/// follow them.
const DETAIL_CHOICES: [&str; 2] = ["Game default", "Level 0 preview"];

/// Marks where the Detail control goes, and the control itself.
#[derive(Component, Default, Clone)]
struct DetailSlot;

/// Marks the Detail control's select.
#[derive(Component, Default, Clone)]
struct DetailSelect;

/// Marks where the Environment settings go.
#[derive(Component, Default, Clone)]
pub struct EnvironmentSlot;

/// Keeps the view frame's controls in step with the selected document.
pub struct ViewFramePlugin;

impl Plugin for ViewFramePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(super::camera::CameraControlsPlugin)
            .add_systems(Update, (show_detail_choices, hide_environment_settings));
    }
}

/// The bar above the 3D view: the Detail control, then whatever is given
/// as its children, such as the camera controls.
pub fn view_frame_bar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(12),
            padding: UiRect::axes(px(8), px(6)),
        }
        on(|mut drag: On<PointerDrag>| drag.propagate(false))
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
            }
            DetailSlot
            Children [ @label_dim("Detail") ]
        ]
    }
}

/// The Environment settings, folded out over the top-right corner of the
/// 3D view. Drags that start on them do not move the camera.
pub fn environment_popover() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            right: px(8),
            width: px(300),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(4)),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(POPOVER_BG)
        EnvironmentSlot
        on(|mut drag: On<PointerDrag>| drag.propagate(false))
    }
}

/// Spawns the Environment settings, which send their values to the scene on
/// every edit.
pub fn spawn_environment(mut commands: Commands, slot: Single<Entity, With<EnvironmentSlot>>) {
    let panel = spawn_param_panel(
        &mut commands,
        *slot,
        EnvironmentSettings::default(),
        crate::environment::sections(),
        &[],
    );
    commands.entity(panel).observe(
        |changed: On<ParamsChanged>,
         panels: Query<&ParamPanel<EnvironmentSettings>>,
         mut viewer: MessageWriter<ViewerCommand>| {
            if let Ok(panel) = panels.get(changed.entity) {
                viewer.write(ViewerCommand::SetEnvironment(*panel.value()));
            }
        },
    );
}

/// Hides the Environment settings the selected document's kind doesn't use.
fn hide_environment_settings(
    selection: Res<Selection>,
    documents: Query<&Document>,
    kinds: Res<Kinds>,
    mut panels: Query<&mut ParamPanel<EnvironmentSettings>>,
) {
    if !selection.is_changed() {
        return;
    }
    let hidden = selection
        .document
        .and_then(|document| documents.get(document).ok())
        .and_then(|document| kinds.get(document.kind))
        .map_or(&[][..], |kind| kind.hidden_environment);
    for mut panel in &mut panels {
        panel.set_hidden_keys(hidden.iter().copied());
    }
}

/// Offers the selected document's detail levels in the Detail control,
/// with its choice picked, and hides the control for kinds without levels.
fn show_detail_choices(
    selection: Res<Selection>,
    documents: Query<&Document>,
    kinds: Res<Kinds>,
    slot: Single<(Entity, &mut Node), With<DetailSlot>>,
    selects: Query<Entity, With<DetailSelect>>,
    mut commands: Commands,
) {
    if !selection.is_changed() {
        return;
    }
    for select in &selects {
        commands.entity(select).despawn();
    }
    let (slot, mut node) = slot.into_inner();
    let document = selection
        .document
        .and_then(|document| documents.get(document).ok());
    let levels = document
        .and_then(|document| kinds.get(document.kind))
        .map_or(&[][..], |kind| kind.detail_levels);
    node.display = if levels.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    if levels.is_empty() {
        return;
    }

    // A new select, since a select's choices are fixed when it is made.
    let choices: Vec<&str> = DETAIL_CHOICES.iter().chain(levels).copied().collect();
    let picked = document.map_or(0, |document| document.detail.index());
    commands
        .spawn_scene(bsn! {
            @FeathersSelect { @options: list_rows_from_strings(choices, Some(picked)) }
            Node { width: px(170) }
            DetailSelect
            on(choose_detail)
        })
        .insert(ChildOf(slot));
}

/// Shows the chosen detail, and keeps it with the selected document.
fn choose_detail(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    selection: Res<Selection>,
    mut documents: Query<&mut Document>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let Ok(index) = rows.get(change.value) else {
        return;
    };
    let choice = DetailChoice::at(index.0);
    if let Some(mut document) = selection
        .document
        .and_then(|document| documents.get_mut(document).ok())
    {
        document.detail = choice;
    }
    viewer.write(ViewerCommand::SetDetail(choice.detail()));
}
