//! Procedural clumps of snow, moss, mud and litter on top of a surface mesh.
//!
//! Give `procgena_clump` a triangle mesh (or use the built-in ground patch) and
//! settings, and it returns a separate clump mesh that sits on that surface.
//! The input mesh is never changed. The clump's edges thin out and sink just
//! below the surface, so no seam shows where the two meet.
//!
//! ```
//! use procgena_clump::{Clump, ClumpOptions};
//! use procgena_specimen::SurfaceMesh;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ground = SurfaceMesh::ground(2.0, 8);
//! let clump = Clump::new(ground.surface()?, &ClumpOptions::default())?;
//! println!("{} triangles", clump.mesh().triangle_count());
//! # Ok(())
//! # }
//! ```
//!
//! Start from a [`ClumpPreset`], or from a `.clump.yml` file that names a
//! preset as its `base` and gives only the settings it changes:
//!
//! ```
//! use procgena_clump::{Clump, ClumpPreset};
//! use procgena_specimen::SurfaceMesh;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let preset = ClumpPreset::from_yaml(
//!     "deep_snow",
//!     "base: fresh_snow_cap\n\
//!      clump:\n  \
//!        default_seed: 7\n  \
//!        shape:\n    \
//!          thickness: 0.3\n",
//! )?;
//! let ground = SurfaceMesh::ground(2.0, 8);
//! let clump = Clump::from_preset(ground.surface()?, &preset)?;
//! assert!(!clump.mesh().is_empty());
//! # Ok(())
//! # }
//! ```
//!
//! This crate is under construction; see `plan.md` in the repository.

mod blob;
mod bvh;
mod content;
mod field;
#[cfg(feature = "gpu")]
pub mod gpu;
mod material;
mod mesh;
mod options;
mod patch;
mod presets;
pub mod shaders;
mod shell;

pub use content::CLUMP_KIND;
pub use material::{
    MaterialOptions, TEXTURE_SETS, TextureCompression, TextureMap, TextureSet, TextureSize,
    texture_set,
};
pub use mesh::{BuildStats, ClumpMesh};
pub use options::{
    ClumpError, ClumpOptions, Direction, MAX_DIRECTIONS, MAX_NECK, MaskOptions, MeshOptions,
    OptionsError, Placement, PlacementOptions, ShapeOptions,
};
pub use presets::ClumpPreset;

/// The text of `clump.meta.yml`: the clump kind's name, parts, and each
/// setting's label, description, range and conditions, for tools that edit
/// clump presets. Read it with
/// [`KindMeta::parse`](procgena_core::meta::KindMeta::parse).
pub const CLUMP_META: &str = include_str!("../presets/clump.meta.yml");
pub use procgena_core::preset::PresetFileError;

use field::ClumpField;
use patch::Patch;
use procgena_specimen::{Part, Specimen, Surface, SurfaceMesh};

/// A generated clump: its settings, mesh and build measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct Clump {
    options: ClumpOptions,
    mesh: ClumpMesh,
    stats: BuildStats,
}

impl Clump {
    /// Generates a clump on `surface`.
    ///
    /// Returns an error if a setting is out of range. If nothing on the
    /// surface passes the mask, the clump mesh is empty.
    pub fn new(surface: Surface, options: &ClumpOptions) -> Result<Self, ClumpError> {
        options.validate()?;
        let field = ClumpField::new(surface, options);
        let edge_length = options.mesh.edge_length * options.scale;
        let (mesh, stats) = match options.placement.mode {
            Placement::Everywhere => {
                let patch = Patch::new(&field, edge_length);
                if patch.is_empty() {
                    (ClumpMesh::default(), BuildStats::default())
                } else {
                    shell::displaced_shell(
                        &field,
                        patch,
                        edge_length,
                        options.shape.max_slope,
                        options.mesh.closed,
                        options.mesh.strict,
                    )
                }
            }
            Placement::Scatter => blob::scatter(&field, options),
        };
        Ok(Self {
            options: options.clone(),
            mesh,
            stats,
        })
    }

