//! Test input meshes for the spike: a lumpy rock, a forked branch and planes.

use std::collections::HashMap;
use std::f32::consts::TAU;

use glam::Vec3;
use procgena_mesh::SurfaceMesh;

/// A lumpy, roughly spherical rock of about `radius`, resting on y = 0.
///
/// With `crevice`, a narrow groove about a sixth of the radius wide and 40%
/// of it deep runs across the top along the z axis.
pub fn rock(radius: f32, subdivisions: u32, crevice: bool) -> SurfaceMesh {
    let (mut positions, indices) = icosphere(subdivisions);

    // Push each vertex in or out with a few overlapping waves, then flatten
    // the top a little so there is somewhere for snow to settle.
    for p in &mut positions {
        let d = *p;
        let bumps = 0.12 * (3.1 * d.x + 1.3).sin() * (2.7 * d.z - 0.4).cos()
            + 0.07 * (5.3 * d.y + 2.0 * d.x).sin()
            + 0.04 * (9.1 * d.z + 4.0 * d.y).sin()
            - 0.18 * d.y.max(0.0).powi(3);

        // Cut the groove into the upper half.
        let half_width = 0.08;
        let groove = if crevice && d.y > 0.0 && d.x.abs() < half_width {
            0.4 * (1.0 - (d.x / half_width).powi(2)) * d.y.min(0.5) * 2.0
        } else {
            0.0
        };
        *p = d * radius * (1.0 + bumps - groove);
        p.y *= 0.75;
    }
    let lowest = positions.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    for p in &mut positions {
        p.y -= lowest;
    }
    finish(positions, indices)
}

/// An open trunk with two open side branches: overlapping tubes, like a tree
/// mesh's bark. The trunk rises from y = 0.
pub fn branch_fork() -> SurfaceMesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let tubes = [
        (Vec3::ZERO, Vec3::new(0.0, 2.4, 0.0), 0.16, 0.11),
        (
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.1, 1.9, 0.2),
            0.10,
            0.05,
        ),
        (
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(-1.2, 1.75, -0.4),
            0.09,
            0.04,
        ),
    ];
    for (start, end, r0, r1) in tubes {
        tube(&mut positions, &mut indices, start, end, r0, r1, 16, 24);
    }
    finish(positions, indices)
}

/// A procgena_stone rock from `preset`, in metres and resting on y = 0, with
/// the name of its procgena_stone texture set.
pub fn stone(preset: procgena_stone::StonePreset) -> (SurfaceMesh, String) {
    let stone = procgena_stone::Stone::from_preset(preset);
    let mesh = stone.mesh();
    let surface = SurfaceMesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        indices: mesh.indices.clone(),
    };
    (surface, stone.options().material.texture.clone())
}

/// A flat ground patch facing down, so nothing should settle on it.
pub fn upside_down_plane() -> SurfaceMesh {
    let mut mesh = SurfaceMesh::ground(2.0, 8);
    for p in &mut mesh.positions {
        p[2] = -p[2];
        p[1] = 1.0;
    }
    for n in &mut mesh.normals {
        n[1] = -1.0;
    }
    mesh
}

/// Appends an open tapered tube from `start` (radius `r0`) to `end` (radius `r1`).
#[allow(clippy::too_many_arguments)]
fn tube(
    positions: &mut Vec<Vec3>,
    indices: &mut Vec<u32>,
    start: Vec3,
    end: Vec3,
    r0: f32,
    r1: f32,
    sides: u32,
    rings: u32,
) {
    let axis = (end - start).normalize();
    let (u, v) = axis.any_orthonormal_pair();
    let base = positions.len() as u32;
    for ring in 0..=rings {
        let t = ring as f32 / rings as f32;
        let centre = start.lerp(end, t);
        let r = r0 + (r1 - r0) * t;
        for side in 0..sides {
            let a = side as f32 / sides as f32 * TAU;
            positions.push(centre + (u * a.cos() + v * a.sin()) * r);
        }
    }
    for ring in 0..rings {
        for side in 0..sides {
            let a = base + ring * sides + side;
            let b = base + ring * sides + (side + 1) % sides;
            let c = a + sides;
            let d = b + sides;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
}

/// Builds a unit icosphere with shared vertices.
fn icosphere(subdivisions: u32) -> (Vec<Vec3>, Vec<u32>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut positions: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .into_iter()
    .map(|(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut indices: Vec<u32> = vec![
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11, 1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7,
        1, 8, 3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9, 4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9,
        8, 1,
    ];

    // Split every triangle into four, sharing midpoints between neighbours.
    for _ in 0..subdivisions {
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut next = Vec::with_capacity(indices.len() * 4);
        for tri in indices.as_chunks::<3>().0 {
            let mut mid = |a: u32, b: u32| {
                *midpoints.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    positions
                        .push(((positions[a as usize] + positions[b as usize]) / 2.0).normalize());
                    positions.len() as u32 - 1
                })
            };
            let (a, b, c) = (tri[0], tri[1], tri[2]);
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            next.extend_from_slice(&[a, ab, ca, b, bc, ab, c, ca, bc, ab, bc, ca]);
        }
        indices = next;
    }
    (positions, indices)
}

/// Packs positions and indices into a mesh with area-weighted normals.
fn finish(positions: Vec<Vec3>, indices: Vec<u32>) -> SurfaceMesh {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| positions[i as usize]);
        let n = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += n;
        }
    }
    SurfaceMesh {
        positions: positions.iter().map(|p| p.to_array()).collect(),
        normals: normals
            .iter()
            .map(|n| n.normalize_or_zero().to_array())
            .collect(),
        indices,
    }
}
