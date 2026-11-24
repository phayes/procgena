//! Measuring the instanced renderer, for benchmarks: how long the GPU spends
//! culling, and how many copies the views draw.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use bevy::ecs::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, PollType,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};

use super::{DRAW_ARGS_SIZE, ScatterViews, Scatters};
use crate::instanced::material::{ATTACHED_SIZE, INSTANCE_SIZE};

/// Culling stages, each one compute pass, in the order they run.
const STAGES: u32 = 4;

/// How long the GPU spent on each culling stage in one frame, over every
/// camera, shadow view and Scatter.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CullTimes {
    /// Choosing the copies each view draws and their levels.
    pub copies: Duration,
    /// Attached copies' first step, once per host copy.
    pub hosts: Duration,
    /// Attached copies' second step, once per row of each host copy.
    pub rows: Duration,
    /// Writing each part's instance count into its draw.
    pub args: Duration,
}

impl CullTimes {
    /// The four stages together.
    pub fn total(&self) -> Duration {
        self.copies + self.hosts + self.rows + self.args
    }
}

/// Times the instanced renderer's culling on the GPU, every frame.
///
/// Insert it into the render app's world once the renderer has started
/// (after `App::finish`), for example with
/// `app.sub_app_mut(RenderApp).world_mut().init_resource::<InstancedTimings>()`,
/// then read [`last`](Self::last) there. Times arrive a frame or two late.
///
/// Needs the GPU feature `TIMESTAMP_QUERY`; without it, nothing is timed and
/// `last` stays `None` ([`is_supported`](Self::is_supported) says which).
/// Timing costs a little GPU time, so leave it out of shipped apps.
#[derive(Resource)]
pub struct InstancedTimings {
    gpu: Option<TimerGpu>,
    last: Option<CullTimes>,
}

/// The query set and buffers of [`InstancedTimings`].
struct TimerGpu {
    queries: wgpu::QuerySet,
    resolve: Buffer,
    read: Buffer,
    /// Where `read` stands: one of the `IDLE`.. `READY` values.
    state: Arc<AtomicU8>,
    /// Nanoseconds per timestamp tick.
    period: f32,
}

/// `read` is free to take this frame's timestamps.
const IDLE: u8 = 0;
/// `read` takes this frame's timestamps once the frame is submitted.
const COPIED: u8 = 1;
/// `read` is being mapped.
const MAPPING: u8 = 2;
/// `read` is mapped and holds a frame's timestamps.
const READY: u8 = 3;

impl FromWorld for InstancedTimings {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return Self {
                gpu: None,
                last: None,
            };
        }
        let size = u64::from(STAGES) * 2 * 8;
        let buffer = |label: &'static str, usage: BufferUsages| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let gpu = TimerGpu {
            queries: device
                .wgpu_device()
                .create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("procgena_instanced_timings"),
                    ty: wgpu::QueryType::Timestamp,
                    count: STAGES * 2,
                }),
            resolve: buffer(
                "procgena_instanced_timings_resolve",
                BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            ),
            read: buffer(
                "procgena_instanced_timings_read",
                BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            ),
            state: Arc::new(AtomicU8::new(IDLE)),
            period: world.resource::<RenderQueue>().get_timestamp_period(),
        };
        Self {
            gpu: Some(gpu),
            last: None,
        }
    }
}

impl InstancedTimings {
    /// Whether the GPU can time the culling.
    pub fn is_supported(&self) -> bool {
        self.gpu.is_some()
    }

    /// The latest frame's times, once one has been read back.
    pub fn last(&self) -> Option<CullTimes> {
        self.last
    }

