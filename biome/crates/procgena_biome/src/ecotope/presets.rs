//! Ready-made Ecotopes, each an embedded `.ecotope.yml` file in the crate's
//! `ecotopes` folder.

use std::borrow::Cow;

use super::Ecotope;

/// A named, ready-made [`Ecotope`].
///
/// Every built-in preset has a function of its own, such as
/// [`EcotopePreset::pine_forest_floor`]. Clone its
/// [`ecotope`](Self::ecotope) to customise it. A `.ecotope.yml` file loads
/// with [`EcotopePreset::from_yaml`]:
///
/// ```yaml
/// # mossy_pine_floor.ecotope.yml
/// base: pine_forest_floor
/// name: Mossy pine floor
/// ecotope:
///   scatters:
///     logs:                 # change one setting of one Scatter
///       density: 0.02
///     chanterelles:         # add a Scatter under a new key
///       priority: 160
///       kind: mushroom
///       mushroom: chanterelle
///       density: 0.1
///       min_spacing: 1.0
/// ```
///
/// Scatters are listed by key, and a file gives only what differs from its
/// `base`. A new Scatter gives its `priority` and `kind` (`tree`, `stone`,
/// `deadwood` or `mushroom`), an optional `clearance`, and the settings of
/// its kind that differ from the kind's defaults. A tree Scatter's settings
/// are a [`TreeScatter`](procgena_tree::TreeScatter)'s, a stone Scatter's a
/// [`StoneScatterOptions`](procgena_stone::StoneScatterOptions)'s, a dead
/// wood Scatter's a [`procgena_deadwood::ScatterOptions`]' (defaults: fallen
/// logs), and a mushroom Scatter's a [`procgena_mushroom::ScatterOptions`]'
/// plus its `mushroom`. Each stone class gives what differs from
/// [`StoneClass::new`](procgena_stone::StoneClass::new) with default stone
/// settings, a density of 0.01 and a spacing of 2.
///
/// A stone class's `stone`, a dead wood Scatter's `piece` and a mushroom
/// Scatter's `mushroom` each name a built-in preset of their generator by
/// key, or give settings laid over one (`{ base: boulder, scale: 2.0 }`) or
/// over the generator's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct EcotopePreset {
    /// The preset's key, such as `"pine_forest_floor"`, or a loaded file's
    /// name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Pine forest floor"`.
    pub name: Cow<'static, str>,
    /// What the Ecotope looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Forest"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The Ecotope itself.
    pub ecotope: Ecotope,
}

/// The Ecotope of an embedded preset file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded(key: &str, text: &str) -> Ecotope {
    procgena_core::preset::read::<EcotopePreset>(key, text)
        .unwrap_or_else(|error| {
            panic!("built-in preset {key} is not a valid Ecotope file: {error}")
        })
        .ecotope
}

/// Reads the embedded file of a built-in preset in `ecotopes/`.
macro_rules! ecotope_file {
    ($key:ident) => {
        embedded(
            stringify!($key),
            include_str!(concat!("../../ecotopes/", stringify!($key), ".ecotope.yml")),
        )
    };
}

procgena_core::catalogue! {
    EcotopePreset {
        ecotope: Ecotope,
        check: |ecotope| ecotope.validate().map_err(|error| error.to_string()),
    }
    "Forest" {
        pine_forest_floor {
            name: "Pine forest floor",
            description: "Pines of three sizes over boulders, mossy fallen logs, porcini and a \
                few fly agarics.",
        } => ecotope_file!(pine_forest_floor);
    }
}
