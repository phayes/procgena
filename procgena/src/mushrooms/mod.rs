//! The mushroom editor: controls for customising a mushroom preset's shape
//! and colours.

use std::sync::LazyLock;

use crate::studio::inspector::MetaKind;

pub mod editor;
/// Hand-written mushroom settings, still used by the Ecotope's mushroom
/// Scatter panels until the Ecotope's Inspector is built from metadata.
pub mod params;

/// The key of the mushroom preset opened when the studio starts on
/// mushrooms.
pub const START_PRESET: &str = "fly_agaric";

/// The mushroom settings' metadata, from `mushroom.meta.yml`.
pub static META: LazyLock<MetaKind> = LazyLock::new(|| {
    MetaKind::new(
        procgena_mushroom::MUSHROOM_META,
        procgena_mushroom::meta_choices(),
    )
});
