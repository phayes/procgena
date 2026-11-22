//! Creates a rock and provides access to its settings, shape, and triangle meshes.

use std::fmt;

use procgena_mesh::{Level, Object, Part, SurfaceMesh, tags};

use crate::detail::DetailLevel;
use crate::field::StoneField;
use crate::mesh::{StoneMesh, mesh_field};
use crate::options::{Mesher, OptionsError, StoneOptions};
use crate::placement::StonePlacement;
use crate::presets::StonePreset;

/// A generated rock with validated settings, a sampleable shape, and a grounded mesh.
///
/// The mesh is in metres and sits approximately at ground level (`y = 0`).
/// Its centre of mass, the balance point assuming uniform density, is above
/// the origin. With
/// [`PlacementOptions::settle`](crate::PlacementOptions::settle), it is also
/// turned onto an estimated resting side. Lower it by
/// [`embed_depth`](Self::embed_depth) to apply the suggested burial depth.
///
/// The field keeps the generated orientation in an unscaled, unit-width cube;
/// use [`placement`](Self::placement) to convert between the two.
/// A rock can be empty if carving removes it or the mesh resolution misses it;
/// simpler levels can be empty even when the most detailed one is not.
/// To change the stored settings or meshes, create another `Stone`.
#[derive(Debug, Clone)]
pub struct Stone {
    options: StoneOptions,
    field: StoneField,
    placement: StonePlacement,
    /// One mesh per level of `options.mesh.detail`, in the same order.
    meshes: Vec<StoneMesh>,
}

impl Stone {
    /// Creates a rock using the supplied shape, placement, and mesh settings,
    /// with one mesh for each level in
    /// [`MeshOptions::detail`](crate::MeshOptions::detail).
    ///
    /// Meshes are placed on the ground and scaled to metres using
    /// [`StoneOptions::scale`]. The suggested embed depth is not applied.
    /// Every level is built, so use a single-level set such as
    /// [`StoneDetail::single`](crate::StoneDetail::single) when you need only one mesh.
    ///
    /// # Errors
    ///
    /// Returns the first invalid setting found by [`StoneOptions::validate`].
    pub fn new(options: StoneOptions) -> Result<Self, OptionsError> {
        let field = StoneField::new(&options)?;
        let placement = StonePlacement::new(&field, &options.placement);
        let mut stone = Self {
            options,
            field,
            placement,
            meshes: Vec::new(),
        };

        // Mesh the same placed shape once per detail level.
        let mesher = stone.options.mesh.mesher;
        stone.meshes = stone
            .options
            .mesh
            .detail
            .levels()
            .iter()
            .map(|level| stone.mesh_with(mesher, level.resolution))
            .collect::<Result<_, _>>()?;
        Ok(stone)
    }

    /// Generates a rock, including its 3D mesh, using the selected preset.
    /// To adjust settings before generation, pass [`StonePreset::options`] to
    /// [`Self::new`] instead.
    pub fn from_preset(preset: StonePreset) -> Self {
        Self::new(preset.options()).expect("built-in presets are valid")
    }

    /// Creates a rock from a full or partial JSON settings object.
    ///
    /// Omitted settings use defaults, including fields in nested objects. For
    /// example, `{"seed": 7, "mesh": {"mesher": "dual_contouring"}}` changes only two settings.
    ///
    /// # Errors
    ///
    /// Returns [`StoneError::Json`] for malformed JSON or values of the wrong type.
    /// Unknown names are ignored; see [`StoneOptions::from_json`]. Returns [`StoneError::Options`] if a setting fails validation.
    pub fn from_json(json: &str) -> Result<Self, StoneError> {
        let options = StoneOptions::from_json(json).map_err(StoneError::Json)?;
        Self::new(options).map_err(StoneError::Options)
    }

    /// Returns the validated settings used to create this rock.
    pub fn options(&self) -> &StoneOptions {
        &self.options
    }

    /// Returns the distance field describing the original, unplaced rock shape:
    /// sample values are negative inside the rock and nonnegative outside it.
    ///
    /// Unlike the mesh, this field is neither placed on the ground nor scaled by
    /// [`StoneOptions::scale`]. To sample it at a mesh position, divide the
    /// position by the scale and pass it to [`StonePlacement::field_position`]
    /// on this rock's placement. Undo any additional scene transform first.
    pub fn field(&self) -> &StoneField {
        &self.field
    }

