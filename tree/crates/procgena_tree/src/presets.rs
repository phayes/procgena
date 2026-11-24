//! Ready-made tree settings covering several species and sizes: the presets
//! from ez-tree, plus trees that grow fruit or flowers.
//!
//! [`TreePreset::all`] lists the built-in presets in menu order, and
//! [`TreePreset::named`] looks one up by its key. Each built-in preset is a
//! `.tree.yml` file in the crate's `presets` folder, embedded in the crate; a
//! file of your own loads with [`TreePreset::from_yaml`].

use std::borrow::Cow;

use crate::options::TreeOptions;

/// Steepest ground shrub and sapling presets grow on in a Scatter, in degrees.
const SHRUB_MAX_SLOPE: f32 = 45.0;

/// A named, ready-made tree.
///
/// Every built-in preset has a function of its own, such as
/// [`TreePreset::oak_small`]. Clone its [`tree`](Self::tree) options to
/// customise them.
#[derive(Debug, Clone, PartialEq)]
pub struct TreePreset {
    /// The preset's key, such as `"oak_small"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Oak Small"`.
    pub name: Cow<'static, str>,
    /// What the tree looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Oak"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The tree's settings.
    pub tree: TreeOptions,
    /// How densely and how far apart the tree grows in a
    /// [`TreeScatter`](crate::TreeScatter) of its own kind. `None` when the
    /// preset suggests nothing, such as a file read without a `base`; a
    /// file with a `base` takes its base's stand.
    pub stand: Option<TreeStand>,
}

/// How densely a tree preset grows in a [`TreeScatter`](crate::TreeScatter)
/// of its own kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeStand {
    /// Trees per hectare (10,000 m²): about one tree for each crown's area,
    /// so the canopy closes. Spacing then leaves out a few of these, about
    /// one in six. 0 for a tree that doesn't suit a Scatter, such as the
    /// trellis.
    pub density: f32,
    /// The smallest distance between this tree's trunk and a neighbour's,
    /// in metres: about half its crown's radius, so neighbouring crowns can
    /// interlock but trunks never crowd.
    pub spacing: f32,
    /// The steepest ground the tree grows on, in degrees, when it tolerates
    /// steeper ground than the Scatter's own
    /// [`max_slope`](crate::TreeScatterVariation::max_slope): 45° for shrubs
    /// and saplings, `None` for everything else.
    pub max_slope: Option<f32>,
}

/// A stand from a tree's widest branch or leaf `r` from the trunk: a
/// density of 10,000 / (π r²) and a spacing of 0.55 r, rounded.
const fn stand(density: f32, spacing: f32) -> Option<TreeStand> {
    Some(TreeStand {
        density,
        spacing,
        max_slope: None,
    })
}

/// A stand for shrubs and saplings, which grow on steeper ground.
const fn shrub(density: f32, spacing: f32) -> Option<TreeStand> {
    Some(TreeStand {
        density,
        spacing,
        max_slope: Some(SHRUB_MAX_SLOPE),
    })
}

/// The options of an embedded preset file, which names no `base`.
///
/// # Panics
///
/// Panics if the file can't be read; a test checks every embedded file.
fn embedded(key: &str, text: &str) -> TreeOptions {
    procgena_core::preset::read::<TreePreset>(key, text)
        .unwrap_or_else(|error| panic!("built-in preset {key} is not a valid tree file: {error}"))
        .tree
}

/// Declares the catalogue from the embedded files in `presets/`.
macro_rules! tree_file {
    ($key:ident) => {
        embedded(
            stringify!($key),
            include_str!(concat!("../presets/", stringify!($key), ".tree.yml")),
        )
    };
}

