//! Procedural clumps of snow, moss, mud and litter on top of a surface mesh.
//!
//! Give `procgena_clump` a triangle mesh (or use the built-in ground patch) and
//! settings, and it returns a separate clump mesh that sits on that surface.
//! The input mesh is never changed. The clump's edges thin out and sink just
//! below the surface, so no seam shows where the two meet.
//!
//! ```
//! use procgena_clump::{Clump, ClumpOptions, SurfaceMesh};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ground = SurfaceMesh::ground(2.0, 8);
//! let clump = Clump::new(ground.surface()?, &ClumpOptions::default())?;
//! println!("{} triangles", clump.mesh().triangle_count());
//! # Ok(())
//! # }
//! ```
//!
//! This crate is under construction; see `plan.md` in the repository.

mod bvh;
mod field;
mod hash;
mod mesh;
mod options;
mod presets;
mod shell;
mod surface;

pub use mesh::{BuildStats, ClumpMesh};
pub use options::{
    ClumpError, ClumpOptions, Direction, MAX_DIRECTIONS, MaskOptions, MeshOptions, OptionsError,
    ShapeOptions,
};
pub use presets::ClumpPreset;
pub use surface::{Surface, SurfaceError, SurfaceMesh};

use field::ClumpField;

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
        let (mesh, stats) = if field.is_empty() {
            (ClumpMesh::default(), BuildStats::default())
        } else {
            shell::displaced_shell(
                &field,
                options.mesh.edge_length * options.scale,
                options.shape.max_slope,
                options.mesh.closed,
                options.mesh.strict,
            )
        };
        Ok(Self {
            options: options.clone(),
            mesh,
            stats,
        })
    }

    /// Generates a clump on `surface` with a preset's settings.
    pub fn from_preset(surface: Surface, preset: ClumpPreset) -> Self {
        // Presets are checked by this crate's tests.
        Self::new(surface, &preset.options()).expect("presets have valid settings")
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

    use crate::bvh::Bvh;
    use crate::surface::Surface;

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
    use super::*;

    #[test]
    fn same_settings_give_the_same_clump() {
        let ground = SurfaceMesh::ground(1.0, 4);
        let surface = ground.surface().unwrap();
        let options = ClumpOptions::default();
        let a = Clump::new(surface, &options).unwrap();
        let b = Clump::new(surface, &options).unwrap();
        assert_eq!(a.mesh(), b.mesh());
        assert!(!a.mesh().is_empty());

        let other = Clump::new(surface, &ClumpOptions { seed: 1, ..options }).unwrap();
        assert_ne!(a.mesh(), other.mesh());
    }

    #[test]
    fn every_preset_covers_the_ground() {
        let ground = SurfaceMesh::ground(1.0, 4);
        for preset in ClumpPreset::ALL {
            let clump = Clump::from_preset(ground.surface().unwrap(), preset);
            assert!(!clump.mesh().is_empty(), "{}", preset.name());
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

    /// Returns options for `directions` with sky rays off, so only facing counts.
    fn facing_only(directions: Vec<Direction>) -> ClumpOptions {
        let mut options = ClumpOptions {
            directions,
            ..ClumpOptions::default()
        };
        options.mask.sky_rays = 0;
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
        // An even cap: no lumps, no sky test, full coverage on flat ground.
        let ground = SurfaceMesh::ground(1.0, 4);
        let mut options = ClumpOptions::default();
        options.shape.lumpiness = 0.0;
        options.mask.sky_rays = 0;
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
        // Lumpy snow over a bumpy patch, with sky rays and partial coverage.
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
    fn buried_parts_stay_inside_sharp_edged_inputs() {
        // A slab thinner than the underside wants to go, with sharp edges.
        let slab = sharp_box(0.04);
        let surface = slab.surface().unwrap();
        assert!(surface.is_closed());
        let mut options = ClumpOptions::default();
        options.mask.sky_rays = 0;
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
        let mut options = procgena_stone::StonePreset::FlatSlab.options();
        options.mesh.detail =
            procgena_stone::StoneDetail::single(48, procgena_stone::TextureSize::Size1k);
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
}
