//! The window layout: the Library and Outline, the view frame around the 3D
//! view, a divider, and the Inspector.

use bevy::feathers::controls::FeathersScrollbar;
use bevy::feathers::display::label;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::picking::events::PointerScroll;
use bevy::prelude::*;
use bevy::scene::Scene;

use super::camera::camera_controls;
use super::documents::InspectorPages;
use super::documents::save::header_buttons;
use super::library::library;
use super::outline::outline;
use super::overlays::{overlay_legend, overlay_picker};
use super::status::status_overlay;
use super::view_frame::{environment_popover, view_frame_bar};
use crate::scene::SceneViewport;
use crate::ui::divider;

/// Width of the Library and Outline column, in logical pixels.
pub const LEFT_WIDTH: f32 = 256.0;

/// Width of the Inspector when the window opens, and its limits, in logical pixels.
const PANEL_WIDTH: f32 = 360.0;
const PANEL_MIN: f32 = 280.0;
const PANEL_MAX: f32 = 640.0;

/// Pixels per scrolled line, for wheels that report lines.
const SCROLL_LINE_HEIGHT: f32 = 16.0;

/// A column that scrolls with the wheel or trackpad.
#[derive(Component, Default, Clone)]
pub struct ScrollColumn;

/// Marks the Inspector's header text: the selected document's name.
#[derive(Component, Default, Clone)]
pub struct InspectorTitle;

/// Spawns the UI camera and the window layout.
pub fn spawn_layout(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera));
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
            // Library above Outline.
            Node {
                width: px(LEFT_WIDTH),
                height: percent(100),
                min_height: px(0),
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                padding: px(8),
                row_gap: px(8),
            }
            ThemeBackgroundColor(tokens::PANE_BODY_BG)
            Children [ @library() -- @outline() ]
            --
            Node { width: px(4), height: percent(100), flex_shrink: 0.0 }
            --
            // The view frame around the 3D view.
            Node {
                flex_grow: 1.0,
                height: percent(100),
                min_width: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [
                @view_frame_bar() Children [ @overlay_picker() -- @camera_controls() ]
                --
                Node { flex_grow: 1.0, min_height: px(0) }
                SceneViewport
                Children [ @status_overlay() -- @overlay_legend() -- @environment_popover() ]
            ]
            --
            @divider(PANEL_MIN, PANEL_MAX)
            --
            @inspector()
        ]
    });
}

/// The Inspector: the selected document's name and its editor.
fn inspector() -> impl Scene {
    bsn! {
        Node {
            width: px(PANEL_WIDTH),
            height: percent(100),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Children [
            Node {
                padding: px(10),
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
            }
            Children [
                @label("") InspectorTitle
                --
                @header_buttons()
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
                ScrollColumn
                InspectorPages
                --
                @FeathersScrollbar { @target: #Scroll }
                Node { width: px(8), margin: UiRect::vertical(px(4)) }
            ]
        ]
    }
}

/// Scrolls a [`ScrollColumn`] with the wheel or trackpad.
pub fn scroll_column(
    mut scroll: On<PointerScroll>,
    mut columns: Query<(&mut ScrollPosition, &ComputedNode), With<ScrollColumn>>,
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
