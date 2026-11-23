//! Stable radix sort of element indices by multi-word keys, over regions of
//! a `u32` arena, recorded into a compute pass.

use std::num::NonZeroU64;

use super::scan::{ScanLevel, Scanner, dispatch_groups, plan_scan, sums_words};
use crate::shaders::{COMPILED_WGSL, SORT_BLOCK, SORT_DIGITS};

/// Byte size of one `SortParams` in `sort.wesl`.
pub(crate) const SORT_PARAMS_SIZE: u64 = 32;

/// One key word to sort by: where the words are (indexed by element), and
/// how many of their low bits matter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SortKey {
    pub(crate) offset: u32,
    pub(crate) bits: u32,
}

/// One pass of a planned sort, packed as `SortParams` by [`Self::params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortPass {
    pub(crate) count_offset: u32,
    pub(crate) capacity: u32,
    pub(crate) src_offset: u32,
    pub(crate) dst_offset: u32,
    pub(crate) key_offset: u32,
    pub(crate) shift: u32,
    pub(crate) hist_offset: u32,
    pub(crate) blocks: u32,
}

impl SortPass {
    /// Returns this pass packed as `SortParams`.
    pub(crate) fn params(&self) -> [u8; SORT_PARAMS_SIZE as usize] {
        let words = [
            self.count_offset,
            self.capacity,
            self.src_offset,
            self.dst_offset,
            self.key_offset,
            self.shift,
            self.hist_offset,
            self.blocks,
        ];
        let mut bytes = [0; SORT_PARAMS_SIZE as usize];
        for (k, w) in words.iter().enumerate() {
            bytes[4 * k..4 * k + 4].copy_from_slice(&w.to_le_bytes());
        }
        bytes
    }
}

/// A planned sort: its passes, the scan of digit counts every pass shares,
/// and which of the two lists holds the result.
#[derive(Debug, Clone)]
pub(crate) struct SortPlan {
    pub(crate) passes: Vec<SortPass>,
    pub(crate) scan: Vec<ScanLevel>,
    pub(crate) result_offset: u32,
}

/// Returns the scratch words a sort of up to `capacity` elements needs at
/// `hist_offset`: the digit counts and their scan totals.
pub(crate) fn hist_words(capacity: u32) -> u32 {
    let entries = SORT_DIGITS * capacity.div_ceil(SORT_BLOCK).max(1);
    entries + sums_words(entries)
}

/// Plans a stable sort of the element list at `list_a` (holding up to
/// `capacity` elements; the count is read from word `count_offset`, or
/// `u32::MAX` sorts the whole capacity) by `keys`, least significant word
/// first.
///
/// `list_b` is a second list region of the same size, used in turns with
/// `list_a`; the sorted list ends in [`SortPlan::result_offset`], one of the
/// two. `hist_offset` is a scratch region of [`hist_words`]`(capacity)` words.
pub(crate) fn plan_sort(
    capacity: u32,
    count_offset: u32,
    list_a: u32,
    list_b: u32,
    keys: &[SortKey],
    hist_offset: u32,
) -> SortPlan {
    let blocks = capacity.div_ceil(SORT_BLOCK).max(1);
    let entries = SORT_DIGITS * blocks;
    let scan = plan_scan(entries, hist_offset, hist_offset + entries);

    // One pass per 4-bit digit, least significant first, swapping lists.
    let mut passes = Vec::new();
    let (mut src, mut dst) = (list_a, list_b);
    for key in keys {
        for shift in (0..key.bits).step_by(4) {
            passes.push(SortPass {
                count_offset,
                capacity,
                src_offset: src,
                dst_offset: dst,
                key_offset: key.offset,
                shift,
                hist_offset,
                blocks,
            });
            (src, dst) = (dst, src);
        }
    }
    SortPlan {
        passes,
        scan,
        result_offset: src,
    }
}

/// The sort pipelines and their bind group layout.
pub(crate) struct Sorter {
    pub(crate) layout: wgpu::BindGroupLayout,
    count: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
}

impl Sorter {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("procgena_clump sort"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(SORT_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("procgena_clump sort"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let source = COMPILED_WGSL
            .iter()
            .find(|(name, _)| *name == "sort")
            .map(|(_, source)| *source)
            .expect("sort module is compiled");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("procgena_clump sort"),
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
            count: pipeline("sort_count"),
            scatter: pipeline("sort_scatter"),
            layout,
        }
    }

    /// Records the sort `plan`. Each pass's params sit in `sort_group`'s
    /// uniform at the byte offsets `pass_offsets`, and the digit-count scan's
    /// levels in `scan_group`'s at `scan_offsets`. `max_groups` is the
    /// device's workgroup limit per dimension.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        scanner: &Scanner,
        sort_group: &wgpu::BindGroup,
        scan_group: &wgpu::BindGroup,
        plan: &SortPlan,
        pass_offsets: &[u32],
        scan_offsets: &[u32],
        max_groups: u32,
    ) {
        for (step, &offset) in plan.passes.iter().zip(pass_offsets) {
            pass.set_pipeline(&self.count);
            pass.set_bind_group(0, sort_group, &[offset]);
            dispatch_groups(pass, step.blocks, max_groups);
            scanner.record(pass, scan_group, &plan.scan, scan_offsets, max_groups);
            pass.set_pipeline(&self.scatter);
            pass.set_bind_group(0, sort_group, &[offset]);
            dispatch_groups(pass, step.blocks, max_groups);
        }
    }
}
