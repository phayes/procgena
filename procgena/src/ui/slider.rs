//! A number slider whose track can be spread evenly or exponentially.
//!
//! It looks like the feathers slider, but its track runs from 0 to 1 and the
//! value it shows comes from [`ScaledSlider::value_at`]. An exponential slider
//! gives more of its track to small values, which suits sizes, densities and
//! counts that matter most near the bottom of a wide range.

use core::f32::consts::PI;

use bevy::feathers::constants::{fonts, size};
use bevy::feathers::display::caption;
use bevy::feathers::focus::FocusIndicator;
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::rounded_corners::RoundedCorners;
use bevy::feathers::theme::{InheritableThemeTextColor, SurfaceLevel, ThemeContext, UiTheme};
use bevy::feathers::tokens;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::text::{FontWeight, TextLayout};
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{
    Slider, SliderOrientation, SliderRange, SliderStep, SliderValue, TrackClick,
};
use bevy::window::SystemCursorIcon;

/// Exponent of the curve used by [`SliderScale::Exponential`] when the range
/// reaches or crosses zero. Half way along the track shows one eighth of the
/// range.
const POWER: f32 = 3.0;

/// Track distance moved by one arrow-key press, as a share of the track.
const KEY_STEP: f32 = 0.01;

/// How values are spread along a slider's track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliderScale {
    /// Equal track distances give equal value changes.
    #[default]
    Linear,
    /// Small values get more of the track. A range above zero is spread
    /// logarithmically, so each stretch of track multiplies the value by the
    /// same amount. A range starting at zero rises along a cubic curve, and a
    /// range crossing zero uses that curve on each side of zero.
    Exponential,
}

/// A slider's range, step and scale. Add it to a slider made with
/// [`scaled_slider`]; replace it to change them.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct ScaledSlider {
    /// The smallest value.
    pub min: f32,
    /// The largest value; must be greater than `min`.
    pub max: f32,
    /// Values are rounded to whole steps from `min`.
    pub step: f32,
    /// How values are spread along the track.
    pub scale: SliderScale,
    /// Decimal places shown in the label.
    pub precision: usize,
}

impl ScaledSlider {
    /// The value at `position` along the track, from 0 at the left to 1 at
    /// the right, rounded to a whole step and kept within the range.
    pub fn value_at(&self, position: f32) -> f32 {
        let t = position.clamp(0.0, 1.0);
        let raw = match self.scale {
            SliderScale::Linear => self.min + (self.max - self.min) * t,
            SliderScale::Exponential if self.min > 0.0 => self.min * (self.max / self.min).powf(t),
            SliderScale::Exponential if self.min >= 0.0 => {
                self.min + (self.max - self.min) * t.powf(POWER)
            }
            SliderScale::Exponential => {
                // Each side of zero gets track in proportion to its length.
                let zero = -self.min / (self.max - self.min);
                if t < zero {
                    self.min * ((zero - t) / zero).powf(POWER)
                } else {
                    self.max * ((t - zero) / (1.0 - zero)).powf(POWER)
                }
            }
        };
        self.snap(raw)
    }

