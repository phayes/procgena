//! Declarative descriptions of editable settings, independent of any widget.

use std::borrow::Cow;
use std::sync::Arc;

pub use crate::ui::SliderScale;

/// Reads a `V` from the edited value.
type Getter<T, V> = Arc<dyn Fn(&T) -> V + Send + Sync>;
/// Writes a `V` into the edited value.
type Setter<T, V> = Arc<dyn Fn(&mut T, V) + Send + Sync>;

/// How one setting is edited.
pub enum Control<T> {
    /// A slider over `min..=max` in increments of `step`, with values spread
    /// along it by `scale`. Whole-number settings use a step of 1.
    Number {
        min: f32,
        max: f32,
        step: f32,
        scale: SliderScale,
        get: Getter<T, f32>,
        set: Setter<T, f32>,
    },
    /// An on/off switch.
    Toggle {
        get: Getter<T, bool>,
        set: Setter<T, bool>,
    },
    /// An sRGB colour written as `0xRRGGBB`.
    Color {
        get: Getter<T, u32>,
        set: Setter<T, u32>,
    },
    /// One of a fixed list of named options, identified by index.
    Choice {
        options: Cow<'static, [&'static str]>,
        get: Getter<T, usize>,
        set: Setter<T, usize>,
    },
}

/// One editable setting of a `T`.
pub struct Param<T> {
    /// Unique, stable identifier, written as the setting's JSON path
    /// (for example `branch.length.0`).
    pub key: String,
    /// Label shown next to the control.
    pub label: String,
    pub control: Control<T>,
    /// Whether the setting currently has any effect; hidden when it returns
    /// false. Always shown when `None`.
    pub shown: Option<Getter<T, bool>>,
    /// For number settings, the value to use instead of a dead one (zero or
    /// the minimum) when an edit makes the setting appear; see [`revive`].
    pub revive: Option<Getter<T, f32>>,
    /// What the setting does, shown as the row's tooltip, if anything.
    pub description: Option<&'static str>,
    /// The part of the asset the setting targets, such as `"leaf"`; the
    /// panel can show only one part's settings. `None` for every part.
    pub part: Option<&'static str>,
}

/// Settings shown together under an optional sub-heading.
pub struct Group<T> {
    pub title: Option<&'static str>,
    pub params: Vec<Param<T>>,
}

/// A collapsible section of groups.
pub struct Section<T> {
    pub title: &'static str,
    pub groups: Vec<Group<T>>,
}

