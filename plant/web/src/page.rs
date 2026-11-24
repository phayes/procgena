//! The page's way in: the calls `index.html` makes into the demo, and the
//! status and settings it reads back for its panel and the smoke test.
//!
//! The page calls in between frames, on the one thread the browser gives both,
//! so the mutex is never contended: it is only how a `static` is shared
//! without `unsafe`.

use std::fmt::Write as _;
use std::sync::{Mutex, MutexGuard, PoisonError};

use bevy::prelude::*;
use procgena_plant_bevy::prelude::*;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::tune::{self, TuneEdit, Tuning};
use crate::{Meadow, QUALITIES, Requests};

/// What the page asked for, and the status it reads.
struct Shared {
    requests: Requests,
    /// Settings to change, in the order the page changed them.
    edits: Vec<TuneEdit>,
    /// JSON, rewritten by `publish_status`.
    status: String,
    /// Every setting the page tunes, as JSON (see `tune::write_node`).
    tunables: String,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    requests: Requests {
        quality: None,
        preset: None,
        shadows: None,
        antialiasing: None,
    },
    edits: Vec::new(),
    status: String::new(),
    tunables: String::new(),
});

fn shared() -> MutexGuard<'static, Shared> {
    // A panic ends the app, so a poisoned lock has no later reader to mislead.
    SHARED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Asks for the tier called `name`, `"LOW"` to `"ULTRA"`, from the next frame.
/// False if there is no such tier.
#[wasm_bindgen]
pub fn set_quality(name: &str) -> bool {
    let at = QUALITIES.iter().position(|(quality, _)| *quality == name);
    if at.is_some() {
        shared().requests.quality = at;
    }
    at.is_some()
}

/// Requests a built-in Scatter preset for the next frame.
///
/// `name` is an exact preset key, such as `"wheat"`. Returns `false` if unknown.
#[wasm_bindgen]
pub fn set_preset(name: &str) -> bool {
    let found = crate::preset(name).map(|preset| &*preset.key);
    if found.is_some() {
        shared().requests.preset = found;
    }
    found.is_some()
}

/// Asks for shadows to be enabled or disabled from the next frame.
#[wasm_bindgen]
pub fn set_shadows(enabled: bool) -> bool {
    shared().requests.shadows = Some(enabled);
    true
}

/// Asks for the sample count to be set from the next frame.
/// False if the sample count is not 1, 2, 4 or 8.
#[wasm_bindgen]
pub fn set_antialiasing(samples: u32) -> bool {
    if [1, 2, 4, 8].contains(&samples) {
        shared().requests.antialiasing = Some(samples);
        true
    } else {
        false
    }
}

/// Sets one setting from the next frame: `target` is one of `tune::TARGETS`,
/// `path` selects a nested field (such as `plant.height.y`); `value` is a number,
/// `true` or `false`, a `#rrggbb` colour or an enum variant (`"Some:150"`
/// also sets its value). A value that does not fit is logged and skipped.
#[wasm_bindgen]
pub fn set(target: &str, path: &str, value: &str) {
    shared().edits.push(TuneEdit::Set {
        target: target.to_owned(),
        path: path.to_owned(),
        value: value.to_owned(),
    });
}

/// Puts `target` back as its tier, preset or default has it.
#[wasm_bindgen]
pub fn reset(target: &str) {
    shared().edits.push(TuneEdit::Reset(target.to_owned()));
}

/// Asks for the wind preset called `name`, such as `"STORM"`. False if there
/// is no such preset.
#[wasm_bindgen]
pub fn set_wind(name: &str) -> bool {
    let found = tune::WINDS.iter().any(|(wind, _)| *wind == name);
    if found {
        shared().edits.push(TuneEdit::Wind(name.to_owned()));
    }
    found
}

/// Requests a plant preset before the field's last plant.
///
/// `name` is an exact key, such as `"poppy"`. Returns `false` for an unknown
/// key. A recognised key queues the edit for the next frame.
#[wasm_bindgen]
pub fn add_plant(name: &str) -> bool {
    let found = PlantPreset::named(name).is_some();
    if found {
        shared().edits.push(TuneEdit::AddPlant(name.to_owned()));
    }
    found
}

/// Removes the field's plant at `index`.
#[wasm_bindgen]
pub fn remove_plant(index: usize) {
    shared().edits.push(TuneEdit::RemovePlant(index));
}

