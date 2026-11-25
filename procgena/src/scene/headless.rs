//! Running Bevy without a window: the plugins to use, and reading rendered
//! frames back to the CPU.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::app::{PluginGroupBuilder, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

/// Bytes per pixel of a render target (four 8-bit channels).
const BYTES_PER_PIXEL: usize = 4;

/// Row alignment the GPU uses when copying a texture into a buffer.
const COPY_ROW_ALIGNMENT: usize = 256;

/// Bevy's default plugins without a window, event loop or Ctrl-C handler,
/// with the studio's thread pools.
///
/// Pipelined rendering is off, so each frame is finished when `update`
/// returns. Bevy's logging can be set up only once per process, so only the
/// first call includes [`LogPlugin`]; later apps log through it.
pub fn headless_plugins() -> PluginGroupBuilder {
    static LOGGING_STARTED: AtomicBool = AtomicBool::new(false);
    let plugins = DefaultPlugins
        .build()
        .disable::<PipelinedRenderingPlugin>()
        .disable::<TerminalCtrlCHandlerPlugin>()
        .disable::<WinitPlugin>()
        .set(super::task_pool_plugin())
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        });
    if LOGGING_STARTED.swap(true, Ordering::Relaxed) {
        plugins.disable::<LogPlugin>()
    } else {
        plugins
    }
}

/// Strips the per-row padding from a texture readback.
///
/// `data` is what Bevy's `ReadbackComplete` delivers for a `width` x `height`
/// texture with four bytes per pixel. Returns `None` when `data` has a
/// different length than such a readback would, which happens for frames
/// rendered at another size.
pub fn unpad_rows(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (width, height) = (width as usize, height as usize);
    let row = width * BYTES_PER_PIXEL;
    // A single-row copy is not padded.
    let padded_row = if height > 1 {
        row.next_multiple_of(COPY_ROW_ALIGNMENT)
    } else {
        row
    };
    if width == 0 || height == 0 || data.len() != padded_row * height {
        return None;
    }

    if padded_row == row {
        return Some(data.to_vec());
    }
    let mut packed = Vec::with_capacity(row * height);
    for chunk in data.chunks_exact(padded_row) {
        packed.extend_from_slice(&chunk[..row]);
    }
    Some(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpadded_width_is_copied_verbatim() {
        // 64 px * 4 bytes = 256, already aligned.
        let data: Vec<u8> = (0..256 * 3).map(|i| i as u8).collect();
        assert_eq!(unpad_rows(&data, 64, 3), Some(data));
    }

    #[test]
    fn padding_is_removed_from_every_row() {
        // 3 px * 4 bytes = 12 bytes per row, padded to 256.
        let mut data = vec![0xEE; 256 * 2];
        data[..12].copy_from_slice(&[1; 12]);
        data[256..268].copy_from_slice(&[2; 12]);
        let packed = unpad_rows(&data, 3, 2).unwrap();
        assert_eq!(packed.len(), 24);
        assert_eq!(&packed[..12], &[1; 12]);
        assert_eq!(&packed[12..], &[2; 12]);
    }

    #[test]
    fn single_row_is_not_padded() {
        let data = vec![7; 12];
        assert_eq!(unpad_rows(&data, 3, 1), Some(data));
    }

    #[test]
    fn wrong_length_is_rejected() {
        assert_eq!(unpad_rows(&[0; 100], 3, 2), None);
        assert_eq!(unpad_rows(&[], 0, 0), None);
    }
}
