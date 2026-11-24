//! Ready-made tree Scatters, each an embedded `.tree_scatter.yml` file in the
//! crate's `scatters` folder.

use std::borrow::Cow;

use super::TreeScatter;

/// A named, ready-made [`TreeScatter`]: a grove, a woodland, an orchard or a
/// seasonal mix.
///
/// Every built-in preset has a function of its own, such as
/// [`TreeScatterPreset::pine_stand`]. Clone its
/// [`tree_scatter`](Self::tree_scatter) to customise it. A
/// `.tree_scatter.yml` file loads with [`TreeScatterPreset::from_yaml`]; its
/// trees name [`TreePreset`](crate::TreePreset)s by key, or give their own
/// settings.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeScatterPreset {
    /// The preset's key, such as `"pine_stand"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Pine Stand"`.
    pub name: Cow<'static, str>,
    /// What the Scatter looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Woodland"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The Scatter itself.
    pub tree_scatter: TreeScatter,
}

/// The Scatter of an embedded preset file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded(key: &str, text: &str) -> TreeScatter {
    procgena_core::preset::read::<TreeScatterPreset>(key, text)
        .unwrap_or_else(|error| {
            panic!("built-in preset {key} is not a valid tree Scatter file: {error}")
        })
        .tree_scatter
}

/// Reads the embedded file of a built-in preset in `scatters/`.
macro_rules! scatter_file {
    ($key:ident) => {
        embedded(
            stringify!($key),
            include_str!(concat!(
                "../../scatters/",
                stringify!($key),
                ".tree_scatter.yml"
            )),
        )
    };
}

procgena_core::catalogue! {
    TreeScatterPreset {
        tree_scatter: TreeScatter,
        check: |scatter| scatter.validate().map_err(|error| error.to_string()),
    }
    "Woodland" {
        oak_woodland {
            name: "Oak Woodland",
            description: "Medium oaks with a few large ones, small stands of ash and scattered \
                bushes.",
        } => scatter_file!(oak_woodland);
        aspen_stand {
            name: "Aspen Stand",
            description: "Aspens of three sizes, the young ones in dense clumps.",
        } => scatter_file!(aspen_stand);
        mixed_deciduous {
            name: "Mixed Deciduous",
            description: "Big-leaf maple, ash, aspen and oak, each in loose stands.",
        } => scatter_file!(mixed_deciduous);
    }
    "Conifer" {
        pine_stand {
            name: "Pine Stand",
            description: "Pines of three sizes, the young ones in dense clumps.",
        } => scatter_file!(pine_stand);
        cedar_grove {
            name: "Cedar Grove",
            description: "Cedars with a few pines and clumps of evergreen bushes.",
        } => scatter_file!(cedar_grove);
    }
    "Cultivated" {
        orchard {
            name: "Orchard",
            description: "Apple trees spread evenly, standing straight and of similar size.",
        } => scatter_file!(orchard);
    }
    "Seasonal" {
        spring_blossom {
            name: "Spring Blossom",
            description: "Flowering dogwoods with clumps of pussy willow in catkin.",
        } => scatter_file!(spring_blossom);
        winter_copse {
            name: "Winter Copse",
            description: "Leafless winter thickets with a few pussy willows.",
        } => scatter_file!(winter_copse);
    }
}
