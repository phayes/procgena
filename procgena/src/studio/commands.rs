//! Named studio commands, such as `reset-camera`, that scripts can run.
//!
//! Run one with [`run_command`]. Register more with
//! [`StudioCommands::register`].

use bevy::prelude::*;

use super::documents::{CloseDocument, Document, Selection};
use crate::scene::ViewerCommand;

/// One named command.
#[derive(Clone, Copy)]
pub struct StudioCommand {
    /// The command's name, such as `reset-camera`.
    pub name: &'static str,
    /// What it does, in a sentence.
    pub description: &'static str,
    /// Runs it.
    pub run: fn(&mut World),
}

/// Every command, in the order registered.
#[derive(Resource, Default)]
pub struct StudioCommands(Vec<StudioCommand>);

impl StudioCommands {
    /// Adds `command`.
    ///
    /// # Panics
    ///
    /// Panics if a command with the same name is registered already.
    pub fn register(&mut self, command: StudioCommand) {
        assert!(
            self.get(command.name).is_none(),
            "command {} is registered twice",
            command.name
        );
        self.0.push(command);
    }

    /// The command named `name`.
    pub fn get(&self, name: &str) -> Option<&StudioCommand> {
        self.0.iter().find(|command| command.name == name)
    }

    /// Every command, in the order registered.
    pub fn iter(&self) -> impl Iterator<Item = &StudioCommand> {
        self.0.iter()
    }
}

/// Runs the command named `name`.
///
/// # Errors
///
/// Fails if no command has that name, listing the commands there are.
pub fn run_command(world: &mut World, name: &str) -> Result<(), String> {
    let commands = world.resource::<StudioCommands>();
    let Some(command) = commands.get(name) else {
        let known: Vec<String> = commands
            .iter()
            .map(|command| format!("{} ({})", command.name, command.description))
            .collect();
        return Err(format!(
            "no command {name:?}; there are: {}",
            known.join("; ")
        ));
    };
    (command.run)(world);
    Ok(())
}

/// Registers the studio's own commands.
pub struct CommandsPlugin;

impl Plugin for CommandsPlugin {
    fn build(&self, app: &mut App) {
        let mut commands = StudioCommands::default();
        for command in [
            StudioCommand {
                name: "reset-camera",
                description: "Fits the camera to the selected document.",
                run: |world| {
                    world.write_message(ViewerCommand::ResetCamera);
                },
            },
            StudioCommand {
                name: "close-document",
                description: "Closes the selected document.",
                run: |world| {
                    if let Some(document) = world.resource::<Selection>().document {
                        world.trigger(CloseDocument(document));
                    }
                },
            },
            StudioCommand {
                name: "next-document",
                description: "Selects the document opened after the selected one.",
                run: |world| step_document(world, 1),
            },
            StudioCommand {
                name: "previous-document",
                description: "Selects the document opened before the selected one.",
                run: |world| step_document(world, -1),
            },
        ] {
            commands.register(command);
        }
        app.insert_resource(commands);
    }
}

/// Selects the document `step` places after the selected one in opening
/// order, wrapping around.
fn step_document(world: &mut World, step: isize) {
    let mut open: Vec<(u64, Entity)> = world
        .query::<(Entity, &Document)>()
        .iter(world)
        .map(|(entity, document)| (document.opened, entity))
        .collect();
    if open.is_empty() {
        return;
    }
    open.sort();
    let open: Vec<Entity> = open.into_iter().map(|(_, entity)| entity).collect();
    let selected = world.resource::<Selection>().document;
    let at = selected
        .and_then(|selected| open.iter().position(|open| *open == selected))
        .unwrap_or(0) as isize;
    let next = open[(at + step).rem_euclid(open.len() as isize) as usize];
    world.resource_mut::<Selection>().set_if_neq(Selection {
        document: Some(next),
        ..default()
    });
}

/// Selects `path`: a document by kind and key, opening it if it isn't
/// open, and optionally a node inside it and a part, as
/// `KIND/KEY[/NODE…][#PART]`, such as `tree/oak_medium#branch`.
///
/// # Errors
///
/// Fails if the path has no kind and key, or names an unknown kind or
/// preset.
pub fn select_path(world: &mut World, path: &str) -> Result<(), String> {
    // Split off the part, then the kind, key and node.
    let (path, part) = match path.split_once('#') {
        Some((path, part)) => (path, Some(part.to_owned())),
        None => (path, None),
    };
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let (Some(kind), Some(key)) = (segments.next(), segments.next()) else {
        return Err(format!("expected KIND/KEY, got {path:?}"));
    };
    let node: Vec<String> = segments.map(str::to_owned).collect();

    // Open or select the document.
    let kinds = world.resource::<super::kinds::Kinds>();
    let Some(kind) = kinds.get(kind) else {
        let ids: Vec<&str> = kinds.iter().map(|kind| kind.id).collect();
        return Err(format!("no kind {kind:?}; there are {}", ids.join(", ")));
    };
    if !(kind.presets)().iter().any(|preset| preset.key == key) {
        return Err(format!("no {} preset {key:?}", kind.id));
    }
    world.trigger(super::OpenDocument {
        kind: kind.id,
        key: key.to_owned(),
    });
    world.flush();

    // Narrow the selection to the node and part.
    let mut selection = world.resource_mut::<Selection>();
    selection.node = node;
    selection.part = part;
    Ok(())
}