impl<T> Param<T> {
    /// A setting edited by `control`, always shown, with no revive value,
    /// description or part.
    pub fn new(key: impl Into<String>, label: impl Into<String>, control: Control<T>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            control,
            shown: None,
            revive: None,
            description: None,
            part: None,
        }
    }

    /// A number setting over `min..=max` in increments of `step`.
    pub fn number(
        key: impl Into<String>,
        label: impl Into<String>,
        (min, max, step): (f32, f32, f32),
        get: impl Fn(&T) -> f32 + Send + Sync + 'static,
        set: impl Fn(&mut T, f32) + Send + Sync + 'static,
    ) -> Self {
        Self::new(
            key,
            label,
            Control::Number {
                min,
                max,
                step,
                scale: SliderScale::Linear,
                get: Arc::new(get),
                set: Arc::new(set),
            },
        )
    }

    /// A whole-number setting over `min..=max`.
    pub fn count(
        key: impl Into<String>,
        label: impl Into<String>,
        (min, max): (u32, u32),
        get: impl Fn(&T) -> u32 + Send + Sync + 'static,
        set: impl Fn(&mut T, u32) + Send + Sync + 'static,
    ) -> Self {
        Self::number(
            key,
            label,
            (min as f32, max as f32, 1.0),
            move |value| get(value) as f32,
            move |value, n| set(value, n.round().max(0.0) as u32),
        )
    }

    /// An on/off setting.
    pub fn toggle(
        key: impl Into<String>,
        label: impl Into<String>,
        get: impl Fn(&T) -> bool + Send + Sync + 'static,
        set: impl Fn(&mut T, bool) + Send + Sync + 'static,
    ) -> Self {
        Self::new(
            key,
            label,
            Control::Toggle {
                get: Arc::new(get),
                set: Arc::new(set),
            },
        )
    }

    /// A colour setting stored as `0xRRGGBB`.
    pub fn color(
        key: impl Into<String>,
        label: impl Into<String>,
        get: impl Fn(&T) -> u32 + Send + Sync + 'static,
        set: impl Fn(&mut T, u32) + Send + Sync + 'static,
    ) -> Self {
        Self::new(
            key,
            label,
            Control::Color {
                get: Arc::new(get),
                set: Arc::new(set),
            },
        )
    }

    /// A choice between `options`, stored as an index into them.
    pub fn choice(
        key: impl Into<String>,
        label: impl Into<String>,
        options: &'static [&'static str],
        get: impl Fn(&T) -> usize + Send + Sync + 'static,
        set: impl Fn(&mut T, usize) + Send + Sync + 'static,
    ) -> Self {
        Self::choice_of(key, label, Cow::Borrowed(options), get, set)
    }

    /// Like [`Param::choice`], with options worked out at run time.
    pub fn choice_of(
        key: impl Into<String>,
        label: impl Into<String>,
        options: Cow<'static, [&'static str]>,
        get: impl Fn(&T) -> usize + Send + Sync + 'static,
        set: impl Fn(&mut T, usize) + Send + Sync + 'static,
    ) -> Self {
        Self::new(
            key,
            label,
            Control::Choice {
                options,
                get: Arc::new(get),
                set: Arc::new(set),
            },
        )
    }

    /// Shows the setting only while `when` holds, in addition to any
    /// condition it already has.
    pub fn shown_if(self, when: impl Fn(&T) -> bool + Send + Sync + 'static) -> Self
    where
        T: 'static,
    {
        self.shown_if_rc(Arc::new(when))
    }

    fn shown_if_rc(mut self, when: Getter<T, bool>) -> Self
    where
        T: 'static,
    {
        self.shown = Some(match self.shown {
            Some(before) => Arc::new(move |value| before(value) && when(value)),
            None => when,
        });
        self
    }

    /// Whether the setting is shown while editing `value`.
    pub fn is_shown(&self, value: &T) -> bool {
        self.shown.as_ref().is_none_or(|shown| shown(value))
    }

    /// Spreads a number setting's values exponentially along its slider,
    /// giving small values more room; see [`SliderScale::Exponential`].
    /// Other settings are unchanged.
    pub fn exponential(mut self) -> Self {
        if let Control::Number { scale, .. } = &mut self.control {
            *scale = SliderScale::Exponential;
        }
        self
    }

    /// Gives a number setting a usable `value` for when an edit makes it
    /// appear while it is dead (zero or its minimum); see [`revive`].
    pub fn revive(self, value: f32) -> Self {
        self.revive_with(move |_| value)
    }

    /// Like [`Param::revive`], with the value worked out from the rest of the
    /// settings, for example from a parent level's value.
    pub fn revive_with(mut self, value: impl Fn(&T) -> f32 + Send + Sync + 'static) -> Self {
        self.revive = Some(Arc::new(value));
        self
    }

    /// Whether this is a number setting sitting at zero or its minimum in `value`.
    fn is_dead(&self, value: &T) -> bool {
        match &self.control {
            Control::Number { min, get, .. } => {
                let current = get(value);
                current == 0.0 || current == *min
            }
            _ => false,
        }
    }
}

