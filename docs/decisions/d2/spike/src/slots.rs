//! Slot choice for D3: the 32-bit jump hash and rendezvous hashing, on the
//! CPU and the GPU.

use crate::int::hash;

/// The subset used for "4 slots out of 64".
pub const SUBSET: [u32; 4] = [3, 17, 40, 58];

/// Jump consistent hash with 32-bit steps and an exact integer jump.
pub fn jump32(key: u32, n: u32) -> u32 {
    let mut b: i64 = -1;
    let mut j: u32 = 0;
    let mut h = key;
    while j < n {
        b = j as i64;
        h = hash(h ^ 0x9E37_79B9);
        j = ((b as u32 + 1) << 16) / ((h >> 16) + 1);
    }
    b as u32
}

/// Rendezvous hashing over slots `0..n`: highest score wins.
pub fn rendezvous(key: u32, n: u32) -> u32 {
    let mut best = 0u32;
    let mut slot = 0u32;
    for k in 0..n {
        let s = hash(key ^ hash(k));
        if k == 0 || s > best {
            best = s;
            slot = k;
        }
    }
    slot
}

pub fn rendezvous_subset(key: u32) -> u32 {
    let mut best = 0u32;
    let mut slot = SUBSET[0];
    for (i, &k) in SUBSET.iter().enumerate() {
        let s = hash(key ^ hash(k));
        if i == 0 || s > best {
            best = s;
            slot = k;
        }
    }
    slot
}

pub fn jump_subset(key: u32) -> u32 {
    SUBSET[jump32(key, SUBSET.len() as u32) as usize]
}

pub fn key(i: u32) -> u32 {
    hash(i.wrapping_mul(2_654_435_761))
}

/// Modes of the slot kernel: 0 baseline (the key only), 1 jump, 2
/// rendezvous, 3 jump into the subset, 4 rendezvous over the subset.
pub fn cpu(mode: u32, n: u32, i: u32) -> u32 {
    let k = key(i);
    match mode {
        0 => k,
        1 => jump32(k, n),
        2 => rendezvous(k, n),
        3 => jump_subset(k),
        _ => rendezvous_subset(k),
    }
}

const SHADER: &str = r#"
struct S { mode: u32, n: u32, count: u32, pad: u32 }
@group(0) @binding(0) var<uniform> s: S;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
const SUBSET = array<u32, 4>(3u, 17u, 40u, 58u);

fn hash(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn jump32(key: u32, n: u32) -> u32 {
    var b = 0u;
    var j = 0u;
    var h = key;
    loop {
        if j >= n { break; }
        b = j;
        h = hash(h ^ 0x9E3779B9u);
        j = ((b + 1u) << 16u) / ((h >> 16u) + 1u);
    }
    return b;
}

fn rendezvous(key: u32, n: u32) -> u32 {
    var best = 0u;
    var slot = 0u;
    for (var k = 0u; k < n; k++) {
        let sc = hash(key ^ hash(k));
        if k == 0u || sc > best { best = sc; slot = k; }
    }
    return slot;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= s.count { return; }
    let key = hash(id.x * 2654435761u);
    var v = key;
    switch s.mode {
        case 1u: { v = jump32(key, s.n); }
        case 2u: { v = rendezvous(key, s.n); }
        case 3u: { v = SUBSET[jump32(key, 4u)]; }
        case 4u: {
            var best = 0u;
            v = SUBSET[0];
            for (var i = 0u; i < 4u; i++) {
                let sc = hash(key ^ hash(SUBSET[i]));
                if i == 0u || sc > best { best = sc; v = SUBSET[i]; }
            }
        }
        default: {}
    }
    out[id.x] = v;
}
"#;

pub struct Result {
    pub label: String,
    /// Median GPU time of the kernel over `count` keys, minus the baseline.
    pub per_copy_ns: f64,
    pub kernel_us: f64,
    pub mismatches: usize,
}

/// Times each slot mode on `count` keys and checks parity with the CPU.
pub fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    period_ns: f64,
    count: u32,
    reps: usize,
) -> Vec<Result> {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("slots"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("slots"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let uni = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: count as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipe.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uni.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: out.as_entire_binding(),
            },
        ],
    });
    let qs = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = |src: &wgpu::Buffer, size: u64| -> Vec<u32> {
        let b = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(src, 0, &b, 0, size);
        queue.submit([enc.finish()]);
        b.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let v = b.slice(..).get_mapped_range().expect("mapped");
        let o = v
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        drop(v);
        o
    };
    let time = |mode: u32, n: u32| -> f64 {
        let bytes: Vec<u8> = [mode, n, count, 0]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        queue.write_buffer(&uni, 0, &bytes);
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &qs,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            });
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
        }
        enc.resolve_query_set(&qs, 0..2, &resolve, 0);
        queue.submit([enc.finish()]);
        let t = read(&resolve, 16);
        let t0 = t[0] as u64 | (t[1] as u64) << 32;
        let t1 = t[2] as u64 | (t[3] as u64) << 32;
        t1.saturating_sub(t0) as f64 * period_ns
    };
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let modes: Vec<(String, u32, u32)> = vec![
        ("baseline (key only)".into(), 0, 0),
        ("jump32 N=4".into(), 1, 4),
        ("jump32 N=16".into(), 1, 16),
        ("jump32 N=64".into(), 1, 64),
        ("rendezvous N=4".into(), 2, 4),
        ("rendezvous N=16".into(), 2, 16),
        ("rendezvous N=64".into(), 2, 64),
        ("jump32 into 4 of 64".into(), 3, 0),
        ("rendezvous over 4 of 64".into(), 4, 0),
    ];
    for _ in 0..5 {
        time(2, 64);
    }
    let mut base = 0.0;
    let mut results = Vec::new();
    for (label, mode, n) in modes {
        let t = median((0..reps).map(|_| time(mode, n)).collect());
        let gpu = read(&out, count as u64 * 4);
        let mismatches = (0..count)
            .filter(|&i| cpu(mode, n, i) != gpu[i as usize])
            .count();
        if mode == 0 {
            base = t;
        }
        results.push(Result {
            label,
            per_copy_ns: (t - base) / count as f64,
            kernel_us: t / 1000.0,
            mismatches,
        });
    }
    results
}
