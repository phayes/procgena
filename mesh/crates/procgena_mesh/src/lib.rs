//! Tagged triangle meshes that procgena generators hand to each other.
//!
//! Every generator returns an [`Object`]: a list of detail levels, each made
//! of [`Part`]s. A part is a triangle mesh with [`tags`] that say what it is,
//! such as `bark` and `trunk`, plus whether it is watertight and how it
//! splits into separate pieces. Another generator can then pick the parts it
//! wants with a [`TagFilter`]: snow can grow on rock and bark but skip
//! leaves.
//!
//! ```
//! use procgena_mesh::{Object, Part, SurfaceMesh, TagFilter, tags};
//!
//! let ground = Part::new([tags::GROUND], SurfaceMesh::ground(2.0, 4));
//! let object = Object::single(vec![ground]);
//! object.validate().unwrap();
//!
//! let snowy = TagFilter::any_of([tags::GROUND, tags::ROCK]);
//! let surface = object.merged(0, &snowy).unwrap();
//! assert_eq!(surface.triangle_count(), 32);
//! ```
//!
//! With the `bevy` feature, [`GeneratedObject`] puts an object on a Bevy
//! entity.

#[cfg(feature = "bevy")]
mod bevy;
mod ground;
mod object;
mod surface;
mod tag;

#[cfg(feature = "bevy")]
pub use bevy::GeneratedObject;
pub use ground::{FlatGround, Ground, HeightGrid};
pub use object::{Level, Object, ObjectError, Part};
pub use surface::{Surface, SurfaceError, SurfaceMesh};
pub use tag::{Tag, TagFilter, TagSet, tags};
