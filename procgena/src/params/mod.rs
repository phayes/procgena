//! Editable settings described as data, and the panel that edits them.
//!
//! The macros below build a [`Param`] from a field path, so each table entry
//! names its field once. Each takes the edited type, the key, the label, any
//! range, and a closure-like `|value| value.path.to.field`.

#[cfg(test)]
pub mod checks;
pub mod panel;
pub mod spec;

pub use panel::{
    PanelWidgetsPlugin, ParamPanel, ParamPanelPlugin, ParamsChanged, spawn_param_panel,
};
#[cfg(test)]
pub use spec::Control;
pub use spec::{Group, Param, Section};

/// A number setting for an `f32` field over `(min, max, step)`.
macro_rules! number {
    ($t:ty, $key:expr, $label:expr, $range:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::number(
            $key,
            $label,
            $range,
            |$o: &$t| $place,
            |$o: &mut $t, value| $place = value,
        )
    };
}

/// A whole-number setting for a `u32` field over `(min, max)`.
macro_rules! count {
    ($t:ty, $key:expr, $label:expr, $range:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::count(
            $key,
            $label,
            $range,
            |$o: &$t| $place,
            |$o: &mut $t, value| $place = value,
        )
    };
}

/// An on/off setting for a `bool` field.
macro_rules! toggle {
    ($t:ty, $key:expr, $label:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::toggle(
            $key,
            $label,
            |$o: &$t| $place,
            |$o: &mut $t, value| $place = value,
        )
    };
}

/// A colour setting for a `0xRRGGBB` field.
macro_rules! color {
    ($t:ty, $key:expr, $label:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::color(
            $key,
            $label,
            |$o: &$t| $place,
            |$o: &mut $t, value| $place = value,
        )
    };
}

/// A choice between enum variants, each listed as `"Name" => Variant`.
/// A value not in the list shows as the first option.
macro_rules! choice {
    ($t:ty, $key:expr, $label:expr, [$($name:literal => $variant:expr),+ $(,)?], |$o:ident| $place:expr) => {
        $crate::params::Param::choice(
            $key,
            $label,
            &[$($name),+],
            |$o: &$t| {
                let current = $place;
                [$($variant),+].iter().position(|v| *v == current).unwrap_or(0)
            },
            |$o: &mut $t, index| {
                if let Some(v) = [$($variant),+].get(index) {
                    $place = *v;
                }
            },
        )
    };
}

/// A choice between the names in `$names`, for a `String` field.
/// A value not in the list shows as the first option.
macro_rules! name_choice {
    ($t:ty, $key:expr, $label:expr, $names:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::choice(
            $key,
            $label,
            &$names,
            |$o: &$t| $names.iter().position(|name| *name == $place).unwrap_or(0),
            |$o: &mut $t, index| {
                if let Some(name) = $names.get(index) {
                    $place = (*name).to_owned();
                }
            },
        )
    };
}

/// Edits a Bevy [`Color`](bevy::color::Color) using an sRGB colour picker.
///
/// The picker uses `0xRRGGBB`, with 8 bits per channel. Merely displaying
/// a colour preserves its original precision; choosing a colour stores the
/// picker's rounded, opaque value.
macro_rules! srgb {
    ($t:ty, $key:expr, $label:expr, |$o:ident| $place:expr) => {
        $crate::params::Param::color(
            $key,
            $label,
            |$o: &$t| $crate::params::srgb_to_hex($place),
            |$o: &mut $t, value| $place = $crate::params::srgb_from_hex(value),
        )
    };
}

pub(crate) use {choice, color, count, name_choice, number, srgb, toggle};

/// Converts a colour to sRGB `0xRRGGBB`, rounding each channel to 8 bits.
pub fn srgb_to_hex(color: bevy::color::Color) -> u32 {
    use bevy::color::ColorToPacked;
    let [r, g, b, _] = color.to_srgba().to_u8_array();
    u32::from_be_bytes([0, r, g, b])
}

/// Converts sRGB `0xRRGGBB` to an opaque Bevy colour.
pub fn srgb_from_hex(hex: u32) -> bevy::color::Color {
    let [_, r, g, b] = hex.to_be_bytes();
    bevy::color::Color::srgb_u8(r, g, b)
}

/// Converts a linear RGB material colour, as procgena's material options
/// store it, to the colour picker's sRGB `0xRRGGBB`. Out-of-range channels
/// are clipped.
pub fn linear_to_hex([r, g, b]: [f32; 3]) -> u32 {
    srgb_to_hex(bevy::color::Color::linear_rgb(r, g, b))
}

/// Converts the colour picker's sRGB `0xRRGGBB` to a linear RGB material colour.
pub fn linear_from_hex(hex: u32) -> [f32; 3] {
    use bevy::color::ColorToComponents;
    srgb_from_hex(hex).to_linear().to_vec3().to_array()
}

#[cfg(test)]
mod tests {
    use super::{srgb_from_hex, srgb_to_hex};

    #[test]
    fn colours_round_trip_through_hex() {
        for hex in [0x000000, 0xffffff, 0x123456, 0xfedcba] {
            assert_eq!(srgb_to_hex(srgb_from_hex(hex)), hex);
        }
    }
}