    /// Generates a clump on the parts of `specimen`'s most detailed level that
    /// [`ClumpOptions::grows_on`] picks, joined into one surface.
    ///
    /// Returns an error if `specimen` is malformed (see
    /// [`Specimen::validate`]) or a setting is out of range. If no part
    /// matches, or nothing on them passes the mask, the clump mesh is empty.
    pub fn on(specimen: &Specimen, options: &ClumpOptions) -> Result<Self, ClumpError> {
        options.validate()?;
        specimen.validate()?;
        match specimen.merged(0, &options.grows_on) {
            Some(part) => Self::new(part.surface()?, options),
            None => Ok(Self {
                options: options.clone(),
                mesh: ClumpMesh::default(),
                stats: BuildStats::default(),
            }),
        }
    }

    /// Generates a clump on `surface` with a preset's settings.
    ///
    /// Returns an error if a setting is out of range, which can happen only
    /// for a preset whose [`ClumpPreset::clump`] options were edited.
    pub fn from_preset(surface: Surface, preset: &ClumpPreset) -> Result<Self, ClumpError> {
        Self::new(surface, &preset.clump)
    }

    /// Returns the clump as a tagged [`Specimen`], so other clumps can grow on
    /// it, such as snow on moss.
    ///
    /// The specimen has one level. It holds one part with
    /// [`ClumpOptions::tags`], or no parts if the clump mesh is empty. The
    /// part is marked closed when [`MeshOptions::closed`] is on; its shells
    /// are left unknown.
    pub fn specimen(&self) -> Specimen {
        let parts = (!self.mesh.is_empty())
            .then(|| {
                let mesh = SurfaceMesh {
                    positions: self.mesh.positions.clone(),
                    normals: self.mesh.normals.clone(),
                    indices: self.mesh.indices.clone(),
                };
                Part {
                    closed: self.options.mesh.closed,
                    ..Part::new(self.options.tags.clone(), mesh)
                }
            })
            .into_iter()
            .collect();
        Specimen::single(parts)
    }

    /// Returns the settings the clump was built with.
    pub fn options(&self) -> &ClumpOptions {
        &self.options
    }

    /// Returns the clump mesh.
    pub fn mesh(&self) -> &ClumpMesh {
        &self.mesh
    }

    /// Returns measurements from the build.
    pub fn stats(&self) -> BuildStats {
        self.stats
    }
}

/// Measurement helpers for checking clump meshes against their surface.
///
/// These exist to evaluate meshing approaches and may change or disappear.
#[doc(hidden)]
pub mod diagnostics {
    use glam::Vec3;

    use procgena_specimen::Surface;

    use crate::bvh::Bvh;

    /// Returns the signed distance from each point to `surface`: positive on
    /// the side its normals face, negative behind it.
    pub fn signed_distances(surface: &Surface, points: &[[f32; 3]]) -> Vec<f32> {
        let bvh = Bvh::new(surface);
        points
            .iter()
            .map(|&p| {
                let p = Vec3::from(p);
                let c = bvh.closest_point(p);
                let corners = surface.triangle(c.triangle);
                let [u, v, w] = c.barycentric;
                let n = surface.normal(corners[0]) * u
                    + surface.normal(corners[1]) * v
                    + surface.normal(corners[2]) * w;
                let d = c.distance_squared.sqrt();
                if (p - c.point).dot(n) < 0.0 { -d } else { d }
            })
            .collect()
    }

    /// Returns, for each point, whether it is inside `surface`, which must be
    /// closed. Rays are cast from the point in three fixed, skewed directions;
    /// an odd number of crossings counts as inside, and the majority decides.
    pub fn inside_closed(surface: &Surface, points: &[[f32; 3]]) -> Vec<bool> {
        let bvh = Bvh::new(surface);
        let dirs = [
            Vec3::new(0.137, 0.981, 0.093).normalize(),
            Vec3::new(-0.713, 0.212, 0.668).normalize(),
            Vec3::new(0.402, -0.587, -0.703).normalize(),
        ];
        let (lo, hi) = surface.bounds();
        let step = (hi - lo).length() * 1e-5;
        points
            .iter()
            .map(|&p| {
                // Count crossings along each ray, stepping just past each hit.
                let votes = dirs
                    .iter()
                    .filter(|&&dir| {
                        let mut origin = Vec3::from(p);
                        let mut crossings = 0;
                        while let Some((t, _)) = bvh.ray_hit(origin, dir, f32::INFINITY) {
                            crossings += 1;
                            origin += dir * (t + step);
                        }
                        crossings % 2 == 1
                    })
                    .count();
                votes >= 2
            })
            .collect()
    }

    /// Counts the triangles of `mesh` that pass through another of its
    /// triangles, using the same test as strict mode.
    pub fn crossing_count(mesh: &crate::ClumpMesh) -> usize {
        crate::shell::crossing_triangles(mesh).len()
    }