    /// Takes in the times read back since the last frame, if any.
    pub(super) fn begin_frame(&mut self) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        if gpu.state.load(Ordering::Acquire) != READY {
            return;
        }
        let ticks: Vec<u64> = {
            let Ok(range) = gpu.read.slice(..).get_mapped_range() else {
                gpu.state.store(IDLE, Ordering::Release);
                return;
            };
            range
                .as_chunks::<8>()
                .0
                .iter()
                .map(|&b| u64::from_le_bytes(b))
                .collect()
        };
        gpu.read.unmap();
        gpu.state.store(IDLE, Ordering::Release);
        let stage = |s: usize| {
            let elapsed = ticks[2 * s + 1].saturating_sub(ticks[2 * s]);
            Duration::from_nanos((elapsed as f64 * f64::from(gpu.period)) as u64)
        };
        self.last = Some(CullTimes {
            copies: stage(0),
            hosts: stage(1),
            rows: stage(2),
            args: stage(3),
        });
    }

    /// Where the pass of culling stage `stage` writes its timestamps.
    pub(super) fn timestamp_writes(
        &self,
        stage: u32,
    ) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        let gpu = self.gpu.as_ref()?;
        Some(wgpu::ComputePassTimestampWrites {
            query_set: &gpu.queries,
            beginning_of_pass_write_index: Some(stage * 2),
            end_of_pass_write_index: Some(stage * 2 + 1),
        })
    }

    /// Resolves this frame's timestamps, and copies them for reading back
    /// unless an earlier frame's are still being read.
    pub(super) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        encoder.resolve_query_set(&gpu.queries, 0..STAGES * 2, &gpu.resolve, 0);
        if gpu.state.load(Ordering::Acquire) == IDLE {
            encoder.copy_buffer_to_buffer(&gpu.resolve, 0, &gpu.read, 0, gpu.resolve.size());
            gpu.state.store(COPIED, Ordering::Release);
        }
    }
}

/// Starts reading back the timestamps a submitted frame copied.
pub(super) fn map_timings(timings: Option<Res<InstancedTimings>>) {
    let Some(gpu) = timings.as_ref().and_then(|t| t.gpu.as_ref()) else {
        return;
    };
    if gpu.state.load(Ordering::Acquire) != COPIED {
        return;
    }
    gpu.state.store(MAPPING, Ordering::Release);
    let state = gpu.state.clone();
    gpu.read.slice(..).map_async(MapMode::Read, move |result| {
        state.store(if result.is_ok() { READY } else { IDLE }, Ordering::Release);
    });
}

/// How many copies the views drew in the latest frame, over every camera,
/// shadow view and Scatter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstanceCounts {
    /// Views that culled a Scatter: one per camera or shadow view and
    /// Scatter.
    pub views: usize,
    /// Copies drawn, counting each detail level a copy is drawn at.
    pub copies: u64,
    /// Attached copies drawn, counting each detail level.
    pub attached: u64,
}

impl InstanceCounts {
    /// The bytes the culling wrote for the drawn copies: the per-copy input
    /// each draw reads.
    pub fn bytes(&self) -> u64 {
        self.copies * INSTANCE_SIZE + self.attached * ATTACHED_SIZE
    }
}

/// Reads back how many copies every view drew in the latest rendered frame.
///
/// Pass the render app's world. This waits for the GPU to finish, so it is
/// for tests and benchmarks only.
///
/// # Panics
///
/// Panics if the GPU can't be waited on or the counts can't be read, as
/// when the device is lost.
pub fn read_instance_counts(render_world: &World) -> InstanceCounts {
    let device = render_world.resource::<RenderDevice>();
    let queue = render_world.resource::<RenderQueue>();
    let scatters = render_world.resource::<Scatters>();
    let mut counts = InstanceCounts::default();
    for ((_, entity), view) in &render_world.resource::<ScatterViews>().0 {
        let (Some(gpu), Some(buffers), Some(_)) = (
            scatters.0.get(entity),
            view.buffers.as_ref(),
            view.cull_group.as_ref(),
        ) else {
            continue;
        };
        let size = gpu.parts.len() as u64 * DRAW_ARGS_SIZE;
        if size == 0 {
            continue;
        }
        let words = read_words(device, queue, &buffers.args, size);
        counts.views += 1;
        for (index, part) in gpu.parts.iter().enumerate() {
            let instances = u64::from(words[index * 5 + 1]);
            if part.attached {
                counts.attached += instances;
            } else {
                counts.copies += instances;
            }
        }
    }
    counts
}

/// The first `size` bytes of `source` as `u32` words, waiting for the GPU.
fn read_words(device: &RenderDevice, queue: &RenderQueue, source: &Buffer, size: u64) -> Vec<u32> {
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("procgena_instanced_read_counts"),
        size,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    staging
        .slice(..)
        .map_async(MapMode::Read, |result| result.expect("map the counts"));
    device
        .poll(PollType::wait_indefinitely())
        .expect("wait for the GPU");
    let words = staging
        .slice(..)
        .get_mapped_range()
        .expect("read the counts")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    staging.unmap();
    words
}
