//! The window layout: the 3D view, a divider, and the settings panel.

use crate::scene::{EnvironmentSettings, SceneViewport, ViewerCommand};
use bevy::feathers::controls::{
    FeathersScrollbar, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::picking::events::PointerScroll;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::ValueChange;

use super::camera::camera_overlay;
use super::status::status_overlay;
use super::tabs::StudioTab;
use super::{
    ActiveTab, DEADWOOD_LOD_CHOICES, LOD_CHOICES, MUSHROOM_LOD_CHOICES, PLANT_LOD_CHOICES,
};
use crate::params::{ParamPanel, ParamsChanged, spawn_param_panel};
use crate::ui::{TabSelected, divider, labelled_row, tab_bar};

/// Width of the settings panel when the window opens, and its limits, in logical pixels.
const PANEL_WIDTH: f32 = 360.0;
const PANEL_MIN: f32 = 280.0;
const PANEL_MAX: f32 = 640.0;

/// Pixels per scrolled line, for wheels that report lines.
const SCROLL_LINE_HEIGHT: f32 = 16.0;

/// The scrolling column of settings.
#[derive(Component, Default, Clone)]
pub struct PanelContent;

/// The settings shown only on one tab.
#[derive(Component, Default, Clone)]
pub struct TabPage(pub StudioTab);

/// The plant Level of Detail row, shown on both plant tabs.
#[derive(Component, Default, Clone)]
pub struct PlantLodRow;

/// Where the Environment panel goes, at the bottom of the settings column.
#[derive(Component, Default, Clone)]
pub struct EnvironmentSlot;

/// Spawns the UI camera and the window layout.
pub fn spawn_layout(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera));
    let labels = StudioTab::ALL.map(StudioTab::label);
    commands.spawn_scene(bsn! {
        Node {
            width: vw(100),
            height: vh(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
        }
        TabGroup
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            Node { flex_grow: 1.0, height: percent(100) }
            SceneViewport
            Children [ @status_overlay() -- @camera_overlay() ]
            --
            @divider(PANEL_MIN, PANEL_MAX)
            --
            Node {
                width: px(PANEL_WIDTH),
                height: percent(100),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            ThemeBackgroundColor(tokens::PANE_BODY_BG)
            Children [
                Node { padding: px(8), flex_shrink: 0.0 }
                Children [
                    @tab_bar(&labels, 0)
                    // Narrow to the panel, so tabs that don't fit wrap.
                    Node { flex_grow: 1.0, flex_shrink: 1.0 }
                    on(|picked: On<TabSelected>, mut tab: ResMut<ActiveTab>| {
                        if let Some(picked) = StudioTab::at(picked.index) {
                            tab.0 = picked;
                        }
                    })
                ]
                --
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                }
                Children [
                    #Scroll
                    Node {
                        flex_grow: 1.0,
                        min_height: px(0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect { left: px(10), right: px(6), top: px(0), bottom: px(16) },
                        row_gap: px(14),
                    }
                    ScrollPosition
                    PanelContent
                    Children [
                        @page(StudioTab::Trees) Children [ @lod_row() ]
                        --
                        @page(StudioTab::TreeScatter)
                        --
                        @page(StudioTab::Plant)
                        --
                        @page(StudioTab::PlantScatter)
                        --
                        @plant_lod_row()
                        --
                        @page(StudioTab::Stones)
                        --
                        @page(StudioTab::Clumps)
                        --
                        @page(StudioTab::Mushrooms) Children [ @mushroom_lod_row() ]
                        --
                        @page(StudioTab::Deadwood) Children [ @deadwood_lod_row() ]
                        --
                        @shared_settings()
                    ]
                    --
                    @FeathersScrollbar { @target: #Scroll }
                    Node { width: px(8), margin: UiRect::vertical(px(4)) }
                ]
            ]
        ]
    });
}

/// An empty page of settings for `tab`.
fn page(tab: StudioTab) -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(8),
        }
        TabPage({tab})
    }
}

/// The tree Level of Detail picker.
fn lod_row() -> impl Scene {
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(LOD_CHOICES, Some(0)) }
        Node { flex_grow: 1.0 }
        on(|change: On<ValueChange<Entity>>,
            rows: Query<&OptionIndex>,
            mut viewer: MessageWriter<ViewerCommand>| {
            // The first choice switches by distance; the rest are levels 0, 1, ….
            if let Ok(index) = rows.get(change.value) {
                viewer.write(ViewerCommand::SetLod(index.0.checked_sub(1)));
            }
        })
    });
    bsn! { @labelled_row("Level of Detail", picker) }
}

/// The mushroom Level of Detail picker.
fn mushroom_lod_row() -> impl Scene {
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(MUSHROOM_LOD_CHOICES, Some(0)) }
        Node { flex_grow: 1.0 }
        on(|change: On<ValueChange<Entity>>,
            rows: Query<&OptionIndex>,
            mut viewer: MessageWriter<ViewerCommand>| {
            // The first choice switches by distance; the rest are levels 0, 1, ….
            if let Ok(index) = rows.get(change.value) {
                viewer.write(ViewerCommand::SetMushroomLod(index.0.checked_sub(1)));
            }
        })
    });
    bsn! { @labelled_row("Level of Detail", picker) }
}

/// The dead wood Level of Detail picker.
fn deadwood_lod_row() -> impl Scene {
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(DEADWOOD_LOD_CHOICES, Some(0)) }
        Node { flex_grow: 1.0 }
        on(|change: On<ValueChange<Entity>>,
            rows: Query<&OptionIndex>,
            mut viewer: MessageWriter<ViewerCommand>| {
            // The first choice switches by distance; the rest are levels 0, 1, ….
            if let Ok(index) = rows.get(change.value) {
                viewer.write(ViewerCommand::SetDeadwoodLod(index.0.checked_sub(1)));
            }
        })
    });
    bsn! { @labelled_row("Level of Detail", picker) }
}

/// The grass and plant Level of Detail picker, shown on both plant tabs.
fn plant_lod_row() -> impl Scene {
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(PLANT_LOD_CHOICES, Some(0)) }
        Node { flex_grow: 1.0 }
        on(|change: On<ValueChange<Entity>>,
            rows: Query<&OptionIndex>,
            mut viewer: MessageWriter<ViewerCommand>| {
            // The first choice switches by distance; the rest are levels 0, 1, 2.
            if let Ok(index) = rows.get(change.value) {
                let level = index.0.checked_sub(1).map(|level| level as u32);
                viewer.write(ViewerCommand::SetPlantLod(level));
            }
        })
    });
    bsn! { @labelled_row("Level of Detail", picker) PlantLodRow }
}

/// The settings every tab shares: the Environment panel.
fn shared_settings() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
        }
        EnvironmentSlot
    }
}

/// Spawns the Environment panel, which sends its settings to the scene on every edit.
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

/// Scrolls the settings column with the wheel or trackpad.
pub fn scroll_settings(
    mut scroll: On<PointerScroll>,
    mut columns: Query<(&mut ScrollPosition, &ComputedNode), With<PanelContent>>,
) {
    let Ok((mut position, computed)) = columns.get_mut(scroll.entity) else {
        return;
    };
    let scale = match scroll.unit {
        MouseScrollUnit::Line => SCROLL_LINE_HEIGHT,
        MouseScrollUnit::Pixel => 1.0,
    };
    let max = ((computed.content_size() - computed.size()) * computed.inverse_scale_factor()).y;
    position.y = (position.y - scroll.y * scale).clamp(0.0, max.max(0.0));
    scroll.propagate(false);
}