    /// Counts edges shared by other than exactly two triangles.
    /// Returns `(boundary edges, edges with three or more triangles)`.
    pub fn edge_defects(indices: &[u32]) -> (usize, usize) {
        let mut uses = std::collections::HashMap::<(u32, u32), u32>::new();
        for tri in indices.as_chunks::<3>().0 {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let boundary = uses.values().filter(|&&n| n == 1).count();
        let overfull = uses.values().filter(|&&n| n > 2).count();
        (boundary, overfull)
    }
}

#[cfg(test)]
mod tests {
    use procgena_specimen::{SpecimenError, tags};

    use super::*;

    /// A small flat card hovering 30 cm over the ground, tagged as a leaf.
    fn leaf_card() -> Part {
        let mut card = SurfaceMesh::ground(0.3, 1);
        for p in &mut card.positions {
            p[1] = 0.3;
        }
        Part::new([tags::LEAF, tags::CARD], card)
    }

    #[test]
    fn clumps_grow_only_on_the_parts_they_pick() {
        let ground = SurfaceMesh::ground(1.0, 4);
        let specimen =
            Specimen::single(vec![Part::new([tags::GROUND], ground.clone()), leaf_card()]);

        // Snow skips the leaf card, so it is the same as snow on the ground alone.
        let snow = Clump::on(&specimen, &ClumpOptions::default()).unwrap();
        let bare = Clump::new(ground.surface().unwrap(), &ClumpOptions::default()).unwrap();
        assert!(!snow.mesh().is_empty());
        assert_eq!(snow.mesh(), bare.mesh());

        // A clump that grows only on bark finds nothing here.
        let burl = Clump::on(&specimen, &ClumpPreset::burl().clump).unwrap();
        assert!(burl.mesh().is_empty());
        assert!(burl.specimen().levels[0].parts.is_empty());
    }

    #[test]
    fn malformed_specimens_are_rejected() {
        let mut specimen = Specimen::single(vec![leaf_card()]);
        // The card has two triangles, so a shell list must cover 0..2.
        specimen.levels[0].parts[0].shells = vec![0..1, 2..3];
        assert!(matches!(
            Clump::on(&specimen, &ClumpOptions::default()),
            Err(ClumpError::Specimen(SpecimenError::Shells {
                level: 0,
                part: 0
            }))
        ));
    }

    #[test]
    fn a_clump_is_a_specimen_other_clumps_grow_on() {
        let ground = Specimen::single(vec![Part::new([tags::GROUND], SurfaceMesh::ground(1.0, 4))]);
        let moss = Clump::on(&ground, &ClumpPreset::moss_patches().clump).unwrap();
        let specimen = moss.specimen();
        specimen.validate().unwrap();
        let [part] = &specimen.levels[0].parts[..] else {
            panic!("expected one part");
        };
        assert_eq!(part.tags, [tags::CLUMP, tags::MOSS].into());
        assert!(part.closed);
        assert_eq!(part.mesh.indices, moss.mesh().indices);

        // Snow grows on moss.
        let snow = Clump::on(&specimen, &ClumpOptions::default()).unwrap();
        assert!(!snow.mesh().is_empty());
    }

    #[test]
    fn same_settings_give_the_same_clump() {
        let ground = SurfaceMesh::ground(1.0, 4);
        let surface = ground.surface().unwrap();
        let options = ClumpOptions::default();
        let a = Clump::new(surface, &options).unwrap();
        let b = Clump::new(surface, &options).unwrap();
        assert_eq!(a.mesh(), b.mesh());
        assert!(!a.mesh().is_empty());

        let other = Clump::new(
            surface,
            &ClumpOptions {
                default_seed: 1,
                ..options
            },
        )
        .unwrap();
        assert_ne!(a.mesh(), other.mesh());
    }

    #[test]
    fn every_preset_covers_the_ground() {
        let ground = SurfaceMesh::ground(1.0, 4);
        for preset in ClumpPreset::all() {
            let clump = Clump::from_preset(ground.surface().unwrap(), preset).unwrap();
            assert!(!clump.mesh().is_empty(), "{}", preset.key);
        }
    }