    /// Returns the ground placement shared by all meshes generated from this rock.
    pub fn placement(&self) -> &StonePlacement {
        &self.placement
    }

    /// Returns the suggested burial depth in metres, ready to subtract from y.
    ///
    /// This puts the ground near the widest horizontal slice within
    /// [`PlacementOptions::max_embed_fraction`](crate::PlacementOptions::max_embed_fraction).
    /// The stored mesh is not lowered automatically.
    pub fn embed_depth(&self) -> f32 {
        self.placement.embed_depth() * self.options.scale
    }

    /// Borrows the most detailed mesh in metres, placed at ground level without burial.
    pub fn mesh(&self) -> &StoneMesh {
        &self.meshes[0]
    }

    /// Iterates over every detail level with its mesh, most detailed first.
    ///
    /// Meshes are in metres and use the same placement as [`mesh`](Self::mesh).
    /// A simpler level's mesh can be empty if its resolution misses the rock.
    pub fn levels(&self) -> impl ExactSizeIterator<Item = (&DetailLevel, &StoneMesh)> {
        self.options.mesh.detail.levels().iter().zip(&self.meshes)
    }

    /// Returns the rock as a tagged [`Object`], for generators that grow on
    /// it, such as procgena_clump.
    ///
    /// There is one object level per detail level, most detailed first. Each
    /// level holds a single part tagged [`tags::ROCK`], or no parts if that
    /// level's mesh is empty. Level start distances are in metres, scaled to
    /// this rock's size the same way the Bevy plugin scales them. A part is
    /// marked closed only if its mesh is watertight; its shells are left
    /// unknown.
    pub fn object(&self) -> Object {
        let size_factor = self.options.scale / StoneOptions::default().scale;
        let levels = self
            .levels()
            .enumerate()
            .map(|(i, (detail, mesh))| {
                let start = if i == 0 {
                    0.0
                } else {
                    detail.distance * size_factor
                };
                let parts = self.rock_part(mesh).into_iter().collect();
                Level { start, parts }
            })
            .collect();
        Object { levels }
    }

    /// Wraps `mesh` as a part tagged `rock`, or returns `None` if it is empty.
    fn rock_part(&self, mesh: &StoneMesh) -> Option<Part> {
        let mesh = SurfaceMesh {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            indices: mesh.indices.clone(),
        };
        let closed = mesh.surface().ok()?.is_closed();
        Some(Part {
            closed,
            ..Part::new([tags::ROCK], mesh)
        })
    }

    /// Builds another mesh using `mesher` and `resolution` grid cells per axis.
    ///
    /// The result is in metres and uses the same rock shape and placement as the
    /// stored meshes. This does not replace the stored meshes or change the rock's
    /// settings. Ground contact may differ slightly at a different resolution.
    ///
    /// # Errors
    ///
    /// Returns [`OptionsError::IntOutOfRange`] if `resolution` is outside
    /// [`crate::MIN_RESOLUTION`] through [`crate::MAX_RESOLUTION`].
    pub fn mesh_with(&self, mesher: Mesher, resolution: u32) -> Result<StoneMesh, OptionsError> {
        let mut mesh = mesh_field(&self.field, mesher, resolution)?;
        self.placement.apply(&mut mesh);
        mesh.scale(self.options.scale);
        Ok(mesh)
    }

    /// Consumes the rock, returning its settings, original unscaled field,
    /// placement, and grounded meshes in metres, in that order. There is
    /// one mesh per detail level, most detailed first. The meshes already
    /// include placement; do not apply it again.
    pub fn into_parts(self) -> (StoneOptions, StoneField, StonePlacement, Vec<StoneMesh>) {
        (self.options, self.field, self.placement, self.meshes)
    }
}

/// A failure to read or validate settings while creating a rock from JSON.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoneError {
    /// The JSON is malformed or contains incompatible value types.
    Json(serde_json::Error),
    /// A parsed setting is non-finite or outside its allowed range.
    Options(OptionsError),
}

impl fmt::Display for StoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "invalid stone options JSON: {err}"),
            Self::Options(err) => write!(f, "invalid stone options: {err}"),
        }
    }
}

