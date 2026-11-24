//! Impostors for generated objects: an object drawn from a hemisphere of
//! views into two atlases (colour and normals), then drawn far away as one
//! quad that shows the view nearest the camera, lit by the scene.
//!
//! Add [`ImpostorPlugin`], then spawn an [`ImpostorRequest`] naming the
//! object's meshes and bake materials. The impostor is baked in the
//! background; when it is ready the request's entity gets a
//! [`BakedImpostor`] and [`ImpostorReady`] is triggered on it. Draw it with
//! [`Impostor::bundle`] and [`Impostor::material`].
//!
//! Each impostor is named by an [`ImpostorKey`]: the
//! [`SpecimenId`](procgena_core::SpecimenId) of what is baked, plus its
//! [`BakeSettings`]. An asset that draws its specimens with an impostor as
//! their farthest level asks for them through [`Impostors::request`], which
//! bakes each key once and keeps it with a shared material, and places the
//! impostor among its levels of detail with an [`ImpostorLevel`].
//!
//! Bake materials draw base colour or normals instead of lit colour.
//! [`bake_standard`] makes them from a [`StandardMaterial`]; a generator
//! with its own material extension calls [`use_bake_shader`] from its
//! `specialize` instead, keeping its vertex shader.
//!
//! Impostors don't move with their object's animation, such as wind, and
//! show a fixed set of views, so use them only far away.
//!
//! Each bake briefly spawns its own 3D cameras, marked
//! [`ImpostorBakeCamera`]; leave them out of queries for your own camera.

mod bake;
mod batch;
mod cache;
mod key;
mod ktx2;
mod layout;
mod level;
mod material;
mod mips;

use std::path::PathBuf;

use bevy::asset::{embedded_asset, load_internal_asset};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::{Shader, load_shader_library};

pub use cache::{ImpostorWrites, compress_impostors};

pub use bake::{
    BakePart, BakedImpostor, Impostor, ImpostorBakeCamera, ImpostorQuad, ImpostorReady,
    ImpostorRequest, Impostors,
};
pub use batch::{ImpostorBatch, ImpostorBatchFinished, ImpostorBatchProgress};
pub use key::{BakeLayout, BakeSettings, ImpostorKey};
pub use layout::{
    elevation_basis, elevation_direction, elevation_index, view_basis, view_cell, view_direction,
};
pub use level::ImpostorLevel;
pub use material::{
    BAKE_SHADER, BakeExtension, BakeKey, BakeOutput, BakeStandardMaterial, INSTANCE_LOCATION,
    INSTANCE_SIZE, ImpostorExtension, ImpostorMaterial, ImpostorPipelineKey, ImpostorUniform,
    bake_standard, instance_buffer_layout, use_bake_shader,
};

/// How impostors are baked and stored. Each impostor's view count and size
/// come from its key's [`BakeSettings`].
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ImpostorSettings {
    /// Most bakes in progress at once.
    pub concurrent_bakes: usize,
    /// Frames a bake keeps drawing once every part's pipeline is built and
    /// its textures are loaded, before its picture is kept. At least 1.
    pub settle_frames: u32,
    /// A folder baked impostors are saved to and read back from on later
    /// runs, created if missing. `None` keeps impostors in memory only.
    pub cache_dir: Option<PathBuf>,
    /// A read-only folder of impostors shipped with the game, looked in
    /// before `cache_dir`; fill it with [`ImpostorWrites`]-tracked bakes
    /// saved to a `cache_dir` ahead of time, then ideally run
    /// [`compress_impostors`] on them.
    pub shipped_dir: Option<PathBuf>,
}

impl Default for ImpostorSettings {
    fn default() -> Self {
        Self {
            concurrent_bakes: 2,
            settle_frames: 3,
            cache_dir: None,
            shipped_dir: None,
        }
    }
}

/// Bakes [`ImpostorRequest`]s and draws [`ImpostorMaterial`]s. Add it after
/// Bevy's default plugins.
#[derive(Debug, Clone, Default)]
pub struct ImpostorPlugin {
    /// How impostors are baked.
    pub settings: ImpostorSettings,
}