    /// A 1 m wall in the plane x = 0 facing -X, rising from y = 0.
    fn wall() -> SurfaceMesh {
        let mut wall = SurfaceMesh::ground(1.0, 8);
        for p in &mut wall.positions {
            *p = [-p[1], p[0] + 0.5, p[2]];
        }
        for n in &mut wall.normals {
            *n = [-n[1], n[0], n[2]];
        }
        wall
    }

    /// Joins two meshes into one.
    fn join(a: &SurfaceMesh, b: &SurfaceMesh) -> SurfaceMesh {
        let mut out = a.clone();
        let base = a.positions.len() as u32;
        out.positions.extend_from_slice(&b.positions);
        out.normals.extend_from_slice(&b.normals);
        out.indices.extend(b.indices.iter().map(|i| i + base));
        out
    }

    /// Returns options for `directions` with exposure rays off, so only facing counts.
    fn facing_only(directions: Vec<Direction>) -> ClumpOptions {
        let mut options = ClumpOptions {
            directions,
            ..ClumpOptions::default()
        };
        options.mask.exposure_rays = 0;
        options
    }

    #[test]
    fn a_side_direction_coats_only_surfaces_facing_into_it() {
        let wall = wall();
        let surface = wall.surface().unwrap();
        let toward_wall = facing_only(vec![Direction::new([1.0, 0.0, 0.0])]);
        let away = facing_only(vec![Direction::new([-1.0, 0.0, 0.0])]);
        let down = facing_only(vec![Direction::DOWN]);
        assert!(!Clump::new(surface, &toward_wall).unwrap().mesh().is_empty());
        assert!(Clump::new(surface, &away).unwrap().mesh().is_empty());
        assert!(Clump::new(surface, &down).unwrap().mesh().is_empty());
    }

    #[test]
    fn directions_add_up_into_one_mesh() {
        let both = join(&SurfaceMesh::ground(1.0, 8), &wall());
        let surface = both.surface().unwrap();
        let options = facing_only(vec![Direction::DOWN, Direction::new([1.0, 0.0, 0.0])]);
        let mesh = Clump::new(surface, &options).unwrap().mesh().clone();

        // Visible clump on the ground (pushed up) and on the wall (pushed toward -X).
        let on_ground = mesh
            .positions
            .iter()
            .zip(&mesh.fade)
            .any(|(p, &f)| f > 0.5 && p[1] > 0.05 && p[1] < 0.2 && p[0] > 0.1);
        let on_wall = mesh
            .positions
            .iter()
            .zip(&mesh.fade)
            .any(|(p, &f)| f > 0.5 && p[0] < -0.05 && p[1] > 0.3);
        assert!(on_ground && on_wall);

        // A strength of 0 switches a direction off.
        let off = facing_only(vec![Direction {
            strength: 0.0,
            ..Direction::new([1.0, 0.0, 0.0])
        }]);
        assert!(
            Clump::new(wall().surface().unwrap(), &off)
                .unwrap()
                .mesh()
                .is_empty()
        );
    }

    #[test]
    fn direction_lists_are_checked() {
        let error = |directions: Vec<Direction>| {
            ClumpOptions {
                directions,
                ..ClumpOptions::default()
            }
            .validate()
            .unwrap_err()
            .field
        };
        assert_eq!(error(vec![]), "directions");
        assert_eq!(
            error(vec![Direction::DOWN; MAX_DIRECTIONS + 1]),
            "directions"
        );
        assert_eq!(error(vec![Direction::new([0.0; 3])]), "directions.vector");
        assert_eq!(
            error(vec![Direction {
                strength: -1.0,
                ..Direction::DOWN
            }]),
            "directions.strength"
        );
        assert_eq!(Direction::default(), Direction::DOWN);
        assert_eq!(Direction::new([1.0, 0.0, 0.0]).strength, 1.0);
    }

    /// Returns how many directed edges have no matching reverse edge, plus
    /// how many appear more than once. Both are 0 for a closed, consistently
    /// wound mesh.
    fn unpaired_edges(indices: &[u32]) -> (usize, usize) {
        let mut count = std::collections::HashMap::<(u32, u32), u32>::new();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            for e in [(a, b), (b, c), (c, a)] {
                *count.entry(e).or_default() += 1;
            }
        }
        let unmatched = count
            .keys()
            .filter(|&&(a, b)| !count.contains_key(&(b, a)))
            .count();
        let repeated = count.values().filter(|&&n| n > 1).count();
        (unmatched, repeated)
    }

