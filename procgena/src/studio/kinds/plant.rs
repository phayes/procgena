//! Plants: one plant, from a `.plant.yml` preset, shown alone in a field.

use bevy::prelude::*;
use procgena_plant_bevy::prelude::{PlantParams, PlantPreset, PlantScatterOptions, ScatterPlant};

use super::{Kind, RegisterKind, entries};
use crate::params::ParamPanel;
use crate::plant::editor::{PlantEdited, PlantEditor, PlantEditorPlugin, spawn_plant_editor};
use crate::scene::{FieldSettings, PlantsSubject, Show};
use crate::studio::documents::DocumentEdited;
use crate::studio::status;

/// The plant detail levels, most detailed first.
pub const LEVELS: &[&str] = &["0 · Near", "1 · Mid", "2 · Far"];

/// Registers plants.
pub fn register(app: &mut App) {
    app.add_plugins(PlantEditorPlugin)
        .add_observer(|_: On<PlantEdited>, mut commands: Commands| {
            commands.trigger(DocumentEdited::default());
        })
        .register_kind(Kind {
            id: "plant",
            label: "Plant",
            presets: || entries(PlantPreset::all(), |p| &p.key, |p| &p.name, |p| &p.group),
            start: crate::plant::START_PRESET,
            spawn_editor: spawn_plant_editor,
            show,
            detail_levels: LEVELS,
            hidden_environment: crate::environment::PLANTS,
            status_line: status::plants_line,
        });
}

/// The plant the editor `editor` holds, alone in the start Plant Scatter's
/// field.
fn show(world: &mut World, editor: Entity, document: u64) -> Option<Show> {
    let panel = world.get::<PlantEditor>(editor)?.panel();
    let plant = *world.get::<ParamPanel<PlantParams>>(panel)?.value();
    let settings = plant_alone(crate::field::editor::start_field(), plant);
    Some(Show::new::<PlantsSubject>(document, settings))
}

/// `field` with `plant` as its only plant, at the plant's natural plants per
/// square metre, keeping the field's other shared settings.
fn plant_alone(mut field: PlantScatterOptions, plant: PlantParams) -> FieldSettings {
    field.plants = vec![ScatterPlant::new(plant)].into();
    field.density_scale = 1.0;
    FieldSettings { field }
}

#[cfg(test)]
mod tests {
    use procgena_plant_bevy::prelude::PlantScatterPreset;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};
    use crate::scene::ViewerCommand;

    /// The field in the last `Show` of plants among `commands`.
    fn last_field(commands: &[ViewerCommand]) -> PlantScatterOptions {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                ViewerCommand::Show(show) => show
                    .settings::<PlantsSubject>()
                    .map(|settings| settings.field.clone()),
                _ => None,
            })
            .expect("a field was shown")
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn each_plant_tab_shows_its_own_plants() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();
        let field = crate::field::editor::start_field();

        // A plant shows alone; opening another shows that one.
        studio.click("Plant").unwrap();
        studio.click("Meadow grass").unwrap();
        let shown = last_field(&lock(&sent));
        assert_eq!(
            shown.plants.as_ref(),
            &[ScatterPlant::new(crate::plant::editor::start_plant())]
        );
        studio.click("Wheat").unwrap();
        let wheat = PlantPreset::all()
            .find(|p| p.name == "Wheat")
            .unwrap()
            .plant;
        assert_eq!(
            last_field(&lock(&sent)).plants.as_ref(),
            &[ScatterPlant::new(wheat)]
        );

        // A Plant Scatter shows the whole field, and its edits.
        studio.click("Plant Scatter").unwrap();
        studio.click("Meadow").unwrap();
        assert_eq!(last_field(&lock(&sent)), field);
        studio.click("+ Add Plant").unwrap();
        assert_eq!(
            last_field(&lock(&sent)).plants.len(),
            field.plants.len() + 1
        );

        // Another Plant Scatter preset shows every one of its own rows.
        studio.click("Summer wild").unwrap();
        let preset = PlantScatterPreset::all()
            .find(|p| p.name == "Summer wild")
            .unwrap();
        assert_eq!(last_field(&lock(&sent)), preset.plant_scatter);
        let shares = studio
            .texts()
            .into_iter()
            .filter(|t| t.text.ends_with('%'))
            .count();
        assert_eq!(shares, preset.plant_scatter.plants.len());
    }

    #[test]
    fn the_plant_tab_shows_its_plant_alone_at_natural_density() {
        let field = PlantScatterPreset::all()
            .next()
            .unwrap()
            .plant_scatter
            .clone();
        let plant = PlantParams::CLOVER;

        let alone = plant_alone(field.clone(), plant);
        assert_eq!(alone.field.plants.as_ref(), &[ScatterPlant::new(plant)]);
        assert_eq!(alone.field.density_scale, 1.0);
        assert_eq!(alone.field.wind_scale, field.wind_scale);
    }
}
