//! Every preset across the whole decay range, several seeds and the
//! strongest overrides: shells stay closed and outward, and normals agree
//! with the triangles' winding.

use glam::Vec3;
use procgena_deadwood::{
    DeadwoodDetail, DeadwoodMesh, DeadwoodOptions, DeadwoodPreset, MeshBuffers, Piece,
};
use procgena_specimen::Surface;

/// Signed volume enclosed by triangles `indices` of `b`; positive when the
/// faces point outward. Measured about the first vertex in double
/// precision.
fn signed_volume(b: &MeshBuffers, indices: &[u32]) -> f64 {
    let at = |i: u32| Vec3::from(b.positions[i as usize]).as_dvec3();
    let origin = at(indices[0]);
    indices
        .chunks(3)
        .map(|t| {
            let [p, q, r] = [t[0], t[1], t[2]].map(|i| at(i) - origin);
            p.dot(q.cross(r)) / 6.0
        })
        .sum()
}

/// Checks that every stem's shell is closed and faces outward, and that
/// vertex normals agree with the triangles' winding (the order of their
/// corners, which sets which side faces out).
fn check(mesh: &DeadwoodMesh, name: &str) {
    let b = &mesh.surfaces;
    for part in &mesh.parts {
        let part_indices = &b.indices[part.indices.start as usize..part.indices.end as usize];
        for (s, shell) in part.shells.iter().enumerate() {
            let tris = &part_indices[shell.start as usize * 3..shell.end as usize * 3];
            let surface = Surface::new(&b.positions, &b.normals, tris).unwrap();
            assert!(surface.is_closed(), "{name}: shell {s} open");
            assert!(signed_volume(b, tris) > 0.0, "{name}: shell {s} inside out");
        }
    }

    // Each triangle's face direction should agree with its vertex normals,
    // apart from a few on sharp splinters and creases.
    let (mut agree, mut total) = (0usize, 0usize);
    for t in b.indices.chunks(3) {
        let [p, q, r] = [t[0], t[1], t[2]].map(|i| Vec3::from(b.positions[i as usize]));
        let face = (q - p).cross(r - p);
        if face.length_squared() < 1e-14 {
            continue;
        }
        let n: Vec3 = t.iter().map(|&i| Vec3::from(b.normals[i as usize])).sum();
        total += 1;
        if face.dot(n) > 0.0 {
            agree += 1;
        }
    }
    let share = agree as f32 / total.max(1) as f32;
    assert!(
        share > 0.97,
        "{name}: only {share} of triangles agree with their normals"
    );
}

#[test]
fn presets_stay_closed_across_decay_seeds_and_overrides() {
    for preset in DeadwoodPreset::ALL {
        for decay in [0.0, 1.0, 2.0, 3.0, 4.0, 5.0] {
            for seed in [1, 2, 3] {
                for strong in [false, true] {
                    let mut o: DeadwoodOptions = preset.options();
                    o.decay = decay;
                    o.seed = seed;
                    if strong {
                        o.settle.sag = Some(1.0);
                        o.stubs.keep = Some(1.0);
                        o.branching.keep = Some(1.0);
                    }
                    let piece = Piece::new(o).unwrap();
                    for (label, detail) in
                        [("full", DeadwoodDetail::FULL), ("low", DeadwoodDetail::LOW)]
                    {
                        let name = format!(
                            "{} decay {decay} seed {seed} strong {strong} {label}",
                            preset.name()
                        );
                        check(&piece.mesh_with_detail(&detail), &name);
                    }
                }
            }
        }
    }
}