procgena_core::catalogue! {
    TreePreset {
        tree: TreeOptions,
        inherit: [stand],
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Ash" {
        ash_small {
            name: "Ash Small",
            description: "A young ash with a light, open crown.",
            stand: stand(510.0, 1.4),
        } => tree_file!(ash_small);
        ash_medium {
            name: "Ash Medium",
            description: "A medium ash with a rounded crown.",
            stand: stand(260.0, 1.9),
        } => tree_file!(ash_medium);
        ash_large {
            name: "Ash Large",
            description: "A tall, mature ash with a broad crown.",
            stand: stand(160.0, 2.5),
        } => tree_file!(ash_large);
    }
    "Aspen" {
        aspen_small {
            name: "Aspen Small",
            description: "A young aspen sapling.",
            stand: shrub(2600.0, 0.6),
        } => tree_file!(aspen_small);
        aspen_medium {
            name: "Aspen Medium",
            description: "A slender aspen with a narrow crown.",
            stand: stand(980.0, 1.0),
        } => tree_file!(aspen_medium);
        aspen_large {
            name: "Aspen Large",
            description: "A tall aspen with a narrow crown.",
            stand: stand(550.0, 1.3),
        } => tree_file!(aspen_large);
    }
    "Bushes" {
        bush_1 {
            name: "Bush 1",
            description: "A low, rounded bush.",
            stand: shrub(880.0, 1.05),
        } => tree_file!(bush_1);
        bush_2 {
            name: "Bush 2",
            description: "A dense, compact bush.",
            stand: shrub(1100.0, 0.95),
        } => tree_file!(bush_2);
        bush_3 {
            name: "Bush 3",
            description: "A loose, spreading bush.",
            stand: shrub(800.0, 1.1),
        } => tree_file!(bush_3);
    }
    "Oak" {
        oak_small {
            name: "Oak Small",
            description: "A young oak with a rounded crown.",
            stand: stand(720.0, 1.15),
        } => tree_file!(oak_small);
        oak_medium {
            name: "Oak Medium",
            description: "A medium oak with spreading branches.",
            stand: stand(400.0, 1.55),
        } => tree_file!(oak_medium);
        oak_large {
            name: "Oak Large",
            description: "A broad, mature oak with a wide crown.",
            stand: stand(130.0, 2.7),
        } => tree_file!(oak_large);
    }
    "Pine" {
        pine_small {
            name: "Pine Small",
            description: "A young pine sapling.",
            stand: shrub(2600.0, 0.6),
        } => tree_file!(pine_small);
        pine_medium {
            name: "Pine Medium",
            description: "A medium pine with a conical crown.",
            stand: stand(1100.0, 0.95),
        } => tree_file!(pine_medium);
        pine_large {
            name: "Pine Large",
            description: "A tall pine with a conical crown.",
            stand: stand(550.0, 1.3),
        } => tree_file!(pine_large);
    }
    "Garden" {
        trellis {
            name: "Trellis",
            description: "A broadleaf tree configured to grow along a grid of guide wires. It \
                suits a garden rather than a Scatter, so its Scatter density is 0.",
            stand: stand(0.0, 0.8),
        } => tree_file!(trellis);
    }
    "Maple" {
        big_leaf_maple_medium {
            name: "Big Leaf Maple Medium",
            description: "A single-trunked bigleaf maple of moderate height with a broad \
                crown of large autumn-coloured leaves.",
            stand: stand(310.0, 1.75),
        } => tree_file!(big_leaf_maple_medium);
        big_leaf_maple_large {
            name: "Big Leaf Maple Large",
            description: "A tall, many-stemmed bigleaf maple with a broad crown of large \
                autumn-coloured leaves.",
            stand: stand(140.0, 2.6),
        } => tree_file!(big_leaf_maple_large);
    }
    "Fruit and Blossom" {
        apple {
            name: "Apple",
            description: "A short, spreading apple tree hung with ripe apples.",
            stand: stand(650.0, 1.2),
        } => tree_file!(apple);
        flowering_dogwood {
            name: "Flowering Dogwood",
            description: "A low-forking flowering dogwood in full bloom, covered in white, \
                four-bracted flowers that face the sky.",
            stand: stand(470.0, 1.45),
        } => tree_file!(flowering_dogwood);
        spring_pussy_willow {
            name: "Spring Pussy Willow",
            description: "A many-stemmed pussy willow shrub in early spring: bare upright \
                stems studded with fuzzy, upward-pointing catkins.",
            stand: shrub(2600.0, 0.6),
        } => tree_file!(spring_pussy_willow);
    }
    "Winter" {
        winter_thicket {
            name: "Winter Thicket",
            description: "A dense, many-stemmed willow thicket in winter: bare upright stems \
                with no leaves, fruit or flowers.",
            stand: stand(1250.0, 0.9),
        } => tree_file!(winter_thicket);
    }
    "Conifer" {
        cedar {
            name: "Cedar",
            description: "A tall, narrow cedar with a single straight trunk and layered \
                branches carrying flat, horizontal sprays of green foliage.",
            stand: stand(480.0, 1.4),
        } => tree_file!(cedar);
    }
}

impl TreePreset {
    /// The 13 presets whose settings match ez-tree's own presets, in ez-tree
    /// display order. They grow no fruit or flowers. Their shapes differ from
    /// ez-tree's, since this crate draws its own random numbers.
    pub fn ez_tree() -> impl Iterator<Item = &'static Self> {
        const KEYS: [&str; 13] = [
            "ash_small",
            "ash_medium",
            "ash_large",
            "aspen_small",
            "aspen_medium",
            "aspen_large",
            "bush_1",
            "bush_2",
            "bush_3",
            "pine_small",
            "pine_medium",
            "pine_large",
            "trellis",
        ];
        KEYS.into_iter()
            .map(|key| Self::named(key).expect("every ez-tree preset is in the catalogue"))
    }
}
