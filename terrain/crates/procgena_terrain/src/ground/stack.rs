//! Stacks of ground layers, and laying them over each other.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::DVec2;
use procgena_core::{ChildKey, child_seed};

use super::{GroundError, GroundLayer, GroundSite, MaterialCatalogue, MaterialId};
use crate::SampleDetail;

/// How much of each material lies at one point. The weights are more than
/// 0 and add up to 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroundMix {
    /// By material id, each more than 0.
    weights: Vec<(MaterialId, f32)>,
}

impl GroundMix {
    /// All of one material.
    pub fn single(material: MaterialId) -> Self {
        Self {
            weights: vec![(material, 1.0)],
        }
    }

    /// The weight of `material`, 0 if it isn't in the mix.
    pub fn weight(&self, material: MaterialId) -> f32 {
        self.weights
            .iter()
            .find(|(id, _)| *id == material)
            .map_or(0.0, |&(_, w)| w)
    }

    /// Each material in the mix with its weight, by material id.
    pub fn iter(&self) -> impl Iterator<Item = (MaterialId, f32)> + '_ {
        self.weights.iter().copied()
    }

    /// The number of materials in the mix.
    pub fn len(&self) -> usize {
        self.weights.len()
    }

    /// Whether the mix has no material, which only an empty default mix
    /// has.
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }

    /// The mix's colour: each material's colour from `catalogue` (the one
    /// the mix was painted with) times its weight, as linear RGB.
    pub fn colour(&self, catalogue: &MaterialCatalogue) -> [f32; 3] {
        let mut colour = [0.0; 3];
        for &(id, w) in &self.weights {
            let (_, material) = catalogue.get(id);
            for (c, m) in colour.iter_mut().zip(material.colour) {
                *c += m * w;
            }
        }
        colour
    }

    /// Lays `w` of `material` over the mix, covering that share of what is
    /// already there.
    fn over(&mut self, material: MaterialId, w: f32) {
        if w <= 0.0 {
            return;
        }
        if w >= 1.0 {
            *self = Self::single(material);
            return;
        }
        for (_, weight) in &mut self.weights {
            *weight *= 1.0 - w;
        }
        match self.weights.binary_search_by_key(&material, |&(id, _)| id) {
            Ok(i) => self.weights[i].1 += w,
            Err(i) => self.weights.insert(i, (material, w)),
        }
    }
}

/// One layer of a stack, ready to paint with.
#[derive(Debug, Clone)]
struct Prepared {
    key: ChildKey,
    layer: GroundLayer,
    material: MaterialId,
    seed: u32,
}

/// Checks `layers` against `catalogue` and sorts them by order, seeding
/// each with `child_seed(seed, seed_kind, key)`.
fn prepare(
    layers: &BTreeMap<ChildKey, GroundLayer>,
    catalogue: &MaterialCatalogue,
    seed_kind: &str,
    seed: u32,
) -> Result<Vec<Prepared>, GroundError> {
    // Each layer on its own.
    let mut prepared = Vec::with_capacity(layers.len());
    for (key, layer) in layers {
        if !layer.is_valid() {
            return Err(GroundError::Layer { key: key.clone() });
        }
        let material =
            catalogue
                .id(layer.material.as_str())
                .ok_or_else(|| GroundError::UnknownMaterial {
                    key: key.clone(),
                    material: layer.material.clone(),
                })?;
        prepared.push(Prepared {
            key: key.clone(),
            layer: layer.clone(),
            material,
            seed: child_seed(seed, seed_kind, key),
        });
    }

    // The order, which must be settled, and seeds, which must differ.
    prepared.sort_by_key(|p| p.layer.order);
    for pair in prepared.windows(2) {
        if pair[0].layer.order == pair[1].layer.order {
            return Err(GroundError::SharedOrder {
                order: pair[0].layer.order,
                keys: [pair[0].key.clone(), pair[1].key.clone()],
            });
        }
    }
    let keys: Vec<&ChildKey> = layers.keys().collect();
    for (i, a) in keys.iter().enumerate() {
        if let Some(b) = keys[i + 1..].iter().find(|b| b.salt() == a.salt()) {
            return Err(GroundError::SharedSalt {
                keys: [(*a).clone(), (*b).clone()],
            });
        }
    }
    Ok(prepared)
}

/// A complete stack of ground layers over one catalogue, such as the
/// look of bare terrain or a Biome's base ground. Its lowest layer covers
/// everything, so every point gets a full mix.
///
/// Paint a point with [`paint`](Self::paint), or with an Ecotope's layers
/// laid in with [`paint_with`](Self::paint_with).
#[derive(Debug, Clone)]
pub struct GroundStack {
    catalogue: Arc<MaterialCatalogue>,
    vertical: f32,
    layers: Vec<Prepared>,
}

