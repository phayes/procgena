//! Choosing a feathers select's option from code.

use bevy::ecs::system::SystemParam;
use bevy::feathers::controls::OptionIndex;
use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::ui_widgets::SetSelected;

/// Picks options of feathers selects from code.
#[derive(SystemParam)]
pub struct SelectOptions<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    options: Query<'w, 's, (&'static OptionIndex, Has<Selected>)>,
}

impl SelectOptions<'_, '_> {
    /// Selects option `index` of the select `select`, unless it already is.
    ///
    /// The select then reports the choice with a `ValueChange<Entity>`, exactly
    /// as if the user had picked it, so its observers must expect that.
    pub fn select(&self, commands: &mut Commands, select: Entity, index: usize) {
        let row = self.children.iter_descendants(select).find(|row| {
            self.options
                .get(*row)
                .is_ok_and(|(option, selected)| option.0 == index && !selected)
        });
        if let Some(row) = row {
            commands.trigger(SetSelected {
                entity: select,
                row,
            });
        }
    }
}
