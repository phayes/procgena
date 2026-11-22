//! A thin vertical bar that resizes the panel to its right when dragged.

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::events::PointerDrag;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::window::SystemCursorIcon;

/// Width of the divider bar, in logical pixels.
const WIDTH: f32 = 4.0;

/// A divider. Dragging it changes the width of the next sibling node, kept
/// between `min` and `max` logical pixels.
#[derive(Component, Default, Clone, Debug)]
pub struct Divider {
    /// Narrowest the resized node may become.
    pub min: f32,
    /// Widest the resized node may become.
    pub max: f32,
}

/// Registers the observer that makes dividers work.
pub struct DividerPlugin;

impl Plugin for DividerPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(drag_divider);
    }
}

/// A divider that keeps the node after it between `min` and `max` logical
/// pixels wide. That node must have a width in pixels.
pub fn divider(min: f32, max: f32) -> impl Scene {
    bsn! {
        Node { width: px(WIDTH), height: percent(100), flex_shrink: 0.0 }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        EntityCursor::System(SystemCursorIcon::EwResize)
        Divider { min: {min}, max: {max} }
    }
}

/// Resizes the node after a dragged divider. Dragging left widens it.
fn drag_divider(
    drag: On<PointerDrag>,
    dividers: Query<(&Divider, &ChildOf)>,
    children: Query<&Children>,
    mut nodes: Query<&mut Node>,
) {
    let Ok((divider, child_of)) = dividers.get(drag.entity) else {
        return;
    };
    let Some(target) = children.get(child_of.parent()).ok().and_then(|siblings| {
        let at = siblings.iter().position(|sibling| sibling == drag.entity)?;
        siblings.get(at + 1).copied()
    }) else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(target)
        && let Val::Px(width) = node.width
    {
        node.width = px((width - drag.delta.x).clamp(divider.min, divider.max));
    }
}