    /// Where `value` sits along the track, from 0 to 1. Values outside the
    /// range sit at the nearer end.
    pub fn position_of(&self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        let t = match self.scale {
            SliderScale::Linear => (value - self.min) / (self.max - self.min),
            SliderScale::Exponential if self.min > 0.0 => {
                (value / self.min).ln() / (self.max / self.min).ln()
            }
            SliderScale::Exponential if self.min >= 0.0 => {
                ((value - self.min) / (self.max - self.min)).powf(POWER.recip())
            }
            SliderScale::Exponential => {
                let zero = -self.min / (self.max - self.min);
                if value < 0.0 {
                    zero - zero * (value / self.min).powf(POWER.recip())
                } else {
                    zero + (1.0 - zero) * (value / self.max).powf(POWER.recip())
                }
            }
        };
        if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// `value` rounded to a whole step from `min` and kept within the range.
    fn snap(&self, value: f32) -> f32 {
        let steps = ((value - self.min) / self.step).round();
        (self.min + steps * self.step).clamp(self.min, self.max)
    }

    /// `value` as the label shows it.
    fn label(&self, value: f32) -> String {
        format!("{value:.*}", self.precision)
    }
}

/// Marks a scaled slider's value text.
#[derive(Component, Default, Clone)]
struct ScaledSliderText;

/// A slider for `slider`'s range, showing `value`.
///
/// Its [`SliderValue`] is the track position, from 0 to 1, and the
/// `ValueChange<f32>` events it sends carry the new track position. Convert
/// with [`ScaledSlider::value_at`] and [`ScaledSlider::position_of`]. Needs
/// [`ScaledSliderPlugin`].
pub fn scaled_slider(slider: ScaledSlider, value: f32) -> impl Scene {
    let position = slider.position_of(value);
    let text = slider.label(slider.value_at(position));
    bsn! {
        Node {
            height: size::ROW_HEIGHT,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            flex_grow: 1.0,
            border_radius: {RoundedCorners::All.to_border_radius(6.0)},
        }
        Hovered
        Slider {
            track_click: TrackClick::Drag,
            orientation: SliderOrientation::Horizontal,
        }
        SliderValue({position})
        SliderRange::new(0.0, 1.0)
        SliderStep(KEY_STEP)
        ScaledSlider {
            min: {slider.min},
            max: {slider.max},
            step: {slider.step},
            scale: {slider.scale},
            precision: {slider.precision},
        }
        EntityCursor::System(SystemCursorIcon::EwResize)
        TabIndex(0)
        FocusIndicator
        InheritableThemeTextColor(tokens::SLIDER_TEXT)
        // The filled part of the track is drawn with a hard-edged gradient.
        BackgroundGradient(vec![Gradient::Linear(LinearGradient {
            angle: PI * 0.5,
            stops: vec![
                ColorStop::new(Color::NONE, percent(0)),
                ColorStop::new(Color::NONE, percent(50)),
                ColorStop::new(Color::NONE, percent(50)),
                ColorStop::new(Color::NONE, percent(100)),
            ],
            color_space: InterpolationColorSpace::Srgba,
        })])
        Children [
            Node {
                display: Display::Flex,
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            InheritableFont {
                font: fonts::MONO,
                font_size: size::SMALL_FONT,
                weight: FontWeight::NORMAL,
            }
            Children [ @caption(text) TextLayout::no_wrap() ScaledSliderText ]
        ]
    }
}

/// Draws [`scaled_slider`]s: their colours, filled track and value text.
pub struct ScaledSliderPlugin;

impl Plugin for ScaledSliderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, (style_sliders, show_slider_values));
    }
}

/// The slider parts that change colour with its state.
type StyleParts<'a> = (
    Entity,
    Has<InteractionDisabled>,
    Has<Pressed>,
    &'a Hovered,
    &'a mut BackgroundGradient,
    Option<&'a ThemeContext>,
);

/// Colours each slider for its hovered, pressed or disabled state.
fn style_sliders(
    mut sliders: Query<StyleParts, With<ScaledSlider>>,
    theme: Res<UiTheme>,
    mut commands: Commands,
) {
    for (entity, disabled, pressed, hovered, mut gradient, context) in &mut sliders {
        let context = context.map_or(SurfaceLevel::Base, |c| c.0);
        let pick = |normal, hover, press, off| {
            theme.context_color(
                &if disabled {
                    off
                } else if pressed {
                    press
                } else if hovered.0 {
                    hover
                } else {
                    normal
                },
                context,
            )
        };
        let bar = pick(
            tokens::SLIDER_BAR,
            tokens::SLIDER_BAR_HOVER,
            tokens::SLIDER_BAR_PRESSED,
            tokens::SLIDER_BAR_DISABLED,
        );
        let background = pick(
            tokens::SLIDER_BG,
            tokens::SLIDER_BG_HOVER,
            tokens::SLIDER_BG_PRESSED,
            tokens::SLIDER_BG_DISABLED,
        );

        // Only touch the gradient when a colour changed, to avoid redrawing.
        if let [Gradient::Linear(linear)] = &gradient.0[..]
            && linear.stops[0].color == bar
            && linear.stops[2].color == background
        {
            continue;
        }
        if let [Gradient::Linear(linear)] = &mut gradient.0[..] {
            linear.stops[0].color = bar;
            linear.stops[1].color = bar;
            linear.stops[2].color = background;
            linear.stops[3].color = background;
        }
        let cursor = if disabled {
            SystemCursorIcon::NotAllowed
        } else {
            SystemCursorIcon::EwResize
        };
        commands.entity(entity).insert(EntityCursor::System(cursor));
    }
}

