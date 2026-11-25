//! Compute ground weights once per material, directly into its two textures.

use crate::{TerrainMaterial, TerrainSurface};
use bevy::asset::RenderAssetUsages;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, texture_storage_2d,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph};
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::{asset::embedded_asset, core_pipeline::schedule::camera_driver, prelude::*};
use procgena_terrain::ground::{
    Condition, GroundGrid, GroundStack, HeightRule, MaterialId, PALETTE_SIZE, Ramp,
};
use procgena_terrain::{ChunkKey, RoadTags};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Opaque inputs for GPU ground painting. Materials keep these alive until
/// their weight textures have been generated; queries never read them back.
#[derive(Debug)]
pub struct GpuGroundPaint {
    grid: Arc<GroundGrid>,
    program: Arc<Program>,
    from: ChunkKey,
    key: ChunkKey,
}

#[derive(Debug)]
struct Program {
    palette: Vec<MaterialId>,
    words: Vec<[f32; 4]>,
    layers: u32,
    vertical: f32,
}

fn tags(tags: RoadTags) -> u32 {
    [
        RoadTags::ROAD,
        RoadTags::CUT,
        RoadTags::EMBANKMENT,
        RoadTags::RETAINING_WALL,
        RoadTags::BLUFF,
        RoadTags::CLIFF,
    ]
    .into_iter()
    .enumerate()
    .fold(0, |bits, (i, tag)| {
        bits | if tags.contains(tag) { 1 << i } else { 0 }
    })
}

impl Program {
    fn new(stack: &GroundStack) -> Option<Self> {
        let layers: Vec<_> = stack.layers().map(|(_, l)| l).collect();
        // No approximation: stacks needing noise or palette reduction use
        // the CPU until their GPU implementations have parity coverage.
        if layers
            .iter()
            .any(|l| l.noise.is_some() || !l.under.is_empty())
        {
            return None;
        }
        let mut palette: Vec<_> = layers
            .iter()
            .map(|l| stack.catalogue().id(l.material.as_str()).unwrap())
            .collect();
        palette.sort();
        palette.dedup();
        if palette.len() > PALETTE_SIZE {
            return None;
        }
        let mut words = Vec::new();
        for layer in &layers {
            let slot = palette
                .binary_search(&stack.catalogue().id(layer.material.as_str()).unwrap())
                .unwrap();
            words.push([slot as f32, layer.opacity, layer.when.len() as f32, 0.0]);
            for condition in &layer.when {
                let (kind, ramp, mask) = match condition {
                    Condition::Slope(r) => (0, *r, 0),
                    Condition::Height(HeightRule::Absolute(r)) => (1, *r, 0),
                    Condition::Height(HeightRule::Relief(r)) => (2, *r, 0),
                    Condition::HeightAboveWater(r) => (3, *r, 0),
                    Condition::RoadTags(t) => (4, Ramp::default(), tags(*t)),
                };
                if ramp
                    .rise
                    .into_iter()
                    .chain(ramp.fall)
                    .flatten()
                    .any(|v| !v.is_finite())
                {
                    return None;
                }
                words.push([
                    kind as f32,
                    ramp.rise.is_some() as u32 as f32,
                    ramp.fall.is_some() as u32 as f32,
                    mask as f32,
                ]);
                let [a, b] = ramp.rise.unwrap_or([0.0, 1.0]);
                let [c, d] = ramp.fall.unwrap_or([0.0, 1.0]);
                words.push([a, b, c, d]);
            }
        }
        Some(Self {
            palette,
            words,
            layers: layers.len() as u32,
            vertical: stack.vertical(),
        })
    }
}

impl GpuGroundPaint {
    pub(crate) fn prepare(
        stack: &GroundStack,
        grid: Arc<GroundGrid>,
        key: ChunkKey,
    ) -> Option<(Arc<Self>, TerrainSurface)> {
        let program = Arc::new(Program::new(stack)?);
        let surface =
            TerrainSurface::from_palette(grid.size(), &program.palette, stack.catalogue());
        Some((
            Arc::new(Self {
                grid,
                program,
                from: key,
                key,
            }),
            surface,
        ))
    }

    pub(crate) fn part(&self, key: ChunkKey) -> Arc<Self> {
        Arc::new(Self {
            grid: self.grid.clone(),
            program: self.program.clone(),
            from: self.from,
            key,
        })
    }

