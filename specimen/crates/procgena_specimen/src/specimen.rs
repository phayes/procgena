//! Generated specimens: detail levels made of tagged parts.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::surface::{Surface, SurfaceError, SurfaceMesh};
use crate::tag::{TagFilter, TagSet};

/// One tagged piece of a generated specimen, such as a tree's trunk or a
/// rock.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// What the part is, such as `bark` and `trunk`. See [`tags`](crate::tags).
    pub tags: TagSet,
    /// The part's triangles, in metres. Shared, so cloning a part is cheap.
    pub mesh: Arc<SurfaceMesh>,
    /// True if the part is watertight, so that points can be inside it.
    /// Leave it false when unsure.
    pub closed: bool,
    /// The part's separate pieces, such as one per tree branch, as ranges
    /// of triangle numbers (not index-buffer offsets).
    ///
    /// Either empty, meaning unknown, or ranges in order that together cover
    /// every triangle exactly once, with none empty.
    pub shells: Vec<Range<u32>>,
}

impl Part {
    /// Creates an open part with unknown shells.
    pub fn new(tags: impl Into<TagSet>, mesh: SurfaceMesh) -> Self {
        Self {
            tags: tags.into(),
            mesh: Arc::new(mesh),
            closed: false,
            shells: Vec::new(),
        }
    }

    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.mesh.indices.len() / 3
    }

    /// Borrows the part's mesh as a [`Surface`], checking its buffers.
    pub fn surface(&self) -> Result<Surface<'_>, SurfaceError> {
        self.mesh.surface()
    }

    /// Returns true if [`shells`](Self::shells) is empty, or covers every
    /// triangle exactly once, in order, with no empty range.
    fn shells_are_valid(&self) -> bool {
        if self.shells.is_empty() {
            return true;
        }
        let mut next = 0;
        for shell in &self.shells {
            if shell.start != next || shell.end <= shell.start {
                return false;
            }
            next = shell.end;
        }
        next as usize == self.triangle_count()
    }
}

/// One detail level of a [`Specimen`].
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// Camera distance in metres at which this level starts to be drawn.
    /// 0 for the most detailed level.
    pub start: f32,
    /// The level's parts.
    pub parts: Vec<Part>,
}

/// One generated example of an asset, such as a rock, a tree, a ground
/// patch or a clump, which many placed copies can share.
///
/// Other generators read it to decide what to grow on: a snow clump, for
/// example, picks the parts tagged `rock` or `bark` with a [`TagFilter`].
#[derive(Debug, Clone, PartialEq)]
pub struct Specimen {
    /// Detail levels, most detailed first. Must hold at least one.
    pub levels: Vec<Level>,
}

impl Specimen {
    /// Creates a specimen with a single detail level made of `parts`.
    pub fn single(parts: Vec<Part>) -> Self {
        Self {
            levels: vec![Level { start: 0.0, parts }],
        }
    }

    /// Checks that the specimen is well formed.
    ///
    /// Returns the first problem found: no levels, a level start that is
    /// negative, not finite or smaller than the one before it, a part whose
    /// mesh is not a valid [`Surface`], or shells that don't follow the
    /// rules on [`Part::shells`].
    pub fn validate(&self) -> Result<(), SpecimenError> {
        if self.levels.is_empty() {
            return Err(SpecimenError::NoLevels);
        }
        let mut previous = 0.0;
        for (l, level) in self.levels.iter().enumerate() {
            // Levels start at increasing distances.
            if !level.start.is_finite() || level.start < previous {
                return Err(SpecimenError::LevelStart {
                    level: l,
                    start: level.start,
                });
            }
            previous = level.start;

            // Every part is a valid mesh with valid shells.
            for (p, part) in level.parts.iter().enumerate() {
                if let Err(error) = part.surface() {
                    return Err(SpecimenError::Mesh {
                        level: l,
                        part: p,
                        error,
                    });
                }
                if !part.shells_are_valid() {
                    return Err(SpecimenError::Shells { level: l, part: p });
                }
            }
        }
        Ok(())
    }

    /// Returns the parts of detail level `level` that `filter` matches, in order.
    ///
    /// # Panics
    ///
    /// Panics if `level` is not less than the number of levels.
    pub fn parts<'a>(
        &'a self,
        level: usize,
        filter: &'a TagFilter,
    ) -> impl Iterator<Item = &'a Part> {
        self.levels[level]
            .parts
            .iter()
            .filter(|part| filter.matches(&part.tags))
    }

    /// Joins the parts of detail level `level` that `filter` matches into
    /// one part, or returns `None` if none match.
    ///
    /// The joined part has every tag of the parts it was made from. It is
    /// closed only if all of them are closed. Its shells are known only if
    /// every part's shells are known; otherwise they are empty.
    ///
    /// # Panics
    ///
    /// Panics if `level` is not less than the number of levels.
    pub fn merged(&self, level: usize, filter: &TagFilter) -> Option<Part> {
        let mut parts = self.parts(level, filter).peekable();
        parts.peek()?;

        let mut mesh = SurfaceMesh::default();
        let mut merged = Part {
            tags: TagSet::new(),
            mesh: Arc::default(),
            closed: true,
            shells: Vec::new(),
        };
        let mut shells_known = true;
        for part in parts {
            // Append the part's buffers, shifting its indices and shells.
            let first_vertex = mesh.positions.len() as u32;
            let first_triangle = (mesh.indices.len() / 3) as u32;
            mesh.positions.extend_from_slice(&part.mesh.positions);
            mesh.normals.extend_from_slice(&part.mesh.normals);
            mesh.indices
                .extend(part.mesh.indices.iter().map(|i| i + first_vertex));
            merged.shells.extend(
                part.shells
                    .iter()
                    .map(|s| s.start + first_triangle..s.end + first_triangle),
            );

            // Combine what is known about the parts.
            merged.tags.extend(part.tags.iter().cloned());
            merged.closed &= part.closed;
            shells_known &= !part.shells.is_empty();
        }
        if !shells_known {
            merged.shells.clear();
        }
        merged.mesh = Arc::new(mesh);
        Some(merged)
    }
}