impl Plugin for ImpostorPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/layout.wesl");
        load_shader_library!(app, "shaders/common.wesl");
        embedded_asset!(app, "shaders/impostor.wesl");
        embedded_asset!(app, "shaders/impostor_prepass.wesl");
        load_internal_asset!(app, BAKE_SHADER, "shaders/bake.wesl", Shader::from_wesl);

        // The quad every impostor is drawn with: corners at ±1, placed by
        // the shaders.
        let quad = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(2.0, 2.0));

        app.add_plugins((
            MaterialPlugin::<ImpostorMaterial>::default(),
            MaterialPlugin::<BakeStandardMaterial>::default(),
        ))
        .insert_resource(self.settings.clone())
        .insert_resource(ImpostorQuad(quad))
        .init_resource::<Impostors>()
        .init_resource::<bake::ActiveBakes>()
        .init_resource::<cache::DiskReads>()
        .init_resource::<ImpostorWrites>()
        .add_systems(Update, systems());

        // The render world reports when each bake's pipelines are built.
        let readiness = bake::BakeReadiness::for_app(app);
        app.insert_resource(readiness.clone());
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(readiness)
                .init_resource::<bake::ExtractedBakeCameras>()
                .add_systems(ExtractSchedule, bake::extract_bake_cameras)
                .add_systems(
                    Render,
                    bake::check_bake_pipelines.in_set(RenderSystems::Cleanup),
                );
        }
    }
}

