//! Widgets and styling shared across the studio, on top of bevy_feathers.

mod divider;
mod row;
mod select;
mod tabs;
pub mod toast;

use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::{ThemeProps, UiTheme};
use bevy::feathers::tokens::semantic;
use bevy::prelude::*;

pub use divider::divider;
pub use row::{LABEL_WIDTH, labelled_row};
pub use select::SelectOptions;
pub use tabs::{TabSelected, tab_bar};

/// procgena's accent colour for selected and active controls: a leaf green.
const ACCENT: Color = Color::hsl(100.0, 0.45, 0.45);

/// Installs procgena's theme and the shared widgets' observers and systems.
/// Add it after `FeathersPlugins`.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiTheme(theme())).add_plugins((
            divider::DividerPlugin,
            tabs::TabsPlugin,
            toast::ToastPlugin,
        ));
    }
}

/// The feathers dark theme with procgena's accent colour.
fn theme() -> ThemeProps {
    let mut props = create_dark_theme();
    let accents = [
        (semantic::FILL_ACCENT_DEFAULT, ACCENT),
        (semantic::FILL_ACCENT_HOVER, ACCENT.lighter(0.05)),
        (semantic::FILL_ACCENT_PRESSED, ACCENT.lighter(0.1)),
    ];
    for (token, color) in accents {
        props.semantic_base.insert(token, color);
    }
    props
}