/// Why a [`Specimen`] is not well formed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SpecimenError {
    /// The specimen has no detail levels.
    NoLevels,
    /// A level's start distance is negative, not finite, or smaller than
    /// the previous level's.
    LevelStart { level: usize, start: f32 },
    /// A part's mesh buffers are not a valid [`Surface`].
    Mesh {
        level: usize,
        part: usize,
        error: SurfaceError,
    },
    /// A part's shells don't cover its triangles exactly once, in order.
    Shells { level: usize, part: usize },
}

impl fmt::Display for SpecimenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLevels => write!(f, "the specimen has no detail levels"),
            Self::LevelStart { level, start } => write!(
                f,
                "level {level} starts at {start}, which is not a finite distance \
                 at least as far as the level before"
            ),
            Self::Mesh { level, part, error } => {
                write!(f, "part {part} of level {level}: {error}")
            }
            Self::Shells { level, part } => write!(
                f,
                "the shells of part {part} of level {level} don't cover its triangles \
                 exactly once, in order"
            ),
        }
    }
}

impl std::error::Error for SpecimenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mesh { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::*;

    /// A ground patch with 2 triangles, tagged `tags`, cut into one shell
    /// per triangle.
    fn quad(tags: impl Into<TagSet>, closed: bool) -> Part {
        Part {
            closed,
            shells: vec![0..1, 1..2],
            ..Part::new(tags, SurfaceMesh::ground(1.0, 1))
        }
    }

    fn tree() -> Specimen {
        Specimen::single(vec![
            quad([BARK, TRUNK], true),
            quad([BARK, LIMB], true),
            Part::new([LEAF, CARD], SurfaceMesh::ground(1.0, 1)),
        ])
    }

    #[test]
    fn parts_are_picked_by_tag() {
        let tree = tree();
        let bark = TagFilter::any_of([BARK]);
        assert_eq!(tree.parts(0, &bark).count(), 2);
        let trunk = TagFilter::any_of([TRUNK]);
        assert_eq!(tree.parts(0, &trunk).count(), 1);
        let solid = TagFilter::default().excluding([CARD]);
        assert_eq!(tree.parts(0, &solid).count(), 2);
        assert!(tree.merged(0, &TagFilter::any_of([ROCK])).is_none());
    }

    #[test]
    fn merging_shifts_indices_and_shells() {
        let tree = tree();
        let bark = tree.merged(0, &TagFilter::any_of([BARK])).unwrap();
        assert_eq!(bark.tags, TagSet::from([BARK, TRUNK, LIMB]));
        assert!(bark.closed);
        assert_eq!(bark.mesh.positions.len(), 8);
        assert_eq!(bark.mesh.indices[6..], [4, 6, 5, 5, 6, 7]);
        assert_eq!(bark.shells, [0..1, 1..2, 2..3, 3..4]);
        assert!(Specimen::single(vec![bark]).validate().is_ok());
    }

    #[test]
    fn merging_an_open_part_or_unknown_shells_loses_those_facts() {
        let everything = tree().merged(0, &TagFilter::default()).unwrap();
        assert!(!everything.closed);
        assert!(everything.shells.is_empty());
        assert_eq!(everything.triangle_count(), 6);
    }

    #[test]
    // Single-range shell lists are some of the cases under test.
    #[allow(clippy::single_range_in_vec_init)]
    fn malformed_specimens_are_rejected() {
        assert!(tree().validate().is_ok());
        assert_eq!(
            Specimen { levels: vec![] }.validate(),
            Err(SpecimenError::NoLevels)
        );

        // Levels must start at increasing, finite distances.
        let mut far_first = tree();
        far_first.levels.push(Level {
            start: 20.0,
            parts: vec![],
        });
        far_first.levels.push(Level {
            start: 10.0,
            parts: vec![],
        });
        assert_eq!(
            far_first.validate(),
            Err(SpecimenError::LevelStart {
                level: 2,
                start: 10.0
            })
        );
        let mut nan = tree();
        nan.levels[0].start = f32::NAN;
        assert!(matches!(
            nan.validate(),
            Err(SpecimenError::LevelStart { level: 0, .. })
        ));

        // A part must be a valid mesh.
        let mut empty = tree();
        empty.levels[0].parts[1] = Part::new([BARK], SurfaceMesh::default());
        assert_eq!(
            empty.validate(),
            Err(SpecimenError::Mesh {
                level: 0,
                part: 1,
                error: SurfaceError::Empty
            })
        );

        // Shells must cover every triangle once, in order, with none empty.
        for shells in [
            vec![0..1],
            vec![1..2, 0..1],
            vec![0..2, 1..2],
            vec![0..0, 0..2],
            vec![0..3],
        ] {
            let mut bad = tree();
            bad.levels[0].parts[0].shells = shells;
            assert_eq!(
                bad.validate(),
                Err(SpecimenError::Shells { level: 0, part: 0 })
            );
        }
    }
}
