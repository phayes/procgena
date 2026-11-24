//! Ground layers and the rules that say where each one lies.

use glam::DVec2;
use procgena_core::ChildKey;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::GroundSite;
use crate::RoadTags;
use crate::noise::gradient_noise;

/// A soft band over one measure, such as a slope or a height: 0 outside
/// it, 1 inside it, with smooth edges.
///
/// `rise: [a, b]` brings the band in from 0 at `a` to 1 at `b`, and
/// `fall: [c, d]` takes it out again from 1 at `c` to 0 at `d`. Leave out
/// `rise` for a band that is full from the lowest values, and `fall` for
/// one that stays full up to the highest. Both edges are smoothsteps.
///
/// ```yaml
/// rise: [0.45, 0.65]   # steeper than about 27°, fully by about 40°
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

    /// The band's value at `x`, from 0 to 1.
    pub fn at(&self, x: f32) -> f32 {
        let rise = self.rise.map_or(1.0, |[a, b]| smoothstep(a, b, x));
        let fall = self.fall.map_or(1.0, |[c, d]| 1.0 - smoothstep(c, d, x));
        rise * fall
    }

    /// Whether every value is a number (infinities allowed) and each edge
    /// runs from low to high.
    pub fn is_valid(&self) -> bool {
        let edge =
            |e: Option<[f32; 2]>| e.is_none_or(|[a, b]| !a.is_nan() && !b.is_nan() && a <= b);
        edge(self.rise) && edge(self.fall)
    }
}

