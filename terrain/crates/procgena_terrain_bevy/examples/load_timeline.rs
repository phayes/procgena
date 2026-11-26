//! Measures a fresh terrain load without opening a window. Terrain is loaded
//! in pieces called chunks; larger regions supply the generated terrain data.
//! The timeline records region generation stages and per-frame load progress.
//!
//! Run from the Rust workspace, with an output directory as the first argument:
//!
//! ```bash
//! cargo run --release -p procgena_terrain_bevy --example load_timeline -- ./timeline
//! ```
//!
//! The example still uses Bevy's renderer, drawing to an in-memory image.
//! It reports when the first chunk is marked as drawn, when all currently drawn
//! chunks at the finest level reach their requested detail, and when loading
//! settles. These milestones track terrain state, not completed image rendering.
//! Times start at entry to `main`, including world creation and camera placement.
//!
//! Options:
//! - `--preset <key>` selects a terrain preset; by default, terrain varies with
//!   elevation. `--seed <n>` sets the world seed (default: 1).
//! - `--eye x y z` and `--target x y z` set the camera position and look-at point.
//!   Their y values are heights above the ground. Defaults are `0 120 0` and
//!   `900 0 -300`, respectively.
//! - `--view <metres>` sets the view distance (default: 4000).
//! - `--rough-from <level>` and `--mid-from <level>` limit feature detail at
//!   that chunk level and above (from 1: level 0 always has every feature).
//!   Higher levels cover coarser chunks. Rough detail omits roads; mid detail
//!   uses coarse road routes. Rough takes precedence; other levels finish at
//!   fine detail.
//! - `--roads` turns roads on in every region, and `--rivers <n>` gives
//!   every region `n` river sources per km². The built-in presets have
//!   neither.
//! - `--timeout <s>` stops recording after this many seconds (default: 900),
//!   checked each frame. This is not a hard deadline for the process.
//! - `--footprint` counts drawn chunks containing roads at mid and fine detail
//!   after recording ends, including after a timeout. Use `--rough-from 1` to
//!   leave roads to level 0 only during loading. Its work is timed
//!   separately and is not included in the CSV files.
//!
//! The output directory is created if needed. Each run overwrites two files:
//! - `stages.csv`: one row per region stage start or finish, with elapsed time,
//!   thread, region, stage, event type, and stage duration in seconds. Start
//!   events have a duration of zero.
//! - `frames.csv`: one row per recorded frame, with elapsed seconds, frame
//!   duration in milliseconds, and counts of drawn, refining (drawn by a
//!   coarser chunk for now), wanted (to draw), building (to draw or within a
//!   build radius) and reading chunks. It also records the finest drawn level
//!   (`-1` if none) and the number of chunks at that level.
//!
//! A console summary accompanies the files: when the first chunk and the
//! first level-0 chunk were drawn, when loading settled, the ground points the
//! stream read, and the triangles drawn. Recording ends when loading settles
//! with at least one drawn chunk, or when the timeout is observed.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use glam::DVec2;
use procgena_terrain::tin::TinTolerance;
use procgena_terrain::{
    ChunkDetail, ChunkFeature, ChunkKey, ChunkSource, Elevation, FeatureKind, FeatureLevel,
    FeatureTier, RegionId, RegionProgress, RegionStage, SurfacePoint, TerrainPreset, TierLevels,
    WorldTerrain, WorldTerrainOptions,
};
use procgena_terrain_bevy::{
    RegionTracker, TerrainChunk, TerrainPlugin, TerrainProgress, TerrainStream, TerrainViewer,
    TerrainWater,
};

/// Shared starting point for elapsed times, initialized at entry to `main`.
static START: OnceLock<Instant> = OnceLock::new();

/// Returns elapsed seconds since [`START`], initializing it if needed.
fn now() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// A timestamped start or finish of one terrain generation stage in a region.
struct StageEvent {
    at: f64,
    thread: String,
    region: RegionId,
    stage: RegionStage,
    finished: Option<Duration>,
}

/// Collected load measurements, output settings, and milestone times in seconds.
#[derive(Resource)]
struct Timeline {
    dir: String,
    events: Arc<Mutex<Vec<StageEvent>>>,
    frames: String,
    first_picture: Option<f64>,
    near_final: Option<f64>,
    settled: Option<f64>,
    timeout: f64,
    /// Number of recorded loading frames lasting more than 50 milliseconds.
    long_frames: usize,
    /// Combined duration of those long frames, in seconds.
    long_total: f64,
    /// Longest recorded frame duration, in seconds.
    longest_frame: f64,
    /// World retained for the optional road count after recording ends.
    footprint: Option<Arc<WorldTerrain>>,
    /// The source the stream reads, counting its ground reads.
    reads: Arc<Counting>,
}

