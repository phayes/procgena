//! Putting a generated specimen on a Bevy entity.

use std::sync::Arc;

use bevy_ecs::component::Component;

use crate::specimen::Specimen;

/// The [`Specimen`] an entity was generated as, in the entity's own
/// coordinates.
///
/// procgena's Bevy plugins add it to an entity once its generation finishes
/// and remove it when the entity's settings change, so its presence means
/// the specimen is up to date. Other plugins, such as clump growing, read it
/// to find surfaces to grow on.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct GeneratedSpecimen(pub Arc<Specimen>);