    /// Returns the volume enclosed by a closed mesh, positive when its
    /// triangles face outward.
    fn enclosed_volume(mesh: &ClumpMesh) -> f32 {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| glam::Vec3::from(mesh.positions[i as usize]));
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn closed_clumps_are_watertight_with_the_expected_volume() {
        // An even cap: no lumps, no exposure test, full coverage on flat ground.
        let ground = SurfaceMesh::ground(1.0, 4);
        let mut options = ClumpOptions::default();
        options.shape.lumpiness = 0.0;
        options.mask.exposure_rays = 0;
        let clump = Clump::new(ground.surface().unwrap(), &options).unwrap();
        let mesh = clump.mesh();
        assert_eq!(unpaired_edges(&mesh.indices), (0, 0));

        // Thickness above the ground plus the underside depth below it.
        let s = &options.shape;
        let expected = 1.0 * (s.thickness + s.sink + 0.25 * s.thickness);
        let volume = enclosed_volume(mesh);
        assert!(
            (volume - expected).abs() < expected * 0.01,
            "{volume} vs {expected}"
        );
    }

    #[test]
    fn closed_clumps_on_a_rough_input_are_watertight() {
        // Lumpy snow over a bumpy patch, with exposure rays and partial coverage.
        let mut ground = SurfaceMesh::ground(2.0, 24);
        for p in &mut ground.positions {
            p[1] = (p[0] * 3.1).sin() * 0.15 + (p[2] * 2.3).cos() * 0.1;
        }
        let options = ClumpOptions {
            directions: vec![Direction::DOWN, Direction::new([1.0, -0.5, 0.0])],
            ..ClumpOptions::default()
        };
        let mesh = Clump::new(ground.surface().unwrap(), &options)
            .unwrap()
            .mesh()
            .clone();
        assert!(!mesh.is_empty());
        assert_eq!(unpaired_edges(&mesh.indices), (0, 0));
        assert!(enclosed_volume(&mesh) > 0.0);
    }

