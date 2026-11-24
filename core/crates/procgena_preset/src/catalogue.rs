//! The [`catalogue!`](crate::catalogue) macro.

/// Declares a crate's built-in presets and implements [`Preset`](crate::Preset)
/// for its preset type.
///
/// The preset type must be a struct with public fields `key`, `name`,
/// `description` and `group`, all `Cow<'static, str>`, plus one field
/// holding the options, named after the file kind. The macro adds to it:
///
/// - one function per preset, named by its key, such as
///   `StonePreset::boulder()`, returning the built-in preset;
/// - `all()`, every preset in the order listed; `named(key)`, a preset by
///   its exact key; and `groups()`, the presets grouped for a menu;
/// - `from_yaml(key, text)` and `to_yaml(&self, base)`, which call
///   [`read`](crate::read) and [`write`](crate::write);
/// - the [`Preset`](crate::Preset) impl, whose `check` runs the given
///   check on options read from a file.
///
/// Each preset's options are built once, the first time any preset is
/// used. Keys must be unique; listing one twice makes `named` find only
/// the first.
///
/// ```
/// use std::borrow::Cow;
///
/// #[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
/// pub struct RockOptions {
///     pub size: f32,
/// }
///
/// pub struct RockPreset {
///     pub key: Cow<'static, str>,
///     pub name: Cow<'static, str>,
///     pub description: Cow<'static, str>,
///     pub group: Cow<'static, str>,
///     pub rock: RockOptions,
/// }
///
/// procgena_preset::catalogue! {
///     RockPreset { rock: RockOptions, check: |_options| Ok(()) }
///     "Large" {
///         boulder { name: "Boulder", description: "A big rock." }
///             => RockOptions { size: 2.5 };
///     }
///     "Small" {
///         pebble { name: "Pebble", description: "A small rock." }
///             => RockOptions { size: 0.15 };
///     }
/// }
///
/// assert_eq!(RockPreset::pebble().rock.size, 0.15);
/// assert_eq!(RockPreset::named("boulder").unwrap().name, "Boulder");
/// let mine = RockPreset::from_yaml("mine", "base: boulder\nrock:\n  size: 3.0\n").unwrap();
/// assert_eq!(mine.rock.size, 3.0);
/// ```
#[macro_export]
macro_rules! catalogue {
    (
        $preset:ident { $field:ident : $options:ty, check: $check:expr $(,)? }
        $(
            $group:literal {
                $(
                    $key:ident { name: $name:literal, description: $description:literal $(,)? }
                        => $value:expr;
                )*
            }
        )*
    ) => {
        impl $preset {
            /// Every built-in preset, in catalogue order: grouped, and in
            /// display order within each group.
            pub fn all() -> impl ::std::iter::Iterator<Item = &'static Self> {
                static CATALOGUE: ::std::sync::LazyLock<::std::vec::Vec<$preset>> =
                    ::std::sync::LazyLock::new(|| {
                        ::std::vec![
                            $($(
                                $preset {
                                    key: ::std::borrow::Cow::Borrowed(::std::stringify!($key)),
                                    name: ::std::borrow::Cow::Borrowed($name),
                                    description: ::std::borrow::Cow::Borrowed($description),
                                    group: ::std::borrow::Cow::Borrowed($group),
                                    $field: $value,
                                },
                            )*)*
                        ]
                    });
                CATALOGUE.iter()
            }

            /// The built-in preset with this exact key, such as the
            /// file-style key of one of this type's preset functions.
            /// Returns `None` for an unknown key.
            #[must_use]
            pub fn named(key: &str) -> ::std::option::Option<&'static Self> {
                Self::all().find(|preset| preset.key == key)
            }

            /// The built-in presets grouped for a menu: each group's name
            /// and its presets, in catalogue order.
            #[must_use]
            pub fn groups() -> ::std::vec::Vec<(&'static str, ::std::vec::Vec<&'static Self>)> {
                $crate::groups::<Self>()
            }

            /// Reads a preset from the text of a file; see
            /// [`procgena_preset::read`]. `key` is usually the file name
            /// without its extension.
            ///
            /// # Errors
            ///
            /// Returns a [`procgena_preset::PresetFileError`] for text that
            /// isn't YAML, unknown keys or settings, an unknown `base`, and
            /// settings that fail the options' check.
            pub fn from_yaml(
                key: &str,
                text: &str,
            ) -> ::std::result::Result<Self, $crate::PresetFileError> {
                $crate::read(key, text)
            }

            /// Writes the preset as the text of a file holding only what
            /// differs from `base`, or from the default options when `base`
            /// is `None`; see [`procgena_preset::write`].
            #[must_use]
            pub fn to_yaml(&self, base: ::std::option::Option<&Self>) -> ::std::string::String {
                $crate::write(self, base)
            }

            $($(
                #[doc = $description]
                #[must_use]
                pub fn $key() -> &'static Self {
                    Self::named(::std::stringify!($key))
                        .expect("every listed preset is in the catalogue")
                }
            )*)*
        }

        impl $crate::Preset for $preset {
            type Options = $options;
            const KIND: &'static str = ::std::stringify!($field);

            fn key(&self) -> &str {
                &self.key
            }
            fn name(&self) -> &str {
                &self.name
            }
            fn description(&self) -> &str {
                &self.description
            }
            fn group(&self) -> &str {
                &self.group
            }
            fn options(&self) -> &$options {
                &self.$field
            }
            fn from_parts(meta: $crate::PresetMeta, options: $options) -> Self {
                Self {
                    key: ::std::borrow::Cow::Owned(meta.key),
                    name: ::std::borrow::Cow::Owned(meta.name),
                    description: ::std::borrow::Cow::Owned(meta.description),
                    group: ::std::borrow::Cow::Owned(meta.group),
                    $field: options,
                }
            }
            fn all() -> impl ::std::iter::Iterator<Item = &'static Self> {
                $preset::all()
            }
            fn check(options: &$options) -> ::std::result::Result<(), ::std::string::String> {
                let check: fn(&$options) -> ::std::result::Result<(), ::std::string::String> = $check;
                check(options)
            }
        }
    };
}
