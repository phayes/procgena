//! The demo's own tunables, read from `presets/lumberjack.demo.yml`.

use std::borrow::Cow;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Every tunable of the demo that isn't a World, Ecotope or asset setting.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DemoOptions {
    /// The square of land the forest covers.
    pub play_area: PlayArea,
    /// How the lumberjack moves on foot.
    pub walking: Walking,
    /// The third- and first-person camera.
    pub camera: CameraRig,
}

/// The play area: a square of Ecotope tiles, centred on the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayArea {
    /// The World position, in metres (x and z), of the area's centre, where
    /// the lumberjack starts.
    pub centre: [f64; 2],
    /// How many tiles across each side.
    pub tiles: u32,
    /// The side of one tile, in metres.
    pub tile_size: f32,
    /// How far from the lumberjack, in metres, the ground and trunks are
    /// solid.
    pub collider_radius: f32,
}

impl Default for PlayArea {
    fn default() -> Self {
        Self {
            centre: [0.0, 0.0],
            tiles: 6,
            tile_size: 256.0,
            collider_radius: 100.0,
        }
    }
}

impl PlayArea {
    /// The side of the whole area, in metres.
    pub fn size(&self) -> f32 {
        self.tiles as f32 * self.tile_size
    }
}

/// Walking, running and jumping, and the speeds the clips were made at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Walking {
    /// Walking speed, in metres per second.
    pub walk_speed: f32,
    /// Running speed (with Shift held), in metres per second.
    pub run_speed: f32,
    /// How high a jump rises, in metres.
    pub jump_height: f32,
    /// The steepest slope that can be walked up, in degrees.
    pub max_slope: f32,
    /// How fast the walk clip's feet move over the ground when played at
    /// its own speed, in metres per second.
    pub walk_clip_speed: f32,
    /// The same for the run clip.
    pub run_clip_speed: f32,
    /// Below this speed, in metres per second, the lumberjack stands; above
    /// it, the walk clip plays.
    pub idle_below: f32,
    /// Above this speed, in metres per second, the run clip plays instead
    /// of the walk clip.
    pub run_above: f32,
}

impl Default for Walking {
    fn default() -> Self {
        Self {
            walk_speed: 1.4,
            run_speed: 5.0,
            jump_height: 1.0,
            max_slope: 40.0,
            walk_clip_speed: 0.975,
            run_clip_speed: 5.36,
            idle_below: 0.2,
            run_above: 3.0,
        }
    }
}

/// Where the camera sits, behind the lumberjack or in his eyes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraRig {
    /// How far behind the lumberjack the third-person camera sits, in
    /// metres.
    pub distance: f32,
    /// The height above his feet that the third-person camera looks at,
    /// in metres.
    pub look_height: f32,
    /// The height of his eyes above his feet, for the first-person camera,
    /// in metres.
    pub eye_height: f32,
    /// How far the mouse turns the view, in radians per pixel.
    pub mouse_sensitivity: f32,
    /// The vertical field of view, in degrees.
    pub fov: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            distance: 4.5,
            look_height: 1.5,
            eye_height: 1.68,
            mouse_sensitivity: 0.003,
            fov: 60.0,
        }
    }
}

/// A named set of [`DemoOptions`], as read from a `.demo.yml` file.
pub struct DemoPreset {
    /// The preset's key, such as `"standard"`.
    pub key: Cow<'static, str>,
    /// A short display name.
    pub name: Cow<'static, str>,
    /// What the preset is for.
    pub description: Cow<'static, str>,
    /// The group it is listed under.
    pub group: Cow<'static, str>,
    /// The tunables.
    pub demo: DemoOptions,
}

procgena_core::catalogue! {
    DemoPreset { demo: DemoOptions, check: |options| options.check() }
    "Demo" {
        standard {
            name: "Standard",
            description: "The tunables the demo's code starts from.",
        } => DemoOptions::default();
    }
}

impl DemoOptions {
    /// The demo's own file, `presets/lumberjack.demo.yml`, as built into the
    /// binary.
    pub fn lumberjack() -> Self {
        let text = include_str!("../presets/lumberjack.demo.yml");
        DemoPreset::from_yaml("lumberjack", text)
            .expect("the demo's own preset file is valid")
            .demo
    }

    /// Checks that every value is in range.
    ///
    /// # Errors
    ///
    /// Returns a message naming the first value out of range.
    pub fn check(&self) -> Result<(), String> {
        let positive = [
            ("play_area.tile_size", self.play_area.tile_size),
            ("play_area.collider_radius", self.play_area.collider_radius),
            ("walking.walk_speed", self.walking.walk_speed),
            ("walking.run_speed", self.walking.run_speed),
            ("walking.jump_height", self.walking.jump_height),
            ("walking.walk_clip_speed", self.walking.walk_clip_speed),
            ("walking.run_clip_speed", self.walking.run_clip_speed),
            ("camera.distance", self.camera.distance),
            ("camera.fov", self.camera.fov),
        ];
        for (name, value) in positive {
            if !(value > 0.0 && value.is_finite()) {
                return Err(format!("{name} must be above 0"));
            }
        }
        if self.play_area.tiles == 0 {
            return Err("play_area.tiles must be at least 1".to_owned());
        }
        if !(0.0..90.0).contains(&self.walking.max_slope) {
            return Err("walking.max_slope must be from 0 up to 90 degrees".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demos_own_file_reads() {
        let options = DemoOptions::lumberjack();
        assert_eq!(options.check(), Ok(()));
    }

    #[test]
    fn values_out_of_range_are_refused() {
        let mut options = DemoOptions::default();
        options.walking.run_speed = 0.0;
        assert!(options.check().unwrap_err().contains("run_speed"));
    }
}
