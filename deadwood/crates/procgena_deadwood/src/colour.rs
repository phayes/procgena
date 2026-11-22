//! Reading hex colours and converting them for use in meshes.
//!
//! Colours in options are sRGB, the encoding image editors and the web use.
//! Lighting maths and vertex colours need "linear" values, which are
//! proportional to the actual amount of light, so colours are converted
//! before use.

use crate::options::MaterialOptions;

/// Reads `"#rrggbb"` (the `#` is optional) as three sRGB channels from 0 to
/// 1, or `None` if the text is not a six-digit hex colour.
pub(crate) fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?].map(|c| f32::from(c) / 255.0))
}

/// Converts one sRGB colour channel, from 0 to 1, to a linear value, also
/// from 0 to 1. Use it for colours picked in an image editor before passing
/// them to lighting or vertex colours.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Reads an sRGB colour written as `"#rrggbb"` (the `#` is optional) and
/// returns it as linear RGB, each channel from 0 to 1. Returns `None` if the
/// text is not a six-digit hex colour.
pub fn linear_from_hex(text: &str) -> Option<[f32; 3]> {
    parse_hex(text).map(|c| c.map(srgb_to_linear))
}

/// The material's colours, converted to linear RGB.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    /// See [`MaterialOptions::bark`].
    pub bark: [f32; 3],
    /// See [`MaterialOptions::bark_weathered`].
    pub bark_weathered: [f32; 3],
    /// See [`MaterialOptions::sapwood`].
    pub sapwood: [f32; 3],
    /// See [`MaterialOptions::heartwood`].
    pub heartwood: [f32; 3],
    /// See [`MaterialOptions::rot`].
    pub rot: [f32; 3],
}

impl Palette {
    /// Converts `m`'s colours. An invalid colour, which validation would
    /// have rejected, becomes mid grey instead of failing.
    pub(crate) fn new(m: &MaterialOptions) -> Self {
        let c = |s: &str| linear_from_hex(s).unwrap_or([0.5; 3]);
        Self {
            bark: c(&m.bark),
            bark_weathered: c(&m.bark_weathered),
            sapwood: c(&m.sapwood),
            heartwood: c(&m.heartwood),
            rot: c(&m.rot),
        }
    }
}

/// Blends from `a` (at `t = 0`) to `b` (at `t = 1`).
pub(crate) fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// Adds full opacity to a colour.
pub(crate) fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(parse_hex("#ff0000"), Some([1.0, 0.0, 0.0]));
        assert_eq!(parse_hex("00ff00"), Some([0.0, 1.0, 0.0]));
        assert_eq!(parse_hex("#ff00"), None);
        assert_eq!(parse_hex("#gg0000"), None);
    }
}