impl GroundStack {
    /// A stack of `layers` over `catalogue`.
    ///
    /// Each layer's noise is seeded `child_seed(seed, seed_kind, key)`,
    /// where `seed_kind` names the stack's owner, such as
    /// `"biome.child.ground"`. `vertical` is the World's vertical scale,
    /// which multiplies the heights of absolute height rules.
    ///
    /// # Errors
    ///
    /// - [`GroundError::Empty`] for no layers, and
    ///   [`GroundError::BaseNotOpaque`] if the lowest layer has conditions,
    ///   noise or less than full opacity.
    /// - [`GroundError::Influence`] for a layer that follows a Scatter.
    /// - [`GroundError::Vertical`] if `vertical` isn't more than 0.
    /// - Any error of the catalogue's [`validate`](MaterialCatalogue::validate),
    ///   and for the layers: [`GroundError::Layer`] for unusable
    ///   settings, [`GroundError::UnknownMaterial`],
    ///   [`GroundError::SharedOrder`] and [`GroundError::SharedSalt`].
    pub fn new(
        layers: &BTreeMap<ChildKey, GroundLayer>,
        catalogue: Arc<MaterialCatalogue>,
        seed_kind: &str,
        seed: u32,
        vertical: f32,
    ) -> Result<Self, GroundError> {
        catalogue.validate()?;
        if !(vertical > 0.0 && vertical.is_finite()) {
            return Err(GroundError::Vertical);
        }
        let layers = prepare(layers, &catalogue, seed_kind, seed)?;
        let lowest = layers.first().ok_or(GroundError::Empty)?;
        if !lowest.layer.is_opaque() {
            return Err(GroundError::BaseNotOpaque {
                key: lowest.key.clone(),
            });
        }
        if let Some(p) = layers.iter().find(|p| !p.layer.under.is_empty()) {
            return Err(GroundError::Influence { key: p.key.clone() });
        }
        Ok(Self {
            catalogue,
            vertical,
            layers,
        })
    }

    /// The catalogue the stack's materials are in.
    pub fn catalogue(&self) -> &Arc<MaterialCatalogue> {
        &self.catalogue
    }

    /// The World's vertical scale the stack was made for.
    pub fn vertical(&self) -> f32 {
        self.vertical
    }

    /// The minimum sampling detail needed by all layers in this stack.
    /// Pass it to the [`GroundSource`](super::GroundSource) when sampling
    /// sites for painting; see [`GroundLayer::detail`].
    pub fn detail(&self) -> SampleDetail {
        detail_of(&self.layers)
    }

    /// The stack's layers with their keys, from the bottom up.
    pub fn layers(&self) -> impl Iterator<Item = (&ChildKey, &GroundLayer)> {
        self.layers.iter().map(|p| (&p.key, &p.layer))
    }

    /// The mix of materials at world position `p`, whose site is `site`.
    pub fn paint(&self, p: DVec2, site: &GroundSite) -> GroundMix {
        self.paint_with(None, |_| 1.0, p, site)
    }

    /// The mix at `p` with `overlay`'s layers laid in among the stack's by
    /// order. At equal orders the overlay's layer lies on top, and overlay
    /// layers below the stack's lowest layer are covered by it.
    ///
    /// `influence(i)` gives the Scatter influence at `p` of the overlay's
    /// `i`th layer from the bottom (as [`GroundOverlay::layers`] lists
    /// them), from 0 to 1. It is asked only of layers that follow a
    /// Scatter, and only where the rest of their rule leaves some weight.
    ///
    /// # Panics
    ///
    /// If `overlay` was made over a stack with another catalogue.
    pub fn paint_with(
        &self,
        overlay: Option<&GroundOverlay>,
        influence: impl Fn(usize) -> f32,
        p: DVec2,
        site: &GroundSite,
    ) -> GroundMix {
        let upper: &[Prepared] = overlay.map_or(&[], |o| {
            assert!(
                Arc::ptr_eq(&o.catalogue, &self.catalogue) || o.catalogue == self.catalogue,
                "the overlay was made over another catalogue"
            );
            &o.layers
        });
        let weight =
            |prepared: &Prepared| prepared.layer.weight(p, site, prepared.seed, self.vertical);

        // The lowest layer covers everything; overlay layers below it are
        // hidden.
        let lowest = &self.layers[0];
        let mut mix = GroundMix::single(lowest.material);
        let mut j = upper
            .iter()
            .position(|u| u.layer.order >= lowest.layer.order)
            .unwrap_or(upper.len());
        let mut i = 1;

        // The rest, bottom to top, the overlay on top at equal orders.
        while i < self.layers.len() || j < upper.len() {
            let take_base = match (self.layers.get(i), upper.get(j)) {
                (Some(b), Some(u)) => b.layer.order <= u.layer.order,
                (Some(_), None) => true,
                _ => false,
            };
            if take_base {
                let b = &self.layers[i];
                mix.over(b.material, weight(b));
                i += 1;
            } else {
                let u = &upper[j];
                let mut w = weight(u);
                if w > 0.0 && !u.layer.under.is_empty() {
                    w *= influence(j);
                }
                mix.over(u.material, w);
                j += 1;
            }
        }
        mix
    }
}

