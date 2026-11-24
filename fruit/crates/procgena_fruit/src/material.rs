//! Appearance settings for fruit and flowers in a renderer.

use crate::ItemShape;

/// Appearance settings for a cluster's surfaces in your renderer.
///
/// Use [`FruitMesh::colors`](crate::FruitMesh::colors) as the surface
/// colour; those per-vertex values are already linear RGBA for lighting.
/// No texture is needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FruitMaterial {
    /// Metallic appearance from 0 (nonmetal) to 1 (metal).
    pub metallic: f32,
    /// Surface roughness from 0 (smooth and shiny) to 1 (matte).
    pub roughness: f32,
    /// Whether to draw both faces of each triangle. Always true for the
    /// built-in materials, because stalk strips, flat fruit and petals are
    /// single surfaces seen from both sides.
    pub double_sided: bool,
}

impl FruitMaterial {
    /// Glossy fruit skin: roughness 0.45.
    pub const FRUIT: Self = Self {
        metallic: 0.0,
        roughness: 0.45,
        double_sided: true,
    };

    /// Matte petals: roughness 0.8.
    pub const FLOWER: Self = Self {
        metallic: 0.0,
        roughness: 0.8,
        double_sided: true,
    };

    /// The material for clusters of `item`: [`FRUIT`](Self::FRUIT) or
    /// [`FLOWER`](Self::FLOWER).
    pub fn for_item(item: &ItemShape) -> Self {
        match item {
            ItemShape::Fruit(_) => Self::FRUIT,
            ItemShape::Flower(_) => Self::FLOWER,
        }
    }
}
