//! Exclusive prefix sums over regions of a `u32` arena, recorded into a
//! compute pass.

use std::num::NonZeroU64;

use crate::shaders::{COMPILED_WGSL, SCAN_BLOCK};

/// Byte size of one `ScanParams` in `scan.wesl`.
pub(crate) const SCAN_PARAMS_SIZE: u64 = 16;

/// One level of a planned scan: how many entries, and where they and their
/// block totals are, as word offsets into the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScanLevel {
    pub(crate) count: u32,
    pub(crate) data_offset: u32,
    pub(crate) sums_offset: u32,
}

impl ScanLevel {
    /// Returns this level packed as `ScanParams`.
    pub(crate) fn params(&self) -> [u8; SCAN_PARAMS_SIZE as usize] {
        let mut bytes = [0; SCAN_PARAMS_SIZE as usize];
        for (k, w) in [self.count, self.data_offset, self.sums_offset]
            .iter()
            .enumerate()
        {
            bytes[4 * k..4 * k + 4].copy_from_slice(&w.to_le_bytes());
        }
        bytes
    }
}

/// Returns the words of block totals a scan of up to `capacity` entries
/// needs, over all its levels.
pub(crate) fn sums_words(capacity: u32) -> u32 {
    let mut total = 0;
    let mut count = capacity;
    while count > 1 {
        count = count.div_ceil(SCAN_BLOCK);
        total += count;
    }
    total
}

/// Plans a scan of `capacity` entries at word `data_offset`, with block
/// totals in a region of [`sums_words`]`(capacity)` words at `sums_offset`.
/// Returns its levels, data first; the last level fits in one block.
pub(crate) fn plan_scan(capacity: u32, data_offset: u32, sums_offset: u32) -> Vec<ScanLevel> {
    let mut levels = Vec::new();
    let (mut count, mut data, mut sums) = (capacity, data_offset, sums_offset);
    loop {
        let blocks = count.div_ceil(SCAN_BLOCK);
        levels.push(ScanLevel {
            count,
            data_offset: data,
            sums_offset: sums,
        });
        if blocks <= 1 {
            return levels;
        }
        (count, data, sums) = (blocks, sums, sums + blocks);
    }
}

/// The scan pipelines and their bind group layout.
pub(crate) struct Scanner {
    pub(crate) layout: wgpu::BindGroupLayout,
    scan_blocks: wgpu::ComputePipeline,
    add_block_offsets: wgpu::ComputePipeline,
}

impl Scanner {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let storage = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump scan"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(SCAN_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: storage,
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("procgena_clump scan"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let source = COMPILED_WGSL
            .iter()
            .find(|(name, _)| *name == "scan")
            .map(|(_, source)| *source)
            .expect("scan module is compiled");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("procgena_clump scan"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            scan_blocks: pipeline("scan_blocks"),
            add_block_offsets: pipeline("add_block_offsets"),
            layout,
        }
    }

    /// Records a scan whose levels' params sit in `group`'s uniform at the
    /// byte offsets `param_offsets` (one per level, from [`plan_scan`]).
    /// `max_groups` is the device's workgroup limit per dimension.
    pub(crate) fn record(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        group: &wgpu::BindGroup,
        levels: &[ScanLevel],
        param_offsets: &[u32],
        max_groups: u32,
    ) {
        // Block sums up the levels, then block offsets back down.
        pass.set_pipeline(&self.scan_blocks);
        for (level, &offset) in levels.iter().zip(param_offsets) {
            pass.set_bind_group(0, group, &[offset]);
            dispatch_groups(pass, level.count.div_ceil(SCAN_BLOCK), max_groups);
        }
        pass.set_pipeline(&self.add_block_offsets);
        for (level, &offset) in levels.iter().zip(param_offsets).rev().skip(1) {
            pass.set_bind_group(0, group, &[offset]);
            dispatch_groups(pass, level.count.div_ceil(SCAN_BLOCK), max_groups);
        }
    }
}

/// Dispatches at least `groups` workgroups as rows along x, adding rows along
/// y when one row is not enough. Shaders skip workgroups past the end.
pub(crate) fn dispatch_groups(pass: &mut wgpu::ComputePass<'_>, groups: u32, max_groups: u32) {
    let x = groups.clamp(1, max_groups);
    pass.dispatch_workgroups(x, groups.div_ceil(x), 1);
}
