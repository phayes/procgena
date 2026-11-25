//! Tooltips: a line of text shown beside the pointer while it is over an
//! element.

use bevy::feathers::display::label_small;
use bevy::picking::events::{PointerEnter, PointerLeave};
use bevy::prelude::*;
use bevy::ui::ComputedUiRenderTargetInfo;

/// Shows its text beside the pointer while the pointer is over this
/// element or any of its children. Empty text shows nothing.
#[derive(Component, Default, Clone, Debug)]
pub struct Tooltip(pub &'static str);

/// Shows [`Tooltip`]s. Add once per app.
pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_tooltip_box)
            .add_observer(show_tooltip)
            .add_observer(hide_tooltip);
    }
}

/// Gap between the pointer and the tooltip, in logical pixels.
const OFFSET: Vec2 = Vec2::new(14.0, 18.0);

/// Width of a tooltip, in logical pixels; longer text wraps.
const WIDTH: f32 = 280.0;

/// The one box every tooltip is shown in.
#[derive(Component, Default, Clone)]
struct TooltipBox;

/// The text inside the [`TooltipBox`], in the studio's own label font.
#[derive(Component, Default, Clone)]
struct TooltipText;

fn spawn_tooltip_box(mut commands: Commands) {
    let background = Color::srgba(0.08, 0.08, 0.1, 0.95);
    commands.spawn_scene(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            width: px(WIDTH),
            padding: UiRect::axes(px(8), px(5)),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor({background})
        GlobalZIndex(1000)
        Pickable::IGNORE
        TooltipBox
        Children [ @label_small("") TooltipText Pickable::IGNORE ]
    });
}

/// Shows the tooltip of the element the pointer moved over beside the
/// pointer: to its right, or to its left near the right edge.
fn show_tooltip(
    enter: On<PointerEnter>,
    tips: Query<&Tooltip>,
    mut boxes: Query<(&mut Node, &ComputedUiRenderTargetInfo), With<TooltipBox>>,
    mut texts: Query<&mut Text, With<TooltipText>>,
) {
    let Ok(Tooltip(text)) = tips.get(enter.entity) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let pointer = enter.pointer.position;
    for (mut node, target) in &mut boxes {
        node.display = Display::Flex;
        node.top = px(pointer.y + OFFSET.y);
        let width = target.logical_size().x;
        let left = if pointer.x + OFFSET.x + WIDTH > width {
            pointer.x - OFFSET.x - WIDTH
        } else {
            pointer.x + OFFSET.x
        };
        node.left = px(left.max(0.0));
    }
    for mut shown in &mut texts {
        if shown.0 != *text {
            shown.0 = (*text).to_owned();
        }
    }
}

/// Hides the tooltip when the pointer leaves an element that has one.
fn hide_tooltip(
    leave: On<PointerLeave>,
    tips: Query<(), With<Tooltip>>,
    mut boxes: Query<&mut Node, With<TooltipBox>>,
) {
    if !tips.contains(leave.entity) {
        return;
    }
    for mut node in &mut boxes {
        node.display = Display::None;
    }
}
