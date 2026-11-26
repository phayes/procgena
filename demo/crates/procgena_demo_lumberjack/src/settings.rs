//! The demo's own tunables, read from `presets/lumberjack.demo.yml`.

use std::borrow::Cow;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Every tunable of the demo that isn't a World, Ecotope or asset setting.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DemoOptions {
    /// Which features the World's land is built with.
    pub land: LandFeatures,
    /// The square of land the forest covers.
    pub play_area: PlayArea,
    /// How the lumberjack moves on foot.
    pub walking: Walking,
    /// The third- and first-person camera.
    pub camera: CameraRig,
}

/// Whether the land has roads and rivers. These values replace every
/// Biome's own terrain settings. The default, like the built-in terrain
/// presets, is no roads and no rivers. Both make the land slower to build.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LandFeatures {
    /// Build roads, with each terrain's own road settings.
    pub roads: bool,
    /// How many rivers start in each region (the terrain setting
    /// `features.rivers.density`), with their ponds. 0 means none; the
    /// built-in terrains were tuned with values from 0.6 to 1.2.
    pub river_density: f32,
}

/// The play area: a square of Ecotope tiles, centred on the start.
///
/// Play begins once the tiles nearest the start are placed: the tile under
/// the start and the ring of eight around it. The rest are placed while
/// the lumberjack walks, nearest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayArea {
    /// The World position, in metres (x and z), of the area's centre, where
    /// the lumberjack starts.
    pub centre: [f64; 2],
    /// How many tiles across each side. With an even number the start lies
    /// on a corner between four tiles, and play waits for only those four.
    pub tiles: u32,
    /// The side of one tile, in metres.
    pub tile_size: f32,
    /// How many tiles are placed at the same time. More place the forest
    /// sooner but leave fewer threads to build the land.
    pub placing_at_once: u32,
    /// How far from the lumberjack, in metres, the ground and trunks are
    /// solid.
    pub collider_radius: f32,
    /// Keep the forest off water, roads and cliffs. Placing takes longer:
    /// the land is read at every spot a copy could stand.
    pub keep_off: bool,
}

impl Default for PlayArea {
    fn default() -> Self {
        Self {
            centre: [0.0, 0.0],
            tiles: 7,
            tile_size: 220.0,
            placing_at_once: 2,
            collider_radius: 100.0,
            keep_off: true,
        }
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

    /// Reads a `.demo.yml` file, which may name the built-in `standard`
    /// set as its `base`.
    ///
    /// # Errors
    ///
    /// Returns a message if the file can't be read or isn't a valid demo
    /// preset.
    pub fn read(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let key = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".demo.yml"))
            .unwrap_or("file");
        DemoPreset::from_yaml(key, &text)
            .map(|preset| preset.demo)
            .map_err(|e| format!("{path}: {e}"))
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
        if !(self.land.river_density >= 0.0 && self.land.river_density.is_finite()) {
            return Err("land.river_density must be 0 or above".to_owned());
        }
        if self.play_area.tiles == 0 {
            return Err("play_area.tiles must be at least 1".to_owned());
        }
        if self.play_area.placing_at_once == 0 {
            return Err("play_area.placing_at_once must be at least 1".to_owned());
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
