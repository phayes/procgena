//! Ground materials and the World-wide catalogue that names them.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use procgena_core::ChildKey;
use procgena_core::texture::TextureList;
use serde::{Deserialize, Serialize};

use super::GroundError;

/// One ground material, such as needle litter or a gravel road surface.
///
/// A material is drawn as a flat colour and roughness where there are no
/// textures (far away, or without texture files), and with its texture set
/// near the camera when it names one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundMaterial {
    /// The material's colour, as linear RGB from 0 to 1. For a textured
    /// material this is the texture's average colour, so the ground keeps
    /// its colour where textures stop.
    pub colour: [f32; 3],
    /// How rough the surface is, from 0 (mirror) to 1 (fully matte), where
    /// it has no texture.
    pub roughness: f32,
    /// The id of the material's texture set in [`ground_textures`], or
    /// `None` for a flat colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// Metres of ground one texture repeat covers. It never changes with
    /// the World's scale.
    #[serde(default = "default_tile")]
    pub tile: f32,
    /// How the texture is laid on the ground.
    #[serde(default)]
    pub projection: Projection,
    /// How strongly the texture's height map sharpens the edges where this
    /// material meets others, from 0 (a plain blend) to 1 (raised parts of
    /// the texture poke through first).
    #[serde(default)]
    pub blend_height: f32,
}

/// How a ground material's texture is laid on the ground.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    /// Straight down, along the World's x and z. Cheapest; stretches on
    /// steep slopes.
    #[default]
    Top,
    /// From the side as well on steep ground, so cliffs don't stretch.
    /// Costs three texture reads where the ground is steep.
    Triplanar,
}

/// The tile size of a material that names none, in metres.
const DEFAULT_TILE: f32 = 3.0;

fn default_tile() -> f32 {
    DEFAULT_TILE
}

impl GroundMaterial {
    /// A flat material of `colour` (linear RGB) and `roughness`, with no
    /// texture.
    pub const fn new(colour: [f32; 3], roughness: f32) -> Self {
        Self {
            colour,
            roughness,
            texture: None,
            tile: DEFAULT_TILE,
            projection: Projection::Top,
            blend_height: 0.0,
        }
    }

    /// The same material drawn with texture set `texture`, repeating every
    /// `tile` metres.
    #[must_use]
    pub fn with_texture(mut self, texture: impl Into<String>, tile: f32) -> Self {
        self.texture = Some(texture.into());
        self.tile = tile;
        self
    }
}

/// The texture sets ground materials can name, from `terrain/textures.yml`.
///
/// Install the files with `uv run --project tools textures download terrain
/// --sizes 128,512,1k`.
pub fn ground_textures() -> &'static TextureList {
    static LIST: OnceLock<TextureList> = OnceLock::new();
    LIST.get_or_init(|| {
        TextureList::from_yaml(GROUND_TEXTURES).expect("terrain/textures.yml is a valid texture list")
    })
}

/// The text of `terrain/textures.yml`.
const GROUND_TEXTURES: &str = include_str!("../../../../textures.yml");

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
    ("decayed_leaves", [0.1737, 0.0767, 0.0356], 0.9),
    ("forest_soil", [0.0434, 0.0224, 0.0082], 0.95),
    ("needle_litter", [0.3564, 0.2351, 0.1481], 0.9),
    ("leaf_litter", [0.2005, 0.1198, 0.0468], 0.9),
    ("moss", [0.0761, 0.1419, 0.0116], 0.95),
    ("mud", [0.09, 0.065, 0.04], 0.4),
];

/// The built-in materials' textures: key, texture set, tile in metres,
/// projection and height blend.
const STANDARD_TEXTURES: &[(&str, &str, f32, Projection, f32)] = &[
    ("cliff_rock", "rock_face_03", 6.0, Projection::Triplanar, 0.5),
    ("forest_soil", "mud_forest", 3.0, Projection::Top, 0.3),
    ("needle_litter", "forrest_ground_03", 2.5, Projection::Top, 0.6),
    ("leaf_litter", "leaves_forest_ground", 2.5, Projection::Top, 0.6),
    ("decayed_leaves", "dry_decay_leaves", 2.5, Projection::Top, 0.4),
    ("moss", "moss_002", 2.0, Projection::Top, 0.5),
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
    /// `decayed_leaves`, `forest_soil`, `needle_litter`, `leaf_litter`,
    /// `moss` and `mud`.
    /// Cliff rock and the forest floor's materials have textures.
    pub fn standard() -> Self {
        STANDARD
            .iter()
            .fold(Self::new(), |catalogue, &(key, colour, roughness)| {
                let mut material = GroundMaterial::new(colour, roughness);
                if let Some(&(_, texture, tile, projection, blend_height)) =
                    STANDARD_TEXTURES.iter().find(|entry| entry.0 == key)
                {
                    material = material.with_texture(texture, tile);
                    material.projection = projection;
                    material.blend_height = blend_height;
                }
                catalogue.with(
                    ChildKey::new(key).expect("built-in material keys are well formed"),
                    material,
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
    /// [`GroundError::Material`] for the first material whose colour,
    /// roughness or height blend is outside 0 to 1, whose tile isn't
    /// positive, or whose texture isn't in [`ground_textures`] with an
    /// average colour; and [`GroundError::TooManyMaterials`] if there are
    /// more than 65,536.
    pub fn validate(&self) -> Result<(), GroundError> {
        if self.materials.len() > usize::from(u16::MAX) + 1 {
            return Err(GroundError::TooManyMaterials);
        }
        let textures = ground_textures();
        let unit = |v: f32| (0.0..=1.0).contains(&v);
        for (key, material) in &self.materials {
            let texture_ok = material.texture.as_deref().is_none_or(|id| {
                textures.set(id).is_some_and(|set| set.average.is_some())
            });
            if !(material.colour.iter().all(|&c| unit(c))
                && unit(material.roughness)
                && unit(material.blend_height)
                && material.tile > 0.0
                && texture_ok)
            {
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
