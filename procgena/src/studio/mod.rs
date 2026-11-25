//! The main window: the Library and Outline on the left, the 3D view in its
//! view frame in the middle, and the Inspector on the right.
//!
//! What the studio can open comes from the [kind registry](kinds); what is
//! open are [documents].

mod camera;
pub mod commands;
pub mod documents;
pub mod inspector;
pub mod kinds;
mod layout;
mod library;
mod outline;
pub mod project;
mod quit;
pub mod status;
mod textures;
mod view_frame;

use bevy::prelude::*;

pub use documents::{Dormant, OpenDocument};
use status::BuildState;

/// The kind opened when the studio starts, with its start preset.
pub const START_KIND: &str = "tree";

/// The studio's window contents and behaviour. Add it with the scene plugin.
pub struct StudioPlugin;

impl Plugin for StudioPlugin {
    fn build(&self, app: &mut App) {
        kinds::register_all(app);
        app.init_resource::<BuildState>()
            .add_plugins((
                quit::QuitPlugin,
                documents::DocumentsPlugin,
                library::LibraryPlugin,
                outline::OutlinePlugin,
                view_frame::ViewFramePlugin,
                commands::CommandsPlugin,
                project::ProjectPlugin,
                crate::export::FileOpsPlugin,
            ))
            .add_systems(
                Startup,
                (
                    layout::spawn_layout,
                    view_frame::spawn_environment,
                    open_start_document,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    status::record_commands,
                    status::record_events,
                    status::record_resources,
                    status::update_overlay,
                )
                    .chain(),
            )
            .add_systems(Update, textures::report_texture_failures)
            .add_observer(layout::scroll_column);
    }
}

/// Opens the start preset of [`START_KIND`].
fn open_start_document(kinds: Res<kinds::Kinds>, mut commands: Commands) {
    let kind = kinds.get(START_KIND).expect("the start kind is registered");
    commands.trigger(OpenDocument {
        kind: kind.id,
        key: kind.start.to_owned(),
    });
}
