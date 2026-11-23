//! Tagged specimens that procgena generators hand to each other.
//!
//! Every generator returns a [`Specimen`]: one generated example of a tree,
//! rock or clump, as a list of detail levels, each made of [`Part`]s. A part
//! is a triangle mesh with [`tags`] that say what it is, such as `bark` and
//! `trunk`, plus whether it is watertight and how it splits into separate
//! pieces. Another generator can then pick the parts it wants with a
//! [`TagFilter`]: snow can grow on rock and bark but skip leaves.
//!
//! ```
//! use procgena_specimen::{Part, Specimen, SurfaceMesh, TagFilter, tags};
//!
//! let ground = Part::new([tags::GROUND], SurfaceMesh::ground(2.0, 4));
//! let specimen = Specimen::single(vec![ground]);
//! specimen.validate().unwrap();
//!
//! let snowy = TagFilter::any_of([tags::GROUND, tags::ROCK]);
//! let surface = specimen.merged(0, &snowy).unwrap();
//! assert_eq!(surface.triangle_count(), 32);
//! ```
//!
//! With the `bevy` feature, [`GeneratedSpecimen`] puts a specimen on a Bevy
//! entity.

#[cfg(feature = "bevy")]
mod bevy;
mod ground;
mod specimen;
mod surface;
mod tag;

#[cfg(feature = "bevy")]
pub use bevy::GeneratedSpecimen;
pub use ground::{FlatGround, Ground, HeightGrid};
pub use specimen::{Level, Part, Specimen, SpecimenError};
pub use surface::{Surface, SurfaceError, SurfaceMesh};
pub use tag::{Tag, TagFilter, TagSet, tags};
