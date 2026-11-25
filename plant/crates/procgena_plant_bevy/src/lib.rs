//! Grass and foliage for Bevy games: lush fields that sway in the wind and
//! bend where players walk.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_plant_bevy::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((DefaultPlugins, PlantPlugins))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(64.0)));
//! }
//! ```
//!
//! [`PlantPlugins`] is a plugin group: [`PlantPlugin`] (the grass itself),
//! [`FarPlantPlugin`] (the terrain material for ground past the last blade)
//! and [`PlantFilePlugin`] (`.plant.yml` and `.plant_scatter.yml` files, and
//! the presets as assets). Turn one off with `PlantPlugins.build().disable::<FarPlantPlugin>()`,
//! or add only the ones you want. `PlantDebugPlugin` (cargo feature
//! `debug`) is always added by hand.
//!
//! Every camera draws grass unless it carries [`NoPlants`]. A camera with a
//! depth, normal or motion-vector prepass gets grass in it too, so SSAO, TAA
//! and anything else reading the prepass see it; a deferred camera (with
//! `DefaultOpaqueRendererMethod::deferred()`) draws grass into its G-buffer.
//!
//! A [`PlantScatter`] holds everything about how it looks: the plants that
//! grow in it, each a [`PlantParams`] at its own density, scaled by a
//! [`ScatterPlant`], and
//! the settings they share. Start from a [`PlantScatterPreset`] or build one in
//! code from [`PlantPreset`]s. [`FarPlantMaterial`] carries the grass's
//! colour onto the ground beyond the last blade.
//!
//! A field can vary its grass the way real fields do: several plants
//! (grass, seed heads, flowers, clover, ferns, shrubs), plants gathered into
//! tufts, bare patches, blades that twist, droop and fold, and colour
//! patches, dry tips and dead blades. Every kind is off at 0 and costs
//! nothing while no field uses it. Browse [`PlantScatterPreset::WILD`] and the
//! forest-floor presets for examples of these effects.
//!
//! Plants use curved strips ([`PlantSegment`]) for blades, stalks and leaves. A
//! [`PlantParams`] can add two levels of [`PlantBranches`], from leaflets on a fern
//! stalk to leaves on a bush's twigs.

mod api;
mod clock;
#[cfg(feature = "debug")]
mod debug;
mod edit;
mod far;
mod files;
mod gpu_layout;
mod ground;
#[cfg(test)]
mod mirror;
mod packed;
mod physics;
mod reach;
mod render;
mod scatters;
mod settings;
mod shader_model;
mod stats;
mod sun;
mod trample;
pub mod wind;

pub use api::{
    NoPlants, PlantCamera, PlantGround, PlantInteractor, PlantInteractorFootprint,
    PlantInteractorSpread, PlantScatter, PlantSun,
};
#[cfg(feature = "debug")]
pub use debug::{PlantDebug, PlantDebugPlugin, PlantGizmos};
pub use edit::{
    EditStats, PlantBrush, PlantBrushFootprint, PlantBrushShape, PlantEdit, PlantEditOp,
    PlantRegrowth, PlantSample, PlantSampler,
};
pub use far::{FarPlantMaterial, FarPlantPlugin, FarPlantUniform, FarPlants};
pub use files::{
    PlantFilePlugin, PlantParamsLoader, PlantPresetAsset, PlantScatterLoader,
    PlantScatterPresetAsset,
};
pub use physics::{PlantColliderBounds, PlantColliderSystems, PlantInteractorFromCollider};
/// The engine-free settings, presets and files, from `procgena_plant`.
pub use procgena_plant::{
    GroundTint, MAX_BAKE_TEXELS, MAX_BRANCHES, MAX_PLANTS, MAX_STEMS_PER_PLANT, PlantAccent,
    PlantArrangement, PlantBake, PlantBranches, PlantFileError, PlantParams, PlantPreset,
    PlantScatterOptions, PlantScatterPreset, PlantSegment, PlantSettingsError, PlantVariation,
    PresetFileError, ScatterPlant, Srgb,
};
pub use render::platform::PlantSupport;
pub use settings::{PlantMeshSize, PlantQuality, PlantSettings, PlantShadows};
pub use stats::{GpuStats, PlantDiagnostics, PlantStats, TableStats};
pub use sun::{PlantLightFrame, PlantLightPlugin, PlantLightSystems};
pub use wind::{PlantWind, PlantWindFrame, PlantWindPlugin, PlantWindSystems};