/// Ground layers laid over a [`GroundStack`], such as an Ecotope's, which
/// may follow the Ecotope's Scatters. Paint with
/// [`GroundStack::paint_with`].
#[derive(Debug, Clone)]
pub struct GroundOverlay {
    catalogue: Arc<MaterialCatalogue>,
    layers: Vec<Prepared>,
}

impl GroundOverlay {
    /// `layers` to lay over `stack`, in its catalogue. Each layer's noise
    /// is seeded `child_seed(seed, seed_kind, key)`, where `seed_kind`
    /// names the overlay's owner, such as `"ecotope.child.ground"`. No
    /// layers is fine.
    ///
    /// # Errors
    ///
    /// [`GroundError::Layer`] for unusable settings,
    /// [`GroundError::UnknownMaterial`], [`GroundError::SharedOrder`] and
    /// [`GroundError::SharedSalt`].
    pub fn new(
        layers: &BTreeMap<ChildKey, GroundLayer>,
        stack: &GroundStack,
        seed_kind: &str,
        seed: u32,
    ) -> Result<Self, GroundError> {
        Ok(Self {
            catalogue: stack.catalogue.clone(),
            layers: prepare(layers, &stack.catalogue, seed_kind, seed)?,
        })
    }

    /// The overlay's layers with their keys, from the bottom up.
    pub fn layers(&self) -> impl Iterator<Item = (&ChildKey, &GroundLayer)> {
        self.layers.iter().map(|p| (&p.key, &p.layer))
    }

    /// The number of layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether there are no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The minimum sampling detail needed by this overlay's layers.
    /// Returns [`SampleDetail::Gradient`] for an empty overlay. When
    /// painting over a stack, request the higher of the stack's and
    /// overlay's detail levels.
    pub fn detail(&self) -> SampleDetail {
        detail_of(&self.layers)
    }
}

/// Returns the highest layer requirement, or gradient detail for no layers.
fn detail_of(layers: &[Prepared]) -> SampleDetail {
    layers
        .iter()
        .map(|p| p.layer.detail())
        .max()
        .unwrap_or(SampleDetail::Gradient)
}

/// Anything that paints the ground: a mix of materials at each point.
///
/// A painter must be a pure function of position and site, so that
/// neighbouring chunks agree at their shared edge, and chunks built in any
/// order agree.
pub trait GroundPainter: Sync {
    /// The complete layer stack, when this painter is exactly that stack.
    /// Renderers may use it to accelerate painting. Custom painters keep
    /// the default unless evaluating this stack reproduces `paint`.
    fn layer_stack(&self) -> Option<&GroundStack> {
        None
    }

    /// The sampling detail this painter needs from a ground source.
    /// Override this when `paint` uses measures (distances, nearby water
    /// height or curvature) or labels (landform categories and flags).
    /// The default, [`SampleDetail::Gradient`], requests height, slope,
    /// water and road tags.
    ///
    /// Higher detail requires more terrain calculations. Sampling with
    /// less detail may leave data unknown; ground layers ignore conditions
    /// on unknown data, which can change the resulting materials.
    fn detail(&self) -> SampleDetail {
        SampleDetail::Gradient
    }

    /// The catalogue the mixes' material ids are from.
    fn catalogue(&self) -> &MaterialCatalogue;

    /// The mix of materials at world position `p` (x and z, metres), whose
    /// site is `site`.
    fn paint(&self, p: DVec2, site: &GroundSite) -> GroundMix;
}

impl GroundPainter for GroundStack {
    fn layer_stack(&self) -> Option<&GroundStack> {
        Some(self)
    }

    fn detail(&self) -> SampleDetail {
        GroundStack::detail(self)
    }

    fn catalogue(&self) -> &MaterialCatalogue {
        &self.catalogue
    }

    fn paint(&self, p: DVec2, site: &GroundSite) -> GroundMix {
        GroundStack::paint(self, p, site)
    }
}