/// The world as the stream reads it, counting every ground height the
/// stream asks for (each point of [`ChunkSource::surface`] and
/// [`ChunkSource::surfaces`]). Region builds inside the world read heights
/// of their own; those aren't counted.
struct Counting {
    world: Arc<WorldTerrain>,
    /// Ground points read.
    points: AtomicU64,
    /// Calls that read them.
    calls: AtomicU64,
}

impl ChunkSource for Counting {
    fn surface(&self, p: DVec2, level: FeatureLevel) -> SurfacePoint {
        self.points.fetch_add(1, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.world.surface(p, level)
    }
    fn surfaces(&self, points: &[DVec2], level: FeatureLevel) -> Vec<SurfacePoint> {
        self.points
            .fetch_add(points.len() as u64, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.world.surfaces(points, level)
    }
    fn level(&self, level: FeatureLevel) -> FeatureLevel {
        self.world.level(level)
    }
    fn level_difference(&self, min: DVec2, max: DVec2, from: FeatureLevel) -> f32 {
        self.world.level_difference(min, max, from)
    }
    fn lines_within(&self, min: DVec2, max: DVec2, level: FeatureLevel) -> Vec<ChunkFeature> {
        self.world.lines_within(min, max, level)
    }
    fn tin_tolerance(&self) -> TinTolerance {
        self.world.tin_tolerance()
    }
    fn level_changes(&self, key: ChunkKey, from: FeatureLevel, to: FeatureLevel) -> bool {
        self.world.level_changes(key, from, to)
    }
    fn level_change_known(
        &self,
        key: ChunkKey,
        from: FeatureLevel,
        to: FeatureLevel,
    ) -> Option<bool> {
        self.world.level_change_known(key, from, to)
    }
}

/// Terrain stream and camera placement used to set up the scene.
#[derive(Resource)]
struct Scene {
    stream: Option<TerrainStream>,
    eye: Vec3,
    target: Vec3,
}

fn main() {
    START.get_or_init(Instant::now);
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let vector = |flag: &str| -> Option<Vec3> {
        let i = args.iter().position(|a| a == flag)?;
        let n: Vec<f32> = args[i + 1..i + 4]
            .iter()
            .map(|v| v.parse().expect("three numbers"))
            .collect();
        Some(Vec3::new(n[0], n[1], n[2]))
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let dir = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .expect("usage: load_timeline <dir> [options]");
    std::fs::create_dir_all(&dir).expect("an output folder");

    // Choose one preset, or vary terrain with elevation as in the viewer example.
    let preset = value("--preset").map(|key| {
        TerrainPreset::named(&key)
            .unwrap_or_else(|| panic!("no terrain preset called {key:?}"))
            .terrain
            .clone()
    });
    let roads = has("--roads");
    let rivers: Option<f32> =
        value("--rivers").map(|d| d.parse().expect("--rivers needs sources per km²"));
    let source = move |_: RegionId, site: DVec2, elevation: &Elevation| {
        let mut options = preset
            .clone()
            .unwrap_or_else(|| match elevation.height(site) {
                h if h < 0.0 => TerrainPreset::plains().terrain.clone(),
                h if h < 150.0 => TerrainPreset::rolling_hills().terrain.clone(),
                _ => TerrainPreset::mountains().terrain.clone(),
            });
        if roads {
            options.features.roads.enabled = true;
        }
        if let Some(density) = rivers {
            options.features.rivers.density = density;
        }
        options
    };
    let seed = value("--seed").map_or(1, |s| s.parse().expect("--seed needs a number"));
    let options = WorldTerrainOptions {
        seed,
        ..WorldTerrainOptions::default()
    };
    let world = WorldTerrain::new(options, Arc::new(source)).expect("a valid world");

    // Capture stage timings while also updating the terrain progress tracker.
    let events: Arc<Mutex<Vec<StageEvent>>> = Arc::default();
    let tracker = RegionTracker::default();
    let world = {
        let events = Arc::clone(&events);
        let tracker = tracker.clone();
        world.with_progress(Arc::new(move |p: &RegionProgress| {
            tracker.record(p);
            let thread = std::thread::current();
            let thread = thread
                .name()
                .map_or_else(|| format!("{:?}", thread.id()), str::to_owned);
            let (region, stage, finished) = match *p {
                RegionProgress::Started { region, stage } => (region, stage, None),
                RegionProgress::Finished {
                    region,
                    stage,
                    elapsed,
                    ..
                } => (region, stage, Some(elapsed)),
            };
            events.lock().unwrap().push(StageEvent {
                at: now(),
                thread,
                region,
                stage,
                finished,
            });
        }))
    };

    // Treat both y coordinates as heights above the terrain. Looking up ground
    // height generates terrain before streaming starts; this time is included.
    let on_ground = |p: Vec3| {
        let ground = world.height(DVec2::new(f64::from(p.x), f64::from(p.z)));
        Vec3::new(
            p.x,
            p.y + if ground.is_finite() { ground } else { 0.0 },
            p.z,
        )
    };
    let eye = on_ground(vector("--eye").unwrap_or(Vec3::new(0.0, 120.0, 0.0)));
    let target = on_ground(vector("--target").unwrap_or(Vec3::new(900.0, 0.0, -300.0)));
    println!("camera placed at {:.2} s", now());

    let tier_levels = TierLevels {
        rough_from: value("--rough-from").map(|v| v.parse().expect("--rough-from needs a level")),
        mid_from: value("--mid-from").map(|v| v.parse().expect("--mid-from needs a level")),
    };
    let world = Arc::new(world.with_tier_levels(tier_levels));
    let reads = Arc::new(Counting {
        world: Arc::clone(&world),
        points: AtomicU64::new(0),
        calls: AtomicU64::new(0),
    });
    let stream = TerrainStream {
        regions: Some(tracker),
        ..TerrainStream::new(reads.clone())
    };

    // Set the view distance. Terrain is read on its own single thread.
    let view: f32 = value("--view").map_or(4000.0, |v| v.parse().expect("--view needs a number"));
    let plugin = TerrainPlugin::detailed(ChunkDetail::DEFAULT, view).expect("a view distance");
    let defaults = DefaultPlugins
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .disable::<WinitPlugin>();

    App::new()
        .add_plugins((defaults, ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
        .add_plugins(plugin)
        .insert_resource(Scene {
            stream: Some(stream),
            eye,
            target,
        })
        .insert_resource(Timeline {
            dir,
            events,
            frames: "t,frame_ms,drawn,refining,wanted,building,reading,finest_level,finest\n"
                .to_owned(),
            first_picture: None,
            near_final: None,
            settled: None,
            timeout: value("--timeout").map_or(900.0, |s| s.parse().expect("--timeout seconds")),
            long_frames: 0,
            long_total: 0.0,
            longest_frame: 0.0,
            footprint: has("--footprint").then_some(world),
            reads,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, record)
        .run();
}

/// Starts terrain streaming and sets up a lit camera rendering to a 1280×720
/// in-memory image. No window is opened and the image is not saved.
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut scene: ResMut<Scene>) {
    commands.spawn(scene.stream.take().expect("set up once"));
    let image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    let handle = images.add(image);
    commands.spawn((
        Camera3d::default(),
        TerrainViewer,
        Projection::Perspective(PerspectiveProjection {
            far: 20_000.0,
            ..default()
        }),
        RenderTarget::Image(handle.into()),
        Transform::from_translation(scene.eye).looking_at(scene.target, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(0.3, 1.0, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Records frame progress and first-reached milestones. Saves the measurements
/// and exits when loading settles with drawn chunks or the timeout is observed.
fn record(
    mut timeline: ResMut<Timeline>,
    time: Res<Time<Real>>,
    progress: Query<&TerrainProgress>,
    chunks: Query<(&TerrainChunk, &Mesh3d)>,
    water: Query<(), With<TerrainWater>>,
    meshes: Res<Assets<Mesh>>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = now();
    let Ok(progress) = progress.single() else {
        return;
    };
    let frame = f64::from(time.delta_secs());

    // The most detailed level drawn now (the smallest level number), and
    // how many chunks are drawn at it.
    let finest_level = chunks.iter().map(|(c, _)| c.key.level).min();
    let finest = chunks
        .iter()
        .filter(|(c, _)| Some(c.key.level) == finest_level)
        .count();

    // Append one CSV row, then update the console progress and milestone times.
    let row = format!(
        "{t:.3},{:.2},{},{},{},{},{},{},{finest}\n",
        frame * 1000.0,
        progress.drawn,
        progress.refining,
        progress.wanted,
        progress.building,
        progress.reading,
        finest_level.map_or(-1, i32::from),
    );
    timeline.frames.push_str(&row);
    if (t / 5.0).floor() != ((t - frame) / 5.0).floor() {
        println!(
            "{t:.0} s: drawn {}, refining {}, reading {}",
            progress.drawn, progress.refining, progress.reading
        );
    }
    if timeline.settled.is_none() && frame > 0.05 {
        timeline.long_frames += 1;
        timeline.long_total += frame;
    }
    timeline.longest_frame = timeline.longest_frame.max(frame);
    if timeline.first_picture.is_none() && progress.drawn > 0 {
        timeline.first_picture = Some(t);
        println!("first picture at {t:.2} s ({} chunks)", progress.drawn);
    }
    if timeline.near_final.is_none() && finest_level == Some(0) {
        timeline.near_final = Some(t);
        println!("level 0 first drawn at {t:.2} s ({finest} chunks)");
    }
    let timed_out = t > timeline.timeout;
    if progress.settled && progress.drawn > 0 || timed_out {
        timeline.settled = Some(t);
        if timed_out {
            println!("timed out at {t:.2} s");
        } else {
            println!("settled at {t:.2} s ({} chunks)", progress.drawn);
        }
        println!(
            "chunks: {} drawn, {} built (drawn or within a build radius)",
            progress.drawn, progress.building
        );
        println!(
            "ground reads: {} points in {} calls",
            timeline.reads.points.load(Ordering::Relaxed),
            timeline.reads.calls.load(Ordering::Relaxed)
        );
        // Level-0 chunks with water read their 67 × 67 regular samples
        // once for it, on top of their canonical mesh: included above.
        let wet = water.iter().count();
        println!(
            "  of which water: {wet} level-0 chunks × 4,489 points = {}",
            wet * 4489
        );
        // Triangles drawn, skirts included, by chunk level.
        let mut triangles: std::collections::BTreeMap<u8, (usize, usize)> = Default::default();
        for (chunk, mesh) in &chunks {
            let count = meshes
                .get(&mesh.0)
                .and_then(Mesh::indices)
                .map_or(0, |indices| indices.len() / 3);
            let level = triangles.entry(chunk.key.level).or_default();
            level.0 += 1;
            level.1 += count;
        }
        let total: usize = triangles.values().map(|(_, t)| t).sum();
        print!("triangles drawn: {total}");
        for (level, (chunks, count)) in &triangles {
            print!(", L{level} {count} in {chunks}");
        }
        println!();
        write_files(&timeline);
        if let Some(world) = &timeline.footprint {
            let keys: Vec<_> = chunks.iter().map(|(c, _)| c.key).collect();
            count_road_chunks(world, &keys);
        }
        exit.write(AppExit::Success);
    }
}

/// Prints the number of supplied chunks containing roads at mid and fine
/// detail, grouped by chunk level. Queries may generate additional terrain;
/// the reported duration includes that work as well as counting.
fn count_road_chunks(world: &WorldTerrain, keys: &[ChunkKey]) {
    let started = Instant::now();
    let mut levels: std::collections::BTreeMap<u8, [usize; 3]> = std::collections::BTreeMap::new();
    for key in keys {
        let roads = |tier| {
            world
                .lines_within_tier(key.min_corner(), key.max_corner(), tier)
                .iter()
                .any(|f| f.id.kind == FeatureKind::Road)
        };
        let row = levels.entry(key.level).or_default();
        row[0] += 1;
        row[1] += usize::from(roads(FeatureTier::Mid));
        row[2] += usize::from(roads(FeatureTier::Fine));
    }
    println!(
        "chunks a road reaches (counted in {:.1} s): level, chunks, at mid, at fine",
        started.elapsed().as_secs_f64()
    );
    for (level, [all, mid, fine]) in levels {
        println!("  {level}: {all}, {mid}, {fine}");
    }
}

/// Saves the recorded stage events and frames to the output directory,
/// replacing existing CSV files. Prints stage totals and frame timing statistics.
fn write_files(timeline: &Timeline) {
    let events = timeline.events.lock().unwrap();
    let mut stages = "t,thread,region,stage,event,seconds\n".to_owned();
    for e in events.iter() {
        let (event, seconds) = match e.finished {
            Some(d) => ("finish", d.as_secs_f64()),
            None => ("start", 0.0),
        };
        writeln!(
            stages,
            "{:.4},{},{},{:?},{event},{seconds:.4}",
            e.at, e.thread, e.region.0, e.stage
        )
        .unwrap();
    }
    std::fs::write(format!("{}/stages.csv", timeline.dir), stages).unwrap();
    std::fs::write(format!("{}/frames.csv", timeline.dir), &timeline.frames).unwrap();

    // Summarize completed builds by stage. Summed durations can overlap across
    // threads and nested stages, so they are not elapsed load time.
    let mut totals: std::collections::BTreeMap<RegionStage, (usize, Vec<u64>, f64)> =
        std::collections::BTreeMap::new();
    for e in events.iter() {
        if let Some(d) = e.finished {
            let entry = totals.entry(e.stage).or_default();
            entry.0 += 1;
            entry.1.push(e.region.0);
            entry.2 += d.as_secs_f64();
        }
    }
    for (stage, (builds, mut regions, seconds)) in totals {
        regions.sort_unstable();
        regions.dedup();
        println!(
            "{stage:?}: {builds} builds of {} regions, {seconds:.2} s summed (nested stages included)",
            regions.len()
        );
    }
    println!(
        "frames over 50 ms before settling: {} ({:.2} s); longest {:.0} ms",
        timeline.long_frames,
        timeline.long_total,
        timeline.longest_frame * 1000.0
    );
}
