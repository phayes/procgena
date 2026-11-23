//! Collects small parameter blocks into one uniform buffer, each at an
//! offset a dynamic uniform binding can use.

use wgpu::util::{BufferInitDescriptor, DeviceExt};

/// Parameter blocks being collected, each starting at a multiple of the
/// device's uniform offset alignment.
#[derive(Debug, Default)]
pub(crate) struct ParamSlots {
    bytes: Vec<u8>,
    align: usize,
}

impl ParamSlots {
    /// Starts collecting for a device with `align`, its
    /// `min_uniform_buffer_offset_alignment`.
    pub(crate) fn new(align: u32) -> Self {
        Self {
            bytes: Vec::new(),
            align: align as usize,
        }
    }

    /// Adds `block` and returns its byte offset.
    pub(crate) fn push(&mut self, block: &[u8]) -> u32 {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(block);
        self.bytes
            .resize(self.bytes.len().next_multiple_of(self.align), 0);
        offset as u32
    }

    /// Creates the uniform buffer holding every block.
    pub(crate) fn finish(self, device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer_init(&BufferInitDescriptor {
            label: Some("procgena_clump params"),
            contents: &self.bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        })
    }
}

/// Creates a bind group of a dynamic-offset uniform window of `size` bytes
/// into `params` (binding 0) and the arena (binding 1), for the scan and
/// sort layouts.
pub(crate) fn params_and_arena(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
    size: u64,
    arena: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("procgena_clump params and arena"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params,
                    offset: 0,
                    size: std::num::NonZeroU64::new(size),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: arena.as_entire_binding(),
            },
        ],
    })
}
