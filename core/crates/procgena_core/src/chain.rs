//! The drawing chain every asset kind shares: the preset's game-level mesh,
//! one coarser mesh, then an impostor, chosen by size on screen.
//!
//! A preset says only what its object is, including its `game_level`: the
//! one mesh detail that is the object's real geometry. How a copy is drawn
//! is the same for every kind:
//!
//! 1. [`Step::Game`]: the `game_level` mesh, while the copy is large on
//!    screen.
//! 2. [`Step::Lower`]: a coarser mesh, derived from `game_level` by the
//!    kind's [`LowerDetail`] rule.
//! 3. [`Step::Impostor`]: a baked image of the `game_level` mesh, until the
//!    copy is too small to draw.
//!
//! An [`Lod`] holds the two mesh details. A [`DistanceChain`] says where
//! each step takes over, as a copy's size on screen: its bounding-sphere
//! diameter as a share of the screen's height. [`DistanceChain::factor`]
//! turns a copy's size and the camera's [`ViewScale`] into one number, and
//! [`DistanceChain::ranges`] turns that and the [`Quality`] setting into
//! fade bands in metres, the same [`LodRange`]s the shared dither shader
//! reads.
//!
//! ```
//! use procgena_core::Quality;
//! use procgena_core::chain::{DistanceChain, Step, ViewScale};
//!
//! let chain = DistanceChain::DEFAULT;
//! // A rock 2 m across, seen through a 45° camera at High quality.
//! let factor = DistanceChain::factor(2.0, ViewScale::DEFAULT);
//! let high = Quality::High;
//! assert_eq!(chain.step_at(10.0, factor, high), Some(Step::Game));
//! assert_eq!(chain.step_at(40.0, factor, high), Some(Step::Lower));
//! assert_eq!(chain.step_at(100.0, factor, high), Some(Step::Impostor));
//! assert_eq!(chain.step_at(2000.0, factor, high), None);
//! // At Low quality the same rock reaches its impostor sooner.
//! assert_eq!(chain.step_at(40.0, factor, Quality::Low), Some(Step::Impostor));
//! ```

use std::fmt;
use std::ops::{Range, RangeInclusive};

use crate::{ContentHash, ContentHasher, LodRange, Quality};

/// One step of the drawing chain, nearest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Step {
    /// The preset's `game_level` mesh: the object's real geometry, the shape
    /// the rest of the game uses.
    Game,
    /// A coarser mesh, derived from `game_level` by the kind's
    /// [`LowerDetail`] rule.
    Lower,
    /// A baked image of the `game_level` mesh on a camera-facing quad.
    Impostor,
}

impl Step {
    /// Every step, nearest first.
    pub const ALL: [Self; 3] = [Self::Game, Self::Lower, Self::Impostor];

    /// The step's position in [`ALL`](Self::ALL), and in the arrays
    /// [`DistanceChain::ranges`] returns: 0, 1 or 2.
    pub const fn index(self) -> usize {
        match self {
            Self::Game => 0,
            Self::Lower => 1,
            Self::Impostor => 2,
        }
    }

    /// Whether the step draws a mesh: [`Game`](Self::Game) and
    /// [`Lower`](Self::Lower).
    pub const fn is_mesh(self) -> bool {
        !matches!(self, Self::Impostor)
    }

    /// Whether the step is drawn into shadow maps. The two meshes are;
    /// impostors are not.
    pub const fn casts_shadows(self) -> bool {
        self.is_mesh()
    }

    /// The step's name for menus and read-outs: "Game level", "Lower mesh"
    /// or "Impostor".
    pub const fn name(self) -> &'static str {
        match self {
            Self::Game => "Game level",
            Self::Lower => "Lower mesh",
            Self::Impostor => "Impostor",
        }
    }
}

/// An asset kind's rule for its coarser mesh.
///
/// Each kind implements it for its own detail type, so a preset only needs
/// to say its `game_level` detail.
pub trait LowerDetail: Sized {
    /// The coarser detail drawn after `self` when `self` is the game level.
    /// It should be valid whenever `self` is.
    fn lower(&self) -> Self;
}

/// The two mesh details of one copy's chain, in the kind's own detail type
/// `D`: the `game_level` detail and the coarser one after it. The third
/// step, the impostor, is baked from `game`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lod<D> {
    /// The preset's `game_level` detail: the object's real geometry.
    pub game: D,
    /// The coarser detail drawn when the copy is smaller on screen.
    pub lower: D,
}

