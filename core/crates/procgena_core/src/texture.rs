//! Texture lists: which tiling texture sets a crate ships, in which sizes,
//! and where each file lives.
//!
//! Every crate with textures lists them in a `textures.yml` beside its
//! crates (`terrain/textures.yml`, `stone/textures.yml` and so on). The
//! `textures` tool (`uv run --project tools textures`) reads the same file
//! to download, convert and publish the files. [`TextureList::from_yaml`]
//! reads one list; a [`TextureCatalogue`] holds several, by name.
//!
//! The files are KTX2 GPU textures named
//! `<category>/<id>-<size>-<map>-<format>.ktx2`, relative to the folder the
//! list's textures are installed in.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

/// The side length of a texture file, in pixels. Larger sizes look sharper
/// close up and use more memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum TextureSize {
    /// 128 × 128 pixels.
    #[serde(rename = "128")]
    Size128,
    /// 256 × 256 pixels.
    #[serde(rename = "256")]
    Size256,
    /// 512 × 512 pixels.
    #[serde(rename = "512")]
    Size512,
    /// 1024 × 1024 pixels.
    #[serde(rename = "1k")]
    Size1k,
    /// 2048 × 2048 pixels.
    #[serde(rename = "2k")]
    Size2k,
}

impl TextureSize {
    /// Every size, smallest first.
    pub const ALL: [TextureSize; 5] = [
        Self::Size128,
        Self::Size256,
        Self::Size512,
        Self::Size1k,
        Self::Size2k,
    ];

    /// The size's name in file names and `textures.yml`: `128`, `256`,
    /// `512`, `1k` or `2k`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Size128 => "128",
            Self::Size256 => "256",
            Self::Size512 => "512",
            Self::Size1k => "1k",
            Self::Size2k => "2k",
        }
    }

    /// The side length in pixels.
    pub const fn pixels(self) -> u32 {
        match self {
            Self::Size128 => 128,
            Self::Size256 => 256,
            Self::Size512 => 512,
            Self::Size1k => 1024,
            Self::Size2k => 2048,
        }
    }
}

impl fmt::Display for TextureSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a texture file is compressed for the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureCompression {
    /// BC7, read directly by most desktop GPUs.
    #[default]
    Bc7,
    /// Basis Universal UASTC, converted at load time for the GPU in use
    /// (in Bevy, this needs the `basis-universal` feature).
    Uastc,
}

impl TextureCompression {
    /// The compression's name in file names: `bc7` or `uastc`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bc7 => "bc7",
            Self::Uastc => "uastc",
        }
    }
}

/// Which maps a list's texture sets have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureLayout {
    /// `color`, `normal` and `orh`: ambient occlusion, roughness and height
    /// packed into red, green and blue.
    #[default]
    Orh,
    /// `color`, `normal` and `roughness`.
    Roughness,
}

impl TextureLayout {
    /// The map names in file names, in the order colour, normal, then the
    /// surface map.
    pub const fn maps(self) -> [&'static str; 3] {
        match self {
            Self::Orh => ["color", "normal", "orh"],
            Self::Roughness => ["color", "normal", "roughness"],
        }
    }
}

/// One tiling texture set in a [`TextureList`].
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSet {
    /// The set's id, which names its files.
    pub id: String,
    /// The folder its files are in.
    pub category: String,
    /// The colour image's average, as linear RGB from 0 to 1, if the list
    /// gives one (`uv run --project tools textures average-colors <project>`
    /// prints it).
    pub average: Option<[f32; 3]>,
}

/// The texture sets one crate ships, read from its `textures.yml`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureList {
    layout: TextureLayout,
    sizes: Vec<TextureSize>,
    sets: Vec<TextureSet>,
}

/// The sizes a `textures.yml` gets when it names none.
const DEFAULT_SIZES: [TextureSize; 3] = [
    TextureSize::Size512,
    TextureSize::Size1k,
    TextureSize::Size2k,
];

/// The parts of a `textures.yml` a [`TextureList`] reads. Other settings
/// are for the `textures` tool and are skipped.
#[derive(Deserialize)]
struct ListFile {
    #[serde(default)]
    layout: TextureLayout,
    #[serde(default)]
    sizes: Option<Vec<TextureSize>>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    sets: Vec<SetEntry>,
}

#[derive(Deserialize)]
struct SetEntry {
    id: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    average: Option<[f32; 3]>,
}