    /// A closed 1 m square box `height` tall resting on y = 0, with shared
    /// corners and smoothed normals, so every edge is a sharp ridge.
    fn sharp_box(height: f32) -> SurfaceMesh {
        let corners: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                [
                    (i & 1) as f32 - 0.5,
                    ((i >> 1) & 1) as f32 * height,
                    ((i >> 2) & 1) as f32 - 0.5,
                ]
            })
            .collect();
        let normals = corners
            .iter()
            .map(|&[x, y, z]| {
                glam::Vec3::new(x, if y > 0.0 { 0.5 } else { -0.5 }, z)
                    .normalize()
                    .to_array()
            })
            .collect();
        // Two outward-facing triangles per face.
        let faces = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let indices = faces
            .iter()
            .flat_map(|&[a, b, c, d]| [a, b, c, a, c, d])
            .collect();
        SurfaceMesh {
            positions: corners,
            normals,
            indices,
        }
    }

    #[test]
    fn copies_at_the_same_position_act_as_one_vertex() {
        // The same slab twice: once with shared corners, once with every
        // triangle on its own copies of the corners, as at texture seams.
        let shared = sharp_box(0.3);
        let mut seamed = SurfaceMesh::default();
        for &i in &shared.indices {
            seamed.positions.push(shared.positions[i as usize]);
            seamed.normals.push(shared.normals[i as usize]);
            seamed.indices.push(seamed.indices.len() as u32);
        }

        // The copies are merged, so the clump has the same triangles, with
        // positions equal up to rounding in the merged normals.
        let mut options = ClumpOptions::default();
        options.mask.exposure_rays = 0;
        let a = Clump::new(shared.surface().unwrap(), &options).unwrap();
        let b = Clump::new(seamed.surface().unwrap(), &options).unwrap();
        let (a, b) = (a.mesh(), b.mesh());
        assert!(!a.is_empty());
        assert_eq!(a.indices, b.indices);
        for (p, q) in a.positions.iter().zip(&b.positions) {
            assert!(
                glam::Vec3::from(*p).distance(glam::Vec3::from(*q)) < 1e-5,
                "{p:?} vs {q:?}"
            );
        }
    }

    #[test]
    fn buried_parts_stay_inside_sharp_edged_inputs() {
        // A slab thinner than the underside wants to go, with sharp edges.
        let slab = sharp_box(0.04);
        let surface = slab.surface().unwrap();
        assert!(surface.is_closed());
        let mut options = ClumpOptions::default();
        options.mask.exposure_rays = 0;
        let mesh = Clump::new(surface, &options).unwrap().mesh().clone();
        assert!(!mesh.is_empty());

        // Every rim and underside vertex is inside the slab or on its surface.
        let rim: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .zip(&mesh.fade)
            .filter(|&(_, &f)| f == 0.0)
            .map(|(p, _)| *p)
            .collect();
        let inside = diagnostics::inside_closed(&surface, &rim);
        let distance = diagnostics::signed_distances(&surface, &rim);
        for ((p, inside), d) in rim.iter().zip(inside).zip(distance) {
            assert!(inside || d.abs() < 1e-4, "{p:?} is outside the slab");
        }
    }

    #[test]
    fn no_top_triangle_is_left_inside_out() {
        let slab = low_detail_slab();
        let clump = Clump::new(slab.surface().unwrap(), &ClumpOptions::default()).unwrap();
        assert!(!clump.mesh().is_empty());
        assert_eq!(clump.stats().folded, 0);
    }

    /// A procgena_stone flat slab meshed at a low resolution, as a surface mesh.
    fn low_detail_slab() -> SurfaceMesh {
        let mut options = procgena_stone::StonePreset::flat_slab().stone.clone();
        options.mesh.game_level =
            procgena_stone::StoneDetail::new(48, procgena_stone::TextureSize::Size1k);
        let stone = procgena_stone::Stone::new(options).unwrap();
        let mesh = stone.mesh();
        SurfaceMesh {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            indices: mesh.indices.clone(),
        }
    }

    #[test]
    fn strict_clumps_never_pass_through_themselves() {
        let slab = low_detail_slab();
        let surface = slab.surface().unwrap();

        // The default build crosses itself on these creases.
        let loose = Clump::new(surface, &ClumpOptions::default()).unwrap();
        assert!(diagnostics::crossing_count(loose.mesh()) > 0);

        // Strict mode removes every crossing and keeps the solid closed.
        let mut options = ClumpOptions::default();
        options.mesh.strict = true;
        let strict = Clump::new(surface, &options).unwrap();
        assert!(!strict.mesh().is_empty());
        assert_eq!(strict.stats().crossings, 0);
        assert_eq!(diagnostics::crossing_count(strict.mesh()), 0);
        assert_eq!(unpaired_edges(&strict.mesh().indices), (0, 0));
    }

    #[test]
    fn open_clumps_are_a_single_sheet() {
        let ground = SurfaceMesh::ground(1.0, 4);
        let mut options = ClumpOptions::default();
        options.mesh.closed = false;
        let open = Clump::new(ground.surface().unwrap(), &options).unwrap();
        let closed = Clump::new(ground.surface().unwrap(), &ClumpOptions::default()).unwrap();
        assert!(unpaired_edges(&open.mesh().indices).0 > 0);
        assert!(closed.mesh().triangle_count() > 2 * open.mesh().triangle_count());
    }

    #[test]
    fn nothing_settles_on_a_downward_surface() {
        let mut ground = SurfaceMesh::ground(1.0, 4);
        for n in &mut ground.normals {
            n[1] = -1.0;
        }
        for tri in ground.indices.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
        let clump = Clump::new(ground.surface().unwrap(), &ClumpOptions::default()).unwrap();
        assert!(clump.mesh().is_empty());
    }

    #[test]
    fn edge_length_bounds_the_mesh_edges() {
        let ground = SurfaceMesh::ground(1.0, 1);
        let options = ClumpOptions::default();
        let clump = Clump::new(ground.surface().unwrap(), &options).unwrap();
        let mesh = clump.mesh();

        // Measure edges before displacement by flattening onto the ground plane.
        let longest = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
            .map(|(a, b)| {
                let [pa, pb] = [a, b].map(|i| mesh.positions[i as usize]);
                glam::Vec2::new(pa[0] - pb[0], pa[2] - pb[2]).length()
            })
            .fold(0.0, f32::max);
        assert!(longest <= options.mesh.edge_length * 1.001, "{longest}");
    }

    /// A finely divided `size` metre ground patch with gentle bumps, with
    /// matching smooth normals. Its steepest slope is about 0.4, so every
    /// point faces up enough for full snow-cap coverage.
    fn bumpy_ground(size: f32, divisions: u32) -> SurfaceMesh {
        let (amp, kx, kz) = (0.05, 5.0, 4.0);
        let mut ground = SurfaceMesh::ground(size, divisions);
        for (p, n) in ground.positions.iter_mut().zip(&mut ground.normals) {
            let (x, z) = (p[0], p[2]);
            p[1] = amp * (kx * x).sin() * (kz * z).cos();
            let dx = amp * kx * (kx * x).cos() * (kz * z).cos();
            let dz = -amp * kz * (kx * x).sin() * (kz * z).sin();
            *n = glam::Vec3::new(-dx, 1.0, -dz).normalize().to_array();
        }
        ground
    }

    /// Options for an even snow cap: no exposure rays, so only facing counts.
    fn even_cap(edge_length: f32, lumpiness: f32) -> ClumpOptions {
        let mut options = ClumpOptions::default();
        options.mask.exposure_rays = 0;
        options.shape.lumpiness = lumpiness;
        options.mesh.edge_length = edge_length;
        options
    }

    /// Returns the tallest point of `mesh` above the flat ground at y = 0.
    fn highest(mesh: &ClumpMesh) -> f32 {
        mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max)
    }

    #[test]
    fn lumps_rise_above_the_thickness() {
        let ground = SurfaceMesh::ground(2.0, 4);
        let surface = ground.surface().unwrap();
        let thickness = ClumpOptions::default().shape.thickness;

        // Without lumps the cap is flat at the thickness.
        let flat = Clump::new(surface, &even_cap(0.04, 0.0)).unwrap();
        assert!((highest(flat.mesh()) - thickness).abs() < 1e-4);

        // Lumps add height on top, more as lumpiness rises.
        let some = highest(Clump::new(surface, &even_cap(0.04, 0.5)).unwrap().mesh());
        let full = highest(Clump::new(surface, &even_cap(0.04, 1.0)).unwrap().mesh());
        assert!(some > thickness + 0.01, "{some}");
        assert!(full > some, "{full} vs {some}");
    }

    /// Returns how many separate pieces `indices` form, counting triangles
    /// joined through shared vertices as one piece.
    fn pieces(indices: &[u32], vertex_count: usize) -> usize {
        let mut parent: Vec<usize> = (0..vertex_count).collect();
        fn root(parent: &mut [usize], mut v: usize) -> usize {
            while parent[v] != v {
                parent[v] = parent[parent[v]];
                v = parent[v];
            }
            v
        }
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            for (x, y) in [(a, b), (b, c)] {
                let (rx, ry) = (root(&mut parent, x as usize), root(&mut parent, y as usize));
                parent[rx] = ry;
            }
        }
        let mut used = vec![false; vertex_count];
        for &i in indices {
            used[i as usize] = true;
        }
        (0..vertex_count)
            .filter(|&v| used[v] && root(&mut parent, v) == v)
            .count()
    }

    /// Options for `count` well-spaced, same-sized clumps on flat ground.
    fn scattered(count: u32) -> ClumpOptions {
        let mut options = even_cap(0.04, 0.0);
        options.placement = PlacementOptions {
            mode: Placement::Scatter,
            count,
            radius: 0.1,
            size_variation: 0.0,
            min_spacing: 0.4,
        };
        options
    }

    #[test]
    fn scatter_grows_the_requested_number_of_separate_clumps() {
        let ground = SurfaceMesh::ground(2.0, 4);
        let surface = ground.surface().unwrap();
        for count in [1, 5] {
            let clump = Clump::new(surface, &scattered(count)).unwrap();
            let mesh = clump.mesh();
            assert_eq!(clump.stats().clumps, count as usize);
            assert_eq!(pieces(&mesh.indices, mesh.vertex_count()), count as usize);

            // Each clump is a closed solid, and the same settings repeat.
            assert_eq!(unpaired_edges(&mesh.indices), (0, 0));
            assert!(enclosed_volume(mesh) > 0.0);
            assert_eq!(Clump::new(surface, &scattered(count)).unwrap().mesh(), mesh);
        }

        // More clumps than fit at the spacing gives as many as fit.
        let crowded = Clump::new(surface, &scattered(200)).unwrap();
        assert!(crowded.stats().clumps < 200);
        assert!(crowded.stats().clumps >= 5);
    }

    #[test]
    fn scattered_clumps_follow_the_triangle_size() {
        let ground = SurfaceMesh::ground(2.0, 4);
        let surface = ground.surface().unwrap();
        let triangles = |edge_length: f32| {
            let mut options = scattered(1);
            options.mesh.edge_length = edge_length;
            Clump::new(surface, &options)
                .unwrap()
                .mesh()
                .triangle_count()
        };
        let (fine, coarse) = (triangles(0.01), triangles(0.2));
        assert!(coarse < 50, "{coarse}");
        assert!(fine > 10 * coarse, "{fine} vs {coarse}");
    }

    #[test]
    fn a_neck_makes_the_clump_bulge_past_its_base() {
        let ground = SurfaceMesh::ground(2.0, 4);
        let surface = ground.surface().unwrap();
        let mut options = scattered(1);
        options.shape.thickness = 0.3;
        options.placement.radius = 0.2;
        options.mesh.edge_length = 0.02;

        // Widest distance from the clump's axis, overall and just above the ground.
        let mut widths = |neck: f32| {
            options.shape.neck = neck;
            let clump = Clump::new(surface, &options).unwrap();
            let mesh = clump.mesh();
            let top = mesh
                .positions
                .iter()
                .copied()
                .max_by(|a, b| a[1].total_cmp(&b[1]))
                .unwrap();
            let from_axis = |p: &[f32; 3]| glam::Vec2::new(p[0] - top[0], p[2] - top[2]).length();
            let widest = mesh.positions.iter().map(from_axis).fold(0.0, f32::max);
            let base = mesh
                .positions
                .iter()
                .filter(|p| p[1].abs() < 0.02)
                .map(from_axis)
                .fold(0.0, f32::max);
            (widest, base)
        };

        // Without a neck the clump is widest at its base; with one it bulges.
        let (widest, base) = widths(0.0);
        assert!(base > 0.9 * widest, "{base} vs {widest}");
        let (widest, base) = widths(0.7);
        assert!(base < 0.6 * widest, "{base} vs {widest}");
    }

    #[test]
    fn any_facing_grows_clumps_on_undersides() {
        // Flat ground turned upside down: nothing faces up.
        let mut ceiling = SurfaceMesh::ground(2.0, 4);
        for n in &mut ceiling.normals {
            n[1] = -1.0;
        }
        for tri in ceiling.indices.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
        let surface = ceiling.surface().unwrap();
        let mut options = scattered(3);
        assert_eq!(Clump::new(surface, &options).unwrap().stats().clumps, 0);
        options.mask.any_facing = true;
        let clump = Clump::new(surface, &options).unwrap();
        assert_eq!(clump.stats().clumps, 3);
        let lowest = clump
            .mesh()
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::MAX, f32::min);
        assert!(lowest < -0.1, "clumps hang below the ceiling: {lowest}");
    }

    #[test]
    fn triangle_size_coarsens_fine_input() {
        // 2 cm input triangles.
        let ground = bumpy_ground(2.0, 100);
        let surface = ground.surface().unwrap();
        let triangles = |edge_length: f32| {
            let clump = Clump::new(surface, &even_cap(edge_length, 0.0)).unwrap();
            assert_eq!(unpaired_edges(&clump.mesh().indices), (0, 0));
            clump.mesh().triangle_count()
        };
        let (fine, coarse) = (triangles(0.04), triangles(0.2));
        assert!(coarse * 8 < fine, "{coarse} vs {fine}");
    }

    #[test]
    fn coarse_clumps_still_cover_the_input() {
        // Every input point away from the patch border should sit under the
        // clump: straight up from it, the first thing met is the clump's top.
        let ground = bumpy_ground(2.0, 100);
        let surface = ground.surface().unwrap();
        let mut options = even_cap(0.3, 0.0);
        options.mesh.closed = false;
        let clump = Clump::new(surface, &options).unwrap();
        let mesh = clump.mesh();
        let sheet = Surface::new(&mesh.positions, &mesh.normals, &mesh.indices).unwrap();
        let bvh = bvh::Bvh::new(&sheet);

        let mut uncovered = 0;
        let mut checked = 0;
        for (p, n) in ground.positions.iter().zip(&ground.normals) {
            if p[0].abs() > 0.6 || p[2].abs() > 0.6 {
                continue;
            }
            checked += 1;
            let (p, n) = (glam::Vec3::from(*p), glam::Vec3::from(*n));
            if bvh.ray_hit(p, n, 1.0).is_none() {
                uncovered += 1;
            }
        }
        assert!(checked > 1000);
        assert_eq!(
            uncovered, 0,
            "{uncovered} of {checked} input points poke through"
        );
    }
}
