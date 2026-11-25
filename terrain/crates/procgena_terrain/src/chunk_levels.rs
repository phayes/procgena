//! Chunk levels: which chunk level is drawn at each distance from the
//! viewer, and how far the ground is drawn.

use std::fmt;
use std::ops::Range;

use crate::MAX_CHUNK_LEVEL;

/// Where one chunk level takes over from the finer one before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkLevel {
    /// Distance in metres, at [`Quality::High`](procgena_core::Quality::High),
    /// where this chunk level takes over. Level 0 starts at 0.
    pub start: f32,
    /// Half the width of the switching band around `start`, as a share of
    /// it, from 0 up to but not including 1. For example, 0.1 at 100 m
    /// switches between 90 m and 110 m, depending on which way the viewer
    /// is moving.
    pub hysteresis: f32,
}

/// The terrain's chunk levels, finest first, and the view distance beyond
/// which no ground is drawn.
///
/// Entry `i` of [`levels`](Self::levels) is chunk level `i`. Distances are
/// for [`Quality::High`](procgena_core::Quality::High); a factor such as
/// [`Quality::distance_scale`](procgena_core::Quality::distance_scale)
/// scales them all when chunks are chosen. [`ChunkDetail::levels`](crate::ChunkDetail::levels)
/// and [`chunk_levels`](crate::chunk_levels) give the usual levels.
///
/// ```
/// use procgena_terrain::ChunkLevels;
///
/// // Level 1 from 96 m, level 2 from 192 m, drawn out to 1 km.
/// let levels = ChunkLevels::new([96.0, 192.0], 0.1, 1000.0);
/// levels.validate().unwrap();
/// assert_eq!(levels.levels.len(), 3);
/// assert_eq!(levels.fade_out(1.0), 900.0..1100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkLevels {
    /// The chunk levels, finest first.
    pub levels: Vec<ChunkLevel>,
    /// Distance in metres, at [`Quality::High`](procgena_core::Quality::High),
    /// beyond which no ground is drawn. The ground fades out over the last
    /// level's hysteresis band around it.
    pub view_distance: f32,
}

impl ChunkLevels {
    /// Level 0, then one level starting at each of `starts` in turn, all
    /// with the same `hysteresis`, drawn out to `view_distance` metres.
    pub fn new(starts: impl IntoIterator<Item = f32>, hysteresis: f32, view_distance: f32) -> Self {
        let level = |start| ChunkLevel { start, hysteresis };
        Self {
            levels: std::iter::once(level(0.0))
                .chain(starts.into_iter().map(level))
                .collect(),
            view_distance,
        }
    }

    /// The distances in metres over which the ground fades out at the view
    /// distance, with every distance multiplied by `factor`.
    pub fn fade_out(&self, factor: f32) -> Range<f32> {
        let hysteresis = self.levels.last().map_or(0.0, |level| level.hysteresis);
        band(self.view_distance * factor, hysteresis)
    }

    /// Checks the levels and returns the first problem found.
    ///
    /// # Errors
    ///
    /// Returns a [`ChunkLevelsError`] when there are no levels or more
    /// than [`MAX_CHUNK_LEVEL`] + 1, level 0 doesn't start at 0, the starts
    /// don't increase, a hysteresis is outside 0 up to 1, two switching
    /// bands overlap, or the view distance isn't finite and beyond the last
    /// level's band.
    pub fn validate(&self) -> Result<(), ChunkLevelsError> {
        let Some(first) = self.levels.first() else {
            return Err(ChunkLevelsError::NoLevels);
        };
        if self.levels.len() > MAX_CHUNK_LEVEL as usize + 1 {
            return Err(ChunkLevelsError::TooManyLevels(self.levels.len()));
        }
        if first.start != 0.0 {
            return Err(ChunkLevelsError::FirstStart(first.start));
        }
        for (i, level) in self.levels.iter().enumerate() {
            if !(0.0..1.0).contains(&level.hysteresis) {
                return Err(ChunkLevelsError::Hysteresis {
                    level: i,
                    hysteresis: level.hysteresis,
                });
            }
            let Some(previous) = i.checked_sub(1).map(|p| &self.levels[p]) else {
                continue;
            };
            if !(level.start.is_finite() && level.start > previous.start) {
                return Err(ChunkLevelsError::Start {
                    level: i,
                    start: level.start,
                });
            }
            // A level must finish switching in before the next one starts,
            // or the viewer would be between two switches at once.
            if i >= 2 {
                let before = band(previous.start, previous.hysteresis);
                if band(level.start, level.hysteresis).start < before.end {
                    return Err(ChunkLevelsError::BandsOverlap { level: i });
                }
            }
        }

        // The ground fades out only after the last level has switched in.
        let last = &self.levels[self.levels.len() - 1];
        let last_in = band(last.start, last.hysteresis).end;
        if !(self.view_distance.is_finite()
            && self.view_distance > last.start
            && self.fade_out(1.0).start >= last_in)
        {
            return Err(ChunkLevelsError::ViewDistance(self.view_distance));
        }
        Ok(())
    }
}

/// The band `distance × (1 ± hysteresis)`, never below 0.
fn band(distance: f32, hysteresis: f32) -> Range<f32> {
    let half = distance * hysteresis;
    (distance - half).max(0.0)..distance + half
}

/// A problem found by [`ChunkLevels::validate`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ChunkLevelsError {
    /// There are no levels.
    NoLevels,
    /// More levels than [`MAX_CHUNK_LEVEL`] allows.
    TooManyLevels(usize),
    /// Level 0 starts at this distance instead of 0.
    FirstStart(f32),
    /// This level's start is not finite or not beyond the level before.
    Start {
        /// The chunk level.
        level: usize,
        /// Its start.
        start: f32,
    },
    /// This level's hysteresis is outside 0 up to 1.
    Hysteresis {
        /// The chunk level.
        level: usize,
        /// Its hysteresis.
        hysteresis: f32,
    },
    /// This level's switching band starts before the band of the level
    /// before it ends. Move the levels apart or narrow their hysteresis.
    BandsOverlap {
        /// The chunk level.
        level: usize,
    },
    /// The view distance is not finite, or the ground would start fading
    /// out before the last level has switched in.
    ViewDistance(f32),
}

impl fmt::Display for ChunkLevelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLevels => write!(f, "there are no chunk levels"),
            Self::TooManyLevels(n) => write!(
                f,
                "{n} chunk levels, but at most {} are allowed",
                MAX_CHUNK_LEVEL as usize + 1
            ),
            Self::FirstStart(start) => {
                write!(f, "chunk level 0 starts at {start} m, not 0")
            }
            Self::Start { level, start } => write!(
                f,
                "chunk level {level} starts at {start} m, not beyond the level before"
            ),
            Self::Hysteresis { level, hysteresis } => write!(
                f,
                "chunk level {level} has hysteresis {hysteresis}, outside 0 up to 1"
            ),
            Self::BandsOverlap { level } => write!(
                f,
                "chunk level {level} starts switching in before the level before it has"
            ),
            Self::ViewDistance(distance) => write!(
                f,
                "the view distance {distance} m is not beyond the last chunk level's band"
            ),
        }
    }
}

impl std::error::Error for ChunkLevelsError {}
