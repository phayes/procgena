//! # procgena_fruit_bevy
//!
//! Draws `procgena_fruit` clusters in Bevy: an apple on its stalk, a
//! dogwood bloom or a spray of catkins, on their own.
//!
//! Add [`FruitPlugin`], then spawn an entity with [`FruitParams`]. The
//! cluster is grown from its settings and drawn as its game-level mesh,
//! then its lower mesh once it is small on screen (see
//! `procgena_core::chain`); they fade into each other. Its attachment point
//! is at the entity's origin, and it faces the entity's local +Y.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_fruit::FruitPreset;
//! use procgena_fruit_bevy::{FruitParams, FruitPlugin};
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, FruitPlugin))
//!     .add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn(FruitParams::preset(FruitPreset::apple()));
//!     })
//!     .run();
//! ```
//!
//! [`fruit_mesh`] and [`fruit_material`] convert one cluster mesh and its
//! material for your own drawing.

mod mesh;
mod spawn;

pub use mesh::{fruit_material, fruit_mesh};
pub use spawn::{FruitFinished, FruitParams, FruitPart};

use bevy::prelude::*;
use procgena_core_bevy::LodPlugin;

/// Grows and draws every entity with [`FruitParams`].
pub struct FruitPlugin;

impl Plugin for FruitPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<LodPlugin>() {
            app.add_plugins(LodPlugin);
        }
        app.add_systems(Update, spawn::build_changed);
    }
}
