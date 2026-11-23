//! Bakes tree impostors ahead of time into a folder, to ship with a game:
//! point [`ImpostorSettings::shipped_dir`] at the folder, and those trees
//! draw their impostors as soon as they are generated, without baking.
//!
//! ```sh
//! cargo run -p procgena_tree_bevy --example bake_impostors -- --out impostors
//! cargo run -p procgena_tree_bevy --example bake_impostors -- --out impostors --grove pine_stand --tree oak_large
//! cargo run -p procgena_tree_bevy --example bake_impostors -- --out impostors --compress shipped
//! ```
//!
//! - `--out <dir>`: the folder to fill, created if missing. Impostors
//!   already in it are kept and not baked again.
//! - `--compress <dir>`: once every impostor is saved, also write them all
//!   compressed (KTX2 files of BC7 texels with mip levels) into this
//!   folder, with [`compress_impostors`]. Ship that folder instead: it is
//!   smaller on the GPU and looks smoother far away. It may be `--out`.
//! - `--grove <key>`: bake every tree of this grove preset. Repeat for more.
//! - `--tree <key>`: bake this tree preset. Repeat for more.
//!
//! Without `--grove` or `--tree`, every grove preset is baked. The trees are
//! generated with [`TreePlugin`]'s default detail levels and the
//! repository's textures; impostors baked with other settings are different
//! impostors, so bake with the settings your game uses.
//!
//! Runs without a window and exits once every impostor is saved.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use procgena_impostor::{ImpostorPlugin, ImpostorSettings, ImpostorWrites, compress_impostors};
use procgena_tree::{GrovePreset, GroveTree, TreeGrove, TreePreset};
use procgena_tree_bevy::{GrovePreparing, GroveReady, PrepareGrove, TreePlugin};

/// Seconds to wait for every impostor before giving up.
const TIMEOUT: f32 = 1800.0;

/// The groves still being prepared.
#[derive(Resource)]
struct Remaining {
    groves: usize,
    started: Instant,
    /// The folder impostors are saved to.
    out: PathBuf,
    /// The folder to write compressed impostors to, if any.
    compress: Option<PathBuf>,
}

fn main() -> AppExit {
    // Read the command line.
    let mut out: Option<PathBuf> = None;
    let mut compress: Option<PathBuf> = None;
    let mut groves: Vec<TreeGrove> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value())),
            "--compress" => compress = Some(PathBuf::from(value())),
            "--grove" => {
                let key = value();
                let preset = GrovePreset::from_key(&key)
                    .unwrap_or_else(|| panic!("{key} is not a grove preset"));
                groves.push(preset.grove());
            }
            "--tree" => {
                let key = value();
                let preset = TreePreset::from_key(&key)
                    .unwrap_or_else(|| panic!("{key} is not a tree preset"));
                groves.push(TreeGrove::new(vec![
                    GroveTree::new(preset).with_variants(1),
                ]));
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let out = out.expect("--out <dir> names the folder to fill");
    if groves.is_empty() {
        groves = GrovePreset::ALL
            .iter()
            .map(|preset| preset.grove())
            .collect();
    }

    // Render without a window; baking needs no camera of its own.
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(AssetPlugin {
                file_path: "../../assets".to_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
        ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        ImpostorPlugin {
            settings: ImpostorSettings {
                cache_dir: Some(out.clone()),
                ..default()
            },
        },
        TreePlugin {
            texture_root: Some("textures".to_owned()),
            ..default()
        },
    ))
    .insert_resource(Remaining {
        groves: groves.len(),
        started: Instant::now(),
        out,
        compress,
    })
    .add_observer(grove_ready)
    .add_systems(Update, (report, finish));
    for grove in groves {
        app.world_mut().spawn(PrepareGrove::new(grove));
    }
    app.run()
}

/// Counts down the groves still being prepared.
fn grove_ready(_: On<GroveReady>, mut remaining: ResMut<Remaining>) {
    remaining.groves = remaining.groves.saturating_sub(1);
}

/// Prints the progress of every grove as it changes.
fn report(groves: Query<&GrovePreparing, Changed<GrovePreparing>>) {
    for progress in &groves {
        info!("{} of {} steps", progress.done, progress.total);
    }
}

/// Exits once every grove is prepared and every impostor saved, and
/// compressed if asked.
fn finish(
    remaining: Res<Remaining>,
    writes: Res<ImpostorWrites>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = remaining.started.elapsed().as_secs_f32();
    if remaining.groves == 0 && writes.pending() == 0 {
        info!("every impostor is saved, in {elapsed:.1} s");
        let Some(compress) = &remaining.compress else {
            exit.write(AppExit::Success);
            return;
        };
        let started = Instant::now();
        match compress_impostors(&remaining.out, compress) {
            Ok(count) => {
                let seconds = started.elapsed().as_secs_f32();
                info!("{count} impostors compressed, in {seconds:.1} s");
                exit.write(AppExit::Success);
            }
            Err(error) => {
                error!("cannot compress the impostors: {error}");
                exit.write(AppExit::error());
            }
        }
    } else if elapsed > TIMEOUT {
        error!("impostors were not ready after {TIMEOUT} seconds");
        exit.write(AppExit::error());
    }
}
