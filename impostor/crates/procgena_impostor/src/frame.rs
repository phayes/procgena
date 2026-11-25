//! How many pixels each impostor view needs, from where impostors take over
//! in the shared drawing chain.

/// The screen height, in pixels, that [`frame_size_for`] sizes impostor
/// views for. Taller screens show impostors slightly magnified.
pub const REFERENCE_SCREEN_HEIGHT: u32 = 1440;

/// The smallest view size [`frame_size_for`] returns, in pixels.
pub const MIN_FRAME_SIZE: u32 = 32;

/// The largest view size [`frame_size_for`] returns, in pixels.
pub const MAX_FRAME_SIZE: u32 = 512;

/// Pixels along each side of one impostor view, for an impostor drawn at
/// most `max_size` of the screen's height tall: enough that it is never
/// shown larger than its views on a screen [`REFERENCE_SCREEN_HEIGHT`]
/// pixels tall. Use it as
/// [`BakeSettings::frame_size`](crate::BakeSettings::frame_size).
///
/// The size is a power of two from [`MIN_FRAME_SIZE`] to
/// [`MAX_FRAME_SIZE`]. For impostors drawn by a
/// [`DistanceChain`](procgena_core::chain::DistanceChain), pass its
/// [`largest_impostor_size`](procgena_core::chain::DistanceChain::largest_impostor_size),
/// so one bake serves every quality setting and a change of quality never
/// bakes again; smaller sizes on screen draw from the atlas's smaller
/// mipmaps.
///
/// ```
/// use procgena_core::chain::DistanceChain;
/// use procgena_impostor::frame_size_for;
///
/// // At Low quality the default chain shows an impostor up to 160 pixels
/// // tall.
/// let size = DistanceChain::DEFAULT.largest_impostor_size();
/// assert_eq!(frame_size_for(size), 256);
/// ```
pub fn frame_size_for(max_size: f32) -> u32 {
    let pixels = (max_size * REFERENCE_SCREEN_HEIGHT as f32).ceil() as u32;
    pixels
        .next_power_of_two()
        .clamp(MIN_FRAME_SIZE, MAX_FRAME_SIZE)
}

#[cfg(test)]
mod tests {
    use procgena_core::Quality;
    use procgena_core::chain::DistanceChain;

    use super::*;

    #[test]
    fn frames_cover_the_largest_size_and_stay_in_bounds() {
        let chain = DistanceChain::DEFAULT;
        let size = chain.largest_impostor_size();
        assert_eq!(size, chain.impostor_max_size(Quality::Low));
        let pixels = size * REFERENCE_SCREEN_HEIGHT as f32;
        assert!(pixels <= 256.0 && pixels > 128.0, "{pixels}");
        assert_eq!(frame_size_for(size), 256);
        assert_eq!(frame_size_for(63.5 / 1440.0), 64);
        assert_eq!(frame_size_for(0.0001), MIN_FRAME_SIZE);
        assert_eq!(frame_size_for(2.0), MAX_FRAME_SIZE);
    }
}
