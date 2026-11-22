//! Texture sets for rock surfaces and the file paths of their images.
//!
//! Choose a set with [`MaterialOptions::texture`](crate::MaterialOptions::texture),
//! then use [`texture_path`] to find its KTX2 images for rendering, or
//! [`image_path`] to find its JPEG images for exported files. The repository's
//! texture folder is `assets/textures/`; download its files with
//! `tools/textures/download.sh`. `assets/textures/assets_manifest.json` and
//! `assets/textures/images/images_manifest.json` list every file with its
//! download address and checksum.

use serde::{Deserialize, Serialize};

/// Texture sets available in the repository's texture folder.
///
/// Use these names for [`MaterialOptions::texture`](crate::MaterialOptions::texture).
/// Each set includes all rendering maps ([`TextureMap`]) and export images
/// ([`ImageMap`]) in every [`TextureSize`].
pub const TEXTURE_SETS: [&str; 23] = [
    "rock_face_03",
    "rock_boulder_dry",
    "rock_01",
    "lichen_rock_02",
    "rock_05",
    "rock_08",
    "rock_04",
    "rock_06",
    "rock_pitted_mossy",
    "lichen_rock",
    "rock_boulder_cracked",
    "worn_rock_natural_01",
    "seaside_rock",
    "rock_surface",
    "rock_2",
    "rock_3",
    "rock_face",
    "marble_rock_01",
    "dark_rock",
    "metal",
    "amethyst_1",
    "rock_ore_002",
    "ice",
];

/// Texture image resolution. Images are square.
///
/// JSON names match [`as_str`](Self::as_str): `"512"`, `"1k"` and `"2k"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextureSize {
    /// 512 × 512 pixels: the least memory, for distant rocks.
    #[serde(rename = "512")]
    Size512,
    /// 1024 × 1024 pixels.
    #[serde(rename = "1k")]
    Size1k,
    /// 2048 × 2048 pixels: the most detail, for close-up views.
    #[serde(rename = "2k")]
    Size2k,
}

impl TextureSize {
    /// Every size, smallest first.
    pub const ALL: [TextureSize; 3] = [Self::Size512, Self::Size1k, Self::Size2k];

    /// The size as written in texture paths: `"512"`, `"1k"` or `"2k"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Size512 => "512",
            Self::Size1k => "1k",
            Self::Size2k => "2k",
        }
    }
}

/// GPU compression format of a texture file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureCompression {
    /// BC7, read directly by GPUs that support it, including most desktop GPUs.
    #[default]
    Bc7,
    /// Basis Universal UASTC. The loader converts it to a format the GPU supports;
    /// in Bevy this needs the `basis-universal` feature.
    Uastc,
}

impl TextureCompression {
    /// Both formats.
    pub const ALL: [TextureCompression; 2] = [Self::Bc7, Self::Uastc];

    /// The format as written in texture paths: `"bc7"` or `"uastc"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bc7 => "bc7",
            Self::Uastc => "uastc",
        }
    }
}

/// Selects a KTX2 image for rendering a rock. Use [`texture_path`] to get its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureMap {
    /// The rock's surface colour. Load this image as sRGB colour.
    Color,
    /// Adds the appearance of small bumps without changing the mesh.
    /// Use OpenGL (+Y) tangent-space normal map settings and load as linear
    /// data, without sRGB colour correction.
    Normal,
    /// Combines three surface properties in one image. Load as linear data:
    /// - red: ambient occlusion; darker values reduce light in cracks and hollows;
    /// - green: roughness; dark areas look polished and light areas look matte;
    /// - blue: height; lighter areas are raised and help blend projected textures.
    ///
    /// Height uses a relative 0 to 1 scale for each set, not a physical distance.
    Orh,
}

impl TextureMap {
    /// All rendering maps: colour, normal, then occlusion/roughness/height.
    pub const ALL: [TextureMap; 3] = [Self::Color, Self::Normal, Self::Orh];