impl<D: LowerDetail> Lod<D> {
    /// `game`, and the lower detail the kind's [`LowerDetail`] rule derives
    /// from it.
    pub fn from_game(game: D) -> Self {
        let lower = game.lower();
        Self { game, lower }
    }
}

impl<D> Lod<D> {
    /// The mesh detail drawn at `step`, or `None` for
    /// [`Step::Impostor`].
    pub fn detail(&self, step: Step) -> Option<&D> {
        match step {
            Step::Game => Some(&self.game),
            Step::Lower => Some(&self.lower),
            Step::Impostor => None,
        }
    }

    /// The two mesh steps with their details, nearest first.
    pub fn meshes(&self) -> [(Step, &D); 2] {
        [(Step::Game, &self.game), (Step::Lower, &self.lower)]
    }

    /// The same chain with each detail replaced by `f(detail)`.
    pub fn map<E>(self, mut f: impl FnMut(D) -> E) -> Lod<E> {
        Lod {
            game: f(self.game),
            lower: f(self.lower),
        }
    }
}

impl<D: ContentHash> ContentHash for Lod<D> {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write(&self.game);
        hasher.write(&self.lower);
    }
}

/// How large things look through one camera: a copy `s` metres across,
/// `d` metres away, covers `s × scale / d` of the screen's height.
///
/// Different cameras need different scales: a narrow field of view makes
/// everything look larger, so detail is kept further away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewScale(f32);

impl ViewScale {
    /// The scale of a perspective camera with Bevy's default vertical field
    /// of view, 45°.
    pub const DEFAULT: Self = Self(1.207_106_8);

    /// The scale of a perspective camera whose vertical field of view is
    /// `fov_y` radians: `1 / (2 tan(fov_y / 2))`.
    ///
    /// # Panics
    ///
    /// Panics unless `fov_y` is between 0 and π, both excluded.
    pub fn perspective(fov_y: f32) -> Self {
        assert!(
            fov_y > 0.0 && fov_y < std::f32::consts::PI,
            "field of view {fov_y} is not between 0 and π"
        );
        Self(0.5 / (fov_y * 0.5).tan())
    }

    /// The scale as a number: the share of the screen's height that 1 metre
    /// covers 1 metre from the camera.
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for ViewScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Where each [`Step`] takes over, by a copy's size on screen: its
/// bounding-sphere diameter as a share of the screen's height, at
/// [`Quality::High`].
///
/// One chain serves every asset kind. Lower quality settings switch at
/// larger sizes (sooner), higher ones at smaller sizes (later): each
/// threshold is divided by [`Quality::distance_scale`].
///
/// Check a chain of your own with [`validate`](Self::validate) first; the
/// other methods assume a valid chain. Neighbouring steps cross-fade over a
/// band of `±hysteresis` around each switching distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceChain {
    /// Below this size on screen the lower mesh takes over from the game
    /// level. For example, 0.1 switches when a copy covers a tenth of the
    /// screen's height.
    pub lower_below: f32,
    /// Below this size on screen the impostor takes over from the lower
    /// mesh. Must be smaller than `lower_below`.
    pub impostor_below: f32,
    /// Below this size on screen nothing is drawn. Must be smaller than
    /// `impostor_below`.
    pub cull_below: f32,
    /// Half the width of each cross-fade, as a share of its switching
    /// distance, from 0 up to but not including 1. For example, 0.1 at
    /// 20 m fades from 18 m to 22 m. 0 swaps steps in one frame.
    pub hysteresis: f32,
}

impl DistanceChain {
    /// The project's default chain: the lower mesh below a tenth of the
    /// screen's height, the impostor below 4%, nothing below 0.2% (about
    /// 3 pixels on a 1440-pixel screen), fading over ±10%.
    pub const DEFAULT: Self = Self {
        lower_below: 0.10,
        impostor_below: 0.04,
        cull_below: 0.002,
        hysteresis: 0.1,
    };

    /// The factor for one copy `size` metres across (its bounding-sphere
    /// diameter), seen through `view`: the share of the screen's height it
    /// covers 1 metre from the camera. Pass it to [`ranges`](Self::ranges),
    /// [`step_at`](Self::step_at) and the other methods.
    ///
    /// The factor is linear in `size`, and so is every distance the chain
    /// gives, so a renderer can compute bands once for a 1 m copy and
    /// multiply them by each copy's size.
    pub fn factor(size: f32, view: ViewScale) -> f32 {
        size * view.get()
    }

