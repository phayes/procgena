//! The shared Bevy runtime for procgena assets.
//!
//! Every asset fades between the steps of its chain in the same way, and
//! draws them as far as one quality setting allows. This crate holds the
//! Bevy side of that; the chain itself is
//! [`procgena_core::chain::DistanceChain`].
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
//! - Shaders that texture by projection import
//!   `procgena_core_bevy::shaders::triplanar` (see [`TRIPLANAR_WESL`]).
//! - [`texture`] loads the texture sets of a
//!   [`TextureList`](procgena_core::texture::TextureList) into texture
//!   arrays, one layer per set.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use procgena_core::Quality;
//! use procgena_core::chain::{DistanceChain, ViewScale};
//! use procgena_core_bevy::{LodPlugin, LodQuality, visibility_range};
//!
//! fn spawn(mut commands: Commands, quality: Res<LodQuality>) {
//!     // A copy 2 m across, seen through the default camera.
//!     let factor = DistanceChain::factor(2.0, ViewScale::DEFAULT);
//!     for range in DistanceChain::DEFAULT.ranges(factor, quality.0) {
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
pub mod texture;

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

/// The source of the shared triplanar shader module, which [`LodPlugin`]
/// adds as `procgena_core_bevy::shaders::triplanar`. It reads no textures,
/// so it serves plain textures and texture arrays alike:
///
/// - `triplanar_uvs(p) -> TriplanarUvs`: the x, y and z projections'
///   coordinates of `p`, a position divided by the texture's repeat length.
/// - `triplanar_weights(n, heights) -> vec3<f32>`: how much of each
///   projection to show, from the unit normal and each projection's height.
/// - `unpack_normal(texel) -> vec3<f32>`: an OpenGL normal-map texel as a
///   tangent-space direction.
/// - `triplanar_normal(t_x, t_y, t_z, n, w)` and `top_normal(t_y, n)`: the
///   surface normal with normal-map detail added.
pub const TRIPLANAR_WESL: &str = include_str!("shaders/triplanar.wesl");

/// Adds the dither shader module and the [`LodQuality`] resource.
///
/// Every asset plugin that fades levels adds it. It may be added any number
/// of times; a [`LodQuality`] inserted before or after it is kept.
pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/dither.wesl");
        load_shader_library!(app, "shaders/triplanar.wesl");
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
    bevy::asset::embedded_asset!(app, "shaders/instanced_attach.wesl");
}

/// The quality setting every asset scales its drawing distances by.
///
/// Change it from a settings menu; assets read it when they work out where
/// each copy's steps are drawn (see
/// [`DistanceChain::ranges`](procgena_core::chain::DistanceChain::ranges)).
/// Defaults to [`Quality::High`].
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

    use super::*;

    #[test]
    fn visibility_ranges_carry_the_fade_bands() {
        let near = LodRange {
            fade_in: 0.0..0.0,
            fade_out: 18.0..22.0,
        };
        let far = LodRange {
            fade_in: 18.0..22.0,
            fade_out: 72.0..88.0,
        };
        let ranges: Vec<_> = [near, far].iter().map(visibility_range).collect();
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