use bevy::app::{App, Plugin, PluginGroup, PluginGroupBuilder, PostUpdate, Update};
use bevy::asset::{AssetApp, embedded_asset, load_embedded_asset};
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::ecs::system::Res;
use bevy::log::warn;
use bevy::shader::load_shader_library;

/// The things a game needs, in one import.
pub mod prelude {
    pub use crate::{
        FarPlantMaterial, FarPlantPlugin, FarPlants, GroundTint, NoPlants, PlantAccent,
        PlantArrangement, PlantBranches, PlantBrush, PlantBrushFootprint, PlantBrushShape,
        PlantCamera, PlantColliderBounds, PlantColliderSystems, PlantEdit, PlantEditOp,
        PlantFilePlugin, PlantGround, PlantInteractor, PlantInteractorFootprint,
        PlantInteractorFromCollider, PlantInteractorSpread, PlantLightFrame, PlantLightPlugin,
        PlantLightSystems, PlantParams, PlantPlugin, PlantPlugins, PlantPreset, PlantPresetAsset,
        PlantQuality, PlantRegrowth, PlantSample, PlantSampler, PlantScatter, PlantScatterOptions,
        PlantScatterPreset, PlantScatterPresetAsset, PlantSegment, PlantSettings, PlantShadows,
        PlantStats, PlantSun, PlantSupport, PlantSystems, PlantVariation, PlantWind,
        PlantWindFrame, PlantWindPlugin, PlantWindSystems, ScatterPlant, Srgb,
    };
    #[cfg(feature = "debug")]
    pub use crate::{PlantDebug, PlantDebugPlugin};
}

/// The plugin's systems in the main world, all in `PostUpdate`, in this order.
///
/// Fields, density masks and heightmaps are turned into the tables
/// the GPU draws from once per change, not per frame: [`DetectChanges`] notices
/// what changed and [`Rebuild`] updates the tables. Both run after Bevy has
/// propagated transforms and visibility and flushed asset events, so
/// they see this frame's `GlobalTransform`, `InheritedVisibility` and
/// `AssetEvent`s.
///
/// Order your own systems against them when what you do must show up this
/// frame.
///
/// - **Fields** (their component, transform, visibility, layers): a system
///   that changes them `.before(PlantSystems::DetectChanges)` is seen this
///   frame.
/// - **Edits** ([`PlantEdit`] messages, [`PlantBrush`]es): applied in
///   [`Edit`](PlantSystems::Edit), between the two.
/// - **Images** (a density mask, a heightmap): they are noticed
///   through their `AssetEvent`, which Bevy sends in
///   [`AssetEventSystems`](bevy::asset::AssetEventSystems), so a system that
///   edits one `.before(AssetEventSystems)` is seen this frame.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy::asset::AssetEventSystems;
/// # use procgena_plant_bevy::prelude::*;
/// # fn move_the_meadow() {}
/// # fn paint_density_mask() {}
/// # let mut app = App::new();
/// app.add_systems(PostUpdate, move_the_meadow.before(PlantSystems::DetectChanges))
///     .add_systems(PostUpdate, paint_density_mask.before(AssetEventSystems));
/// ```
///
/// Systems in `Update` need no ordering: they run before all of these.
/// A system `.after(PlantSystems::Rebuild)` sees the tables as this frame's
/// changes left them; its own changes show up the next frame.
///
/// [`DetectChanges`]: PlantSystems::DetectChanges
/// [`Rebuild`]: PlantSystems::Rebuild
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlantSystems {
    /// Notices changed fields (their component, position, visibility or
    /// render layers), masks, heightmaps and meshes, removed fields,
    /// and fields Bevy cannot draw as asked (a tilt, a layer above 31), bakes
    /// mesh grounds, and marks the tables for an update.
    DetectChanges,
    /// Applies [`PlantEdit`]s, [`PlantBrush`]es and [`PlantRegrowth`] to the
    /// fields' density masks. Write a `PlantEdit` from a system that runs
    /// `.before(PlantSystems::Edit)` (any system in `Update` does) and it is
    /// drawn this frame; read [`PlantSampler`] `.after(PlantSystems::Edit)`
    /// to see this frame's edits.
    Edit,
    /// Updates the tables from what changed: a full rebuild for changed
    /// fields and images, or only the entries for a field shown, hidden or
    /// moved to other render layers. A field whose images have not loaded
    /// yet waits.
    Rebuild,
}