/// Imports plant and appearance settings from `.plant_scatter.yml` text.
///
/// Keeps the preview's placement. Returns an empty string on success or a
/// readable error on failure; successful imports are applied on the next frame.
#[wasm_bindgen]
pub fn import_scatter(yaml: &str) -> String {
    match PlantScatterOptions::from_yaml(yaml) {
        Ok(scatter) => {
            shared().edits.push(TuneEdit::Scatter(Box::new(scatter)));
            String::new()
        }
        Err(error) => error.to_string(),
    }
}

/// Grows the meadow back as it was before the brush.
#[wasm_bindgen]
pub fn restore_field() {
    shared().edits.push(TuneEdit::RestoreField);
}

/// Returns the tuning panel's current settings and export text as JSON.
///
/// Each target includes its values and Rust expression; the Scatter also
/// includes its `.plant_scatter.yml` text. A revision identifies changes
/// made outside `set`.
#[wasm_bindgen]
pub fn tunables() -> String {
    shared().tunables.clone()
}

/// The wind presets.
#[wasm_bindgen]
pub fn winds() -> Vec<String> {
    tune::WINDS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// Lists built-in plant keys accepted by [`add_plant`].
///
/// Keys identify presets; they are not the display names.
#[wasm_bindgen]
pub fn plants() -> Vec<String> {
    PlantPreset::all()
        .map(|preset| preset.key.to_string())
        .collect()
}

/// The tiers, in the order the page shows them.
#[wasm_bindgen]
pub fn qualities() -> Vec<String> {
    QUALITIES
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// Lists built-in Scatter keys accepted by [`set_preset`].
///
/// Keys are returned in catalogue order.
#[wasm_bindgen]
pub fn presets() -> Vec<String> {
    PlantScatterPreset::all()
        .map(|preset| preset.key.to_string())
        .collect()
}

/// The demo's status as JSON: whether this device draws grass and why not,
/// the tier and preset shown, the blades of the latest GPU reading, and every
/// `PlantStats` number.
#[wasm_bindgen]
pub fn status() -> String {
    shared().status.clone()
}

pub(crate) fn take_requests(mut requests: ResMut<Requests>, mut tuning: ResMut<Tuning>) {
    let mut shared = shared();
    if !shared.edits.is_empty() {
        tuning.edits.append(&mut shared.edits);
    }
    let page = std::mem::take(&mut shared.requests);
    if page != Requests::default() {
        requests.quality = page.quality.or(requests.quality);
        requests.preset = page.preset.or(requests.preset);
        requests.shadows = page.shadows.or(requests.shadows);
        requests.antialiasing = page.antialiasing.or(requests.antialiasing);
    }
}

pub(crate) fn publish_status(
    meadow: Option<Res<Meadow>>,
    stats: Res<PlantStats>,
    support: Res<PlantSupport>,
    tuning: Res<Tuning>,
    cameras: Query<Ref<Msaa>, With<Camera3d>>,
) {
    let Some(meadow) = meadow else {
        return;
    };
    if tuning.is_changed() && !tuning.json.is_empty() && shared().tunables != tuning.json {
        shared().tunables.clone_from(&tuning.json);
    }
    // What the camera draws with: grass lowers a sample count the GPU lacks.
    let msaa = cameras.iter().next();
    let samples = msaa
        .as_ref()
        .map_or(meadow.antialiasing, |msaa| msaa.samples());
    let msaa_changed = msaa.is_some_and(|msaa| msaa.is_changed());
    if !(meadow.is_changed() || stats.is_changed() || support.is_changed() || msaa_changed) {
        return;
    }
    // Tier and preset names are identifiers; the support message is prose.
    let message = support
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut shared = shared();
    let json = &mut shared.status;
    json.clear();
    let [near, mid, far] = stats.gpu.blades;
    let _ = write!(
        json,
        "{{\"support\":\"{message}\",\"decided\":{},\"supported\":{},\
         \"quality\":\"{}\",\"preset\":\"{}\",\"blades\":[{near},{mid},{far}],\
         \"readings\":{},\"overflows\":{},\
         \"shadows\":{},\"antialiasing\":{},\"stats\":",
        *support != PlantSupport::Unknown,
        support.is_supported(),
        QUALITIES[meadow.quality].0,
        meadow.preset,
        stats.gpu.readings,
        stats.gpu.overflows,
        meadow.shadows,
        samples,
    );
    tune::write_node(&*stats, json);
    json.push('}');
}
