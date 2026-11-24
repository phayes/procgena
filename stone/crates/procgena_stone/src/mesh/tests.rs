use std::collections::HashMap;

use super::*;
use crate::StoneOptions;

const SEEDS: [u32; 6] = [0, 1, 880, 12345, 0xdead_beef, u32::MAX];
const MESHERS: [Mesher; 2] = [Mesher::MarchingCubes, Mesher::DualContouring];

fn field(seed: u32) -> StoneField {
    StoneField::new(&StoneOptions {
        default_seed: seed,
        ..StoneOptions::default()
    })
    .unwrap()
}

/// Measurements of how a mesh connects and which way its triangles face.
#[derive(Debug)]
pub(crate) struct Topology {
    /// Directed edges used a different number of times than their reverse.
    /// Zero indicates balanced edge directions, but does not rule out
    /// self-intersections or edges shared by more than two triangles.
    pub(crate) unbalanced_edges: usize,
    /// Edges shared by more than two triangles. Such edges cannot represent
    /// an ordinary surface with just one inside and one outside.
    pub(crate) non_manifold_edges: usize,
    /// Triangles whose computed area is exactly zero.
    pub(crate) degenerate_triangles: usize,
    /// Sum of signed triangle volumes. For a closed, consistently oriented
    /// surface, a positive result indicates outward-facing triangles.
    pub(crate) signed_volume: f32,
    /// Triangle counts for connected pieces, largest first. Triangles count
    /// as connected even if they share only a single vertex.
    pub(crate) components: Vec<usize>,
}

/// Measures edge sharing, zero-area triangles, signed volume, and connected pieces.
///
/// Indices must refer to existing positions. Any incomplete trailing index
/// triple is ignored.
pub(crate) fn topology(mesh: &StoneMesh) -> Topology {
    // Preserve edge direction so opposite uses can be compared.
    let mut directed: HashMap<(u32, u32), i32> = HashMap::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    let unbalanced_edges = directed
        .iter()
        .filter(|&(&(a, b), &n)| directed.get(&(b, a)).copied().unwrap_or(0) != n)
        .count();
    let mut undirected: HashMap<(u32, u32), i32> = HashMap::new();
    for (&(a, b), &n) in &directed {
        *undirected.entry((a.min(b), a.max(b))).or_default() += n;
    }
    let non_manifold_edges = undirected.values().filter(|&&n| n > 2).count();

    // Count collapsed triangles and accumulate their signed volume contributions.
    let p = |i: u32| Vec3::from(mesh.positions[i as usize]);
    let mut degenerate_triangles = 0;
    let mut signed_volume = 0.0;
    for t in mesh.indices.as_chunks::<3>().0 {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        if (b - a).cross(c - a) == Vec3::ZERO {
            degenerate_triangles += 1;
        }
        signed_volume += a.dot(b.cross(c)) / 6.0;
    }

    // Group triangles into pieces whenever they share a vertex.
    let mut parent: Vec<u32> = (0..mesh.vertex_count() as u32).collect();
    fn root(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        for &other in &t[1..] {
            let (a, b) = (root(&mut parent, t[0]), root(&mut parent, other));
            parent[a as usize] = b;
        }
    }
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        *sizes.entry(root(&mut parent, t[0])).or_default() += 1;
    }
    let mut components: Vec<usize> = sizes.into_values().collect();
    components.sort_unstable_by(|a, b| b.cmp(a));

    Topology {
        unbalanced_edges,
        non_manifold_edges,
        degenerate_triangles,
        signed_volume,
        components,
    }
}

/// Returns the fraction of nonzero-area triangles facing in the same general
/// direction as the sum of their vertex normals.
///
/// Indices must refer to existing positions and normals. Returns NaN if there
/// are no nonzero-area triangles.
fn facing_agreement(mesh: &StoneMesh) -> f32 {
    let p = |i: u32| Vec3::from(mesh.positions[i as usize]);
    let n = |i: u32| Vec3::from(mesh.normals[i as usize]);
    let (mut agree, mut total) = (0, 0);
    for t in mesh.indices.as_chunks::<3>().0 {
        let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        if face == Vec3::ZERO {
            continue;
        }
        total += 1;
        if face.dot(n(t[0]) + n(t[1]) + n(t[2])) > 0.0 {
            agree += 1;
        }
    }
    agree as f32 / total as f32
}

#[test]
fn marching_cubes_is_closed_manifold_and_outward() {
    for seed in SEEDS {
        let mesh = mesh_field(&field(seed), Mesher::MarchingCubes, 48).unwrap();
        let topo = topology(&mesh);
        assert!(mesh.triangle_count() > 0);
        assert_eq!(topo.unbalanced_edges, 0, "seed {seed}: {topo:?}");
        assert_eq!(topo.non_manifold_edges, 0, "seed {seed}: {topo:?}");
        assert!(topo.signed_volume > 0.0, "seed {seed}: {topo:?}");
    }
}