/// Everything a game needs to draw grass, as a plugin group. Add it after
/// `DefaultPlugins`:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use procgena_plant_bevy::prelude::*;
/// App::new()
///     .add_plugins((DefaultPlugins, PlantPlugins))
///     .run();
///
/// // Start from other settings, and leave out the far-grass material:
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(
///         PlantPlugins
///             .set(PlantPlugin {
///                 settings: PlantSettings::MEDIUM,
///                 ..default()
///             })
///             .disable::<FarPlantPlugin>(),
///     )
///     .run();
/// ```
///
/// - [`PlantPlugin`]: fields, presets, wind, interaction and the rendering.
///   The only one that is required.
/// - [`FarPlantPlugin`]: [`FarPlantMaterial`], for ground past the last blade.
/// - [`PlantFilePlugin`]: `.plant.yml` and `.plant_scatter.yml` files, and the
///   presets as assets.
///
/// `PlantDebugPlugin` (cargo feature `debug`)
/// is not in the group; add it by hand.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlantPlugins;

impl PluginGroup for PlantPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(PlantPlugin::default())
            .add(FarPlantPlugin)
            .add(PlantFilePlugin)
    }
}

/// Registers grass rendering: fields, presets, wind, interaction, settings and
/// the passes that draw them. Part of [`PlantPlugins`]; add it alone to leave
/// out the far-grass material or plant and field file loading.
///
/// The settings are the [`PlantSettings`] resource from then on; change that
/// to change them at runtime. The built-in wind, and the built-in choice of
/// light ([`PlantLightPlugin`]), can be left out:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use procgena_plant_bevy::prelude::*;
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(PlantPlugins.set(PlantPlugin {
///         wind: false,
///         ..default()
///     }))
///     // Write `PlantWindFrame` from your own wind plugin.
///     .run();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PlantPlugin {
    /// How much grass is drawn, how far, and at what cost: a tier such as
    /// [`PlantSettings::MEDIUM`], with any setting changed.
    pub settings: PlantSettings,
    /// Add the built-in [`PlantWindPlugin`]. Turn it off to drive the grass
    /// with another wind model that writes [`PlantWindFrame`].
    pub wind: bool,
    /// Add the built-in [`PlantLightPlugin`], which follows Bevy's
    /// directional lights. Turn it off to write [`PlantLightFrame`] yourself.
    pub lights: bool,
}

impl Default for PlantPlugin {
    fn default() -> Self {
        Self {
            settings: PlantSettings::default(),
            wind: true,
            lights: true,
        }
    }
}

impl Plugin for PlantPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/bevy_bridge.wesl");
        load_shader_library!(app, "shaders/blade.wesl");
        load_shader_library!(app, "shaders/common.wesl");
        load_shader_library!(app, "shaders/wind.wesl");
        load_shader_library!(app, "shaders/look.wesl");
        load_shader_library!(app, "shaders/plant.wesl");
        load_shader_library!(app, "shaders/far.wesl");
        embedded_asset!(app, "shaders/plant_main.wesl");
        embedded_asset!(app, "shaders/plant_prepass.wesl");
        embedded_asset!(app, "shaders/plant_shadow.wesl");
        embedded_asset!(app, "shaders/generate.wesl");
        embedded_asset!(app, "shaders/trample.wesl");
        let shaders = render::resources::PlantShaders {
            main: load_embedded_asset!(app, "shaders/plant_main.wesl"),
            prepass: load_embedded_asset!(app, "shaders/plant_prepass.wesl"),
            shadow: load_embedded_asset!(app, "shaders/plant_shadow.wesl"),
            generate: load_embedded_asset!(app, "shaders/generate.wesl"),
            trample: load_embedded_asset!(app, "shaders/trample.wesl"),
        };
        register_types(app);
        app.init_resource::<PlantWind>()
            .init_resource::<PlantWindFrame>()
            .init_resource::<clock::PlantClock>()
            .add_systems(Update, clock::tick)
            .init_resource::<sun::ShadowSetupReport>()
            .insert_resource(self.settings)
            .add_plugins((stats::PlantDiagnosticsPlugin, scatters::plugin))
            .add_systems(
                PostUpdate,
                (
                    api::warn_oversized_interactors,
                    warn_about_settings.run_if(resource_changed::<PlantSettings>),
                    sun::check_shadow_setup.after(PlantSystems::Rebuild),
                ),
            )
            .add_plugins(render::PlantRenderPlugin { shaders });
        physics::plugin(app);
        reach::plugin(app);
        sun::configure_light_systems(app);
        if self.wind {
            app.add_plugins(PlantWindPlugin);
        }
        if self.lights {
            app.add_plugins(PlantLightPlugin);
        }
    }
}