impl TextureList {
    /// Reads a `textures.yml`.
    ///
    /// # Errors
    ///
    /// [`TextureListError`] if the text isn't a `textures.yml`, or a set
    /// has no category.
    pub fn from_yaml(text: &str) -> Result<Self, TextureListError> {
        let file: ListFile = serde_saphyr::from_str(text)
            .map_err(|error| TextureListError(error.to_string()))?;
        let mut sizes = file.sizes.unwrap_or_else(|| DEFAULT_SIZES.to_vec());
        sizes.sort();
        sizes.dedup();
        let sets = file
            .sets
            .into_iter()
            .map(|entry| {
                let category = entry
                    .category
                    .or_else(|| file.category.clone())
                    .ok_or_else(|| TextureListError(format!("set {} has no category", entry.id)))?;
                Ok(TextureSet {
                    id: entry.id,
                    category,
                    average: entry.average,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            layout: file.layout,
            sizes,
            sets,
        })
    }

    /// Which maps every set has.
    pub fn layout(&self) -> TextureLayout {
        self.layout
    }

    /// The sizes every set comes in, smallest first.
    pub fn sizes(&self) -> &[TextureSize] {
        &self.sizes
    }

    /// Every set, in the file's order.
    pub fn sets(&self) -> &[TextureSet] {
        &self.sets
    }

    /// The set with id `id`, if the list has one.
    pub fn set(&self, id: &str) -> Option<&TextureSet> {
        self.sets.iter().find(|set| set.id == id)
    }

    /// The size for detail level `level`: level 0 is the largest size, each
    /// level after it the next smaller one, and levels past the smallest
    /// size get the smallest.
    ///
    /// # Panics
    ///
    /// If the list has no sizes.
    pub fn size_for_level(&self, level: u8) -> TextureSize {
        let largest_first = self.sizes.iter().rev();
        let index = usize::from(level).min(self.sizes.len() - 1);
        *largest_first
            .clone()
            .nth(index)
            .expect("a texture list has at least one size")
    }

    /// The path of one file of set `id`, relative to the folder the list's
    /// textures are installed in, or `None` if the list has no such set.
    /// `map` is one of [`TextureLayout::maps`].
    pub fn path(
        &self,
        id: &str,
        size: TextureSize,
        map: &str,
        compression: TextureCompression,
    ) -> Option<String> {
        let set = self.set(id)?;
        Some(format!(
            "{}/{}-{}-{}-{}.ktx2",
            set.category,
            set.id,
            size.name(),
            map,
            compression.name()
        ))
    }
}

/// A `textures.yml` that couldn't be read, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureListError(pub String);

impl fmt::Display for TextureListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "texture list: {}", self.0)
    }
}

impl std::error::Error for TextureListError {}

/// Several [`TextureList`]s, each under a name such as `terrain` or
/// `stone`, so one place can list every texture set a program can load.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureCatalogue {
    lists: BTreeMap<String, TextureList>,
}

impl TextureCatalogue {
    /// An empty catalogue.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same catalogue with `list` under `name`, replacing any list
    /// already there.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, list: TextureList) -> Self {
        self.lists.insert(name.into(), list);
        self
    }

    /// The list named `name`, if there is one.
    pub fn list(&self, name: &str) -> Option<&TextureList> {
        self.lists.get(name)
    }

    /// Every list with its name, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TextureList)> {
        self.lists.iter().map(|(name, list)| (name.as_str(), list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"
bucket: terrain
layout: orh
sizes: ["1k", "128", "512"]
category: misc
sets:
  - {id: mud_forest, category: soil}
  - {id: moss_002, source: ambientcg, source_id: Moss002, average: [0.1, 0.2, 0.3]}
"#;

    #[test]
    fn a_list_reads_its_sets_and_sorts_its_sizes() {
        let list = TextureList::from_yaml(LIST).unwrap();
        assert_eq!(
            list.sizes(),
            [TextureSize::Size128, TextureSize::Size512, TextureSize::Size1k]
        );
        assert_eq!(list.set("moss_002").unwrap().category, "misc");
        assert_eq!(list.set("moss_002").unwrap().average, Some([0.1, 0.2, 0.3]));
        assert_eq!(list.set("mud_forest").unwrap().average, None);
        assert_eq!(
            list.path("mud_forest", TextureSize::Size512, "orh", TextureCompression::Bc7)
                .as_deref(),
            Some("soil/mud_forest-512-orh-bc7.ktx2")
        );
        assert_eq!(list.path("absent", TextureSize::Size512, "orh", TextureCompression::Bc7), None);
    }

    #[test]
    fn detail_levels_step_down_from_the_largest_size() {
        let list = TextureList::from_yaml(LIST).unwrap();
        let sizes: Vec<_> = (0..4).map(|level| list.size_for_level(level)).collect();
        assert_eq!(
            sizes,
            [
                TextureSize::Size1k,
                TextureSize::Size512,
                TextureSize::Size128,
                TextureSize::Size128
            ]
        );
    }

    #[test]
    fn a_list_without_sizes_gets_the_usual_three() {
        let list = TextureList::from_yaml("sets: [{id: a, category: b}]").unwrap();
        assert_eq!(list.sizes(), DEFAULT_SIZES);
        assert!(TextureList::from_yaml("sets: [{id: a}]").is_err());
    }

    #[test]
    fn every_crate_list_in_the_workspace_reads() {
        for name in ["terrain", "stone", "tree", "clump", "deadwood", "mushroom"] {
            let path = format!("{}/../../../{name}/textures.yml", env!("CARGO_MANIFEST_DIR"));
            let text = std::fs::read_to_string(&path).unwrap();
            let list = TextureList::from_yaml(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!list.sets().is_empty(), "{name}");
        }
    }
}
