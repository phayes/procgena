//! The rock editor: controls for customising a rock preset's shape and
//! appearance.

use std::sync::LazyLock;

use crate::studio::inspector::MetaKind;

pub mod editor;
/// Hand-written rock settings, still used by the Ecotope's stone class
/// panels until the Ecotope's Inspector is built from metadata.
pub mod params;

/// The key of the rock preset opened when the studio starts on rocks.
pub const START_PRESET: &str = "boulder";

/// The rock settings' metadata, from `stone.meta.yml`.
pub static META: LazyLock<MetaKind> =
    LazyLock::new(|| MetaKind::new(procgena_stone::STONE_META, procgena_stone::meta_choices()));
