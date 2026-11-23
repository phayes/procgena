//! Levels of detail: which version of a specimen to draw at which distance.
//!
//! A level of detail (LOD) is a simpler version of a specimen, drawn where
//! the viewer is too far away to see the difference. An [`Lod`] lists an
//! asset's levels from nearest to farthest, the distance where each one
//! starts, and where drawing stops altogether. Its distances are written
//! for one reference size, and are multiplied by a factor that grows with
//! each copy's size and with the [`Quality`] setting, so a copy twice as big
//! keeps its detail twice as far away.
//!
//! Neighbouring levels cross-fade over a band around each switching distance
//! rather than swapping in one frame. [`Lod::ranges`] gives each level's
//! fade bands in metres, and [`LodRange::dither_level`] the fade amount at
//! any distance, as the shared dither shader in `procgena_core_bevy` uses it.

use std::fmt;
use std::ops::{Range, RangeInclusive};

use glam::Vec4;

use crate::Quality;

/// The named, cheap form an asset switches to far away, after its meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[non_exhaustive]
pub enum FarForm {
    /// Stems or branches drawn as flat strips that turn to face the camera.
    Strips,
    /// Leaves drawn as cards that turn to face the camera.
    BillboardLeaves,
    /// The whole specimen drawn as one or a few flat cards.
    Card,
}

/// What kind of geometry a level draws. Levels must run in this order:
/// meshes first, then far forms, then at most one impostor as the last
/// level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum LevelForm {
    /// A triangle mesh of the specimen, simpler at each level.
    #[default]
    Mesh,
    /// The asset's far form.
    Far(FarForm),
    /// A baked image of the specimen on a camera-facing quad, from
    /// `procgena_impostor`.
    Impostor,
}

impl LevelForm {
    /// The position of this kind of level in the required order.
    fn order(self) -> u8 {
        match self {
            Self::Mesh => 0,
            Self::Far(_) => 1,
            Self::Impostor => 2,
        }
    }
}

/// One level of an [`Lod`]: the distance where it starts, how it fades in,
/// what it draws, and the asset's own settings for it in `detail`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LodLevel<D = ()> {
    /// Distance in metres, at the reference size and [`Quality::High`],
    /// where this level takes over from the one before. The first level
    /// must start at 0.
    pub distance: f32,
    /// Half the width of the cross-fade with the level before, as a share
    /// of `distance`, from 0 up to but not including 1. For example, 0.1
    /// at 10 m fades from 9 m to 11 m. 0 swaps levels in one step.
    pub hysteresis: f32,
    /// What kind of geometry this level draws.
    pub form: LevelForm,
    /// Whether this level is drawn into shadow maps. Usually off for the
    /// last, smallest levels.
    pub casts_shadows: bool,
    /// The asset's own settings for this level, such as a mesh resolution.
    pub detail: D,
}

impl<D> LodLevel<D> {
    /// A [`Mesh`](LevelForm::Mesh) level that casts shadows.
    pub fn new(distance: f32, hysteresis: f32, detail: D) -> Self {
        Self {
            distance,
            hysteresis,
            form: LevelForm::Mesh,
            casts_shadows: true,
            detail,
        }
    }

    /// This level with `form` in place of its current form.
    pub fn with_form(mut self, form: LevelForm) -> Self {
        self.form = form;
        self
    }

    /// This level, not drawn into shadow maps.
    pub fn without_shadows(mut self) -> Self {
        self.casts_shadows = false;
        self
    }
}

/// An asset's levels of detail, nearest first, and where it stops being
/// drawn.
///
/// Distances are written for a copy [`reference_size`](Self::reference_size)
/// metres across at [`Quality::High`]. To draw a copy, work out its factor
/// with [`factor`](Self::factor), then pass it to [`ranges`](Self::ranges)
/// or [`level_at`](Self::level_at). Check new values with
/// [`validate`](Self::validate) first; the other methods assume a valid
/// `Lod`.
///
/// ```
/// use procgena_core::{Lod, LodLevel, Quality};
///
/// // Written for a 0.5 m piece: a fine mesh, a coarse one from 10 m, and
/// // nothing beyond 40 m.
/// let lod = Lod::new(0.5, vec![LodLevel::new(0.0, 0.1, 64), LodLevel::new(10.0, 0.1, 16)])
///     .with_cull(40.0);
/// lod.validate().unwrap();
///
/// // A 1 m piece at Low quality keeps each level 2 × 0.4 times as far.
/// let factor = lod.factor(1.0, Quality::Low);
/// assert_eq!(lod.level_at(9.0, factor), Some(1));
/// assert_eq!(lod.level_at(40.0, factor), None);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lod<D = ()> {
    /// Size in metres of the copy the distances are written for. Must be
    /// positive. How an asset measures a copy's size (height, diameter,
    /// cap width) is up to the asset, as long as it measures the same way
    /// when it calls [`factor`](Self::factor).
    pub reference_size: f32,
    /// The levels, nearest first.
    pub levels: Vec<LodLevel<D>>,
    /// Distance in metres, at the reference size and [`Quality::High`],
    /// beyond which nothing is drawn. It fades out over the last level's
    /// hysteresis band. `None` draws the last level at every distance.
    #[cfg_attr(feature = "serde", serde(default))]
    pub cull_distance: Option<f32>,
}

