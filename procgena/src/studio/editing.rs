//! Spawning the editors and sending their edits to the scene.

use std::sync::Arc;

use crate::clump::editor::{ClumpEdited, ClumpEditor, ClumpEditorPlugin, spawn_clump_editor};
use crate::scene::{ClumpSettings, FieldSettings, SceneMode, ViewerCommand};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use procgena_mushroom::MushroomOptions;
use procgena_plant::prelude::{FieldPlant, PlantField, PlantParams};
use procgena_stone::StoneOptions;
use procgena_tree::TreeOptions;

use super::layout::TabPage;
use super::status::{BuildState, BuildStatus};
use super::{ActiveTab, StudioTab};
use crate::export::FileOpsPlugin;
use crate::export::mushroom::spawn_mushroom_export;
use crate::export::stone_panel::spawn_stone_export;
use crate::export::tree_panel::spawn_tree_export;
use crate::field::editor::{FieldEdited, FieldEditorPlugin, FieldReader, spawn_field_editor};
use crate::mushrooms::editor::{
    MushroomEdited, MushroomEditor, MushroomEditorPlugin, spawn_mushroom_editor,
};
use crate::params::ParamPanel;
use crate::plant::editor::{PlantEdited, PlantEditor, PlantEditorPlugin, spawn_plant_editor};
use crate::stones::editor::{StoneEdited, StoneEditor, StoneEditorPlugin, spawn_stone_editor};
use crate::tree::editor::{TreeEdited, TreeEditor, TreeEditorPlugin, spawn_tree_editor};

/// The editors, and the observers sending their edits to the scene.
pub struct EditingPlugin;

impl Plugin for EditingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            TreeEditorPlugin,
            StoneEditorPlugin,
            PlantEditorPlugin,
            FieldEditorPlugin,
            ClumpEditorPlugin,
            MushroomEditorPlugin,
            FileOpsPlugin,
        ))
        .add_observer(send_tree)
        .add_observer(send_stone)
        .add_observer(send_clump)
        .add_observer(send_mushroom)
        .add_observer(send_plant)
        .add_observer(send_field);
    }
}

/// Spawns each tab's editor at the end of its page, followed on the Trees,
/// Stones and Mushrooms tabs by the save, load and export controls.
pub fn spawn_editors(mut commands: Commands, pages: Query<(Entity, &TabPage)>) {
    for (page, tab) in &pages {
        match tab.0 {
            StudioTab::Trees => {
                spawn_tree_editor(&mut commands, page);
                spawn_tree_export(&mut commands, page);
            }
            StudioTab::Plant => spawn_plant_editor(&mut commands, page),
            StudioTab::PlantField => spawn_field_editor(&mut commands, page),
            StudioTab::Stones => {
                spawn_stone_editor(&mut commands, page);
                spawn_stone_export(&mut commands, page);
            }
            StudioTab::Clumps => spawn_clump_editor(&mut commands, page),
            StudioTab::Mushrooms => {
                spawn_mushroom_editor(&mut commands, page);
                spawn_mushroom_export(&mut commands, page);
            }
        }
    }
}

/// The plants the scene shows for the active tab, from the editors.
#[derive(SystemParam)]
pub struct ShownField<'w, 's> {
    tab: Res<'w, ActiveTab>,
    field: FieldReader<'w, 's>,
    plant_editor: Query<'w, 's, &'static PlantEditor>,
    plants: Query<'w, 's, &'static ParamPanel<PlantParams>>,
}

impl ShownField<'_, '_> {
    /// The field for the active tab, or `None` before the editors are spawned.
    pub fn get(&self) -> Option<Arc<FieldSettings>> {
        let field = self.field.field()?;
        let editor = self.plant_editor.single().ok()?;
        let plant = *self.plants.get(editor.panel()).ok()?.value();
        Some(field_shown(self.tab.0, field, plant))
    }
}

/// The field to show on `tab`, from the Plant Field tab's `field` and the
/// Plant tab's `plant`.
///
/// The Plant tab shows `plant` alone, at its natural plants per square
/// metre, with the field's other shared settings. Every other tab shows the
/// field as edited.
pub fn field_shown(
    tab: StudioTab,
    mut field: PlantField,
    plant: PlantParams,
) -> Arc<FieldSettings> {
    if tab == StudioTab::Plant {
        field.plants = vec![FieldPlant::new(plant)].into();
        field.density_scale = 1.0;
    }
    Arc::new(FieldSettings { field })
}

/// Rebuilds the tree after each edit.
fn send_tree(
    _: On<TreeEdited>,
    editor: Single<&TreeEditor>,
    panels: Query<&ParamPanel<TreeOptions>>,
    mut state: ResMut<BuildState>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    if let Ok(panel) = panels.get(editor.panel()) {
        state.tree = BuildStatus::Building;
        viewer.write(ViewerCommand::SetTree(Arc::new(panel.value().clone())));
    }
}