    /// The three thresholds at `quality`, nearest step's first: where
    /// `Lower`, `Impostor` and the cull take over, each divided by
    /// [`Quality::distance_scale`].
    pub fn thresholds(&self, quality: Quality) -> [f32; 3] {
        let scale = quality.distance_scale();
        [
            self.lower_below / scale,
            self.impostor_below / scale,
            self.cull_below / scale,
        ]
    }

    /// The fade band, in metres, around the distance where a copy with
    /// `factor` shrinks to `threshold` of the screen.
    fn band(&self, threshold: f32, factor: f32) -> Range<f32> {
        let distance = factor / threshold;
        let half = distance * self.hysteresis;
        (distance - half).max(0.0)..distance + half
    }

    /// Each step's fade bands, in metres from the camera, for a copy with
    /// `factor` (see [`factor`](Self::factor)) at `quality`. Index the array
    /// with [`Step::index`].
    ///
    /// The game level never fades in. Each later step fades in where the
    /// one before fades out, and the impostor fades out at the cull.
    pub fn ranges(&self, factor: f32, quality: Quality) -> [LodRange; 3] {
        let [lower, impostor, cull] = self.thresholds(quality).map(|t| self.band(t, factor));
        [
            LodRange {
                fade_in: 0.0..0.0,
                fade_out: lower.clone(),
            },
            LodRange {
                fade_in: lower,
                fade_out: impostor.clone(),
            },
            LodRange {
                fade_in: impostor,
                fade_out: cull,
            },
        ]
    }

    /// Like [`ranges`](Self::ranges), but with drawing ending over `end`
    /// (in metres) instead of at the cull: steps that would only start
    /// fading in from `end.start` on are left out, and the last remaining
    /// step fades out over `end`, unless it already ends before `end`
    /// starts. Entry `i` is still step `i`.
    ///
    /// Use it when something else takes over at a known distance, such as
    /// a tree's impostor, which already shows the fruit on it, ending the
    /// fruit's own chain. `end` should start after the last remaining step
    /// has fully faded in.
    pub fn ranges_ending_at(
        &self,
        factor: f32,
        quality: Quality,
        end: Range<f32>,
    ) -> Vec<LodRange> {
        let mut ranges = self.ranges(factor, quality).to_vec();
        ranges.retain(|range| range.fade_in.start < end.start);
        if let Some(last) = ranges.last_mut()
            && end.start < last.fade_out.end
        {
            last.fade_out = end;
        }
        ranges
    }

    /// The single step to draw `distance` metres away for a copy with
    /// `factor` at `quality`, or `None` past the cull, ignoring fade bands.
    ///
    /// Use it where only one step can be drawn, such as for picking or to
    /// report which step is drawn.
    pub fn step_at(&self, distance: f32, factor: f32, quality: Quality) -> Option<Step> {
        let [lower, impostor, cull] = self.thresholds(quality).map(|t| factor / t);
        if distance >= cull {
            None
        } else if distance >= impostor {
            Some(Step::Impostor)
        } else if distance >= lower {
            Some(Step::Lower)
        } else {
            Some(Step::Game)
        }
    }

    /// The steps that copies somewhere within `distances` metres, with a
    /// factor somewhere within `factors`, can be drawn at, fade bands
    /// included; `None` if none can be drawn.
    ///
    /// Use it to sort many small copies at once: when a group of copies
    /// (such as the fruit clusters on one tree) gives a single step, every
    /// copy in the group can be drawn at that step without checking each
    /// one. `factors` must be positive.
    pub fn steps_touched(
        &self,
        distances: RangeInclusive<f32>,
        factors: RangeInclusive<f32>,
        quality: Quality,
    ) -> Option<RangeInclusive<Step>> {
        if distances.start() > distances.end() {
            return None;
        }

        // A copy at distance d with factor f sits at d / f for a factor of
        // 1, where every band is the same for all copies.
        let near = distances.start() / factors.end();
        let far = distances.end() / factors.start();
        let ranges = self.ranges(1.0, quality);
        let touched = |range: &LodRange| range.fade_in.start <= far && near < range.fade_out.end;
        let first = ranges.iter().position(touched)?;
        let last = ranges.iter().rposition(touched)?;
        Some(Step::ALL[first]..=Step::ALL[last])
    }

    /// The largest share of the screen's height an impostor is ever drawn
    /// at, at `quality`: its size where it starts fading in. An impostor's
    /// views need at least this share times the screen's height in pixels
    /// across, so the impostor is never shown magnified.
    pub fn impostor_max_size(&self, quality: Quality) -> f32 {
        let [_, impostor, _] = self.thresholds(quality);
        impostor / (1.0 - self.hysteresis)
    }