impl<D> Lod<D> {
    /// Levels written for a copy `reference_size` metres across, with no
    /// cull distance.
    pub fn new(reference_size: f32, levels: Vec<LodLevel<D>>) -> Self {
        Self {
            reference_size,
            levels,
            cull_distance: None,
        }
    }

    /// These levels, no longer drawn beyond `distance` metres (at the
    /// reference size and [`Quality::High`]).
    pub fn with_cull(mut self, distance: f32) -> Self {
        self.cull_distance = Some(distance);
        self
    }

    /// The same levels with each level's `detail` replaced by `f(detail)`.
    pub fn map_detail<E>(self, mut f: impl FnMut(D) -> E) -> Lod<E> {
        Lod {
            reference_size: self.reference_size,
            levels: self
                .levels
                .into_iter()
                .map(|level| LodLevel {
                    distance: level.distance,
                    hysteresis: level.hysteresis,
                    form: level.form,
                    casts_shadows: level.casts_shadows,
                    detail: f(level.detail),
                })
                .collect(),
            cull_distance: self.cull_distance,
        }
    }

    /// The factor every distance is multiplied by for a copy `size` metres
    /// across, drawn at `quality`: `size / reference_size` times
    /// [`Quality::distance_scale`].
    ///
    /// Further factors, such as one for a zoomed-in camera, can be
    /// multiplied in before the result is passed on.
    pub fn factor(&self, size: f32, quality: Quality) -> f32 {
        size / self.reference_size * quality.distance_scale()
    }

    /// Each level's fade bands, in metres, for a copy with `factor` (see
    /// [`factor`](Self::factor)). One entry per level, nearest first.
    ///
    /// The first level never fades in. Each later level fades in over
    /// `distance × (1 ± hysteresis)` scaled by `factor`, while the level
    /// before it fades out over the same band. The last level fades out
    /// over the cull distance's band, using the last level's hysteresis, or
    /// never fades out when there is no cull distance.
    pub fn ranges(&self, factor: f32) -> Vec<LodRange> {
        let band = |distance: f32, hysteresis: f32| {
            let distance = distance * factor;
            let half = distance * hysteresis;
            (distance - half).max(0.0)..distance + half
        };

        // Where the last level fades out.
        let last_hysteresis = self.levels.last().map_or(0.0, |level| level.hysteresis);
        let end = match self.cull_distance {
            Some(cull) => band(cull, last_hysteresis),
            None => f32::INFINITY..f32::INFINITY,
        };

        // Each level fades out where the next fades in.
        (0..self.levels.len())
            .map(|i| LodRange {
                fade_in: if i == 0 {
                    0.0..0.0
                } else {
                    band(self.levels[i].distance, self.levels[i].hysteresis)
                },
                fade_out: match self.levels.get(i + 1) {
                    Some(next) => band(next.distance, next.hysteresis),
                    None => end.clone(),
                },
            })
            .collect()
    }

    /// Like [`ranges`](Self::ranges), but with drawing ending over `end`
    /// (in metres) as well: levels that would only start fading in from
    /// `end.start` on are left out, and the last remaining level fades out
    /// over `end`, unless it already ends before `end` starts.
    ///
    /// Use it when something else takes over from these levels at a known
    /// distance, such as a tree's impostor, which already shows the fruit
    /// on it, ending the fruit's own levels. `end` should start after the
    /// last remaining level has fully faded in.
    pub fn ranges_ending_at(&self, factor: f32, end: Range<f32>) -> Vec<LodRange> {
        let mut ranges = self.ranges(factor);
        ranges.retain(|range| range.fade_in.start < end.start);
        if let Some(last) = ranges.last_mut()
            && end.start < last.fade_out.end
        {
            last.fade_out = end;
        }
        ranges
    }

    /// The single level to draw `distance` metres away for a copy with
    /// `factor`, or `None` beyond the cull distance: the level whose
    /// switching distance was passed last, ignoring fade bands.
    ///
    /// Use it where only one level can be drawn, such as for physics or
    /// shadow maps, rather than [`ranges`](Self::ranges).
    pub fn level_at(&self, distance: f32, factor: f32) -> Option<usize> {
        if let Some(cull) = self.cull_distance
            && distance >= cull * factor
        {
            return None;
        }
        let passed = self
            .levels
            .iter()
            .skip(1)
            .take_while(|level| distance >= level.distance * factor)
            .count();
        (!self.levels.is_empty()).then_some(passed)
    }