/// Rebuilds the rock when an edit changed it, and fits the camera to it when asked.
fn send_stone(
    edited: On<StoneEdited>,
    editor: Single<&StoneEditor>,
    panels: Query<&ParamPanel<StoneOptions>>,
    mut sent: Local<Option<StoneOptions>>,
    mut state: ResMut<BuildState>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let Ok(panel) = panels.get(editor.panel()) else {
        return;
    };
    let sent = sent.get_or_insert_with(|| crate::stones::START_PRESET.options());
    if panel.value() != sent {
        *sent = panel.value().clone();
        state.stone = BuildStatus::Building;
        viewer.write(ViewerCommand::SetStone(Arc::new(sent.clone())));
    }
    if edited.reframe {
        viewer.write(ViewerCommand::ResetCamera);
    }
}

/// Rebuilds the mushroom when an edit changed it, and fits the camera to it
/// when asked.
fn send_mushroom(
    edited: On<MushroomEdited>,
    editor: Single<&MushroomEditor>,
    panels: Query<&ParamPanel<MushroomOptions>>,
    mut sent: Local<Option<MushroomOptions>>,
    mut state: ResMut<BuildState>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let Ok(panel) = panels.get(editor.panel()) else {
        return;
    };
    let sent = sent.get_or_insert_with(|| crate::mushrooms::START_PRESET.options());
    if panel.value() != sent {
        *sent = panel.value().clone();
        state.mushroom = BuildStatus::Building;
        viewer.write(ViewerCommand::SetMushroom(Arc::new(sent.clone())));
    }
    if edited.reframe {
        viewer.write(ViewerCommand::ResetCamera);
    }
}

/// Sends the clump settings when an edit changed them, and fits the camera
/// to the subject when asked.
fn send_clump(
    edited: On<ClumpEdited>,
    editor: Single<&ClumpEditor>,
    panels: Query<&ParamPanel<ClumpSettings>>,
    mut sent: Local<Option<ClumpSettings>>,
    mut state: ResMut<BuildState>,
    mut viewer: MessageWriter<ViewerCommand>,
) {
    let Ok(panel) = panels.get(editor.panel()) else {
        return;
    };
    let sent = sent.get_or_insert_with(crate::clump::start_settings);
    if panel.value() != sent {
        *sent = panel.value().clone();
        state.clump = BuildStatus::Building;
        viewer.write(ViewerCommand::SetClump(Arc::new(sent.clone())));
    }
    if edited.reframe {
        viewer.write(ViewerCommand::ResetCamera);
    }
}

/// Shows plant edits while the Plant tab is active.
fn send_plant(_: On<PlantEdited>, shown: ShownField, viewer: MessageWriter<ViewerCommand>) {
    send_field_for(StudioTab::Plant, &shown, viewer);
}

/// Shows field edits while the Plant Field tab is active.
fn send_field(_: On<FieldEdited>, shown: ShownField, viewer: MessageWriter<ViewerCommand>) {
    send_field_for(StudioTab::PlantField, &shown, viewer);
}

/// Sends the shown field to the scene if `tab` is the active tab.
fn send_field_for(tab: StudioTab, shown: &ShownField, mut viewer: MessageWriter<ViewerCommand>) {
    if shown.tab.0 == tab
        && let Some(field) = shown.get()
    {
        viewer.write(ViewerCommand::SetField(field));
    }
}

/// Sends the field for the active tab, when it shows plants.
pub fn send_field_on_plant_tab(shown: &ShownField, viewer: &mut MessageWriter<ViewerCommand>) {
    if shown.tab.0.mode() == SceneMode::Plants
        && let Some(field) = shown.get()
    {
        viewer.write(ViewerCommand::SetField(field));
    }
}

