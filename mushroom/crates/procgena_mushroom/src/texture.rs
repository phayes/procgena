//! Names and file paths of the optional cap texture sets.
//!
//! Most mushrooms use no textures: their colours are vertex colours. A
//! preset can name a cap texture set in [`MaterialOptions::cap_texture`];
//! renderers then load that set's images, which are made with the
//! repository's `textures` tool (see `mushroom/textures.yml`).
//!
//! [`MaterialOptions::cap_texture`]: crate::MaterialOptions::cap_texture

use serde::{Deserialize, Serialize};

/// Every cap texture set, by the name used in
/// [`MaterialOptions::cap_texture`](crate::MaterialOptions::cap_texture).
/// Each comes with every [`TextureMap`] in every [`TextureSize`].
pub const CAP_TEXTURE_SETS: [&str; 1] = [
    // A fly agaric cap: red with white warts (3dtextures.me, CC0).
    "mushroom_top_001",
];

/// Width and height of a texture image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextureSize {
    /// 512 × 512 pixels: the least memory.
    #[serde(rename = "512")]
    Size512,
    /// 1024 × 1024 pixels.
    #[default]
    #[serde(rename = "1k")]
    Size1k,
    /// 2048 × 2048 pixels.
    #[serde(rename = "2k")]
    Size2k,
}

impl TextureSize {
    /// Every size, smallest first.
    pub const ALL: [TextureSize; 3] = [Self::Size512, Self::Size1k, Self::Size2k];

    /// Returns the size as written in texture paths: `"512"`, `"1k"` or
    /// `"2k"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Size512 => "512",
            Self::Size1k => "1k",
            Self::Size2k => "2k",
        }
    }
}

/// GPU compression format of a texture file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextureCompression {
    /// BC7, read directly by most desktop GPUs.
    #[default]
    Bc7,
    /// Basis Universal UASTC. The loader converts it to a format the GPU
    /// supports; in Bevy this needs the `basis-universal` feature.
    Uastc,
}

impl TextureCompression {
    /// Both formats.
    pub const ALL: [TextureCompression; 2] = [Self::Bc7, Self::Uastc];

    /// Returns the format as written in texture paths: `"bc7"` or `"uastc"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bc7 => "bc7",
            Self::Uastc => "uastc",
        }
    }
}

/// One image of a texture set. Use [`texture_path`] to get its file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureMap {
    /// The surface colour. Load this image as sRGB colour.
    Color,
    /// Small bumps, as a tangent-space normal map in the OpenGL (+Y)
    /// convention. Load as linear data.
    Normal,
    /// Three surface properties in one image, loaded as linear data: ambient
    /// occlusion in red (darker means less light reaches hollows), roughness
    /// in green (darker is shinier) and height in blue (lighter is raised).
    Orh,
}

impl TextureMap {
    /// Every map.
    pub const ALL: [TextureMap; 3] = [Self::Color, Self::Normal, Self::Orh];

    /// Returns the map as written in texture paths: `"color"`, `"normal"`
    /// or `"orh"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
            Self::Orh => "orh",
        }
    }

    /// Returns true for maps that hold colour and must be loaded as sRGB.
    pub fn is_srgb(self) -> bool {
        self == Self::Color
    }
}

/// Returns the path of one KTX2 texture file relative to the texture folder,
/// such as `"cap/mushroom_top_001-1k-color-bc7.ktx2"`.
///
/// The folder is `mushroom/assets/textures/` in the repository, filled by
/// `uv run --project tools textures download mushroom`. `set` should be one of
/// [`CAP_TEXTURE_SETS`]; other names give paths to files that don't exist.
pub fn texture_path(
    set: &str,
    size: TextureSize,
    map: TextureMap,
    compression: TextureCompression,
) -> String {
    format!(
        "cap/{set}-{}-{}-{}.ktx2",
        size.as_str(),
        map.as_str(),
        compression.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_the_repository_layout() {
        assert_eq!(
            texture_path(
                "mushroom_top_001",
                TextureSize::Size1k,
                TextureMap::Color,
                TextureCompression::Bc7
            ),
            "cap/mushroom_top_001-1k-color-bc7.ktx2"
        );
        assert_eq!(
            texture_path(
                "mushroom_top_001",
                TextureSize::Size512,
                TextureMap::Orh,
                TextureCompression::Uastc
            ),
            "cap/mushroom_top_001-512-orh-uastc.ktx2"
        );
    }

    #[test]
    fn options_accept_only_listed_sets() {
        let mut options = crate::MushroomPreset::FlyAgaric.options();
        assert!(options.validate().is_ok());
        options.material.cap_texture = Some("no_such_set".into());
        assert!(matches!(
            options.validate(),
            Err(crate::OptionsError::UnknownTexture {
                field: "material.cap_texture",
                ..
            })
        ));
    }

    #[test]
    #[ignore = "needs `uv run --project tools textures download mushroom` first"]
    fn every_listed_file_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        for set in CAP_TEXTURE_SETS {
            for size in TextureSize::ALL {
                for map in TextureMap::ALL {
                    for compression in TextureCompression::ALL {
                        let path = root.join(texture_path(set, size, map, compression));
                        assert!(path.exists(), "missing {}", path.display());
                    }
                }
            }
        }
    }
}
