//! Ground textures: the texture sets of `terrain/textures.yml`, loaded once
//! as texture arrays and shared by every [`TerrainMaterial`].
//!
//! Insert [`GroundTextureSettings`] with a `root` before adding
//! [`TerrainMaterialPlugin`](crate::TerrainMaterialPlugin) to draw textures.
//! Without it, or until the files have loaded, or if they fail to load,
//! the ground is drawn in its materials' flat colours.

use bevy::prelude::*;
use procgena_core::texture::{TextureCompression, TextureList};
use procgena_core_bevy::texture::{TextureArrayLoad, TextureArrayState, TextureArrays};
use procgena_terrain::ground::ground_textures;

use crate::TerrainMaterial;

/// Which ground textures to load, and where textures stop.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct GroundTextureSettings {
    /// The asset folder holding the ground texture folders (`soil/`,
    /// `litter/` and so on), such as `procgena_terrain/textures`. Copy the
    /// repository's `terrain/assets/textures` there after running
    /// `uv run --project tools textures download terrain --sizes 128,512,1k`.
    /// `None` draws flat colours.
    pub root: Option<String>,
    /// The detail level: 0 loads the largest size (1k), 1 the next (512,
    /// the default), 2 the smallest (128).
    pub level: u8,
    /// Which files to load. BC7 by default; UASTC needs Bevy's
    /// `basis-universal` feature.
    pub compression: TextureCompression,
    /// The distance in metres where textures start to fade into flat
    /// colours, and where they are gone. Past it the ground reads no
    /// textures at all.
    pub distance: (f32, f32),
    /// The GPU memory the loaded textures may take before a warning is
    /// logged, in bytes.
    pub budget: usize,
}

impl Default for GroundTextureSettings {
    /// No textures; level 1 and BC7 once a root is set, fading from 90 m
    /// to 140 m, with a 16 MB budget.
    fn default() -> Self {
        Self {
            root: None,
            level: 1,
            compression: TextureCompression::Bc7,
            distance: (90.0, 140.0),
            budget: 16 * 1024 * 1024,
        }
    }
}

/// Chunk levels from this one on are never textured: their chunks start
/// past the texture distance.
pub const TEXTURE_LEVELS: u8 = 3;

/// The ground textures' loading state, and the arrays once loaded.
/// [`TerrainMaterialPlugin`](crate::TerrainMaterialPlugin) adds it.
#[derive(Resource, Default)]
pub struct GroundTextures {
    state: State,
}

#[derive(Default)]
enum State {
    /// Not started, or no root.
    #[default]
    Off,
    Loading(TextureArrayLoad),
    Ready(TextureArrays),
    Failed,
}

impl GroundTextures {
    /// The loaded arrays, with layer `i` holding set `i` of
    /// [`ground_textures`], or `None` while loading, without textures, or
    /// after a failure.
    pub fn arrays(&self) -> Option<&TextureArrays> {
        match &self.state {
            State::Ready(arrays) => Some(arrays),
            _ => None,
        }
    }
}

/// The layer of each ground texture set: its place in [`ground_textures`],
/// which is also its layer in the loaded arrays.
pub(crate) fn texture_layer(list: &TextureList, id: &str) -> Option<u32> {
    list.sets()
        .iter()
        .position(|set| set.id == id)
        .map(|i| i as u32)
}

/// Starts loading the ground textures when settings with a root appear or
/// change, and builds the arrays when the files are in. Once they are,
/// every terrain material gets them.
pub(crate) fn load_ground_textures(
    settings: Option<Res<GroundTextureSettings>>,
    asset_server: Res<AssetServer>,
    mut textures: ResMut<GroundTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    // Start (or restart) when the settings change.
    if let Some(settings) = settings.as_ref().filter(|s| s.is_changed()) {
        textures.state = match &settings.root {
            None => State::Off,
            Some(root) => {
                let list = ground_textures();
                let size = list.size_for_level(settings.level);
                let sets: Vec<String> = list.sets().iter().map(|set| set.id.clone()).collect();
                match TextureArrayLoad::start(
                    &asset_server,
                    root,
                    list,
                    &sets,
                    size,
                    settings.compression,
                ) {
                    Ok(load) => State::Loading(load),
                    Err(error) => {
                        warn!("ground textures: {error}");
                        State::Failed
                    }
                }
            }
        };
        if matches!(textures.state, State::Off) {
            set_material_textures(&mut materials, None, settings);
        }
    }

    // Build the arrays once every file is in.
    let State::Loading(load) = &textures.state else {
        return;
    };
    match load.poll(&asset_server, &mut images) {
        TextureArrayState::Loading => {}
        TextureArrayState::Failed(error) => {
            warn!("ground textures failed to load; drawing flat colours: {error}");
            textures.state = State::Failed;
        }
        TextureArrayState::Ready(arrays) => {
            let settings = settings.expect("a load starts from settings");
            if arrays.bytes > settings.budget {
                warn!(
                    "ground textures take {:.1} MB, over the {:.1} MB budget: {} sets at {} px",
                    arrays.bytes as f64 / 1e6,
                    settings.budget as f64 / 1e6,
                    arrays.sets.len(),
                    arrays.size.pixels(),
                );
            } else {
                info!(
                    "ground textures: {} sets at {} px, {:.1} MB",
                    arrays.sets.len(),
                    arrays.size.pixels(),
                    arrays.bytes as f64 / 1e6
                );
            }
            set_material_textures(&mut materials, Some(&arrays), &settings);
            textures.state = State::Ready(arrays);
        }
    }
}

/// Gives every terrain material `arrays` (or takes them away), keeping
/// each material's own choice of whether it is textured at all.
fn set_material_textures(
    materials: &mut Assets<TerrainMaterial>,
    arrays: Option<&TextureArrays>,
    settings: &GroundTextureSettings,
) {
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        if let Some(mut material) = materials.get_mut(id) {
            let textured = material.extension.surface.texture_fade.w > 0.0;
            material.extension.set_textures(arrays, settings, textured);
        }
    }
}