#[cfg(test)]
mod tests {
    use crate::scene::{OrbitCamera, ViewportCamera};
    use bevy::math::Vec2;
    use procgena_plant::prelude::{FieldPreset, PlantPreset};
    use procgena_stone::StonePreset;
    use procgena_tree::TreePreset;

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio, lock};

    /// The value of the only `ParamPanel<T>`.
    fn panel_value<T: Clone + Send + Sync + 'static>(studio: &mut HeadlessStudio) -> T {
        let world = studio.app_mut().world_mut();
        let mut panels = world.query::<&ParamPanel<T>>();
        panels.single(world).unwrap().value().clone()
    }

    /// The scene camera's view.
    fn camera(studio: &mut HeadlessStudio) -> OrbitCamera {
        let world = studio.app_mut().world_mut();
        let mut cameras = world.query_filtered::<&OrbitCamera, With<ViewportCamera>>();
        *cameras.single(world).unwrap()
    }

    /// The field in the last `SetField` among `commands`.
    fn last_field(commands: &[ViewerCommand]) -> PlantField {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                ViewerCommand::SetField(settings) => Some(settings.field.clone()),
                _ => None,
            })
            .expect("a field was sent")
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn tree_presets_and_seed_buttons_rebuild_the_tree() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();

        // Choosing a preset shows and builds it.
        studio.click("Oak Medium").unwrap();
        studio.click("Aspen Small").unwrap();
        let options: TreeOptions = panel_value(&mut studio);
        assert_eq!(options, TreePreset::AspenSmall.options());
        assert!(studio.find(&options.seed.to_string()).is_ok());
        assert!(matches!(
            lock(&sent).last(),
            Some(ViewerCommand::SetTree(built)) if **built == options
        ));

        // Random Seed changes only the seed; Jiggle changes the shape too.
        studio.click("Random Seed").unwrap();
        let reseeded: TreeOptions = panel_value(&mut studio);
        assert_ne!(reseeded.seed, options.seed);
        assert!(studio.find(&reseeded.seed.to_string()).is_ok());
        assert_eq!(
            TreeOptions {
                seed: options.seed,
                ..reseeded.clone()
            },
            options
        );
        studio.click("Jiggle").unwrap();
        let jiggled: TreeOptions = panel_value(&mut studio);
        assert_ne!(
            TreeOptions {
                seed: reseeded.seed,
                ..jiggled
            },
            reseeded
        );
        let texts = studio.texts();
        assert!(
            texts.iter().any(|t| t.text.starts_with("LOD 0 ·")),
            "{texts:?}"
        );

        // Dragging across the seed field scrubs the seed.
        let field = studio.find(&jiggled.seed.to_string()).unwrap();
        studio.drag(field, field + Vec2::new(120.0, 0.0));
        let scrubbed: TreeOptions = panel_value(&mut studio);
        assert!(scrubbed.seed > jiggled.seed);
        assert!(studio.find(&scrubbed.seed.to_string()).is_ok());
        assert!(matches!(
            lock(&sent).last(),
            Some(ViewerCommand::SetTree(built)) if built.seed == scrubbed.seed
        ));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn rock_presets_rebuild_the_rock_and_refit_the_camera() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Stones").unwrap();
        let sent = studio.record::<ViewerCommand>();

        // Turn the camera away, then choose a preset.
        studio.drag(Vec2::new(300.0, 400.0), Vec2::new(600.0, 300.0));
        let turned = camera(&mut studio);
        studio.click("Boulder").unwrap();
        studio.click("Ice Chunk").unwrap();
        let options: StoneOptions = panel_value(&mut studio);
        assert_eq!(options, StonePreset::IceChunk.options());
        assert!(lock(&sent).iter().any(
            |command| matches!(command, ViewerCommand::SetStone(built) if **built == options)
        ));

        // The camera fits the new rock, as Reset Camera would.
        let refit = camera(&mut studio);
        assert_ne!(refit, turned);
        studio.click("Reset Camera").unwrap();
        assert_eq!(camera(&mut studio), refit);

        // Presets below the end of the list are reached by scrolling it,
        // which leaves the settings behind it in place.
        let seed_label = studio.find("Seed").unwrap();
        studio.click("Ice Chunk").unwrap();
        assert!(studio.find("Pebble").is_err());
        let list = studio.find("Ore").unwrap();
        studio.scroll(list, -5.0);
        studio.click("Pebble").unwrap();
        let options: StoneOptions = panel_value(&mut studio);
        assert_eq!(options, StonePreset::Pebble.options());
        assert_eq!(studio.find("Seed").unwrap(), seed_label);

        // Dragging across the seed field scrubs the seed.
        let field = studio.find(&options.seed.to_string()).unwrap();
        studio.drag(field, field + Vec2::new(120.0, 0.0));
        let scrubbed: StoneOptions = panel_value(&mut studio);
        assert!(scrubbed.seed > options.seed);
        assert!(lock(&sent).iter().any(
            |command| matches!(command, ViewerCommand::SetStone(built) if built.seed == scrubbed.seed)
        ));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn mushroom_presets_rebuild_the_mushroom_and_refit_the_camera() {
        use procgena_mushroom::MushroomPreset;

        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Mushrooms").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let texts = studio.texts();
        assert!(
            texts.iter().any(|t| t.text.ends_with(" ms")),
            "no mushroom size shown: {texts:?}"
        );

        // Turn the camera away, then choose a preset.
        studio.drag(Vec2::new(300.0, 400.0), Vec2::new(600.0, 300.0));
        let turned = camera(&mut studio);
        studio.click("Fly agaric").unwrap();
        studio.click("Enoki").unwrap();
        let options: MushroomOptions = panel_value(&mut studio);
        assert_eq!(options, MushroomPreset::Enoki.options());
        assert!(lock(&sent).iter().any(
            |command| matches!(command, ViewerCommand::SetMushroom(built) if **built == options)
        ));

        // The camera fits the new mushroom, as Reset Camera would.
        let refit = camera(&mut studio);
        assert_ne!(refit, turned);
        studio.click("Reset Camera").unwrap();
        assert_eq!(camera(&mut studio), refit);

        // Random Seed changes only the seed, and sends it.
        studio.click("Random Seed").unwrap();
        let reseeded: MushroomOptions = panel_value(&mut studio);
        assert_ne!(reseeded.seed, options.seed);
        assert_eq!(
            MushroomOptions {
                seed: options.seed,
                ..reseeded.clone()
            },
            options
        );
        assert!(studio.find(&reseeded.seed.to_string()).is_ok());
        assert!(matches!(
            lock(&sent).last(),
            Some(ViewerCommand::SetMushroom(built)) if **built == reseeded
        ));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn each_plant_tab_shows_its_own_plants() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let sent = studio.record::<ViewerCommand>();
        let field = crate::field::editor::start_field();

        // The Plant tab shows its plant alone; choosing another shows that.
        studio.click("Plant").unwrap();
        let shown = last_field(&lock(&sent));
        assert_eq!(
            shown.plants.as_ref(),
            &[FieldPlant::new(crate::plant::editor::start_plant())]
        );
        studio.click("Meadow grass").unwrap();
        studio.click("Wheat").unwrap();
        let wheat = PlantPreset::all()
            .find(|p| p.name == "Wheat")
            .unwrap()
            .plant;
        assert_eq!(
            last_field(&lock(&sent)).plants.as_ref(),
            &[FieldPlant::new(wheat)]
        );

        // The Plant Field tab shows the whole field, and its edits.
        studio.click("Plant Field").unwrap();
        assert_eq!(last_field(&lock(&sent)), field);
        studio.click("+ Add Plant").unwrap();
        assert_eq!(
            last_field(&lock(&sent)).plants.len(),
            field.plants.len() + 1
        );

        // A new field preset replaces every row.
        studio.click("Meadow").unwrap();
        studio.click("Summer wild").unwrap();
        let preset = FieldPreset::all()
            .find(|p| p.name == "Summer wild")
            .unwrap();
        assert_eq!(last_field(&lock(&sent)), preset.field);
        let shares = studio
            .texts()
            .into_iter()
            .filter(|t| t.text.ends_with('%'))
            .count();
        assert_eq!(shares, preset.field.plants.len());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn clump_tab_rerolls_the_subject_and_clump_seeds_separately() {
        use crate::clump::subject_seed;

        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.click("Clumps").unwrap();
        let sent = studio.record::<ViewerCommand>();
        let start: ClumpSettings = panel_value(&mut studio);

        // Reroll Clump changes only the clump's seed, and sends it.
        studio.click("Reroll Clump").unwrap();
        let reclumped: ClumpSettings = panel_value(&mut studio);
        assert_ne!(reclumped.clump.seed, start.clump.seed);
        assert_eq!(reclumped.subject, start.subject);
        assert!(studio.find(&reclumped.clump.seed.to_string()).is_ok());

        // Reroll Subject changes only the subject's seed, and sends it.
        studio.click("Reroll Subject").unwrap();
        let resubjected: ClumpSettings = panel_value(&mut studio);
        assert_ne!(
            subject_seed(&resubjected.subject),
            subject_seed(&start.subject)
        );
        assert_eq!(resubjected.clump, reclumped.clump);
        assert!(matches!(
            lock(&sent).last(),
            Some(ViewerCommand::SetClump(built)) if **built == resubjected
        ));

        // A new clump preset keeps the subject.
        studio.click("Snow Cap").unwrap();
        studio.click("Moss Patches").unwrap();
        let moss: ClumpSettings = panel_value(&mut studio);
        assert_eq!(
            moss.clump,
            procgena_clump::ClumpPreset::MossPatches.options()
        );
        assert_eq!(moss.subject, resubjected.subject);
    }

    #[test]
    fn the_plant_tab_shows_its_plant_alone_at_natural_density() {
        let field = FieldPreset::all().next().unwrap().field.clone();
        let plant = PlantParams::CLOVER;

        let alone = field_shown(StudioTab::Plant, field.clone(), plant);
        assert_eq!(alone.field.plants.as_ref(), &[FieldPlant::new(plant)]);
        assert_eq!(alone.field.density_scale, 1.0);
        assert_eq!(alone.field.wind_scale, field.wind_scale);

        let mix = field_shown(StudioTab::PlantField, field.clone(), plant);
        assert_eq!(mix.field, field);
    }
}
