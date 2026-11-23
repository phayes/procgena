//! The GPU side: wgpu setup, the kernels of `place.wgsl` as dispatch lists,
//! per-pass timestamps, and readback for the parity tests.

use std::time::Instant;

use crate::cpu::{ChunkOut, LatticeOut};
use crate::int::{Cand, FP, Grid, Rect};
use crate::stack::{GRASS, Inputs, Kind, Spec, Windows};

/// Bytes between two parameter blocks (the dynamic-offset alignment).
const PARAM_STRIDE: u64 = 256;
/// Parameter blocks in the uniform buffer, so dispatches per submit.
const MAX_DISPATCH: usize = 1024;
/// Cells (u32 slots) per window buffer.
const MAX_CELLS: u64 = 1 << 20;
/// Grass output: `2 × n_keep` u32 per tile cell, for up to a 128 m chunk.
const MAX_GRASS: u64 = 64 * 64 * 800 * 2;
/// The World kept grids cover [-WK_PAD, 1024 + WK_PAD) m.
pub const WK_PAD: i32 = 256;

/// Blocks until a future resolves. wgpu's native futures resolve as soon as
/// they are polled, so no executor is needed.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    Product,
    Candidates,
    KdRound,
    /// `KdRound` with each block's neighbourhood in workgroup memory.
    KdTiled,
    Commit,
    Stamp,
    Grass,
}

/// One dispatch: a kernel, its parameter block, its workgroup counts, and
/// which Scatter it belongs to (for the timing breakdown).
#[derive(Clone)]
pub struct Dispatch {
    pub kernel: Kernel,
    pub params: [u32; 64],
    pub groups: (u32, u32),
    pub scatter: usize,
}

/// A copy to read back after a dispatch list.
pub enum Readback {
    Cand {
        k: usize,
        after: usize,
        rect: Rect,
    },
    Kept {
        k: usize,
        after: usize,
        rect: Rect,
        half: u32,
    },
    Avail {
        k: usize,
        after: usize,
        rect: Rect,
    },
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    pub backend: String,
    pub timestamps: bool,
    /// Whether `K_d` rounds use the workgroup-tiled kernel.
    pub kd_tiled: std::cell::Cell<bool>,
    period_ns: f64,
    pipes: Vec<(Kernel, wgpu::ComputePipeline)>,
    bind: wgpu::BindGroup,
    params: wgpu::Buffer,
    pub avail: wgpu::Buffer,
    pub fm: [wgpu::Buffer; 3],
    pub cand: wgpu::Buffer,
    pub kept: wgpu::Buffer,
    pub wk: [wgpu::Buffer; 3],
    pub gout: wgpu::Buffer,
    query: Option<wgpu::QuerySet>,
    resolve: Option<wgpu::Buffer>,
    wk_rects: [Rect; 3],
    grid: Rect,
    gs: u32,
}

/// Per-dispatch GPU times in nanoseconds, plus the wall time of the submit.
pub struct Timing {
    pub pass_ns: Vec<f64>,
    pub span_ns: f64,
    pub wall_ns: f64,
}