#[test]
fn dual_contouring_is_closed_and_outward() {
    for seed in SEEDS {
        let mesh = mesh_field(&field(seed), Mesher::DualContouring, 48).unwrap();
        let topo = topology(&mesh);
        assert!(mesh.triangle_count() > 0);
        assert_eq!(topo.unbalanced_edges, 0, "seed {seed}: {topo:?}");
        assert!(topo.signed_volume > 0.0, "seed {seed}: {topo:?}");
        println!(
            "seed {seed}: {} non-manifold edges of {} triangles",
            topo.non_manifold_edges,
            mesh.triangle_count()
        );
    }
}

#[test]
fn normals_are_unit_length_and_agree_with_faces() {
    for mesher in MESHERS {
        for seed in SEEDS {
            let mesh = mesh_field(&field(seed), mesher, 48).unwrap();
            assert_eq!(mesh.normals.len(), mesh.positions.len());
            for n in &mesh.normals {
                assert!((Vec3::from(*n).length() - 1.0).abs() < 1e-4);
            }
            let agreement = facing_agreement(&mesh);
            println!("{mesher:?} seed {seed}: {agreement} of faces agree with normals");
            assert!(agreement > 0.99, "{mesher:?} seed {seed}: {agreement}");
        }
    }
}

#[test]
fn positions_stay_inside_the_cube() {
    for mesher in MESHERS {
        let mesh = mesh_field(&field(3), mesher, 32).unwrap();
        let (min, max) = mesh.bounds().unwrap();
        assert!(min.iter().chain(&max).all(|v| v.abs() <= 0.5));
    }
}

#[test]
fn clipped_rock_is_still_closed() {
    // Clipping an oversized sphere should close the surface at the cube faces.
    let mut options = StoneOptions::default();
    options.carve.steps = 0;
    options.carve.base_radius = 1.2;
    options.surface.displacement_scale = 0.0;
    let field = StoneField::new(&options).unwrap();
    for mesher in MESHERS {
        let mesh = mesh_field(&field, mesher, 16).unwrap();
        let topo = topology(&mesh);
        assert_eq!(topo.unbalanced_edges, 0, "{mesher:?}: {topo:?}");
        assert!(topo.signed_volume > 0.0, "{mesher:?}: {topo:?}");
    }
}

#[test]
fn empty_field_gives_empty_mesh() {
    // A cut with radius 1, centred 0.1 from the origin, removes the entire
    // starting sphere and should leave no mesh.
    let mut options = StoneOptions::default();
    options.carve.steps = 1;
    options.carve.radius_min = 0.1;
    options.carve.radius_jitter = 0.0;
    options.carve.depth = 10.0;
    let field = StoneField::new(&options).unwrap();
    for mesher in MESHERS {
        let mesh = mesh_field(&field, mesher, 8).unwrap();
        assert_eq!(mesh, StoneMesh::default());
        assert_eq!(mesh.bounds(), None);
    }
}

#[test]
fn meshing_is_deterministic() {
    for mesher in MESHERS {
        let a = mesh_field(&field(7), mesher, 40).unwrap();
        let b = mesh_field(&field(7), mesher, 40).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, mesh_field(&field(8), mesher, 40).unwrap());
    }
}

#[test]
fn triangle_count_grows_with_the_square_of_resolution() {
    for mesher in MESHERS {
        let coarse = mesh_field(&field(880), mesher, 32)
            .unwrap()
            .triangle_count();
        let fine = mesh_field(&field(880), mesher, 64)
            .unwrap()
            .triangle_count();
        let ratio = fine as f32 / coarse as f32;
        assert!((3.0..5.0).contains(&ratio), "{mesher:?}: ratio {ratio}");
    }
}

#[test]
fn rocks_are_one_piece() {
    for mesher in MESHERS {
        for seed in SEEDS {
            let mesh = mesh_field(&field(seed), mesher, 64).unwrap();
            let topo = topology(&mesh);
            let largest = topo.components[0] as f32 / mesh.triangle_count() as f32;
            println!(
                "{mesher:?} seed {seed}: pieces {:?}, degenerate {}",
                topo.components, topo.degenerate_triangles
            );
            assert!(
                largest >= 0.99,
                "{mesher:?} seed {seed}: {:?}",
                topo.components
            );
        }
    }
}

#[test]
fn resolution_is_checked() {
    let field = field(1);
    for resolution in [0, 7, 513] {
        assert!(matches!(
            mesh_field(&field, Mesher::MarchingCubes, resolution),
            Err(OptionsError::IntOutOfRange {
                field: "resolution",
                ..
            })
        ));
    }
}
