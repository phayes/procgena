//! How a clump looks: its colour, roughness and texture images.

/// A set of texture images for clumps, such as one kind of snow.
///
/// Textures are not stored in the repository. Download them with
/// `uv run --project tools textures download clump`, then find each image
/// with [`TextureSet::path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureSet {
    /// The set's name, as used in [`MaterialOptions::texture`].
    pub name: &'static str,
    /// The kind of material, such as `"snow"`. Also the folder the set's
    /// images are stored in.
    pub category: &'static str,
}

impl TextureSet {
    /// Returns the path of one of this set's KTX2 images, relative to the
    /// texture folder, such as `"snow/snow_02-1k-color-bc7.ktx2"`.
    pub fn path(
        &self,
        size: TextureSize,
        map: TextureMap,
        compression: TextureCompression,
    ) -> String {
        format!(
            "{}/{}-{}-{}-{}.ktx2",
            self.category,
            self.name,
            size.as_str(),
            map.as_str(),
            compression.as_str()
        )
    }
}

/// Every texture set available for clumps.
pub const TEXTURE_SETS: [TextureSet; 9] = [
    TextureSet {
        name: "snow_02",
        category: "snow",
    },
    TextureSet {
        name: "snow_005",
        category: "snow",
    },
    TextureSet {
        name: "snow_014",
        category: "snow",
    },
    TextureSet {
        name: "moss_002",
        category: "moss",
    },
    TextureSet {
        name: "moss_003",
        category: "moss",
    },
    TextureSet {
        name: "moss_004",
        category: "moss",
    },
    TextureSet {
        name: "leaves_dry_decay",
        category: "leaves",
    },
    TextureSet {
        name: "leaves_forest_ground",
        category: "leaves",
    },
    TextureSet {
        name: "leaves_forest_03",
        category: "leaves",
    },
];

/// Returns the texture set called `name`, or `None` if there is none.
pub fn texture_set(name: &str) -> Option<&'static TextureSet> {
    TEXTURE_SETS.iter().find(|set| set.name == name)
}

/// Width and height of a texture image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureSize {
    /// 512 × 512 pixels: the least memory, for distant clumps.
    Size512,
    /// 1024 × 1024 pixels.
    #[default]
    Size1k,
    /// 2048 × 2048 pixels: the most detail, for close-up views.
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

/// How a texture image file is compressed.
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

/// One of the images in a [`TextureSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureMap {
    /// The surface colour. Load this image as sRGB colour.
    Color,
    /// Small bumps, without changing the mesh. Uses OpenGL (+Y) directions;
    /// load as linear data.
    Normal,
    /// Three surface properties in one image; load as linear data:
    /// - red: ambient occlusion, which darkens hollows;
    /// - green: roughness, where light areas look matte;
    /// - blue: height from 0 to 1, where light areas are raised.
    Orh,
}

impl TextureMap {
    /// All maps: colour, normal, then occlusion/roughness/height.
    pub const ALL: [TextureMap; 3] = [Self::Color, Self::Normal, Self::Orh];

    /// The map as written in texture paths: `"color"`, `"normal"` or `"orh"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
            Self::Orh => "orh",
        }
    }
}

/// How a clump looks.
///
/// Clump meshes have no texture coordinates; renderers project the textures
/// onto the clump along its three axes.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialOptions {
    /// Name of a [`TextureSet`] from [`TEXTURE_SETS`], or `None` for a plain
    /// colour.
    pub texture: Option<String>,
    /// Linear RGB colour that multiplies the colour texture, with each
    /// channel 0 or more. Without a texture this is the clump's colour.
    pub color: [f32; 3],
    /// Roughness from 0 (glossy) to 1 (fully matte). Multiplies the
    /// roughness texture; without a texture this is the clump's roughness.
    pub roughness: f32,
    /// Metres covered by one repeat of the texture; must be greater than 0.
    /// Not multiplied by [`ClumpOptions::scale`](crate::ClumpOptions::scale).
    pub texture_size: f32,
}

impl Default for MaterialOptions {
    /// Textured white snow, one repeat per metre.
    fn default() -> Self {
        Self {
            texture: Some("snow_02".to_owned()),
            color: [1.0; 3],
            roughness: 1.0,
            texture_size: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_include_the_category_folder() {
        let snow = texture_set("snow_02").unwrap();
        assert_eq!(
            snow.path(
                TextureSize::Size1k,
                TextureMap::Color,
                TextureCompression::Bc7
            ),
            "snow/snow_02-1k-color-bc7.ktx2"
        );
        let moss = texture_set("moss_004").unwrap();
        assert_eq!(
            moss.path(
                TextureSize::Size512,
                TextureMap::Orh,
                TextureCompression::Uastc
            ),
            "moss/moss_004-512-orh-uastc.ktx2"
        );
        assert!(texture_set("rock_01").is_none());
    }

    #[test]
    fn every_set_name_is_unique() {
        for (i, a) in TEXTURE_SETS.iter().enumerate() {
            assert!(TEXTURE_SETS[i + 1..].iter().all(|b| b.name != a.name));
        }
    }
}
