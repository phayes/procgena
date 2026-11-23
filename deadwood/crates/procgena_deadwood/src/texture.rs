//! Names and file paths of the optional bark, end-grain and leaf card
//! texture sets.
//!
//! A piece's colours are vertex colours. Its options can also name a bark
//! texture set in [`MaterialOptions::bark_texture`] and an end-grain set in
//! [`MaterialOptions::end_texture`]; renderers then load those sets'
//! images, made with the repository's `textures` tool. The bark sets are
//! the same photographs `procgena_tree` uses.
//!
//! [`MaterialOptions::bark_texture`]: crate::MaterialOptions::bark_texture
//! [`MaterialOptions::end_texture`]: crate::MaterialOptions::end_texture

/// Every bark texture set, by the name used in
/// [`MaterialOptions::bark_texture`](crate::MaterialOptions::bark_texture).
/// Each comes with every [`TextureMap`] in every [`TextureSize`].
pub const BARK_TEXTURE_SETS: [&str; 29] = [
    "bark_brown_02",
    "bark_brown_01",
    "bark_willow",
    "bark_willow_02",
    "pine_bark",
    "tree_bark_03",
    "palm_tree_bark",
    "sakura_bark",
    "japanese_sycamore",
    "japanese_hackberry_bark",
    "chinese_cedar_bark",
    "japanese_zelkova_bark",
    "japanese_camphor_bark",
    "knotted_pine_bark",
    "jolcham_oak_bark_01",
    "eucalyptus_bark",
    "trident_maple_bark",
    "palm_bark",
    "japanese_cedar_bark",
    "bark_platanus",
    "bark_bluegum",
    "metasequoia_bark",
    "chinese_hackberry_bark",
    "Bark001",
    "Bark002",
    "Bark003",
    "Bark009",
    "Bark014",
    "Bark016",
];

/// Every end-grain texture set, by the name used in
/// [`MaterialOptions::end_texture`](crate::MaterialOptions::end_texture).
/// Each is a photo of a whole sawn tree end, not a repeating pattern, and
/// comes with every [`TextureMap`] in every [`TextureSize`]:
///
/// - `"TreeEnd003"`: a fresh end with dark bark and a few cracks;
/// - `"TreeEnd005"`: a pale, dried end with many cracks.
pub const END_TEXTURE_SETS: [&str; 2] = ["TreeEnd003", "TreeEnd005"];

/// Every leaf card texture set, by the name used in
/// [`LeafOptions::texture`](crate::LeafOptions::texture). Each is a
/// see-through picture of a branch tip or bare twig, with a
/// [`LeafMap::Color`] and a [`LeafMap::Normal`] image in one size. They are
/// the same cards `procgena_tree` uses, from Nobiax's CC0 "Free 3D branches
/// textures" packs; `"cedar"` and `"maple"` are recoloured a bright green.
pub const LEAF_TEXTURE_SETS: [&str; 22] = [
    "aspen_autumn",
    "european_aspen",
    "bigtooth_aspen",
    "alder_buckthorn",
    "smoke_bush",
    "field_maple_autumn",
    "bamboo",
    "fir",
    "white_pine",
    "arborvitae_autumn",
    "birch_autumn",
    "larch",
    "black_locust",
    "olive",
    "cotoneaster",
    "bare_twig_1",
    "bare_twig_2",
    "white_oak_autumn",
    "white_oak",
    "white_oak_dead",
    "cedar",
    "maple",
];

/// One image of a leaf card texture set. Use [`leaf_texture_path`] to get
/// its file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeafMap {
    /// Colour, with transparency outside the leaves in alpha. Load as sRGB;
    /// alpha is not premultiplied. Draw it with alpha testing (alpha mask),
    /// so the card shows only the leaves' outline.
    Color,
    /// Small bumps, as a tangent-space normal map in the OpenGL (+Y)
    /// convention. Load as linear data.
    Normal,
}

impl LeafMap {
    /// Both maps.
    pub const ALL: [LeafMap; 2] = [Self::Color, Self::Normal];

    /// Returns the map as written in texture paths: `"color"` or
    /// `"normal"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
        }
    }

    /// Returns true for maps that hold colour and must be loaded as sRGB.
    pub fn is_srgb(self) -> bool {
        self == Self::Color
    }
}