fn storage(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn u32_bytes(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn wk_rect(spec: &Spec) -> Rect {
    let lo = (-WK_PAD * FP) >> spec.cs;
    let hi = ((1024 + WK_PAD) * FP) >> spec.cs;
    Rect::new(lo, lo, hi, hi)
}

fn put_rect(p: &mut [u32; 64], at: usize, r: Rect) {
    p[at] = r.x0 as u32;
    p[at + 1] = r.y0 as u32;
    p[at + 2] = r.w().max(0) as u32;
    p[at + 3] = r.h().max(0) as u32;
}

// Offsets of the fields of `P` in `place.wgsl`, in u32s.
const A: usize = 0;
const W: usize = 4;
const O: usize = 8;
const AV: usize = 12;
const FM: usize = 16;
const B: usize = 28;
const C: usize = 32;
const E: usize = 36;
const G: usize = 40;
const WK: usize = 44;
const H: usize = 56;

fn div_up(a: i32, b: u32) -> u32 {
    (a.max(0) as u32).div_ceil(b)
}

impl Gpu {
    pub fn new(specs: &[Spec; 4], inp: &Inputs) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("no GPU adapter");
        let info = adapter.get_info();
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut features = wgpu::Features::empty();
        if timestamps {
            features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("d2 spike"),
            required_features: features,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("device");
        let period_ns = queue.get_timestamp_period() as f64;

        // Pipelines over one shared layout.
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("place"),
            source: wgpu::ShaderSource::Wgsl(include_str!("place.wgsl").into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: None,
            },
            count: None,
        }];
        for b in 1..=13 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: b,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: b <= 3 },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let kernels = [
            (Kernel::Product, "product"),
            (Kernel::Candidates, "candidates"),
            (Kernel::KdRound, "kd_round"),
            (Kernel::KdTiled, "kd_tiled"),
            (Kernel::Commit, "commit"),
            (Kernel::Stamp, "stamp"),
            (Kernel::Grass, "grass"),
        ];
        let pipes = kernels
            .iter()
            .map(|&(k, name)| {
                let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(name),
                    // kd_tiled writes all of its workgroup memory before reading it.
                    compilation_options: wgpu::PipelineCompilationOptions {
                        zero_initialize_workgroup_memory: false,
                        ..Default::default()
                    },
                    cache: None,
                });
                (k, p)
            })
            .collect();

        // Buffers: the inputs are uploaded once; everything else is scratch.
        let to_u32 = |g: &Grid<u8>| g.data.iter().map(|&v| v as u32).collect::<Vec<_>>();
        let cov = storage(&device, "cov", inp.cov.data.len() as u64 * 4);
        queue.write_buffer(&cov, 0, &u32_bytes(&to_u32(&inp.cov)));
        let slope = storage(&device, "slope", inp.slope.data.len() as u64 * 4);
        queue.write_buffer(&slope, 0, &u32_bytes(&to_u32(&inp.slope)));
        let tile = storage(&device, "tile", inp.tile.len() as u64 * 4);
        queue.write_buffer(&tile, 0, &u32_bytes(&inp.tile));
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: PARAM_STRIDE * MAX_DISPATCH as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let avail = storage(&device, "avail", MAX_CELLS * 4);
        let fm = [0, 1, 2].map(|i| storage(&device, &format!("fm{i}"), MAX_CELLS * 4));
        let cand = storage(&device, "cand", MAX_CELLS * 16);
        let kept = storage(&device, "kept", MAX_CELLS * 8);
        let wk_rects = [wk_rect(&specs[0]), wk_rect(&specs[1]), wk_rect(&specs[2])];
        let wk =
            [0, 1, 2].map(|i| storage(&device, &format!("wk{i}"), wk_rects[i].len() as u64 * 4));
        let gout = storage(&device, "gout", MAX_GRASS * 4);
        let buffers: [&wgpu::Buffer; 13] = [
            &cov, &slope, &tile, &avail, &fm[0], &fm[1], &fm[2], &cand, &kept, &wk[0], &wk[1],
            &wk[2], &gout,
        ];
        let mut bg = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &params,
                offset: 0,
                size: std::num::NonZeroU64::new(PARAM_STRIDE),
            }),
        }];
        for (i, b) in buffers.iter().enumerate() {
            bg.push(wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: b.as_entire_binding(),
            });
        }
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &bg,
        });

        let (query, resolve) = if timestamps {
            let q = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("ts"),
                ty: wgpu::QueryType::Timestamp,
                count: 2 * MAX_DISPATCH as u32,
            });
            let r = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ts resolve"),
                size: 16 * MAX_DISPATCH as u64,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            (Some(q), Some(r))
        } else {
            (None, None)
        };
        Self {
            device,
            queue,
            adapter_name: info.name.clone(),
            backend: format!("{:?} ({:?})", info.backend, info.device_type),
            timestamps,
            kd_tiled: std::cell::Cell::new(true),
            period_ns,
            pipes,
            bind,
            params,
            avail,
            fm,
            cand,
            kept,
            wk,
            gout,
            query,
            resolve,
            wk_rects,
            grid: inp.cov.rect,
            gs: inp.gs,
        }
    }

    /// A parameter block with the fields every kernel shares.
    fn base(&self, spec: &Spec, slot: usize, seed: u32, tfs: u32) -> [u32; 64] {
        let mut p = [0u32; 64];
        p[A] = seed;
        p[A + 1] = slot as u32;
        p[A + 2] = spec.cs;
        p[A + 3] = spec.r() as u32;
        p[B] = tfs;
        p[B + 1] = self.gs;
        p[C + 2] = spec.max_slope;
        match spec.kind {
            Kind::Lattice {
                p24,
                share24,
                spacing,
                footprint,
                ..
            } => {
                p[B + 2] = p24;
                p[B + 3] = share24;
                p[C] = spacing[0];
                p[C + 1] = spacing[1];
                p[E + 1] = footprint;
            }
            Kind::Tile { n_keep } => p[C + 3] = n_keep,
        }
        p[E + 3] = spec.shade as u32;
        put_rect(&mut p, G, self.grid);
        for j in 0..3 {
            put_rect(&mut p, WK + 4 * j, self.wk_rects[j]);
        }
        p[H] = MAX_CELLS as u32;
        p[H + 1] = spec.id;
        p
    }

    /// Dispatches that stamp Scatter `j`'s footprint mask over `rect`.
    pub fn stamp(&self, specs: &[Spec; 4], seed: u32, tfs: u32, j: usize, rect: Rect) -> Dispatch {
        let mut p = self.base(&specs[j], j, seed, tfs);
        put_rect(&mut p, FM + 4 * j, rect);
        Dispatch {
            kernel: Kernel::Stamp,
            params: p,
            groups: (div_up(rect.w(), 8), div_up(rect.h(), 8)),
            scatter: j,
        }
    }

    /// Dispatches that place Scatter `k` over its windows in `w`: the mask
    /// product, then candidates, `K_d` and the commit (or grass). Footprint
    /// masks above `k` must already cover `w.avail[k]`, at the rects in
    /// `mask_rects`.
    pub fn place(
        &self,
        specs: &[Spec; 4],
        seed: u32,
        w: &Windows,
        k: usize,
        mask_rects: &[Rect],
    ) -> Vec<Dispatch> {
        let s = &specs[k];
        let mut out = Vec::new();
        let mut p = self.base(s, k, seed, w.tfs);
        put_rect(&mut p, W, w.win[k]);
        put_rect(&mut p, O, w.own[k]);
        put_rect(&mut p, AV, w.avail[k]);
        for j in 0..3 {
            put_rect(&mut p, FM + 4 * j, mask_rects[j]);
        }
        p[E + 2] = k.min(3) as u32;
        let av = w.avail[k];
        out.push(Dispatch {
            kernel: Kernel::Product,
            params: p,
            groups: (div_up(av.w(), 8), div_up(av.h(), 8)),
            scatter: k,
        });
        if k == GRASS {
            let Kind::Tile { n_keep } = s.kind else {
                unreachable!()
            };
            let cells = w.own[k].len() as u32;
            out.push(Dispatch {
                kernel: Kernel::Grass,
                params: p,
                groups: (n_keep.div_ceil(64), cells),
                scatter: k,
            });
            return out;
        }
        let win = w.win[k];
        let g = (div_up(win.w(), 8), div_up(win.h(), 8));
        out.push(Dispatch {
            kernel: Kernel::Candidates,
            params: p,
            groups: g,
            scatter: k,
        });
        for round in 1..=s.d() {
            let mut q = p;
            q[E] = round;
            if self.kd_tiled.get() {
                let gt = (div_up(win.w(), 16), div_up(win.h(), 16));
                out.push(Dispatch {
                    kernel: Kernel::KdTiled,
                    params: q,
                    groups: gt,
                    scatter: k,
                });
            } else {
                out.push(Dispatch {
                    kernel: Kernel::KdRound,
                    params: q,
                    groups: g,
                    scatter: k,
                });
            }
        }
        let mut q = p;
        q[E] = s.d();
        let own = w.own[k];
        out.push(Dispatch {
            kernel: Kernel::Commit,
            params: q,
            groups: (div_up(own.w(), 8), div_up(own.h(), 8)),
            scatter: k,
        });
        out
    }

    /// Window A for one chunk: the whole stack, each Scatter stamping its
    /// mask over the union of its readers' rects.
    pub fn window_a(
        &self,
        specs: &[Spec; 4],
        seed: u32,
        w: &Windows,
    ) -> (Vec<Dispatch>, Vec<Readback>) {
        let mut ds = Vec::new();
        let mut rb = Vec::new();
        for k in 0..=w.lowest {
            ds.extend(self.place(specs, seed, w, k, &w.mask));
            if k != GRASS {
                rb.push(Readback::Cand {
                    k,
                    after: ds.len(),
                    rect: w.win[k],
                });
                rb.push(Readback::Kept {
                    k,
                    after: ds.len(),
                    rect: w.win[k],
                    half: specs[k].d() & 1,
                });
            }
            rb.push(Readback::Avail {
                k,
                after: ds.len(),
                rect: w.avail[k],
            });
            if k != GRASS && w.mask[k].len() > 0 {
                ds.push(self.stamp(specs, seed, w.tfs, k, w.mask[k]));
            }
        }
        (ds, rb)
    }

    /// Runs a dispatch list in one submit, with a timestamp pair per
    /// dispatch when supported. `readback` copies are taken after the
    /// dispatch they name; `read` returns them in order, plus the masks and
    /// grass output when `masks` is set.
    pub fn run(
        &self,
        ds: &[Dispatch],
        readback: &[Readback],
        read: Option<(&[Rect], usize)>,
    ) -> (Timing, Vec<Vec<u32>>) {
        assert!(ds.len() <= MAX_DISPATCH);
        let mut bytes = Vec::with_capacity(ds.len() * PARAM_STRIDE as usize);
        for d in ds {
            bytes.extend(u32_bytes(&d.params));
        }
        self.queue.write_buffer(&self.params, 0, &bytes);
        let mut enc = self.device.create_command_encoder(&Default::default());
        let mut staging: Vec<(wgpu::Buffer, u64)> = Vec::new();
        let mut copy =
            |enc: &mut wgpu::CommandEncoder, src: &wgpu::Buffer, offset: u64, size: u64| {
                let size = size.max(4);
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                enc.copy_buffer_to_buffer(src, offset, &b, 0, size);
                staging.push((b, size));
            };
        let mut next_rb = 0;
        for (i, d) in ds.iter().enumerate() {
            {
                let tw = self
                    .query
                    .as_ref()
                    .map(|q| wgpu::ComputePassTimestampWrites {
                        query_set: q,
                        beginning_of_pass_write_index: Some(2 * i as u32),
                        end_of_pass_write_index: Some(2 * i as u32 + 1),
                    });
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: tw,
                });
                let pipe = &self.pipes.iter().find(|(k, _)| *k == d.kernel).unwrap().1;
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &self.bind, &[(i as u64 * PARAM_STRIDE) as u32]);
                if d.groups.0 > 0 && d.groups.1 > 0 {
                    pass.dispatch_workgroups(d.groups.0, d.groups.1, 1);
                }
            }
            while next_rb < readback.len() {
                let after = match &readback[next_rb] {
                    Readback::Cand { after, .. }
                    | Readback::Kept { after, .. }
                    | Readback::Avail { after, .. } => *after,
                };
                if after != i + 1 {
                    break;
                }
                match &readback[next_rb] {
                    Readback::Cand { rect, .. } => {
                        copy(&mut enc, &self.cand, 0, rect.len() as u64 * 16)
                    }
                    Readback::Kept { rect, half, .. } => copy(
                        &mut enc,
                        &self.kept,
                        *half as u64 * MAX_CELLS * 4,
                        rect.len() as u64 * 4,
                    ),
                    Readback::Avail { rect, .. } => {
                        copy(&mut enc, &self.avail, 0, rect.len() as u64 * 4)
                    }
                }
                next_rb += 1;
            }
        }
        if let Some((masks, grass)) = read {
            for j in 0..3 {
                copy(&mut enc, &self.fm[j], 0, masks[j].len() as u64 * 4);
            }
            copy(&mut enc, &self.gout, 0, grass as u64 * 4);
        }
        let ts_buf = if let (Some(q), Some(r)) = (&self.query, &self.resolve) {
            let n = 2 * ds.len() as u32;
            enc.resolve_query_set(q, 0..n, r, 0);
            let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: n as u64 * 8,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            enc.copy_buffer_to_buffer(r, 0, &b, 0, n as u64 * 8);
            Some(b)
        } else {
            None
        };
        let t0 = Instant::now();
        self.queue.submit([enc.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let wall_ns = t0.elapsed().as_nanos() as f64;

        let read_u32 = |b: &wgpu::Buffer| -> Vec<u32> {
            b.slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            let v = b.slice(..).get_mapped_range().expect("mapped");
            let out = v
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            drop(v);
            b.unmap();
            out
        };
        let mut timing = Timing {
            pass_ns: vec![0.0; ds.len()],
            span_ns: 0.0,
            wall_ns,
        };
        if let Some(b) = ts_buf {
            let raw = read_u32(&b);
            let t: Vec<u64> = raw
                .chunks_exact(2)
                .map(|c| c[0] as u64 | (c[1] as u64) << 32)
                .collect();
            for i in 0..ds.len() {
                timing.pass_ns[i] = t[2 * i + 1].saturating_sub(t[2 * i]) as f64 * self.period_ns;
            }
            timing.span_ns = t[2 * ds.len() - 1].saturating_sub(t[0]) as f64 * self.period_ns;
        }
        let data = staging.iter().map(|(b, _)| read_u32(b)).collect();
        (timing, data)
    }

    /// Clears the World kept grids (before a B build from scratch).
    pub fn clear_world(&self) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        for b in &self.wk {
            enc.clear_buffer(b, 0, None);
        }
        self.queue.submit([enc.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
    }

    /// Builds one chunk over window A and reads everything back in the CPU
    /// twin's shape.
    pub fn build_a(&self, specs: &[Spec; 4], seed: u32, w: &Windows) -> (ChunkOut, Timing) {
        let (ds, rb) = self.window_a(specs, seed, w);
        let grass_len = if w.lowest == GRASS {
            let Kind::Tile { n_keep } = specs[GRASS].kind else {
                unreachable!()
            };
            w.own[GRASS].len() * n_keep as usize * 2
        } else {
            0
        };
        let (timing, data) = self.run(&ds, &rb, Some((&w.mask, grass_len)));
        let mut out = ChunkOut {
            avail: Default::default(),
            lattice: Default::default(),
            masks: Default::default(),
            grass: None,
        };
        let mut cand: [Option<Grid<Cand>>; 3] = Default::default();
        let mut kept: [Option<Grid<u8>>; 3] = Default::default();
        for (r, d) in rb.iter().zip(&data) {
            match r {
                Readback::Cand { k, rect, .. } => {
                    let v = d.chunks_exact(4).take(rect.len());
                    let v = v
                        .map(|c| Cand {
                            x: c[0] as i32,
                            y: c[1] as i32,
                            key: c[2],
                            meta: c[3],
                        })
                        .collect();
                    cand[*k] = Some(Grid {
                        rect: *rect,
                        data: v,
                    });
                }
                Readback::Kept { k, rect, .. } => {
                    kept[*k] = Some(Grid {
                        rect: *rect,
                        data: d.iter().take(rect.len()).map(|&x| x as u8).collect(),
                    });
                }
                Readback::Avail { k, rect, .. } => {
                    out.avail[*k] = Some(Grid {
                        rect: *rect,
                        data: d.iter().take(rect.len()).map(|&x| x as u8).collect(),
                    });
                }
            }
        }
        for k in 0..3.min(w.lowest + 1) {
            let (c, kp) = (cand[k].take().unwrap(), kept[k].take().unwrap());
            let mut own = Grid::new(w.own[k], 0u8);
            for j in w.own[k].y0..w.own[k].y1 {
                for i in w.own[k].x0..w.own[k].x1 {
                    own.set(i, j, kp.get(i, j));
                }
            }
            out.lattice[k] = Some(LatticeOut {
                cand: c,
                kept: kp,
                own,
            });
        }
        let base = rb.len();
        for j in 0..3 {
            if w.mask[j].len() > 0 && j < w.lowest {
                let d = &data[base + j];
                out.masks[j] = Some(Grid {
                    rect: w.mask[j],
                    data: d.iter().take(w.mask[j].len()).map(|&x| x as u8).collect(),
                });
            }
        }
        if w.lowest == GRASS {
            let d = &data[base + 3];
            out.grass = Some(
                d.chunks_exact(2)
                    .take(grass_len / 2)
                    .map(|c| (c[0] as i32, c[1] as i32))
                    .collect(),
            );
        }
        (out, timing)
    }
}