    /// The level to draw into shadow maps `distance` metres away for a copy
    /// with `factor`: [`level_at`](Self::level_at), or `None` if that level
    /// doesn't [cast shadows](LodLevel::casts_shadows).
    pub fn shadow_level_at(&self, distance: f32, factor: f32) -> Option<usize> {
        self.level_at(distance, factor)
            .filter(|&level| self.levels[level].casts_shadows)
    }

    /// The levels that copies somewhere within `distances` metres, with a
    /// factor somewhere within `factors`, can be drawn at, fade bands
    /// included; `None` if none can be drawn.
    ///
    /// Use it to sort many small copies at once: when a group of copies
    /// (such as the fruit clusters on one tree) gives a single level, every
    /// copy in the group can be drawn at that level without checking each
    /// one. `factors` must be positive. To end the levels at a distance, as
    /// [`ranges_ending_at`](Self::ranges_ending_at) does, end `distances`
    /// there first.
    pub fn levels_touched(
        &self,
        distances: RangeInclusive<f32>,
        factors: RangeInclusive<f32>,
    ) -> Option<RangeInclusive<usize>> {
        if distances.start() > distances.end() {
            return None;
        }

        // A copy at distance d with factor f sits at d / f in reference
        // distances, where every band is the same for all copies.
        let near = distances.start() / factors.end();
        let far = distances.end() / factors.start();
        let ranges = self.ranges(1.0);
        let touched = |range: &LodRange| range.fade_in.start <= far && near < range.fade_out.end;
        let first = ranges.iter().position(touched)?;
        let last = ranges.iter().rposition(touched)?;
        Some(first..=last)
    }

    /// Checks the levels and returns the first problem found.
    ///
    /// # Errors
    ///
    /// Returns an [`LodError`] when the reference size isn't positive,
    /// there are no levels, the first level doesn't start at 0, distances
    /// don't increase, a hysteresis is outside 0 up to 1, the levels' forms
    /// are out of order, two fade bands overlap, or the cull distance isn't
    /// beyond the last level.
    pub fn validate(&self) -> Result<(), LodError> {
        if !(self.reference_size.is_finite() && self.reference_size > 0.0) {
            return Err(LodError::ReferenceSize(self.reference_size));
        }
        let Some(first) = self.levels.first() else {
            return Err(LodError::NoLevels);
        };
        if first.distance != 0.0 {
            return Err(LodError::FirstDistance(first.distance));
        }

        // Each level on its own, and against the one before.
        for (i, level) in self.levels.iter().enumerate() {
            if !(0.0..1.0).contains(&level.hysteresis) {
                return Err(LodError::Hysteresis {
                    level: i,
                    hysteresis: level.hysteresis,
                });
            }
            let impostor_not_last = level.form == LevelForm::Impostor && i + 1 != self.levels.len();
            let Some(previous) = i.checked_sub(1).map(|p| &self.levels[p]) else {
                if impostor_not_last {
                    return Err(LodError::FormOrder { level: i });
                }
                continue;
            };
            if !(level.distance.is_finite() && level.distance > previous.distance) {
                return Err(LodError::Distance {
                    level: i,
                    distance: level.distance,
                });
            }
            if impostor_not_last || level.form.order() < previous.form.order() {
                return Err(LodError::FormOrder { level: i });
            }
        }

        // Fade bands must not overlap, or a level would fade in and out at
        // once. The cull band counts as one more band.
        let bands = self.ranges(1.0);
        for i in 2..bands.len() {
            if bands[i].fade_in.start < bands[i - 1].fade_in.end {
                return Err(LodError::BandsOverlap { level: i });
            }
        }
        if let Some(cull) = self.cull_distance {
            let last = bands.last().expect("at least one level");
            if !(cull.is_finite()
                && cull > self.levels[self.levels.len() - 1].distance
                && last.fade_out.start >= last.fade_in.end)
            {
                return Err(LodError::CullDistance(cull));
            }
        }
        Ok(())
    }
}

/// Where one level of detail is drawn, in metres from the viewer: it fades
/// in over `fade_in`, is fully drawn between the two bands, and fades out
/// over `fade_out`.
///
/// The first level's `fade_in` is `0.0..0.0`. A level that never fades out
/// has a `fade_out` of `f32::INFINITY..f32::INFINITY`.
#[derive(Debug, Clone, PartialEq)]
pub struct LodRange {
    /// Distances over which the level fades in.
    pub fade_in: Range<f32>,
    /// Distances over which the level fades out.
    pub fade_out: Range<f32>,
}