/// Moves each changed slider's fill to its position and shows its value.
#[expect(
    clippy::type_complexity,
    reason = "a Bevy query with a change filter reads best written out in place"
)]
fn show_slider_values(
    mut sliders: Query<
        (Entity, &ScaledSlider, &SliderValue, &mut BackgroundGradient),
        Or<(Changed<SliderValue>, Changed<ScaledSlider>)>,
    >,
    children: Query<&Children>,
    mut texts: Query<&mut Text, With<ScaledSliderText>>,
) {
    for (entity, slider, position, mut gradient) in &mut sliders {
        if let [Gradient::Linear(linear)] = &mut gradient.0[..] {
            let filled = percent(position.0.clamp(0.0, 1.0) * 100.0);
            linear.stops[1].point = filled;
            linear.stops[2].point = filled;
        }
        let label = slider.label(slider.value_at(position.0));
        for child in children.iter_descendants(entity) {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != label
            {
                text.0 = label.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(min: f32, max: f32, step: f32, scale: SliderScale) -> ScaledSlider {
        ScaledSlider {
            min,
            max,
            step,
            scale,
            precision: 3,
        }
    }

    #[test]
    fn linear_sliders_spread_values_evenly() {
        let s = slider(0.0, 10.0, 0.01, SliderScale::Linear);
        assert_eq!(s.value_at(0.5), 5.0);
        assert_eq!(s.position_of(2.5), 0.25);
    }

    #[test]
    fn exponential_ranges_above_zero_are_logarithmic() {
        let s = slider(0.1, 10.0, 0.001, SliderScale::Exponential);
        assert_eq!(s.value_at(0.0), 0.1);
        assert_eq!(s.value_at(0.5), 1.0);
        assert_eq!(s.value_at(1.0), 10.0);
        assert!((s.position_of(1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn exponential_ranges_from_zero_reach_zero_and_favour_small_values() {
        let s = slider(0.0, 8.0, 0.01, SliderScale::Exponential);
        assert_eq!(s.value_at(0.0), 0.0);
        assert_eq!(s.value_at(0.5), 1.0);
        assert_eq!(s.value_at(1.0), 8.0);
        assert!((s.position_of(1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn exponential_ranges_across_zero_curve_on_both_sides() {
        let s = slider(-0.01, 0.01, 0.0001, SliderScale::Exponential);
        assert_eq!(s.value_at(0.5), 0.0);
        assert_eq!(s.value_at(0.0), -0.01);
        assert_eq!(s.value_at(1.0), 0.01);
        assert!(s.value_at(0.75) > 0.0 && s.value_at(0.75) < 0.0025);
        assert!((s.position_of(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn every_step_value_round_trips_through_the_track() {
        // Positions found for a value lead back to that value, so a slider
        // never drifts when it redraws a value set elsewhere.
        for scale in [SliderScale::Linear, SliderScale::Exponential] {
            for (min, max, step) in [
                (0.0, 10.0, 0.01),
                (0.001, 0.2, 0.001),
                (-0.01, 0.01, 0.0001),
            ] {
                let s = slider(min, max, step, scale);
                let count = ((max - min) / step).round() as i32;
                for i in 0..=count {
                    let value = s.snap(min + i as f32 * step);
                    let back = s.value_at(s.position_of(value));
                    assert!(
                        (back - value).abs() < step * 0.5,
                        "{scale:?} {min}..{max}: {value} came back as {back}"
                    );
                }
            }
        }
    }

    #[test]
    fn values_are_rounded_to_steps_and_kept_in_range() {
        let s = slider(0.05, 10.0, 0.05, SliderScale::Exponential);
        let v = s.value_at(0.3);
        assert!(((v - 0.05) / 0.05 - ((v - 0.05) / 0.05).round()).abs() < 1e-3);
        assert_eq!(s.value_at(-1.0), 0.05);
        assert_eq!(s.value_at(2.0), 10.0);
    }
}