/// Gives usable values to settings that an edit has just made appear.
///
/// Call it after a user edit that changed `before` into `after`. Every
/// setting in `sections` that was hidden in `before`, is shown in `after`,
/// is dead there (zero or its minimum), and has a revive value (see
/// [`Param::revive`]), is set to that value. Settings that appear because of
/// a revived value are revived in turn. Returns whether anything changed.
pub fn revive<T: Clone>(sections: &[Section<T>], before: &T, after: &mut T) -> bool {
    let params: Vec<&Param<T>> = sections.iter().flat_map(Section::params).collect();
    let mut changed = false;

    // Each pass may reveal more settings; revived settings are no longer dead,
    // so there are at most as many passes as settings.
    let shown_before: Vec<bool> = params.iter().map(|p| p.is_shown(before)).collect();
    for _ in 0..params.len() {
        let mut revived = false;
        for (param, was_shown) in params.iter().zip(&shown_before) {
            let (Some(revive), Control::Number { set, .. }) = (&param.revive, &param.control)
            else {
                continue;
            };
            if !was_shown && param.is_shown(after) && param.is_dead(after) {
                let value = revive(after);
                set(after, value);
                revived |= !param.is_dead(after);
            }
        }
        if !revived {
            break;
        }
        changed = true;
    }
    changed
}

impl<S: 'static> Param<S> {
    /// Turns a setting of an `S` into a setting of a `T` that contains an `S`.
    /// `prefix` is prepended to the key, separated by a dot.
    pub fn lift<T: 'static>(
        self,
        prefix: &str,
        get: fn(&T) -> &S,
        get_mut: fn(&mut T) -> &mut S,
    ) -> Param<T> {
        let control = match self.control {
            Control::Number {
                min,
                max,
                step,
                scale,
                get: read,
                set: write,
            } => Control::Number {
                min,
                max,
                step,
                scale,
                get: Arc::new(move |t| read(get(t))),
                set: Arc::new(move |t, v| write(get_mut(t), v)),
            },
            Control::Toggle {
                get: read,
                set: write,
            } => Control::Toggle {
                get: Arc::new(move |t| read(get(t))),
                set: Arc::new(move |t, v| write(get_mut(t), v)),
            },
            Control::Color {
                get: read,
                set: write,
            } => Control::Color {
                get: Arc::new(move |t| read(get(t))),
                set: Arc::new(move |t, v| write(get_mut(t), v)),
            },
            Control::Choice {
                options,
                get: read,
                set: write,
            } => Control::Choice {
                options,
                get: Arc::new(move |t| read(get(t))),
                set: Arc::new(move |t, v| write(get_mut(t), v)),
            },
        };
        let shown = self
            .shown
            .map(|shown| -> Getter<T, bool> { Arc::new(move |t| shown(get(t))) });
        let revive = self
            .revive
            .map(|revive| -> Getter<T, f32> { Arc::new(move |t| revive(get(t))) });
        Param {
            key: format!("{prefix}.{}", self.key),
            label: self.label,
            control,
            shown,
            revive,
            description: self.description,
            part: self.part,
        }
    }
}

impl<S: 'static> Group<S> {
    /// Lifts every setting in the group; see [`Param::lift`].
    pub fn lift<T: 'static>(
        self,
        prefix: &str,
        get: fn(&T) -> &S,
        get_mut: fn(&mut T) -> &mut S,
    ) -> Group<T> {
        Group {
            title: self.title,
            params: self
                .params
                .into_iter()
                .map(|param| param.lift(prefix, get, get_mut))
                .collect(),
        }
    }
}

impl<T> Group<T> {
    /// A group without a sub-heading.
    pub fn plain(params: Vec<Param<T>>) -> Self {
        Self {
            title: None,
            params,
        }
    }

    /// A group under the sub-heading `title`.
    pub fn titled(title: &'static str, params: Vec<Param<T>>) -> Self {
        Self {
            title: Some(title),
            params,
        }
    }

