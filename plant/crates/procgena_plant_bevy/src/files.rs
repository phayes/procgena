//! Load plant and Scatter files as assets, so a game can change the grass's
//! look without rebuilding.
//!
//! `.plant.yml` and `.plant_scatter.yml` files load as [`PlantPresetAsset`]
//! and [`PlantScatterPresetAsset`] assets. Bevy's `file_watcher` feature
//! reloads them when files change; copy the updated settings to the
//! Scatter component to show the changes. The file format itself is
//! `procgena_plant`'s.

use std::path::{Path, PathBuf};

use bevy::app::{App, Plugin};
use bevy::asset::io::Reader;
use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::asset::{Asset, AssetApp, AssetLoader, LoadContext};
use bevy::reflect::{Reflect, TypePath};

use crate::{PlantFileError, PlantPreset, PlantScatterPreset};

/// A [`PlantPreset`] loaded from a `.plant.yml` file.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct PlantPresetAsset(pub PlantPreset);

/// A [`PlantScatterPreset`] loaded from a `.plant_scatter.yml` file.
#[derive(Asset, Debug, Clone, PartialEq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct PlantScatterPresetAsset(pub PlantScatterPreset);

/// The key of a loaded file: its name without the `.plant.yml` or
/// `.plant_scatter.yml` extension.
fn file_key(load_context: &LoadContext<'_>, extension: &str) -> String {
    let name = load_context
        .path()
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(extension).unwrap_or(&name).to_owned()
}

/// Reads a whole asset as UTF-8 text.
async fn read_text(reader: &mut dyn Reader) -> Result<String, PlantFileError> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(PlantFileError::Io)?;
    String::from_utf8(bytes).map_err(|error| {
        PlantFileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

/// Loads `.plant.yml` files as [`PlantPresetAsset`]s.
///
/// Registered by [`PlantFilePlugin`]. The asset key is the filename without
/// the `.plant.yml` extension.
#[derive(Debug, Default, TypePath)]
pub struct PlantParamsLoader;

impl AssetLoader for PlantParamsLoader {
    type Asset = PlantPresetAsset;
    type Settings = ();
    type Error = PlantFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<PlantPresetAsset, PlantFileError> {
        let text = read_text(reader).await?;
        PlantPreset::from_yaml(&file_key(load_context, ".plant.yml"), &text).map(PlantPresetAsset)
    }

    fn extensions(&self) -> &[&str] {
        &["plant.yml"]
    }
}

/// Loads `.plant_scatter.yml` files as [`PlantScatterPresetAsset`]s.
///
/// Registered by [`PlantFilePlugin`]. The asset key is the filename without
/// the `.plant_scatter.yml` extension.
#[derive(Debug, Default, TypePath)]
pub struct PlantScatterLoader;

impl AssetLoader for PlantScatterLoader {
    type Asset = PlantScatterPresetAsset;
    type Settings = ();
    type Error = PlantFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<PlantScatterPresetAsset, PlantFileError> {
        let text = read_text(reader).await?;
        PlantScatterPreset::from_yaml(&file_key(load_context, ".plant_scatter.yml"), &text)
            .map(PlantScatterPresetAsset)
    }

    fn extensions(&self) -> &[&str] {
        &["plant_scatter.yml"]
    }
}

/// Loads plant and Scatter files and provides the built-in preset assets.
///
/// Part of [`PlantPlugins`](crate::PlantPlugins); requires
/// [`PlantPlugin`](crate::PlantPlugin).
///
/// Every preset is an asset of the `embedded` source, whatever the game's own
/// asset folder holds, so a game can load one by path, to use as it is or to
/// copy. A loaded Scatter is a [`PlantScatterPresetAsset`]; copy its Scatter
/// onto a [`PlantScatter`](crate::PlantScatter) once it has loaded. Keep watching for asset changes and
/// copy the settings again if the Scatter should follow live file edits:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use procgena_plant_bevy::prelude::*;
/// #[derive(Resource)]
/// struct Loading(Handle<PlantScatterPresetAsset>);
///
/// fn load(mut commands: Commands, assets: Res<AssetServer>) {
///     let preset =
///         assets.load("embedded://procgena_plant/scatters/summer_tall.plant_scatter.yml");
///     commands.insert_resource(Loading(preset));
/// }
///
/// fn spawn(
///     mut commands: Commands,
///     loading: Res<Loading>,
///     presets: Res<Assets<PlantScatterPresetAsset>>,
/// ) {
///     if let Some(PlantScatterPresetAsset(preset)) = presets.get(&loading.0) {
///         commands.spawn(PlantScatter::from_preset(preset, Vec2::splat(64.0)));
///         commands.remove_resource::<Loading>();
///     }
/// }
/// ```
///
/// The file names are the presets' keys:
/// `embedded://procgena_plant/scatters/meadow.plant_scatter.yml` and
/// `embedded://procgena_plant/plants/fern.plant.yml`, for example.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlantFilePlugin;

impl Plugin for PlantFilePlugin {
    fn build(&self, app: &mut App) {
        app.init_asset_loader::<PlantParamsLoader>()
            .init_asset_loader::<PlantScatterLoader>();
        if let Some(registry) = app.world().get_resource::<EmbeddedAssetRegistry>() {
            // Generated from the presets, so an embedded preset cannot drift
            // from its constant.
            for preset in PlantPreset::all() {
                let path = format!("procgena_plant/plants/{}.plant.yml", preset.key);
                registry.insert_asset(
                    PathBuf::new(),
                    Path::new(&path),
                    preset.to_yaml(None).into_bytes(),
                );
            }
            for preset in PlantScatterPreset::all() {
                let path = format!("procgena_plant/scatters/{}.plant_scatter.yml", preset.key);
                registry.insert_asset(
                    PathBuf::new(),
                    Path::new(&path),
                    preset.to_yaml(None).into_bytes(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