impl LodRange {
    /// A range drawn fully at every distance.
    pub const ALWAYS: Self = Self {
        fade_in: 0.0..0.0,
        fade_out: f32::INFINITY..f32::INFINITY,
    };

    /// How faded the level is `distance` metres away, as a dither level
    /// from -16 to 16: 0 is fully drawn, -16 to 0 fading in, 0 to 16 fading
    /// out. -16 before the level starts and 16 after it ends mean it isn't
    /// drawn at all.
    ///
    /// The same numbers as Bevy's visibility-range dithering and the
    /// `dither_level` function of `procgena_core_bevy`'s dither shader, so a
    /// level drawn by Bevy and one drawn by a custom renderer cross-fade
    /// pixel for pixel.
    pub fn dither_level(&self, distance: f32) -> i32 {
        if distance < self.fade_in.start {
            return -16;
        }
        if distance >= self.fade_out.end {
            return 16;
        }
        if distance < self.fade_in.end {
            return -16 + band_level(&self.fade_in, distance);
        }
        if distance < self.fade_out.start {
            return 0;
        }
        band_level(&self.fade_out, distance)
    }

    /// Whether any part of the level is drawn `distance` metres away.
    pub fn is_drawn(&self, distance: f32) -> bool {
        (-15..=15).contains(&self.dither_level(distance))
    }

    /// Whether the level is fully drawn at every distance, so a renderer
    /// can skip fading it.
    pub fn is_always(&self) -> bool {
        self.fade_in.end <= 0.0 && self.fade_out.start == f32::INFINITY
    }

    /// The range packed for a GPU buffer as `(fade_in.start, fade_in.end,
    /// fade_out.start, fade_out.end)`, the layout the dither shader reads.
    /// Infinite distances become `f32::MAX`.
    pub fn to_vec4(&self) -> Vec4 {
        Vec4::new(
            self.fade_in.start,
            self.fade_in.end,
            self.fade_out.start,
            self.fade_out.end,
        )
        .min(Vec4::splat(f32::MAX))
    }
}

/// How far `distance` is through `band`, from 0 to 16, rounded as WGSL's
/// `round` does. A band of zero width counts as passed.
fn band_level(band: &Range<f32>, distance: f32) -> i32 {
    let width = band.end - band.start;
    if width <= 0.0 {
        return 16;
    }
    (((distance - band.start) / width * 16.0).round_ties_even() as i32).clamp(0, 16)
}

/// A problem found by [`Lod::validate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LodError {
    /// The reference size is not a positive, finite number.
    ReferenceSize(f32),
    /// There are no levels.
    NoLevels,
    /// The first level starts at this distance instead of 0.
    FirstDistance(f32),
    /// This level's distance is not finite or not beyond the level before.
    Distance {
        /// The level, counting from 0 for the nearest.
        level: usize,
        /// Its distance.
        distance: f32,
    },
    /// This level's hysteresis is outside 0 up to 1.
    Hysteresis {
        /// The level, counting from 0 for the nearest.
        level: usize,
        /// Its hysteresis.
        hysteresis: f32,
    },
    /// This level's form comes before the previous level's in the order
    /// meshes, far forms, impostor, or it is an impostor that isn't last.
    FormOrder {
        /// The level, counting from 0 for the nearest.
        level: usize,
    },
    /// This level starts fading in before the level before it has finished
    /// fading in. Move the levels apart or narrow their hysteresis.
    BandsOverlap {
        /// The level, counting from 0 for the nearest.
        level: usize,
    },
    /// The cull distance is not finite, not beyond the last level, or its
    /// fade band overlaps the last level's.
    CullDistance(f32),
}

impl fmt::Display for LodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceSize(size) => write!(f, "LOD reference size {size} is not positive"),
            Self::NoLevels => write!(f, "LOD has no levels"),
            Self::FirstDistance(distance) => {
                write!(f, "first LOD level starts at {distance} m, not 0")
            }
            Self::Distance { level, distance } => write!(
                f,
                "LOD level {level} starts at {distance} m, not beyond the level before"
            ),
            Self::Hysteresis { level, hysteresis } => write!(
                f,
                "LOD level {level} has hysteresis {hysteresis}, outside 0 up to 1"
            ),
            Self::FormOrder { level } => write!(
                f,
                "LOD level {level} is out of order: meshes, then far forms, then one impostor last"
            ),
            Self::BandsOverlap { level } => write!(
                f,
                "LOD level {level} starts fading in before the level before it has faded in"
            ),
            Self::CullDistance(distance) => write!(
                f,
                "LOD cull distance {distance} m is not beyond the last level's fade band"
            ),
        }
    }
}

impl std::error::Error for LodError {}