    /// Returns the map's filename label: `"color"`, `"normal"` or `"orh"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
            Self::Orh => "orh",
        }
    }

    /// Whether the image holds sRGB colour rather than linear data.
    pub fn is_srgb(self) -> bool {
        self == Self::Color
    }
}

/// Selects a JPEG image for an exported model. Use [`image_path`] to get its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageMap {
    /// The rock's surface colour. Supply an sRGB image.
    Color,
    /// Adds the appearance of small bumps without changing the mesh.
    /// Supply an OpenGL (+Y) tangent-space normal map and treat it as linear
    /// data, without sRGB colour correction. Some OBJ importers need manual
    /// setup to recognise it as a normal map.
    Normal,
    /// Combines three surface properties in the layout used by glTF.
    /// Supply linear data, without sRGB colour correction:
    /// - red: ambient occlusion; darker values reduce light in cracks and hollows;
    /// - green: roughness; dark areas look polished and light areas look matte;
    /// - blue: metalness; zero for rock. glTF exports multiply it by
    ///   [`MaterialOptions::metallic`](crate::MaterialOptions::metallic).
    ///
    /// OBJ exports use only the green channel for roughness. Some importers
    /// do not support selecting this channel.
    Orm,
}

impl ImageMap {
    /// All export images: colour, normal, then occlusion/roughness/metalness.
    pub const ALL: [ImageMap; 3] = [Self::Color, Self::Normal, Self::Orm];

    /// Returns the map's filename label: `"color"`, `"normal"` or `"orm"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
            Self::Orm => "orm",
        }
    }

    /// Returns `true` for colour images. Load other maps as linear data,
    /// without sRGB colour correction.
    pub fn is_srgb(self) -> bool {
        self == Self::Color
    }
}

/// Returns the path of a texture image relative to the texture folder, such as
/// `"rock/rock_01-1k-color-bc7.ktx2"`.
///
/// Pass a name from [`TEXTURE_SETS`] as `set`, or the name of a set you added
/// to the texture folder with the same naming. The path is not checked.
pub fn texture_path(
    set: &str,
    size: TextureSize,
    map: TextureMap,
    compression: TextureCompression,
) -> String {
    format!(
        "rock/{set}-{}-{}-{}.ktx2",
        size.as_str(),
        map.as_str(),
        compression.as_str()
    )
}

/// Returns the path of a JPEG texture image relative to the image folder, such
/// as `"rock/rock_01-1k-color.jpg"`. Use these images where KTX2 files are not
/// supported, such as in exported OBJ and glTF files.
///
/// The repository's image folder is `assets/textures/images/`; download its
/// files with `tools/textures/download.sh --images`. Pass a set name as for
/// [`texture_path`]. The path is not checked.
pub fn image_path(set: &str, size: TextureSize, map: ImageMap) -> String {
    format!("rock/{set}-{}-{}.jpg", size.as_str(), map.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_the_repository_layout() {
        assert_eq!(
            texture_path(
                "rock_01",
                TextureSize::Size1k,
                TextureMap::Color,
                TextureCompression::Bc7
            ),
            "rock/rock_01-1k-color-bc7.ktx2"
        );
        assert_eq!(
            texture_path(
                "lichen_rock_02",
                TextureSize::Size512,
                TextureMap::Orh,
                TextureCompression::Uastc
            ),
            "rock/lichen_rock_02-512-orh-uastc.ktx2"
        );
        assert_eq!(
            image_path("rock_01", TextureSize::Size2k, ImageMap::Normal),
            "rock/rock_01-2k-normal.jpg"
        );
        assert_eq!(
            image_path("rock_01", TextureSize::Size512, ImageMap::Orm),
            "rock/rock_01-512-orm.jpg"
        );
    }

    #[test]
    fn set_names_match_the_fetch_list() {
        let list = include_str!("../../../tools/textures/sets.txt");
        let names: Vec<&str> = list
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert_eq!(names, TEXTURE_SETS);
    }
}