/// Registers everything a game puts in a scene, an inspector or a save file,
/// with the type data to match: components with `ReflectComponent`,
/// resources with `ReflectResource`, the preset assets with `ReflectAsset`.
fn register_types(app: &mut App) {
    app.init_asset::<PlantPresetAsset>()
        .register_asset_reflect::<PlantPresetAsset>()
        .init_asset::<PlantScatterPresetAsset>()
        .register_asset_reflect::<PlantScatterPresetAsset>()
        .register_type::<PlantPreset>()
        .register_type::<PlantScatterPreset>()
        .register_type::<PlantScatterOptions>()
        .register_type::<PlantParams>()
        .register_type::<PlantSegment>()
        .register_type::<PlantAccent>()
        .register_type::<PlantBranches>()
        .register_type::<PlantArrangement>()
        .register_type::<PlantVariation>()
        .register_type::<GroundTint>()
        .register_type::<Srgb>()
        .register_type::<ScatterPlant>()
        .register_type::<PlantScatter>()
        .register_type::<PlantGround>()
        .register_type::<PlantInteractor>()
        .register_type::<PlantInteractorSpread>()
        .register_type::<PlantInteractorFootprint>()
        .register_type::<PlantInteractorFromCollider>()
        .register_type::<PlantColliderBounds>()
        .register_type::<PlantEdit>()
        .register_type::<PlantBrushShape>()
        .register_type::<PlantEditOp>()
        .register_type::<PlantBrush>()
        .register_type::<PlantBrushFootprint>()
        .register_type::<PlantRegrowth>()
        .register_type::<EditStats>()
        .register_type::<NoPlants>()
        .register_type::<PlantSun>()
        .register_type::<PlantCamera>()
        .register_type::<PlantWind>()
        .register_type::<PlantWindFrame>()
        .register_type::<PlantLightFrame>()
        .register_type::<PlantSettings>()
        .register_type::<PlantQuality>()
        .register_type::<PlantShadows>()
        .register_type::<PlantSupport>();
}

/// Says which settings were out of range, once per change. The render world
/// clamps them either way.
fn warn_about_settings(settings: Res<PlantSettings>) {
    for problem in settings.problems() {
        warn!("procgena_plant: PlantSettings::{problem}");
    }
}

#[cfg(test)]
mod tests;