    /// Shows every setting in the group only while `when` holds; see
    /// [`Param::shown_if`]. The sub-heading is hidden with the last setting.
    pub fn shown_if(self, when: impl Fn(&T) -> bool + Send + Sync + 'static) -> Self
    where
        T: 'static,
    {
        let when: Getter<T, bool> = Arc::new(when);
        Self {
            title: self.title,
            params: self
                .params
                .into_iter()
                .map(|param| param.shown_if_rc(when.clone()))
                .collect(),
        }
    }
}

impl<T> Section<T> {
    /// Every setting in the section, in display order.
    pub fn params(&self) -> impl Iterator<Item = &Param<T>> {
        self.groups.iter().flat_map(|group| &group.params)
    }
}

/// Number of decimal places needed to show values in increments of `step`.
pub fn decimals(step: f32) -> usize {
    let mut places = 0;
    let mut scaled = step;
    while places < 4 && (scaled - scaled.round()).abs() > 1e-4 {
        scaled *= 10.0;
        places += 1;
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A feature with a count, whose length shows only while the count is
    /// above zero, and whose width shows only while the length is.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Feature {
        count: u32,
        length: f32,
        width: f32,
        twist: f32,
    }

    fn feature_sections() -> Vec<Section<Feature>> {
        let on = |f: &Feature| f.count > 0;
        vec![Section {
            title: "Feature",
            groups: vec![Group::plain(vec![
                Param::count(
                    "count",
                    "Count",
                    (0, 9),
                    |f: &Feature| f.count,
                    |f, n| f.count = n,
                ),
                Param::number(
                    "length",
                    "Length",
                    (0.0, 3.0, 0.01),
                    |f: &Feature| f.length,
                    |f, v| f.length = v,
                )
                .shown_if(on)
                .revive(0.5),
                Param::number(
                    "width",
                    "Width",
                    (0.0, 1.0, 0.01),
                    |f: &Feature| f.width,
                    |f, v| f.width = v,
                )
                .shown_if(move |f| on(f) && f.length > 0.0)
                .revive_with(|f| f.length / 2.0),
                // Zero twist is meaningful, so it has no revive value.
                Param::number(
                    "twist",
                    "Twist",
                    (0.0, 1.0, 0.01),
                    |f: &Feature| f.twist,
                    |f, v| f.twist = v,
                )
                .shown_if(on),
            ])],
        }]
    }

    #[test]
    fn revealed_dead_settings_are_revived_in_turn() {
        let before = Feature::default();
        let mut after = Feature {
            count: 1,
            ..before.clone()
        };
        assert!(revive(&feature_sections(), &before, &mut after));
        // The width appears only once the length is revived, and follows it.
        assert_eq!(
            after,
            Feature {
                count: 1,
                length: 0.5,
                width: 0.25,
                twist: 0.0,
            }
        );
    }

    #[test]
    fn settings_already_shown_or_in_use_are_left_alone() {
        // The length was shown before the edit, so a zero there was chosen.
        let before = Feature {
            count: 1,
            ..Feature::default()
        };
        let mut after = Feature {
            count: 2,
            ..before.clone()
        };
        assert!(!revive(&feature_sections(), &before, &mut after));
        assert_eq!(after.length, 0.0);

        // A revealed length that is already usable keeps its value.
        let before = Feature {
            length: 1.5,
            ..Feature::default()
        };
        let mut after = Feature {
            count: 1,
            ..before.clone()
        };
        revive(&feature_sections(), &before, &mut after);
        assert_eq!((after.length, after.width), (1.5, 0.75));
    }

    #[test]
    fn decimals_follow_the_step() {
        assert_eq!(decimals(1.0), 0);
        assert_eq!(decimals(0.1), 1);
        assert_eq!(decimals(0.01), 2);
        assert_eq!(decimals(0.005), 3);
        assert_eq!(decimals(0.5), 1);
    }

    #[test]
    fn counts_round_and_never_go_negative() {
        let param = Param::<u32>::count("n", "N", (0, 10), |v| *v, |v, n| *v = n);
        let Control::Number { set, get, .. } = &param.control else {
            panic!("count is a number control");
        };
        let mut value = 0;
        set(&mut value, 2.6);
        assert_eq!(value, 3);
        set(&mut value, -1.0);
        assert_eq!(value, 0);
        assert_eq!(get(&value), 0.0);
    }
}