impl std::error::Error for StoneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(err) => Some(err),
            Self::Options(err) => Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DetailLevel, StoneDetail, TextureSize};

    /// A rock from `json` with a single mesh at `resolution` cells per axis.
    fn small(json: &str, resolution: u32) -> Stone {
        let mut options = StoneOptions::from_json(json).unwrap();
        options.mesh.detail = StoneDetail::single(resolution, TextureSize::Size512);
        Stone::new(options).unwrap()
    }

    #[test]
    fn mesh_is_in_world_units() {
        let stone = small(r#"{"scale": 4.0}"#, 32);
        let (min, max) = stone.mesh().bounds().unwrap();
        let unit = stone.mesh_with(Mesher::MarchingCubes, 32).unwrap();
        let (unit_min, unit_max) = unit.bounds().unwrap();
        assert_eq!((min, max), (unit_min, unit_max));
        // Scaling from 1 to 4 multiplies both placed positions and burial depth by four.
        let small_stone = small(r#"{"scale": 1.0}"#, 32);
        let (small_min, small_max) = small_stone.mesh().bounds().unwrap();
        for axis in 0..3 {
            assert!((min[axis] - 4.0 * small_min[axis]).abs() < 1e-4);
            assert!((max[axis] - 4.0 * small_max[axis]).abs() < 1e-4);
        }
        assert!(max[0] - min[0] > 1.0);
        assert!((stone.embed_depth() - 4.0 * small_stone.embed_depth()).abs() < 1e-5);
    }

    #[test]
    fn options_choose_the_mesher() {
        let stone = small(r#"{"mesh": {"mesher": "dual_contouring"}}"#, 24);
        let dc = stone.mesh_with(Mesher::DualContouring, 24).unwrap();
        assert_eq!(stone.mesh(), &dc);
        assert_ne!(
            stone.mesh(),
            &stone.mesh_with(Mesher::MarchingCubes, 24).unwrap()
        );
    }

    #[test]
    fn levels_follow_the_detail_set() {
        let mut options = StoneOptions::default();
        options.mesh.detail = StoneDetail::new(vec![
            DetailLevel::new(32, 0.0, TextureSize::Size1k),
            DetailLevel::new(16, 10.0, TextureSize::Size512),
        ]);
        let stone = Stone::new(options).unwrap();
        let levels: Vec<_> = stone.levels().collect();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[1].0.resolution, 16);
        assert!(levels[0].1.triangle_count() > levels[1].1.triangle_count());
        assert_eq!(levels[0].1, stone.mesh());
        assert_eq!(
            levels[1].1,
            &stone.mesh_with(Mesher::MarchingCubes, 16).unwrap()
        );
    }

    #[test]
    fn the_object_has_one_closed_rock_part_per_level() {
        let mut options = StoneOptions {
            scale: 5.0,
            ..StoneOptions::default()
        };
        options.mesh.detail = StoneDetail::new(vec![
            DetailLevel::new(32, 0.0, TextureSize::Size1k),
            DetailLevel::new(16, 10.0, TextureSize::Size512),
        ]);
        let stone = Stone::new(options).unwrap();
        let object = stone.object();
        object.validate().unwrap();
        assert_eq!(object.levels.len(), 2);

        // Level starts are scaled to the rock's size: 5 m is twice the default.
        assert_eq!(object.levels[0].start, 0.0);
        assert_eq!(object.levels[1].start, 20.0);

        // Each level is the matching mesh, as one watertight rock part.
        for (level, (_, mesh)) in object.levels.iter().zip(stone.levels()) {
            let [part] = &level.parts[..] else {
                panic!("expected one part, got {}", level.parts.len());
            };
            assert_eq!(part.tags, [tags::ROCK].into());
            assert!(part.closed);
            assert!(part.shells.is_empty());
            assert_eq!(part.mesh.positions, mesh.positions);
            assert_eq!(part.mesh.indices, mesh.indices);
        }
    }

    #[test]
    fn errors_name_the_problem() {
        let err = Stone::from_json(
            r#"{"mesh": {"detail": [
                {"resolution": 4, "distance": 0.0, "hysteresis": 0.0, "texture_size": "1k"}
            ]}}"#,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            StoneError::Options(OptionsError::IntOutOfRange {
                field: "mesh.detail.resolution",
                ..
            })
        ));
        assert!(matches!(Stone::from_json("{"), Err(StoneError::Json(_))));
        let stone = small("{}", 8);
        assert!(stone.mesh_with(Mesher::MarchingCubes, 513).is_err());
    }
}
