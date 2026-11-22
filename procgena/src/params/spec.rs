//! Declarative descriptions of editable settings, independent of any widget.

use std::sync::Arc;

/// Reads a `V` from the edited value.
type Getter<T, V> = Arc<dyn Fn(&T) -> V + Send + Sync>;
/// Writes a `V` into the edited value.
type Setter<T, V> = Arc<dyn Fn(&mut T, V) + Send + Sync>;

/// How one setting is edited.
pub enum Control<T> {
    /// A slider over `min..=max` in increments of `step`. Whole-number
    /// settings use a step of 1.
    Number {
        min: f32,
        max: f32,
        step: f32,
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
        options: &'static [&'static str],
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
    /// A number setting over `min..=max` in increments of `step`.
    pub fn number(
        key: impl Into<String>,
        label: impl Into<String>,
        (min, max, step): (f32, f32, f32),
        get: impl Fn(&T) -> f32 + Send + Sync + 'static,
        set: impl Fn(&mut T, f32) + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            control: Control::Number {
                min,
                max,
                step,
                get: Arc::new(get),
                set: Arc::new(set),
            },
            shown: None,
        }
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
        Self {
            key: key.into(),
            label: label.into(),
            control: Control::Toggle {
                get: Arc::new(get),
                set: Arc::new(set),
            },
            shown: None,
        }
    }

    /// A colour setting stored as `0xRRGGBB`.
    pub fn color(
        key: impl Into<String>,
        label: impl Into<String>,
        get: impl Fn(&T) -> u32 + Send + Sync + 'static,
        set: impl Fn(&mut T, u32) + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            control: Control::Color {
                get: Arc::new(get),
                set: Arc::new(set),
            },
            shown: None,
        }
    }

    /// A choice between `options`, stored as an index into them.
    pub fn choice(
        key: impl Into<String>,
        label: impl Into<String>,
        options: &'static [&'static str],
        get: impl Fn(&T) -> usize + Send + Sync + 'static,
        set: impl Fn(&mut T, usize) + Send + Sync + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            control: Control::Choice {
                options,
                get: Arc::new(get),
                set: Arc::new(set),
            },
            shown: None,
        }
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
                get: read,
                set: write,
            } => Control::Number {
                min,
                max,
                step,
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
        Param {
            key: format!("{prefix}.{}", self.key),
            label: self.label,
            control,
            shown,
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