    fn words(&self) -> Vec<[f32; 4]> {
        let size = self.grid.size();
        let offset =
            ((self.key.min_corner() - self.from.min_corner()) / self.from.size()).as_vec2();
        let scale = (self.key.size() / self.from.size()) as f32;
        let mut words = vec![
            [
                size.x as f32,
                size.y as f32,
                self.program.layers as f32,
                self.program.vertical,
            ],
            [offset.x, offset.y, scale, self.program.palette.len() as f32],
        ];
        for site in self.grid.sites() {
            let water = site.height_above_water();
            words.push([
                site.height,
                site.slope,
                site.relief_height.unwrap_or(0.0),
                water.unwrap_or(0.0).clamp(-f32::MAX, f32::MAX),
            ]);
            words.push([
                site.relief_height.is_some() as u32 as f32,
                water.is_some() as u32 as f32,
                site.road_tags.is_some() as u32 as f32,
                tags(site.road_tags.unwrap_or(RoadTags::NONE)) as f32,
            ]);
        }
        words.extend_from_slice(&self.program.words);
        words
    }

    pub(crate) fn images(&self) -> [Image; 2] {
        let size = self.grid.size();
        std::array::from_fn(|half| {
            let mut image = Image::new_fill(
                Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                if half == 0 {
                    &[255, 0, 0, 0]
                } else {
                    &[0, 0, 0, 0]
                },
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
            image.sampler = bevy::image::ImageSampler::linear();
            image
        })
    }
}

#[derive(Resource, Default)]
pub(crate) struct Enabled(pub bool);

pub(crate) struct GroundComputePlugin;
impl Plugin for GroundComputePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/ground_weights.wgsl");
        app.init_resource::<Enabled>();
        let shader = app
            .world()
            .resource::<AssetServer>()
            .load("embedded://procgena_terrain_bevy/shaders/ground_weights.wgsl");
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(GroundShader(shader))
                .init_resource::<Jobs>()
                .init_resource::<Prepared>()
                .add_systems(RenderStartup, init)
                .add_systems(ExtractSchedule, extract)
                .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
                .add_systems(RenderGraph, dispatch.before(camera_driver));
        }
    }
    fn finish(&self, app: &mut App) {
        let enabled = app
            .get_sub_app(RenderApp)
            .and_then(|r| r.world().get_resource::<RenderDevice>())
            .is_some_and(|d| {
                let limits = d.limits();
                limits.max_compute_workgroups_per_dimension > 0
                    && limits.max_storage_textures_per_shader_stage >= 2
                    && limits.max_storage_buffers_per_shader_stage >= 1
            });
        app.world_mut().resource_mut::<Enabled>().0 = enabled;
    }
}
#[derive(Resource)]
struct GroundShader(Handle<Shader>);
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
}
fn init(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    shader: Res<GroundShader>,
    device: Res<RenderDevice>,
) {
    if device.limits().max_compute_workgroups_per_dimension == 0 {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "terrain ground weights",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("terrain ground weights".into()),
        layout: vec![layout.clone()],
        shader: shader.0.clone(),
        entry_point: Some("paint".into()),
        ..default()
    });
    commands.insert_resource(Pipeline { layout, id });
}
struct Job {
    paint: Arc<GpuGroundPaint>,
    low: AssetId<Image>,
    high: AssetId<Image>,
}
#[derive(Resource, Default)]
struct Jobs(Vec<Job>);
fn extract(mut jobs: ResMut<Jobs>, materials: Extract<Res<Assets<TerrainMaterial>>>) {
    jobs.0.clear();
    for (_, material) in materials.iter() {
        let e = &material.extension;
        if let Some(paint) = &e.gpu_paint {
            jobs.0.push(Job {
                paint: paint.clone(),
                low: e.weights_low.id(),
                high: e.weights_high.id(),
            });
        }
    }
}
struct Ready {
    group: BindGroup,
    size: UVec2,
    done: bool,
    paint: Arc<GpuGroundPaint>,
}
#[derive(Resource, Default)]
struct Prepared(HashMap<AssetId<Image>, Ready>);
fn prepare(
    jobs: Res<Jobs>,
    mut prepared: ResMut<Prepared>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<Pipeline>>,
) {
    let Some(pipeline) = pipeline else {
        return;
    };
    let alive: HashSet<_> = jobs.0.iter().map(|j| j.low).collect();
    prepared.0.retain(|id, _| alive.contains(id));
    for job in &jobs.0 {
        if prepared
            .0
            .get(&job.low)
            .is_some_and(|p| Arc::ptr_eq(&p.paint, &job.paint))
        {
            continue;
        }
        let (Some(low), Some(high)) = (images.get(job.low), images.get(job.high)) else {
            continue;
        };
        let bytes: Vec<u8> = job
            .paint
            .words()
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("terrain paint sites and layers"),
            contents: &bytes,
            usage: BufferUsages::STORAGE,
        });
        let group = device.create_bind_group(
            "terrain ground weights",
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                buffer.as_entire_binding(),
                &low.texture_view,
                &high.texture_view,
            )),
        );
        prepared.0.insert(
            job.low,
            Ready {
                group,
                size: job.paint.grid.size(),
                done: false,
                paint: job.paint.clone(),
            },
        );
    }
}
fn dispatch(
    mut ctx: RenderContext,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    mut prepared: ResMut<Prepared>,
) {
    let Some(pipeline) = pipeline.and_then(|p| cache.get_compute_pipeline(p.id)) else {
        return;
    };
    let mut pass = ctx
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("terrain ground weights"),
            ..default()
        });
    pass.set_pipeline(pipeline);
    for ready in prepared.0.values_mut().filter(|r| !r.done) {
        pass.set_bind_group(0, &ready.group, &[]);
        pass.dispatch_workgroups(ready.size.x.div_ceil(8), ready.size.y.div_ceil(8), 1);
        ready.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
    use bevy::render::renderer::RenderQueue;
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;
    use glam::DVec2;
    use procgena_terrain::{
        FeatureTier,
        ground::{GroundSite, GroundSource, Water},
    };

    struct Sites;
    impl GroundSource for Sites {
        fn site(&self, p: DVec2, _: FeatureTier) -> GroundSite {
            let mut site =
                GroundSite::new(850.0 + p.x as f32 * 5.0, Vec2::new(p.y as f32 / 40.0, 0.0));
            site.water = Some(if p.y < 8.0 {
                Water::Level(site.height + 0.2)
            } else {
                Water::Dry
            });
            site.road_tags = Some(if p.y > 40.0 {
                RoadTags::ROAD
            } else if p.x > 40.0 {
                RoadTags::CUT
            } else {
                RoadTags::NONE
            });
            site
        }
    }

    fn pixels(app: &App, image: AssetId<Image>) -> Vec<u8> {
        let world = app.sub_app(RenderApp).world();
        let device = world.resource::<RenderDevice>();
        let queue = world.resource::<RenderQueue>();
        let images = world.resource::<RenderAssets<GpuImage>>();
        let image = images.get(image).unwrap();
        let size = image.texture.size();
        let row = (size.width * 4).div_ceil(256) * 256;
        let staging = device.create_buffer(&BufferDescriptor {
            label: None,
            size: u64::from(row * size.height),
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            image.texture.as_image_copy(),
            TexelCopyBufferInfo {
                buffer: &staging,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        staging.slice(..).map_async(MapMode::Read, |r| r.unwrap());
        device.poll(PollType::wait_indefinitely()).unwrap();
        let mapped = staging.slice(..).get_mapped_range().unwrap();
        let bytes = mapped
            .chunks(row as usize)
            .flat_map(|r| r[..size.width as usize * 4].iter().copied())
            .collect();
        drop(mapped);
        staging.unmap();
        bytes
    }

    #[test]
    #[ignore = "requires a compute-capable GPU"]
    fn gpu_weights_match_cpu_and_parent_subdivision() {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>()
                .disable::<PipelinedRenderingPlugin>(),
        )
        .add_plugins(crate::TerrainMaterialPlugin);
        app.finish();
        app.cleanup();
        assert!(app.world().resource::<Enabled>().0);
        let stack = GroundStack::bare(3, 1.0);
        let key = ChunkKey::new(0, 0, 0);
        let grid = Arc::new(GroundGrid::chunk(&Sites, key, FeatureTier::Fine).unwrap());
        let cpu = grid.paint(&stack);
        let (paint, surface) = GpuGroundPaint::prepare(&stack, grid, key).unwrap();
        let child = ChunkKey::new(1, 0, 0);
        // A separate parent at level 1 gives a real stand-in with half scale.
        let parent = ChunkKey::new(0, 0, 1);
        let parent_grid = Arc::new(GroundGrid::chunk(&Sites, parent, FeatureTier::Fine).unwrap());
        let parent_cpu = parent_grid.paint(&stack);
        let (parent_paint, parent_surface) =
            GpuGroundPaint::prepare(&stack, parent_grid, parent).unwrap();
        let jobs = [
            (paint, surface, cpu, None),
            (
                parent_paint.part(child),
                parent_surface,
                parent_cpu,
                Some((parent, child)),
            ),
        ];
        let mut handles = Vec::new();
        for (paint, surface, cpu, part) in jobs {
            let palette = paint.program.palette.clone();
            let mut output = paint.images();
            for image in &mut output {
                image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            }
            let [low, high] =
                output.map(|i| app.world_mut().resource_mut::<Assets<Image>>().add(i));
            let mat = app
                .world_mut()
                .resource_mut::<Assets<TerrainMaterial>>()
                .add(TerrainMaterial {
                    extension: crate::TerrainExtension {
                        surface,
                        weights_low: low.clone(),
                        weights_high: high.clone(),
                        gpu_paint: Some(paint),
                        ..default()
                    },
                    ..default()
                });
            handles.push((mat, low, high, palette, cpu, part));
        }
        for _ in 0..600 {
            app.update();
            let ready = app.sub_app(RenderApp).world().resource::<Prepared>();
            if ready.0.len() == handles.len() && ready.0.values().all(|r| r.done) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<Prepared>()
                .0
                .values()
                .filter(|r| r.done)
                .count(),
            handles.len(),
            "compute pipeline did not finish"
        );
        for (_, low, high, palette, cpu, part) in handles {
            let outputs = [pixels(&app, low.id()), pixels(&app, high.id())];
            // Build reference images in the GPU's stable material-id palette.
            let n = cpu.size();
            let reference = std::array::from_fn::<_, 2, _>(|half| {
                let bytes: Vec<u8> = (0..n.y)
                    .flat_map(|j| (0..n.x).map(move |i| (i, j)))
                    .flat_map(|(i, j)| {
                        (0..4)
                            .map(|c| {
                                palette
                                    .get(half * 4 + c)
                                    .map_or(0, |id| cpu.weight(i, j, *id))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let image = Image::new(
                    Extent3d {
                        width: n.x,
                        height: n.y,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    bytes,
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::MAIN_WORLD,
                );
                part.map_or_else(
                    || image.clone(),
                    |(p, c)| crate::material::weights_part(&image, p, c),
                )
                .data
                .unwrap()
            });
            for half in 0..2 {
                for (i, (&actual, &expected)) in
                    outputs[half].iter().zip(&reference[half]).enumerate()
                {
                    assert!(
                        actual.abs_diff(expected) <= 1,
                        "half {half} byte {i}: {actual} != {expected}"
                    );
                }
            }
            if part.is_none() {
                for i in 0..(n.x * n.y) as usize {
                    let sum: u32 = outputs
                        .iter()
                        .flat_map(|b| &b[i * 4..i * 4 + 4])
                        .map(|v| u32::from(*v))
                        .sum();
                    assert_eq!(sum, 255);
                }
            }
        }
    }

    #[test]
    fn noise_painters_stay_on_the_cpu() {
        use procgena_terrain::ground::{
            BARE_SEED_KIND, GroundNoise, MaterialCatalogue, bare_layers,
        };
        let mut layers = bare_layers();
        layers.values_mut().find(|l| l.order == 650).unwrap().noise = Some(GroundNoise::default());
        let stack = GroundStack::new(
            &layers,
            Arc::new(MaterialCatalogue::standard()),
            BARE_SEED_KIND,
            0,
            1.0,
        )
        .unwrap();
        let key = ChunkKey::new(0, 0, 0);
        let grid = Arc::new(GroundGrid::chunk(&Sites, key, FeatureTier::Fine).unwrap());
        assert!(GpuGroundPaint::prepare(&stack, grid, key).is_none());
    }
}