    /// The largest [`impostor_max_size`](Self::impostor_max_size) at any
    /// [`Quality`]: the size on screen impostor views must be baked for
    /// when one bake serves every quality.
    pub fn largest_impostor_size(&self) -> f32 {
        Quality::ALL
            .into_iter()
            .map(|quality| self.impostor_max_size(quality))
            .fold(0.0, f32::max)
    }

    /// Checks the chain and returns the first problem found.
    ///
    /// # Errors
    ///
    /// Returns a [`ChainError`] when a threshold isn't a positive finite
    /// number, the hysteresis is outside 0 up to 1, or, at any [`Quality`],
    /// the thresholds don't shrink from `lower_below` to `cull_below` or
    /// two fade bands overlap.
    pub fn validate(&self) -> Result<(), ChainError> {
        let fields = [self.lower_below, self.impostor_below, self.cull_below];
        for (i, threshold) in fields.into_iter().enumerate() {
            if !(threshold.is_finite() && threshold > 0.0) {
                return Err(ChainError::Threshold {
                    takes_over: TakesOver::ALL[i],
                    threshold,
                });
            }
        }
        if !(0.0..1.0).contains(&self.hysteresis) {
            return Err(ChainError::Hysteresis(self.hysteresis));
        }

        // At every quality, each threshold must be smaller than the one
        // before, and its fade band must start after the one before ends.
        for quality in Quality::ALL {
            let thresholds = self.thresholds(quality);
            let bands = thresholds.map(|t| self.band(t, 1.0));
            for i in 1..3 {
                if thresholds[i] >= thresholds[i - 1] {
                    return Err(ChainError::Order {
                        takes_over: TakesOver::ALL[i],
                    });
                }
                if bands[i].start < bands[i - 1].end {
                    return Err(ChainError::BandsOverlap {
                        takes_over: TakesOver::ALL[i],
                    });
                }
            }
        }
        Ok(())
    }
}

impl Default for DistanceChain {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ContentHash for DistanceChain {
    fn content_hash(&self, hasher: &mut ContentHasher) {
        hasher.write_f32(self.lower_below);
        hasher.write_f32(self.impostor_below);
        hasher.write_f32(self.cull_below);
        hasher.write_f32(self.hysteresis);
    }
}

/// What takes over at one of a [`DistanceChain`]'s thresholds, for naming
/// it in a [`ChainError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakesOver {
    /// The lower mesh, at `lower_below`.
    Lower,
    /// The impostor, at `impostor_below`.
    Impostor,
    /// The cull, at `cull_below`.
    Cull,
}

impl TakesOver {
    /// In the order of the chain.
    const ALL: [Self; 3] = [Self::Lower, Self::Impostor, Self::Cull];

    /// The threshold's field name.
    const fn field(self) -> &'static str {
        match self {
            Self::Lower => "lower_below",
            Self::Impostor => "impostor_below",
            Self::Cull => "cull_below",
        }
    }
}

/// A problem found by [`DistanceChain::validate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChainError {
    /// This threshold is not a positive, finite number.
    Threshold {
        /// Which threshold.
        takes_over: TakesOver,
        /// Its value.
        threshold: f32,
    },
    /// This threshold is not smaller than the one before it.
    Order {
        /// Which threshold.
        takes_over: TakesOver,
    },
    /// The hysteresis is outside 0 up to 1.
    Hysteresis(f32),
    /// This threshold's fade band starts before the one before it has
    /// ended. Move the thresholds apart or narrow the hysteresis.
    BandsOverlap {
        /// Which threshold.
        takes_over: TakesOver,
    },
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Threshold {
                takes_over,
                threshold,
            } => write!(
                f,
                "distance chain {} is {threshold}, not a positive number",
                takes_over.field()
            ),
            Self::Order { takes_over } => write!(
                f,
                "distance chain {} is not smaller than the threshold before it",
                takes_over.field()
            ),
            Self::Hysteresis(hysteresis) => write!(
                f,
                "distance chain hysteresis {hysteresis} is outside 0 up to 1"
            ),
            Self::BandsOverlap { takes_over } => write!(
                f,
                "distance chain {} starts fading before the step before it has faded in",
                takes_over.field()
            ),
        }
    }
}

impl std::error::Error for ChainError {}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_4;

    use super::*;

    #[test]
    fn the_default_scale_is_a_45_degree_camera() {
        let scale = ViewScale::perspective(FRAC_PI_4).get();
        assert!((scale - ViewScale::DEFAULT.get()).abs() < 1e-6, "{scale}");
    }
}
