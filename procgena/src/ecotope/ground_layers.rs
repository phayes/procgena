//! The model the Ecotope editor's ground-layer controls edit: layers of a
//! material, each with a place in the stack, a strength, rules over slope
//! and height, patchy noise, and a Scatter it may follow.
//!
//! Kept for the coming Biome and Ecotope ground paint (ROADMAP BP); not
//! connected. Nothing paints, saves or draws these layers.

use procgena_core::ChildKey;

/// A soft band over one measure, such as a slope or a height: 0 outside
/// it, 1 inside it, with smooth edges. `rise` brings the band in from 0 at
/// its first value to 1 at its second, and `fall` takes it out again; leave
/// either out for a band open at that end.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ramp {
    /// Where the band comes in: 0 at the first value, 1 at the second.
    pub rise: Option<[f32; 2]>,
    /// Where the band goes out: 1 at the first value, 0 at the second.
    pub fall: Option<[f32; 2]>,
}

impl Ramp {
    /// A band that comes in from `a` to `b` and stays full above.
    pub const fn rise(a: f32, b: f32) -> Self {
        Self {
            rise: Some([a, b]),
            fall: None,
        }
    }

    /// A band that is full below `a` and goes out by `b`.
    pub const fn fall(a: f32, b: f32) -> Self {
        Self {
            rise: None,
            fall: Some([a, b]),
        }
    }

    /// Whether every value is a number and each edge runs from low to high.
    #[cfg(test)]
    fn is_valid(&self) -> bool {
        let edge =
            |e: Option<[f32; 2]>| e.is_none_or(|[a, b]| !a.is_nan() && !b.is_nan() && a <= b);
        edge(self.rise) && edge(self.fall)
    }
}

/// What a height rule measures a height against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeightRule {
    /// Height in metres.
    Absolute(Ramp),
}

/// One rule of a ground layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// The ground's slope, as the sine of its angle from flat: 0 flat, 1 a
    /// wall.
    Slope(Ramp),
    /// The ground's height.
    Height(HeightRule),
}

impl Condition {
    /// Whether the rule's settings are usable.
    #[cfg(test)]
    fn is_valid(&self) -> bool {
        match self {
            Self::Slope(ramp) | Self::Height(HeightRule::Absolute(ramp)) => ramp.is_valid(),
        }
    }
}

/// Patchy noise that breaks a layer up: patches about `wavelength` metres
/// across, fewer with a higher `threshold`, sharper with a higher
/// `contrast`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundNoise {
    /// Patch size in metres. More than 0.
    pub wavelength: f32,
    /// How sharp patch edges are. 0 or more.
    pub contrast: f32,
    /// The noise value, from 0 to 1, at which a patch's edge lies.
    pub threshold: f32,
}

impl Default for GroundNoise {
    fn default() -> Self {
        Self {
            wavelength: 8.0,
            contrast: 2.0,
            threshold: 0.5,
        }
    }
}

impl GroundNoise {
    /// Whether the settings are usable.
    #[cfg(test)]
    fn is_valid(&self) -> bool {
        self.wavelength > 0.0
            && self.wavelength.is_finite()
            && self.contrast >= 0.0
            && self.contrast.is_finite()
            && self.threshold.is_finite()
    }
}

/// How far a layer following a Scatter reaches from each copy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reach {
    /// This many metres beyond the edge of the copy's footprint.
    Metres(f32),
    /// This many times the footprint's radius, from its middle.
    Footprint(f32),
}

/// A ground layer following one of its Ecotope's Scatters, such as needle
/// litter under pines.
#[derive(Debug, Clone, PartialEq)]
pub struct ScatterInfluence {
    /// The key of the Scatter followed.
    pub scatter: ChildKey,
    /// How far the layer reaches from each copy.
    pub reach: Reach,
    /// The layer's strength across its reach, from the copy's middle (0)
    /// to the reach's edge (1).
    pub falloff: Ramp,
    /// Lay the layer away from the copies instead of near them.
    pub invert: bool,
}

/// One ground layer: a material and where it lies.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundLayer {
    /// The material's key.
    pub material: ChildKey,
    /// Where the layer lies in the stack: higher numbers lie on top.
    pub order: u32,
    /// The layer's strength, from 0 to 1.
    pub opacity: f32,
    /// The rules the layer keeps to.
    pub when: Vec<Condition>,
    /// Optional noise breaking the layer into patches.
    pub noise: Option<GroundNoise>,
    /// The Scatters the layer follows; empty for none.
    pub under: Vec<ScatterInfluence>,
}

impl GroundLayer {
    /// A layer of `material` at `order`, everywhere, at full opacity.
    pub fn new(material: ChildKey, order: u32) -> Self {
        Self {
            material,
            order,
            opacity: 1.0,
            when: Vec::new(),
            noise: None,
            under: Vec::new(),
        }
    }

    /// The same layer with `condition` added.
    #[cfg(test)]
    #[must_use]
    pub fn when(mut self, condition: Condition) -> Self {
        self.when.push(condition);
        self
    }

    /// The same layer broken up by `noise`.
    #[cfg(test)]
    #[must_use]
    pub fn with_noise(mut self, noise: GroundNoise) -> Self {
        self.noise = Some(noise);
        self
    }

    /// The same layer following `influence`.
    #[cfg(test)]
    #[must_use]
    pub fn under(mut self, influence: ScatterInfluence) -> Self {
        self.under.push(influence);
        self
    }

    /// Whether every setting is usable: opacity from 0 to 1, bands running
    /// from low to high, noise wavelengths more than 0 and reaches 0 or
    /// more.
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        let reach = |r: Reach| match r {
            Reach::Metres(v) | Reach::Footprint(v) => v.is_finite() && v >= 0.0,
        };
        (0.0..=1.0).contains(&self.opacity)
            && self.when.iter().all(Condition::is_valid)
            && self.noise.as_ref().is_none_or(GroundNoise::is_valid)
            && self
                .under
                .iter()
                .all(|i| reach(i.reach) && i.falloff.is_valid())
    }
}
