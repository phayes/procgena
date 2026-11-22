//! A label on the left and a control filling the rest of the line.

use bevy::feathers::display::label_dim;
use bevy::prelude::*;
use bevy::scene::Scene;

/// Width of the label column, in logical pixels.
pub const LABEL_WIDTH: f32 = 110.0;

/// A row showing `text` in the label column and `control` beside it.
///
/// Give the control `Node { flex_grow: 1.0 }` (or a fixed width) to size it.
pub fn labelled_row(text: impl Into<String>, control: Box<dyn Scene>) -> impl Scene {
    let text = text.into();
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [
            Node { width: px(LABEL_WIDTH), flex_shrink: 0.0 }
            Children [ @label_dim(text) ]
            --
            {vec![control]}
        ]
    }
}
