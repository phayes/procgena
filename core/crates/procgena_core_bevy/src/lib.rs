//! The shared Bevy runtime for procgena assets.
//!
//! Every asset fades between its levels of detail in the same way, and draws
//! them as far as one quality setting allows. This crate holds the Bevy side
//! of that; the levels themselves are [`procgena_core::Lod`].
//!
//! - [`LodPlugin`] adds the dither shader module and the [`LodQuality`]
//!   resource. Asset plugins add it themselves; adding it more than once is
//!   harmless.
//! - [`visibility_range`] turns one level's [`LodRange`] into Bevy's
//!   [`VisibilityRange`], for levels drawn as ordinary entities. Bevy then
//!   fades them with the same dither pattern.
//! - Renderers that draw many copies themselves import
//!   `procgena_core_bevy::shaders::dither` in their shaders (see
//!   [`DITHER_WESL`]) to fade each copy in every pass, shadow maps included.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_core::{Lod, LodLevel, Quality};
//! use procgena_core_bevy::{LodPlugin, LodQuality, visibility_range};
//!
//! fn spawn(mut commands: Commands, quality: Res<LodQuality>) {
//!     let lod = Lod::new(1.0, vec![LodLevel::new(0.0, 0.1, ()), LodLevel::new(10.0, 0.1, ())]);
//!     // A copy 2 m across.
//!     for range in lod.ranges(lod.factor(2.0, quality.0)) {
//!         commands.spawn((Transform::default(), visibility_range(&range)));
//!     }
//! }
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, LodPlugin))
//!     .insert_resource(LodQuality(Quality::Low))
//!     .add_systems(Startup, spawn)
//!     .run();
//! ```

pub mod instanced;

use bevy::app::{App, Plugin};
use bevy::camera::visibility::VisibilityRange;
use bevy::ecs::resource::Resource;
use bevy::shader::load_shader_library;
use procgena_core::{LodRange, Quality};

/// The dither shader module's source.
///
/// With [`LodPlugin`] added, Bevy shaders import it as
/// `procgena_core_bevy::shaders::dither`. It needs no bindings or shader
/// defs, so it can also be compiled on its own, for tests or other engines.
/// It offers:
///
/// - `dither_level(range, distance) -> i32`: the dither level of a copy at
///   `distance` metres, for a level whose range is packed by
///   [`LodRange::to_vec4`]. The same numbers as [`LodRange::dither_level`].
/// - `is_drawn(dither) -> bool`: whether a copy with that level is drawn at
///   all.
/// - `switch_distance(range) -> f32`: where a level takes over, for passes
///   that draw one level only, such as shadow maps.
/// - `dither_discard(frag_coord, dither)`: discards the hidden pixels in a
///   fragment shader, exactly as Bevy's visibility-range dither does.
pub const DITHER_WESL: &str = include_str!("shaders/dither.wesl");

/// Adds the dither shader module and the [`LodQuality`] resource.
///
/// Every asset plugin that fades levels adds it. It may be added any number
/// of times; a [`LodQuality`] inserted before or after it is kept.
pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/dither.wesl");
        app.init_resource::<LodQuality>();
    }

    fn is_unique(&self) -> bool {
        false
    }
}

/// Adds the instanced renderer's shaders to the app's embedded assets.
fn load_instanced_shaders(app: &mut App) {
    load_shader_library!(app, "shaders/instanced.wesl");
    bevy::asset::embedded_asset!(app, "shaders/instanced_mesh.wesl");
    bevy::asset::embedded_asset!(app, "shaders/instanced_prepass.wesl");
    bevy::asset::embedded_asset!(app, "shaders/instanced_cull.wesl");
}

/// The quality setting every asset scales its drawing distances by.
///
/// Change it from a settings menu; assets read it when they work out each
/// copy's levels (see [`procgena_core::Lod::factor`]). Defaults to
/// [`Quality::High`].
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LodQuality(pub Quality);

/// Bevy's [`VisibilityRange`] for one level of detail, measured from the
/// copy's origin. Bevy fades the entity in and out over the range's bands
/// with the same dither as the shared shader module.
///
/// A level drawn at every distance ([`LodRange::is_always`]) needs no
/// visibility range; leaving it off saves Bevy the fade work.
pub fn visibility_range(range: &LodRange) -> VisibilityRange {
    VisibilityRange {
        start_margin: range.fade_in.clone(),
        end_margin: range.fade_out.clone(),
        use_aabb: false,
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetPlugin, Assets};
    use bevy::prelude::*;
    use bevy::shader::{Shader, ShaderLoader};
    use procgena_core::{Lod, LodLevel};

    use super::*;

    #[test]
    fn visibility_ranges_carry_the_fade_bands() {
        let lod = Lod::new(
            1.0,
            vec![LodLevel::new(0.0, 0.1, ()), LodLevel::new(10.0, 0.1, ())],
        )
        .with_cull(40.0);
        let ranges: Vec<_> = lod.ranges(2.0).iter().map(visibility_range).collect();
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, 18.0..22.0);
        assert_eq!(ranges[1].start_margin, 18.0..22.0);
        assert_eq!(ranges[1].end_margin, 72.0..88.0);
        assert!(ranges.iter().all(|r| !r.use_aabb));
    }

    #[test]
    fn the_plugin_adds_the_shader_and_keeps_a_chosen_quality() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset_loader::<ShaderLoader>()
            .insert_resource(LodQuality(Quality::Low))
            .add_plugins((LodPlugin, LodPlugin));
        app.update();

        assert_eq!(
            *app.world().resource::<LodQuality>(),
            LodQuality(Quality::Low)
        );
        let server = app.world().resource::<AssetServer>();
        let handle: Handle<Shader> =
            server.load("embedded://procgena_core_bevy/shaders/dither.wesl");
        for _ in 0..100 {
            app.update();
            if app.world().resource::<Assets<Shader>>().contains(&handle) {
                return;
            }
        }
        panic!("the dither shader never loaded");
    }
}