/// Hermite smoothstep from 0 at `a` to 1 at `b`, as in shading languages.
/// A step at `a` when `a == b`.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if a == b {
        return if x < a { 0.0 } else { 1.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What a height rule measures a height against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HeightRule {
    /// Height in metres, written for a World of vertical scale 1: the
    /// band's values are multiplied by the World's vertical scale.
    Absolute(Ramp),
    /// Height within the region's relief, from 0 at the valley floor to 1
    /// at the highest peak. Never scaled.
    Relief(Ramp),
}

/// One condition of a ground layer's rule. Each gives a factor from 0 to
/// 1, and a layer's conditions are multiplied together.
///
/// A condition whose measure the [`GroundSite`] doesn't have counts as 1,
/// so it has no effect: see [`Condition::factor`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// The ground's slope, as the sine of its angle from flat: 0 flat, 0.5
    /// at 30°, 1 for a wall.
    Slope(Ramp),
    /// The ground's height.
    Height(HeightRule),
    /// How far the ground lies above the water surface covering it, in
    /// metres: negative under water. Ground no water covers counts as
    /// infinitely high above it.
    HeightAboveWater(Ramp),
    /// 1 where a road gives the ground any of these tags, otherwise 0.
    RoadTags(#[serde(with = "tag_names")] RoadTags),
}

impl Condition {
    /// The condition's factor at `site`, from 0 to 1, or `None` if the site
    /// lacks the measure it reads.
    ///
    /// `vertical` is the World's vertical scale, which multiplies
    /// [`HeightRule::Absolute`] bands.
    pub fn factor(&self, site: &GroundSite, vertical: f32) -> Option<f32> {
        match self {
            Self::Slope(ramp) => Some(ramp.at(site.slope)),
            Self::Height(HeightRule::Absolute(ramp)) => Some(ramp.at(site.height / vertical)),
            Self::Height(HeightRule::Relief(ramp)) => site.relief_height.map(|h| ramp.at(h)),
            Self::HeightAboveWater(ramp) => site.height_above_water().map(|h| ramp.at(h)),
            Self::RoadTags(wanted) => site.road_tags.map(|tags| {
                let any = ROAD_TAGS
                    .iter()
                    .any(|&tag| wanted.contains(tag) && tags.contains(tag));
                if any { 1.0 } else { 0.0 }
            }),
        }
    }

    /// Whether the condition's settings are usable.
    fn is_valid(&self) -> bool {
        match self {
            Self::Slope(ramp)
            | Self::Height(HeightRule::Absolute(ramp) | HeightRule::Relief(ramp))
            | Self::HeightAboveWater(ramp) => ramp.is_valid(),
            Self::RoadTags(_) => true,
        }
    }
}

/// Every single road tag.
const ROAD_TAGS: [RoadTags; 6] = [
    RoadTags::ROAD,
    RoadTags::CUT,
    RoadTags::EMBANKMENT,
    RoadTags::RETAINING_WALL,
    RoadTags::BLUFF,
    RoadTags::CLIFF,
];

/// Reads and writes [`RoadTags`] as a list of tag names, such as
/// `[road, cut]`.
mod tag_names {
    use super::*;

    pub(super) fn serialize<S: Serializer>(tags: &RoadTags, s: S) -> Result<S::Ok, S::Error> {
        tags.names().serialize(s)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<RoadTags, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        names.iter().try_fold(RoadTags::NONE, |tags, name| {
            let tag = ROAD_TAGS
                .iter()
                .find(|tag| tag.names()[0] == name)
                .ok_or_else(|| {
                    D::Error::custom(format!(
                        "unknown road tag {name:?}; the tags are road, cut, embankment, \
                         retaining_wall, bluff and cliff"
                    ))
                })?;
            Ok(tags | *tag)
        })
    }
}

/// Patchy noise that breaks a layer up, seeded per layer.
///
/// The noise runs from 0 to 1, with patches about `wavelength` metres
/// across. The factor is `(noise − threshold) × contrast + 0.5`, kept
/// within 0 to 1: a higher threshold leaves fewer patches, and a higher
/// contrast sharpens their edges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroundNoise {
    /// Patch size in metres. More than 0.
    pub wavelength: f32,
    /// How sharp patch edges are. 0 or more; 0 gives a flat 0.5.
    pub contrast: f32,
    /// The noise value at which the factor is 0.5.
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
    /// The noise factor at world position `p` for a layer seeded `seed`,
    /// from 0 to 1.
    pub fn factor(&self, p: DVec2, seed: u32) -> f32 {
        let q = p / f64::from(self.wavelength);
        // Two octaves, so patches aren't lined up on one lattice.
        let coarse = gradient_noise(q, seed).value;
        let fine = gradient_noise(q * 2.03 + DVec2::new(17.3, -5.1), seed ^ 0x9e37_79b9).value;
        // Gradient noise lies within about ±0.7.
        let noise = ((0.65 * coarse + 0.35 * fine) / 1.4 + 0.5).clamp(0.0, 1.0);
        ((noise - self.threshold) * self.contrast + 0.5).clamp(0.0, 1.0)
    }

    fn is_valid(&self) -> bool {
        self.wavelength > 0.0
            && self.wavelength.is_finite()
            && self.contrast >= 0.0
            && self.contrast.is_finite()
            && self.threshold.is_finite()
    }
}

/// How far a [`ScatterInfluence`] reaches from each copy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Reach {
    /// This many metres beyond the edge of the copy's footprint.
    Metres(f32),
    /// This many times the footprint's radius, from its middle.
    Footprint(f32),
}

impl Reach {
    /// The distance from a footprint's middle (its centre, or the line
    /// along a lying copy) to the influence's outer edge, for a footprint
    /// of `radius` metres.
    pub fn outer(self, radius: f32) -> f32 {
        match self {
            Self::Metres(m) => radius + m,
            Self::Footprint(k) => radius * k,
        }
    }
}

/// A ground layer following one of its Ecotope's own Scatters, such as
/// needle litter under pines or bare dirt round boulders.
///
/// Only an Ecotope's layers may have influences; the terrain's and a
/// Biome's layers can't, since they have no Scatters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterInfluence {
    /// The key of the Scatter in the same Ecotope.
    pub scatter: ChildKey,
    /// How far the influence reaches from each copy's footprint (the
    /// ground the copy keeps other Scatters out of).
    pub reach: Reach,
    /// The influence across its reach, over 0 at a footprint's middle to 1
    /// at the reach's outer edge. Beyond the reach it is 0.
    #[serde(default = "default_falloff")]
    pub falloff: Ramp,
    /// Whether to turn the influence round: 1 away from the copies and 0
    /// near them.
    #[serde(default)]
    pub invert: bool,
}

