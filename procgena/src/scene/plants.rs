//! Plants subject: a square field of grass and plants.

use bevy::prelude::*;
use procgena_plant::prelude::{PlantGround, PlantScatter, PlantScatterPreset, PlantSettings};

use super::environment::EnvironmentSettings;
use super::orbit::{OrbitCamera, OrbitLimits};
use super::subject::{Detail, Subject, SubjectLook};
use super::{AddSubject, Shown};

/// The coarsest grass and plant detail level.
const COARSEST_PLANT_LEVEL: usize = 2;

/// Plants and appearance for the plant field.
///
/// Set the field's size separately with
/// [`EnvironmentSettings::field_size`].
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct FieldSettings {
    /// Plants, relative weights and shared appearance settings.
    /// The scene supplies size, flat ground and no density map, ignoring
    /// those parts of this value. Keep values within procgena_plant's documented
    /// ranges; assigning the component does not run file-loader validation.
    pub field: PlantScatter,
}

impl Default for FieldSettings {
    /// Starts with the meadow Plant Scatter preset.
    fn default() -> Self {
        Self {
            field: PlantScatterPreset::MEADOW.plant_scatter,
        }
    }
}

/// A square field of grass and plants, [`EnvironmentSettings::field_size`]
/// across, from [`FieldSettings`]. Changes show on the next frame.
///
/// Its detail levels are chosen by camera distance; forcing a level draws
/// every blade and plant at it, and levels above 2 draw level 2. Finer
/// levels can't be left out, so [`Detail::ByDistance`] draws every level.
pub struct PlantsSubject;

impl Subject for PlantsSubject {
    type Settings = FieldSettings;
    const NAME: &'static str = "plants";

    fn spawn(world: &mut World, settings: &FieldSettings) -> Entity {
        let size = world.resource::<EnvironmentSettings>().field_size;
        world.spawn(grass_field(settings, size)).id()
    }

    fn update(world: &mut World, shown: &mut Shown, _: &FieldSettings, new: &FieldSettings) {
        let size = world.resource::<EnvironmentSettings>().field_size;
        world
            .entity_mut(shown.entity)
            .insert(grass_field(new, size));
    }

    fn view(_: &World, _: &FieldSettings) -> OrbitCamera {
        OrbitCamera {
            focus: Vec3::new(0.0, 0.4, 0.0),
            yaw: 0.0,
            pitch: 0.25,
            distance: 6.0,
            limits: OrbitLimits {
                distance: (0.5, 60.0),
                pitch: (-0.05, 1.4),
            },
        }
    }

    fn environment_changed(
        world: &mut World,
        shown: &mut Shown,
        settings: &FieldSettings,
        old: &EnvironmentSettings,
    ) {
        // Resize the field, including one created earlier in this frame.
        let size = world.resource::<EnvironmentSettings>().field_size;
        if size != old.field_size {
            world
                .entity_mut(shown.entity)
                .insert(grass_field(settings, size));
        }
    }
}

/// The field to draw: a flat square with sides of `size` metres and no
/// density map.
fn grass_field(settings: &FieldSettings, size: f32) -> PlantScatter {
    PlantScatter {
        size: Vec2::splat(size),
        ground: PlantGround::default(),
        density_map: None,
        ..settings.field.clone()
    }
}

/// Adds the plants subject.
pub(crate) fn plugin(app: &mut App) {
    app.add_subject::<PlantsSubject>();
}

/// Draws grass and plants at the shown subject's detail when it changes.
pub(crate) fn apply_plant_lod(look: Res<SubjectLook>, settings: Option<ResMut<PlantSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    if look.is_changed() {
        settings.forced_lod = match look.detail {
            Detail::Level(level) => Some(level.min(COARSEST_PLANT_LEVEL) as u32),
            Detail::ByDistance { .. } => None,
        };
    }
}
