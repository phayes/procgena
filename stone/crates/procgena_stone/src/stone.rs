//! Creates a rock and provides access to its settings, shape, and triangle meshes.

use glam::Vec3;
use procgena_core::chain::{DistanceChain, Lod, Step, ViewScale};
use procgena_specimen::{Level, Part, Specimen, SurfaceMesh, tags};

use crate::detail::StoneDetail;
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
/// the lower mesh can be empty even when the game-level mesh is not.
/// To change the stored settings or meshes, create another `Stone`.
#[derive(Debug, Clone)]
pub struct Stone {
    options: StoneOptions,
    field: StoneField,
    placement: StonePlacement,
    /// The game-level mesh, then the lower mesh.
    meshes: [StoneMesh; 2],
}

impl Stone {
    /// Creates a rock using the supplied shape, placement, and mesh settings,
    /// with its game-level mesh
    /// ([`MeshOptions::game_level`](crate::MeshOptions::game_level)) and the
    /// coarser lower mesh derived from it (see [`StoneDetail::lod`]).
    ///
    /// Meshes are placed on the ground and scaled to metres using
    /// [`StoneOptions::scale`]. The suggested embed depth is not applied.
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
            meshes: Default::default(),
        };

        // Mesh the same placed shape at the game level and the lower level.
        let mesher = stone.options.mesh.mesher;
        let lod = stone.lod();
        stone.meshes = [
            stone.mesh_with(mesher, lod.game.resolution)?,
            stone.mesh_with(mesher, lod.lower.resolution)?,
        ];
        Ok(stone)
    }

    /// Generates a rock, including its 3D mesh, from a preset's settings.
    /// To adjust settings before generation, clone [`StonePreset::stone`],
    /// edit it, and pass it to [`Self::new`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`OptionsError`] if the preset's settings fail
    /// [`StoneOptions::validate`]. Built-in presets always pass; a preset
    /// read from a file was checked when it was read.
    pub fn from_preset(preset: &StonePreset) -> Result<Self, OptionsError> {
        Self::new(preset.stone.clone())
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

    /// Borrows the game-level mesh in metres, placed at ground level without
    /// burial: the rock's real shape.
    pub fn mesh(&self) -> &StoneMesh {
        &self.meshes[0]
    }

    /// Borrows the lower mesh in metres, with the same placement as
    /// [`mesh`](Self::mesh). It can be empty if its resolution misses the
    /// rock.
    pub fn lower_mesh(&self) -> &StoneMesh {
        &self.meshes[1]
    }

    /// The diameter, in metres, of a sphere holding the rock's game-level
    /// mesh: the diagonal of the mesh's axis-aligned bounding box, so the
    /// sphere is centred on that box. Renderers measure the rock's size on
    /// screen with it, to choose which mesh or impostor to draw. The GPU
    /// mesher reports the same measure for pooled rocks
    /// ([`GpuStoneStatus::bounding_diameter`](crate::gpu::GpuStoneStatus::bounding_diameter)).
    ///
    /// A rock whose game-level mesh is empty falls back to
    /// [`StoneOptions::bounding_diameter`], the bound of any rock built from
    /// its settings.
    pub fn bounding_diameter(&self) -> f32 {
        let positions = &self.meshes[0].positions;
        if positions.is_empty() {
            return self.options.bounding_diameter();
        }
        let mut min = Vec3::INFINITY;
        let mut max = Vec3::NEG_INFINITY;
        for &p in positions {
            min = min.min(Vec3::from(p));
            max = max.max(Vec3::from(p));
        }
        (max - min).length()
    }

    /// The rock's game-level detail and the lower detail derived from it.
    pub fn lod(&self) -> Lod<StoneDetail> {
        self.options.mesh.game_level.lod()
    }

    /// The game level and the lower mesh, each with its step and detail,
    /// game level first.
    pub fn levels(&self) -> [(Step, StoneDetail, &StoneMesh); 2] {
        let lod = self.lod();
        [
            (Step::Game, lod.game, &self.meshes[0]),
            (Step::Lower, lod.lower, &self.meshes[1]),
        ]
    }

    /// Returns the rock as a tagged [`Specimen`], for generators that grow on
    /// it, such as procgena_clump.
    ///
    /// The specimen has two levels: the game-level mesh, then the lower
    /// mesh. Each holds a single part tagged [`tags::ROCK`], or no parts if
    /// that mesh is empty. The lower level starts where the default
    /// [`DistanceChain`] switches to it for this rock, seen through a
    /// [`ViewScale::DEFAULT`] camera at High quality. A part is marked closed
    /// only if its mesh is watertight; its shells are left unknown.
    pub fn specimen(&self) -> Specimen {
        let factor = DistanceChain::factor(self.bounding_diameter(), ViewScale::DEFAULT);
        let lower_start = factor / DistanceChain::DEFAULT.lower_below;
        let levels = self
            .meshes
            .iter()
            .zip([0.0, lower_start])
            .map(|(mesh, start)| Level {
                start,
                parts: self.rock_part(mesh).into_iter().collect(),
            })
            .collect();
        Specimen { levels }
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
    /// placement, and grounded meshes in metres (the game-level mesh, then
    /// the lower mesh), in that order. The meshes already include placement;
    /// do not apply it again.
    pub fn into_parts(self) -> (StoneOptions, StoneField, StonePlacement, [StoneMesh; 2]) {
        (self.options, self.field, self.placement, self.meshes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextureSize;

    /// A rock from default settings changed by `edit`, with its game level
    /// at `resolution` cells per axis.
    fn small(edit: impl FnOnce(&mut StoneOptions), resolution: u32) -> Stone {
        let mut options = StoneOptions::default();
        edit(&mut options);
        options.mesh.game_level = StoneDetail::new(resolution, TextureSize::Size512);
        Stone::new(options).unwrap()
    }

    #[test]
    fn mesh_is_in_world_units() {
        let stone = small(|o| o.scale = 4.0, 32);
        let (min, max) = stone.mesh().bounds().unwrap();
        let unit = stone.mesh_with(Mesher::MarchingCubes, 32).unwrap();
        let (unit_min, unit_max) = unit.bounds().unwrap();
        assert_eq!((min, max), (unit_min, unit_max));
        // Scaling from 1 to 4 multiplies both placed positions and burial depth by four.
        let small_stone = small(|o| o.scale = 1.0, 32);
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
        let stone = small(|o| o.mesh.mesher = Mesher::DualContouring, 24);
        let dc = stone.mesh_with(Mesher::DualContouring, 24).unwrap();
        assert_eq!(stone.mesh(), &dc);
        assert_ne!(
            stone.mesh(),
            &stone.mesh_with(Mesher::MarchingCubes, 24).unwrap()
        );
    }

    #[test]
    fn the_lower_mesh_is_built_from_the_game_level() {
        let stone = small(|_| {}, 32);
        let [
            (game, game_detail, game_mesh),
            (lower, lower_detail, lower_mesh),
        ] = stone.levels();
        assert_eq!((game, lower), (Step::Game, Step::Lower));
        assert_eq!(game_detail.resolution, 32);
        assert_eq!(lower_detail.resolution, 16);
        assert!(game_mesh.triangle_count() > lower_mesh.triangle_count());
        assert_eq!(game_mesh, stone.mesh());
        assert_eq!(lower_mesh, stone.lower_mesh());
        assert_eq!(
            lower_mesh,
            &stone.mesh_with(Mesher::MarchingCubes, 16).unwrap()
        );
    }

    #[test]
    fn the_specimen_has_one_closed_rock_part_per_level() {
        let mut options = StoneOptions {
            scale: 5.0,
            ..StoneOptions::default()
        };
        options.mesh.game_level = StoneDetail::new(32, TextureSize::Size1k);
        let stone = Stone::new(options).unwrap();
        let specimen = stone.specimen();
        specimen.validate().unwrap();
        assert_eq!(specimen.levels.len(), 2);

        // The lower level starts where the default chain switches to it.
        let factor = DistanceChain::factor(stone.bounding_diameter(), ViewScale::DEFAULT);
        assert_eq!(specimen.levels[0].start, 0.0);
        assert_eq!(
            specimen.levels[1].start,
            factor / DistanceChain::DEFAULT.lower_below
        );

        // Each level is the matching mesh, as one watertight rock part.
        for (level, (_, _, mesh)) in specimen.levels.iter().zip(stone.levels()) {
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

    /// Every preset's mesh fits inside both its rock's bounding diameter
    /// and its settings' bound: no two of its vertices are further apart.
    /// The rock's own measure, a box diagonal, is at most √3 times the
    /// widest span. Prints how much larger each is than the widest span.
    #[test]
    fn the_bounding_diameters_enclose_every_preset() {
        for preset in crate::StonePreset::all() {
            let mut options = preset.stone.clone();
            options.mesh.game_level = StoneDetail::new(32, TextureSize::Size512);
            let stone = Stone::new(options).unwrap();
            let positions = &stone.mesh().positions;
            let mut span = 0.0f32;
            for (i, a) in positions.iter().enumerate() {
                for b in &positions[i + 1..] {
                    span = span.max(glam::Vec3::from(*a).distance(glam::Vec3::from(*b)));
                }
            }
            let bound = stone.options().bounding_diameter();
            let sphere = stone.bounding_diameter();
            assert!(
                span <= sphere,
                "{}: span {span} > sphere {sphere}",
                preset.key
            );
            assert!(span <= bound, "{}: span {span} > bound {bound}", preset.key);
            assert!(
                sphere <= 3f32.sqrt() * span,
                "{}: sphere {sphere} > √3 × span {span}",
                preset.key
            );
            eprintln!(
                "{}: sphere / widest span = {:.2}, bound / widest span = {:.2}",
                preset.key,
                sphere / span,
                bound / span
            );
        }
    }

    #[test]
    fn errors_name_the_problem() {
        let mut options = StoneOptions::default();
        options.mesh.game_level = StoneDetail::new(4, TextureSize::Size1k);
        let err = Stone::new(options).unwrap_err();
        assert!(matches!(
            err,
            OptionsError::IntOutOfRange {
                field: "mesh.game_level.resolution",
                ..
            }
        ));
        let stone = small(|_| {}, 8);
        assert!(stone.mesh_with(Mesher::MarchingCubes, 513).is_err());
    }
}
