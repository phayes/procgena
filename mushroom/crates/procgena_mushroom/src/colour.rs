//! Reading hex colours and converting them for use in meshes.
//!
//! Colours in options are sRGB, the encoding image editors and the web use.
//! Lighting maths and vertex colours need "linear" values, which are
//! proportional to the actual amount of light, so colours are converted
//! before use.

use crate::options::MaterialOptions;

/// Reads `"#rrggbb"` (the `#` may be left out) as sRGB components from 0 to
/// 1. Returns `None` for anything else.
pub(crate) fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?].map(|c| f32::from(c) / 255.0))
}

/// Converts one sRGB colour component, from 0 to 1, to linear light, also
/// from 0 to 1.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Reads a `"#rrggbb"` sRGB colour, as used in [`MaterialOptions`], and
/// returns its red, green and blue in linear light. Returns `None` if the
/// text is not in that form.
pub fn linear_from_hex(text: &str) -> Option<[f32; 3]> {
    parse_hex(text).map(|c| c.map(srgb_to_linear))
}

/// The material's colours in linear RGB, ready for blending. Each field
/// matches the [`MaterialOptions`] field of the same name.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub cap_centre: [f32; 3],
    pub cap_rim: [f32; 3],
    pub underside: [f32; 3],
    pub stem_top: [f32; 3],
    pub stem_base: [f32; 3],
    pub annulus: [f32; 3],
    pub wart: [f32; 3],
}

impl Palette {
    /// Converts the colours in `m`, which should already be validated. Any
    /// colour that can't be read becomes mid grey.
    pub(crate) fn new(m: &MaterialOptions) -> Self {
        let c = |s: &str| linear_from_hex(s).unwrap_or([0.5; 3]);
        Self {
            cap_centre: c(&m.cap_centre),
            cap_rim: c(&m.cap_rim),
            underside: c(&m.underside),
            stem_top: c(&m.stem_top),
            stem_base: c(&m.stem_base),
            annulus: c(&m.annulus),
            wart: c(&m.wart),
        }
    }
}

/// Blends two colours: `t = 0` gives `a`, `t = 1` gives `b`, and values in
/// between mix them.
pub(crate) fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// Adds a fully opaque alpha (1) to a colour.
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
        assert_eq!(parse_hex("#ééé"), None);
    }
}
