//! Atlases as KTX2 files of BC7-compressed texels with their mip levels,
//! which GPUs sample directly at a quarter of the memory of plain RGBA.

use intel_tex_2::{RgbaSurface, bc7};

/// The 12 bytes every KTX2 file starts with.
const MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// Vulkan's numbers for BC7 texels, linear and sRGB.
const VK_FORMAT_BC7_UNORM_BLOCK: u32 = 145;
const VK_FORMAT_BC7_SRGB_BLOCK: u32 = 146;

/// Bytes in a BC7 block, which holds 4 × 4 texels.
const BLOCK_BYTES: usize = 16;

/// Compresses `levels`, square RGBA images with 8 bits per channel from
/// the full size (`size` pixels along a side) down, halving each time,
/// into the bytes of a KTX2 file. `srgb` marks the colour as sRGB.
///
/// Returns `None` if a level's side isn't a multiple of 4, which BC7
/// needs, or a level has the wrong number of bytes.
pub(crate) fn encode_bc7(levels: &[Vec<u8>], size: u32, srgb: bool) -> Option<Vec<u8>> {
    // Compress each level, checking its size first.
    let settings = bc7::alpha_basic_settings();
    let mut blocks = Vec::with_capacity(levels.len());
    for (level, pixels) in levels.iter().enumerate() {
        let side = size >> level;
        if side == 0 || !side.is_multiple_of(4) || pixels.len() != (side * side * 4) as usize {
            return None;
        }
        let surface = RgbaSurface {
            data: pixels,
            width: side,
            height: side,
            stride: side * 4,
        };
        blocks.push(bc7::compress_blocks(&settings, &surface));
    }

    // Header, level index and data format descriptor, then the levels,
    // smallest first, as the format asks.
    let level_count = levels.len();
    let dfd = data_format_descriptor(srgb);
    let dfd_offset = 80 + 24 * level_count;
    let data_offset = (dfd_offset + dfd.len()).next_multiple_of(BLOCK_BYTES);
    let mut offsets = vec![0; level_count];
    let mut offset = data_offset;
    for level in (0..level_count).rev() {
        offsets[level] = offset;
        offset += blocks[level].len();
    }

    let mut file = Vec::with_capacity(offset);
    file.extend_from_slice(&MAGIC);
    let format = if srgb {
        VK_FORMAT_BC7_SRGB_BLOCK
    } else {
        VK_FORMAT_BC7_UNORM_BLOCK
    };
    let words = [
        format,
        1, // type size: 1 for block-compressed formats
        size,
        size,
        0, // depth: not a 3D texture
        0, // layers: not an array
        1, // faces: not a cube map
        level_count as u32,
        0, // no supercompression
        dfd_offset as u32,
        dfd.len() as u32,
        0, // no key/value data
        0,
    ];
    for word in words {
        file.extend_from_slice(&word.to_le_bytes());
    }
    file.extend_from_slice(&0u64.to_le_bytes()); // no supercompression data
    file.extend_from_slice(&0u64.to_le_bytes());
    for (level, data) in blocks.iter().enumerate() {
        let length = data.len() as u64;
        file.extend_from_slice(&(offsets[level] as u64).to_le_bytes());
        file.extend_from_slice(&length.to_le_bytes());
        file.extend_from_slice(&length.to_le_bytes());
    }
    file.extend_from_slice(&dfd);
    file.resize(data_offset, 0);
    for data in blocks.iter().rev() {
        file.extend_from_slice(data);
    }
    Some(file)
}

/// The data format descriptor of BC7 texels: one basic block with one
/// 128-bit sample, preceded by the descriptor's total length.
fn data_format_descriptor(srgb: bool) -> Vec<u8> {
    const BLOCK_LENGTH: u16 = 24 + 16;
    const COLOR_MODEL_BC7: u8 = 136;
    const PRIMARIES_BT709: u8 = 1;
    const TRANSFER_LINEAR: u8 = 1;
    const TRANSFER_SRGB: u8 = 2;

    let mut dfd = Vec::with_capacity(4 + usize::from(BLOCK_LENGTH));
    dfd.extend_from_slice(&(4 + u32::from(BLOCK_LENGTH)).to_le_bytes());
    // Vendor Khronos and descriptor type basic (both 0), version 2.
    dfd.extend_from_slice(&0u32.to_le_bytes());
    dfd.extend_from_slice(&2u16.to_le_bytes());
    dfd.extend_from_slice(&BLOCK_LENGTH.to_le_bytes());
    let transfer = if srgb { TRANSFER_SRGB } else { TRANSFER_LINEAR };
    dfd.extend_from_slice(&[COLOR_MODEL_BC7, PRIMARIES_BT709, transfer, 0]);
    // Blocks of 4 × 4 × 1 × 1 texels, stored as one less; 16 bytes a block.
    dfd.extend_from_slice(&[3, 3, 0, 0]);
    dfd.extend_from_slice(&[BLOCK_BYTES as u8, 0, 0, 0, 0, 0, 0, 0]);
    // The sample: bits 0–127 (length stored as one less), the BC7 colour
    // channel, at the block's origin, covering the full range.
    dfd.extend_from_slice(&0u16.to_le_bytes());
    dfd.extend_from_slice(&[127, 0]);
    dfd.extend_from_slice(&[0, 0, 0, 0]);
    dfd.extend_from_slice(&0u32.to_le_bytes());
    dfd.extend_from_slice(&u32::MAX.to_le_bytes());
    dfd
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
    use bevy::render::render_resource::TextureFormat;

    use super::*;

    /// Levels of a `size` image, each a flat colour.
    fn levels(size: u32, count: u32) -> Vec<Vec<u8>> {
        (0..count)
            .map(|level| {
                let side = size >> level;
                [200, 40, 10, 255].repeat((side * side) as usize)
            })
            .collect()
    }

    fn load(file: &[u8], srgb: bool) -> Image {
        Image::from_buffer(
            file,
            ImageType::Extension("ktx2"),
            CompressedImageFormats::BC,
            srgb,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        )
        .unwrap()
    }

    #[test]
    fn bevy_reads_every_level_back() {
        let file = encode_bc7(&levels(32, 3), 32, true).unwrap();
        let image = load(&file, true);
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Bc7RgbaUnormSrgb
        );
        assert_eq!(image.texture_descriptor.mip_level_count, 3);
        assert_eq!(image.width(), 32);
        // 64, 16 and 4 blocks of 16 bytes, largest level first.
        assert_eq!(image.data.as_ref().unwrap().len(), (64 + 16 + 4) * 16);

        let linear = load(&encode_bc7(&levels(16, 1), 16, false).unwrap(), false);
        assert_eq!(
            linear.texture_descriptor.format,
            TextureFormat::Bc7RgbaUnorm
        );
    }

    #[test]
    fn sides_that_are_not_whole_blocks_are_refused() {
        // 16, 8 and 4 pixels are whole blocks; 2 isn't.
        assert!(encode_bc7(&levels(16, 3), 16, true).is_some());
        assert!(encode_bc7(&levels(16, 4), 16, true).is_none());
        assert!(encode_bc7(&levels(18, 1), 18, true).is_none());

        // A level with the wrong number of bytes.
        let mut short = levels(16, 2);
        short[1].pop();
        assert!(encode_bc7(&short, 16, true).is_none());
    }
}