/// Full near the copy, fading out over the outer half of the reach.
fn default_falloff() -> Ramp {
    Ramp::fall(0.5, 1.0)
}

impl ScatterInfluence {
    /// Whether the settings are usable.
    fn is_valid(&self) -> bool {
        let reach = match self.reach {
            Reach::Metres(v) | Reach::Footprint(v) => v.is_finite() && v >= 0.0,
        };
        reach && self.falloff.is_valid()
    }
}

/// One ground layer: a material and the rule that says how much of it
/// lies at each point.
///
/// A layer's weight at a point is its `opacity` times every condition in
/// `when`, times its noise, times its Scatter influence. Layers are laid
/// bottom to top by `order`, each covering its weight of what lies below.
///
/// ```yaml
/// material: cliff_rock
/// order: 650
/// when:
///   - slope: { rise: [0.45, 0.65] }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundLayer {
    /// The key of the material in the World's catalogue.
    pub material: ChildKey,
    /// Where the layer lies in the stack: higher numbers lie on top. One
    /// scale for every stack, so a Biome's and an Ecotope's layers
    /// interleave. Suggested bands: 0 to 299 soils and bare ground, 300 to
    /// 599 litter and moss, 600 to 899 surfaces that lie on everything
    /// (road, riverbed, cliff rock), 900 and up weather (snow).
    pub order: u32,
    /// The layer's strength, from 0 to 1.
    #[serde(default = "one")]
    pub opacity: f32,
    /// The conditions, all multiplied together. None means everywhere.
    #[serde(default)]
    pub when: Vec<Condition>,
    /// Optional noise breaking the layer into patches.
    #[serde(default)]
    pub noise: Option<GroundNoise>,
    /// Scatters the layer follows, in an Ecotope; empty elsewhere. Where
    /// there are several, the strongest at each point counts. None means
    /// the layer doesn't follow any Scatter.
    #[serde(default)]
    pub under: Vec<ScatterInfluence>,
}

fn one() -> f32 {
    1.0
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
    #[must_use]
    pub fn when(mut self, condition: Condition) -> Self {
        self.when.push(condition);
        self
    }

    /// The same layer at `opacity`.
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// The same layer broken up by `noise`.
    #[must_use]
    pub fn with_noise(mut self, noise: GroundNoise) -> Self {
        self.noise = Some(noise);
        self
    }

    /// The same layer following `influence`.
    #[must_use]
    pub fn under(mut self, influence: ScatterInfluence) -> Self {
        self.under.push(influence);
        self
    }

    /// Whether the layer covers everything at full strength: full opacity,
    /// no conditions, no noise and no influences.
    pub fn is_opaque(&self) -> bool {
        self.opacity == 1.0 && self.when.is_empty() && self.noise.is_none() && self.under.is_empty()
    }

    /// The layer's weight at `p` and `site` before any Scatter influence:
    /// its opacity times its conditions and noise. `seed` is the layer's
    /// own seed, and `vertical` the World's vertical scale.
    pub fn weight(&self, p: DVec2, site: &GroundSite, seed: u32, vertical: f32) -> f32 {
        let mut weight = self.opacity;
        for condition in &self.when {
            if weight == 0.0 {
                return 0.0;
            }
            weight *= condition.factor(site, vertical).unwrap_or(1.0);
        }
        if let Some(noise) = &self.noise
            && weight > 0.0
        {
            weight *= noise.factor(p, seed);
        }
        weight
    }

    /// Whether every setting is usable (apart from the material key, which
    /// only a catalogue can check).
    pub(super) fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.opacity)
            && self.when.iter().all(Condition::is_valid)
            && self.noise.as_ref().is_none_or(GroundNoise::is_valid)
            && self.under.iter().all(ScatterInfluence::is_valid)
    }
}