/// Returns the path of one leaf card texture file relative to the texture
/// folder, such as `"leaves/maple-color-bc7.ktx2"`. Leaf cards come in one
/// size only.
///
/// The files come from the same folder as [`bark_texture_path`]'s. `set`
/// should be one of [`LEAF_TEXTURE_SETS`]; other names give paths to files
/// that don't exist.
pub fn leaf_texture_path(set: &str, map: LeafMap, compression: TextureCompression) -> String {
    format!(
        "leaves/{set}-{}-{}.ktx2",
        map.as_str(),
        compression.as_str()
    )
}

/// Width and height of a texture image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureSize {
    /// 512 × 512 pixels: the least memory.
    Size512,
    /// 1024 × 1024 pixels.
    #[default]
    Size1k,
    /// 2048 × 2048 pixels.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

/// One image of a bark or end-grain texture set. Use [`bark_texture_path`]
/// or [`end_texture_path`] to get its file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureMap {
    /// The surface colour. Load this image as sRGB colour.
    Color,
    /// Small bumps, as a tangent-space normal map in the OpenGL (+Y)
    /// convention. Load as linear data.
    Normal,
    /// How matte the surface is, with the same value in red, green and
    /// blue (lighter is more matte). Load as linear data.
    Roughness,
}

impl TextureMap {
    /// Every map.
    pub const ALL: [TextureMap; 3] = [Self::Color, Self::Normal, Self::Roughness];

    /// Returns the map as written in texture paths: `"color"`, `"normal"`
    /// or `"roughness"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Normal => "normal",
            Self::Roughness => "roughness",
        }
    }

    /// Returns true for maps that hold colour and must be loaded as sRGB.
    pub fn is_srgb(self) -> bool {
        self == Self::Color
    }
}

/// Returns the path of one bark texture file relative to the texture
/// folder, such as `"bark/bark_brown_02-1k-color-bc7.ktx2"`.
///
/// The repository's `deadwood/assets/textures/` folder holds these files
/// after `uv run --project tools textures download deadwood`. `set` should
/// be one of [`BARK_TEXTURE_SETS`]; other names give paths to files that
/// don't exist.
pub fn bark_texture_path(
    set: &str,
    size: TextureSize,
    map: TextureMap,
    compression: TextureCompression,
) -> String {
    texture_path("bark", set, size, map, compression)
}

/// Returns the path of one end-grain texture file relative to the texture
/// folder, such as `"end/TreeEnd005-1k-color-bc7.ktx2"`.
///
/// The files come from the same folder as [`bark_texture_path`]'s. `set`
/// should be one of [`END_TEXTURE_SETS`]; other names give paths to files
/// that don't exist.
pub fn end_texture_path(
    set: &str,
    size: TextureSize,
    map: TextureMap,
    compression: TextureCompression,
) -> String {
    texture_path("end", set, size, map, compression)
}

/// Returns `<folder>/<set>-<size>-<map>-<compression>.ktx2`.
fn texture_path(
    folder: &str,
    set: &str,
    size: TextureSize,
    map: TextureMap,
    compression: TextureCompression,
) -> String {
    format!(
        "{folder}/{set}-{}-{}-{}.ktx2",
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
            bark_texture_path(
                "bark_brown_02",
                TextureSize::Size1k,
                TextureMap::Color,
                TextureCompression::Bc7
            ),
            "bark/bark_brown_02-1k-color-bc7.ktx2"
        );
        assert_eq!(
            end_texture_path(
                "TreeEnd005",
                TextureSize::Size2k,
                TextureMap::Normal,
                TextureCompression::Bc7
            ),
            "end/TreeEnd005-2k-normal-bc7.ktx2"
        );
        assert_eq!(
            bark_texture_path(
                "Bark001",
                TextureSize::Size512,
                TextureMap::Roughness,
                TextureCompression::Uastc
            ),
            "bark/Bark001-512-roughness-uastc.ktx2"
        );
        assert_eq!(
            leaf_texture_path("maple", LeafMap::Normal, TextureCompression::Uastc),
            "leaves/maple-normal-uastc.ktx2"
        );
    }
}
