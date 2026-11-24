//! Ground materials and the World-wide catalogue that names them.

use std::collections::BTreeMap;

use procgena_core::ChildKey;
use serde::{Deserialize, Serialize};

use super::GroundError;

/// One ground material, such as needle litter or a gravel road surface.
///
/// For now a material is a flat colour and a roughness. Textures come
/// later.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundMaterial {
    /// The material's colour, as linear RGB from 0 to 1.
    pub colour: [f32; 3],
    /// How rough the surface is, from 0 (mirror) to 1 (fully matte).
    pub roughness: f32,
}

impl GroundMaterial {
    /// A material of `colour` (linear RGB) and `roughness`.
    pub const fn new(colour: [f32; 3], roughness: f32) -> Self {
        Self { colour, roughness }
    }
}

/// A material's place in a [`MaterialCatalogue`]: its index in key order.
///
/// Ids are only meaningful for the catalogue they came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialId(pub u16);

/// The ground materials of a World, each named by a frozen key.
///
/// Every ground layer names its material by key in one catalogue, so two
/// Ecotopes that both use `moss` share one material. Start from
/// [`MaterialCatalogue::standard`] and add to it with
/// [`with`](Self::with).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialCatalogue {
    materials: BTreeMap<ChildKey, GroundMaterial>,
}

/// The built-in materials, by key. Colours are linear RGB.
const STANDARD: &[(&str, [f32; 3], f32)] = &[
    ("grass", [0.14, 0.22, 0.06], 0.9),
    ("cliff_rock", [0.24, 0.22, 0.2], 0.8),
    ("snow", [0.85, 0.87, 0.9], 0.6),
    ("gravel_road", [0.33, 0.3, 0.26], 0.9),
    ("riverbed", [0.17, 0.15, 0.12], 0.5),
    ("bare_dirt", [0.2, 0.13, 0.07], 0.95),
    ("forest_soil", [0.11, 0.075, 0.045], 0.95),
    ("needle_litter", [0.3, 0.15, 0.06], 0.9),
    ("moss", [0.09, 0.18, 0.035], 0.95),
    ("mud", [0.09, 0.065, 0.04], 0.4),
];

impl MaterialCatalogue {
    /// An empty catalogue.
    pub fn new() -> Self {
        Self {
            materials: BTreeMap::new(),
        }
    }

    /// The built-in materials: `grass`, `cliff_rock` and `snow` (the
    /// colours of bare terrain), `gravel_road`, `riverbed`, `bare_dirt`,
    /// `forest_soil`, `needle_litter`, `moss` and `mud`.
    pub fn standard() -> Self {
        STANDARD
            .iter()
            .fold(Self::new(), |catalogue, &(key, colour, roughness)| {
                catalogue.with(
                    ChildKey::new(key).expect("built-in material keys are well formed"),
                    GroundMaterial::new(colour, roughness),
                )
            })
    }

    /// The same catalogue with `material` under `key`, replacing any
    /// material already there.
    #[must_use]
    pub fn with(mut self, key: ChildKey, material: GroundMaterial) -> Self {
        self.materials.insert(key, material);
        self
    }

    /// The number of materials.
    pub fn len(&self) -> usize {
        self.materials.len()
    }

    /// Whether the catalogue has no materials.
    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }

    /// The id of the material keyed `key`, if the catalogue has one.
    pub fn id(&self, key: &str) -> Option<MaterialId> {
        self.materials
            .keys()
            .position(|k| k.as_str() == key)
            .map(|i| MaterialId(i as u16))
    }

    /// The material with id `id`, with its key.
    ///
    /// # Panics
    ///
    /// If `id` is not from this catalogue.
    pub fn get(&self, id: MaterialId) -> (&ChildKey, &GroundMaterial) {
        self.materials
            .iter()
            .nth(usize::from(id.0))
            .expect("the material id is from this catalogue")
    }

    /// Every material with its id and key, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (MaterialId, &ChildKey, &GroundMaterial)> {
        self.materials
            .iter()
            .enumerate()
            .map(|(i, (key, material))| (MaterialId(i as u16), key, material))
    }

    /// Checks every material.
    ///
    /// # Errors
    ///
    /// [`GroundError::Material`] for the first material whose colour or
    /// roughness is outside 0 to 1, and [`GroundError::TooManyMaterials`]
    /// if there are more than 65,536.
    pub fn validate(&self) -> Result<(), GroundError> {
        if self.materials.len() > usize::from(u16::MAX) + 1 {
            return Err(GroundError::TooManyMaterials);
        }
        let unit = |v: f32| (0.0..=1.0).contains(&v);
        for (key, material) in &self.materials {
            if !(material.colour.iter().all(|&c| unit(c)) && unit(material.roughness)) {
                return Err(GroundError::Material { key: key.clone() });
            }
        }
        Ok(())
    }
}

impl Default for MaterialCatalogue {
    /// The [`standard`](Self::standard) catalogue.
    fn default() -> Self {
        Self::standard()
    }
}
