//! Identifies reusable tree materials, which control surface colour, texture
//! and shine. Trees with matching material settings can share these assets
//! even when their shapes differ.

use procgena_tree::{
    BarkMaterial, BarkOptions, BarkUvMode, LeafMaterial, LeafOptions, TextureSize, TrellisMaterial,
    TrellisOptions,
};

/// Converts numbers to their stored bits so cache keys can compare and hash them.
fn bits<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

/// Selects how bark gets its surface colour and detail.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BarkLook {
    /// Uses only the chosen bark colour, without a texture image.
    Tint,
    /// Uses texture images at the chosen resolution. An optional normal map
    /// adds the appearance of surface bumps without changing the shape.
    Textured {
        kind: String,
        size: TextureSize,
        normal_map: bool,
    },
}

/// Groups matching bark surface settings so their material can be reused.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BarkKey {
    look: BarkLook,
    color: [u32; 3],
    metallic: u32,
    roughness: u32,
}

impl BarkKey {
    /// Identifies the requested bark appearance: textured at `size`, or with
    /// no size, the tint alone.
    pub(crate) fn new(bark: &BarkOptions, size: Option<TextureSize>) -> Self {
        let settings = BarkMaterial::new(bark);
        // When bark textures are disabled, only the tint affects its colour.
        let look = match size {
            Some(size) if settings.textured => BarkLook::Textured {
                kind: bark.kind.clone(),
                size,
                normal_map: bark.uv_mode == BarkUvMode::Continuous,
            },
            _ => BarkLook::Tint,
        };
        Self {
            look,
            color: bits(settings.base_color),
            metallic: settings.metallic.to_bits(),
            roughness: settings.roughness.to_bits(),
        }
    }
}

/// Groups matching leaf surface settings so their material can be reused.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LeafKey {
    /// The leaf kind whose images are drawn; `None` means no texture is used.
    textures: Option<String>,
    color: [u32; 3],
    alpha_cutoff: u32,
    double_sided: bool,
    rounded_normals: bool,
    metallic: u32,
    roughness: u32,
}

impl LeafKey {
    /// Returns a reusable material key for these leaf settings, with or
    /// without the leaf kind's images.
    pub(crate) fn new(leaves: &LeafOptions, textured: bool) -> Self {
        let settings = LeafMaterial::new(leaves);
        Self {
            textures: textured.then(|| leaves.kind.clone()),
            color: bits(settings.base_color),
            alpha_cutoff: settings.alpha_cutoff.to_bits(),
            double_sided: settings.double_sided,
            rounded_normals: settings.rounded_normals,
            metallic: settings.metallic.to_bits(),
            roughness: settings.roughness.to_bits(),
        }
    }
}

/// Groups matching surface settings for the wires of a tree support frame (trellis).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TrellisKey([u32; 5]);

impl TrellisKey {
    /// Identifies the wire colour and how the surface reflects light.
    pub(crate) fn new(trellis: &TrellisOptions) -> Self {
        let settings = TrellisMaterial::new(trellis);
        let [r, g, b] = settings.base_color;
        Self(bits([r, g, b, settings.metallic, settings.roughness]))
    }
}

#[cfg(test)]
mod tests {
    use procgena_tree::TreeOptions;

    use super::*;

    #[test]
    fn bark_keys_follow_what_the_material_shows() {
        let bark = TreeOptions::default().bark;
        let textured = Some(TextureSize::Size1k);
        let key = BarkKey::new(&bark, textured);

        // Matching settings reuse a key; surface bumps, tint and image size
        // require distinct keys.
        assert_eq!(key, BarkKey::new(&bark.clone(), textured));
        let mut mirrored = bark.clone();
        mirrored.uv_mode = BarkUvMode::Mirrored;
        assert_ne!(key, BarkKey::new(&mirrored, textured));
        let mut tinted = bark.clone();
        tinted.tint ^= 0x10;
        assert_ne!(key, BarkKey::new(&tinted, textured));
        assert_ne!(key, BarkKey::new(&bark, Some(TextureSize::Size512)));

        // With textures disabled, changing the texture selection or bark kind
        // does not change the material key.
        let mut plain = bark.clone();
        plain.textured = false;
        let tint_only = BarkKey::new(&plain, None);
        assert_eq!(tint_only, BarkKey::new(&plain, textured));
        plain.kind = "other".to_owned();
        assert_eq!(tint_only, BarkKey::new(&plain, textured));
    }

    #[test]
    fn leaf_keys_follow_what_the_material_shows() {
        let leaves = TreeOptions::default().leaves;
        let key = LeafKey::new(&leaves, true);
        assert_ne!(key, LeafKey::new(&leaves, false));

        // Changing the number or size of leaves preserves the material key.
        let mut more = leaves.clone();
        more.count += 3;
        more.size *= 2.0;
        assert_eq!(key, LeafKey::new(&more, true));
        let mut sharper = leaves.clone();
        sharper.alpha_test += 0.1;
        assert_ne!(key, LeafKey::new(&sharper, true));
    }
}