/// The plugin's systems, in order.
fn systems() -> impl IntoScheduleConfigs<ScheduleSystem, ()> {
    (
        batch::start_batches,
        cache::finish_reads,
        bake::finish_bakes,
        bake::start_bakes,
        batch::track_batches,
        cache::finish_writes,
    )
        .chain()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`ImpostorReady`] and [`ImpostorBatchFinished`], in order.
    #[derive(Resource, Default)]
    struct Seen {
        ready: Vec<(Entity, ImpostorKey)>,
        finished: Vec<Entity>,
    }

    /// The plugin's bake and batch systems without rendering: bakes spawn
    /// their copies and cameras as plain entities.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<BakeStandardMaterial>()
            .init_asset::<ImpostorMaterial>()
            .insert_resource(ImpostorSettings {
                settle_frames: 3,
                ..default()
            })
            .init_resource::<Impostors>()
            .init_resource::<bake::ActiveBakes>()
            .init_resource::<bake::BakeReadiness>()
            .init_resource::<cache::DiskReads>()
            .init_resource::<ImpostorWrites>()
            .init_resource::<Seen>()
            .add_systems(Update, systems())
            .add_observer(|event: On<ImpostorReady>, mut seen: ResMut<Seen>| {
                seen.ready.push((event.entity, event.key));
            })
            .add_observer(|event: On<ImpostorBatchFinished>, mut seen: ResMut<Seen>| {
                seen.finished.push(event.entity);
            });
        app
    }

    /// A request for a unit cube's impostor under test key `id`.
    fn request(app: &mut App, id: u32) -> ImpostorRequest {
        cube_request(app, ImpostorKey::test(id))
    }

    /// A request for a unit cube's impostor under `key`.
    fn cube_request(app: &mut App, key: ImpostorKey) -> ImpostorRequest {
        let world = app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let mut materials = world.resource_mut::<Assets<BakeStandardMaterial>>();
        let base = StandardMaterial::default();
        let part = BakePart::new(
            mesh,
            materials.add(bake_standard(&base, BakeOutput::Albedo)),
            materials.add(bake_standard(&base, BakeOutput::Normal)),
        );
        ImpostorRequest {
            key,
            parts: vec![part],
            wait_for: Vec::new(),
        }
    }

    fn cameras(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<(), With<Camera3d>>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn a_request_is_baked_once_and_kept_for_later_requests() {
        let mut app = app();
        let first = request(&mut app, 1);
        let key = first.key;
        let entity = app.world_mut().spawn(first).id();

        // The bake draws for its settle frames, then cleans up.
        app.update();
        assert_eq!(cameras(&mut app), 2, "one camera per atlas");
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(cameras(&mut app), 0);
        let baked = app.world().get::<BakedImpostor>(entity).unwrap().0.clone();
        assert_eq!(baked.frames, 8);
        // A unit cube's corners are √3/2 from its centre.
        assert!((baked.radius - 3f32.sqrt() / 2.0).abs() < 1e-4);
        assert_eq!(app.world().resource::<Seen>().ready, vec![(entity, key)]);

        // The same key later is answered in the next frame, without a bake.
        let again = request(&mut app, 1);
        let second = app.world_mut().spawn(again).id();
        app.update();
        assert_eq!(cameras(&mut app), 0);
        assert_eq!(app.world().get::<BakedImpostor>(second).unwrap().0, baked);
    }

    /// Asks for `key` through [`Impostors::request`], as an asset does each
    /// frame, and returns whether it is ready and how often `make` ran.
    fn ask(app: &mut App, key: ImpostorKey, made: &mut usize) -> bool {
        let request = cube_request(app, key);
        let world = app.world_mut();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let ready = world.resource_scope(|world, mut impostors: Mut<Impostors>| {
            let mut commands = Commands::new(&mut queue, world);
            impostors
                .request(&mut commands, key, || {
                    *made += 1;
                    request
                })
                .is_some()
        });
        queue.apply(world);
        ready
    }

    #[test]
    fn assets_asking_for_one_key_share_one_bake_and_material() {
        let mut app = app();
        // A tree and a rock: two kinds of specimen, two bake sizes.
        let pool = |kind| {
            procgena_core::PoolId::new(
                procgena_core::GeneratorKind::from_key(kind),
                procgena_core::OptionsHash(5),
            )
        };
        let tree = ImpostorKey::specimen(
            procgena_core::SpecimenId::new(1, pool("tree.generator.kind"), 0),
            BakeSettings::default(),
            &[],
        );
        let rock = ImpostorKey::specimen(
            procgena_core::SpecimenId::new(1, pool("stone.generator.kind"), 0),
            BakeSettings::new(4, 64),
            &[],
        );

        // Every asset asks every frame; each key is requested once.
        let (mut tree_made, mut rock_made) = (0, 0);
        let mut frames = 0;
        while !(ask(&mut app, tree, &mut tree_made) && ask(&mut app, rock, &mut rock_made)) {
            app.update();
            frames += 1;
            assert!(frames < 20, "the impostors never arrived");
        }
        assert_eq!((tree_made, rock_made), (1, 1));

        // Each is baked with its own settings, and has one shared material.
        let impostors = app.world().resource::<Impostors>();
        assert_eq!(impostors.get(tree).unwrap().frames, 8);
        assert_eq!(impostors.get(rock).unwrap().frames, 4);
        assert_ne!(impostors.material(tree), impostors.material(rock));
        let ready: Vec<_> = app.world().resource::<Seen>().ready.clone();
        assert_eq!(ready.len(), 2);

        // The plugin's own request entities are gone once answered.
        app.update();
        let left = app
            .world_mut()
            .query_filtered::<(), With<ImpostorRequest>>()
            .iter(app.world())
            .count();
        assert_eq!(left, 0);
    }

    #[test]
    fn stored_impostors_are_used_instead_of_baking_shipped_first() {
        let shipped = std::env::temp_dir().join(format!("pi_shipped_{}", std::process::id()));
        let cached = std::env::temp_dir().join(format!("pi_cached_{}", std::process::id()));
        let mut app = app();
        let in_both = request(&mut app, 10);
        let in_cache = request(&mut app, 11);
        let stored = |radius: f32| cache::StoredImpostor {
            frames: 2,
            size: 4,
            center: Vec3::ZERO,
            radius,
            albedo: vec![255; 64],
            normal: vec![128; 64],
        };
        cache::write(&shipped, in_both.key, &stored(1.0)).unwrap();
        cache::write(&cached, in_both.key, &stored(2.0)).unwrap();
        cache::write(&cached, in_cache.key, &stored(3.0)).unwrap();
        {
            let mut settings = app.world_mut().resource_mut::<ImpostorSettings>();
            settings.shipped_dir = Some(shipped.clone());
            settings.cache_dir = Some(cached.clone());
        }
        let both = app.world_mut().spawn(in_both).id();
        let cache_only = app.world_mut().spawn(in_cache).id();

        // Read in the background, without any bake.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.world().get::<BakedImpostor>(cache_only).is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the stored impostors never arrived"
            );
            app.update();
            assert_eq!(cameras(&mut app), 0, "a stored impostor was baked");
        }
        app.update();
        let radius = |app: &App, e| app.world().get::<BakedImpostor>(e).unwrap().0.radius;
        assert_eq!(radius(&app, both), 1.0, "the shipped folder comes first");
        assert_eq!(radius(&app, cache_only), 3.0);

        // A key in neither folder is baked.
        let missing = request(&mut app, 12);
        app.world_mut().spawn(missing);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(cameras(&mut app), 2);
        let _ = std::fs::remove_dir_all(shipped);
        let _ = std::fs::remove_dir_all(cached);
    }

    #[test]
    fn a_batch_reports_progress_and_finishes_once() {
        let mut app = app();
        // Three impostors, more than the two bakes allowed at once.
        let requests = (1..=3).map(|id| request(&mut app, id)).collect();
        let batch = app.world_mut().spawn(ImpostorBatch::new(requests)).id();
        app.update();
        let progress = |app: &App| *app.world().get::<ImpostorBatchProgress>(batch).unwrap();
        assert_eq!(progress(&app), ImpostorBatchProgress { done: 0, total: 3 });
        assert!(cameras(&mut app) <= 4, "at most two bakes at once");

        for _ in 0..20 {
            app.update();
        }
        assert_eq!(progress(&app), ImpostorBatchProgress { done: 3, total: 3 });
        assert_eq!(progress(&app).fraction(), 1.0);
        assert_eq!(app.world().resource::<Seen>().finished, vec![batch]);
        assert_eq!(app.world().resource::<Seen>().ready.len(), 3);

        // An empty batch finishes at once.
        let empty = app.world_mut().spawn(ImpostorBatch::new(Vec::new())).id();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Seen>().finished, vec![batch, empty]);
    }
}
