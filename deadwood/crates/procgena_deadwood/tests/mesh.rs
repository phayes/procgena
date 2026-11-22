//! Mesh checks across presets, end shapes and levels of detail.

use glam::Vec3;
use procgena_deadwood::{
    DeadwoodDetail, DeadwoodLod, DeadwoodMesh, DeadwoodOptions, DeadwoodPreset, EndKind,
    EndsOptions, HollowOptions, MeshBuffers, Piece, PieceKind,
};
use procgena_mesh::Surface;

/// Every named detail level.
fn details() -> impl Iterator<Item = DeadwoodDetail> {
    DeadwoodDetail::PRESETS.into_iter().map(|(_, d)| d)
}

/// Signed volume enclosed by triangles `indices` of `b`; positive when
/// faces point outward. Measured about the first vertex in double
/// precision, so tiny twigs far from the origin keep their sign.
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

/// Checks that every stem's shell in `mesh` is closed and faces outward.
fn assert_shells_closed(mesh: &DeadwoodMesh, name: &str) {
    let b = &mesh.surfaces;
    for part in &mesh.parts {
        let part_indices = &b.indices[part.indices.start as usize..part.indices.end as usize];
        let mut covered = 0;
        for (s, shell) in part.shells.iter().enumerate() {
            let tris = &part_indices[shell.start as usize * 3..shell.end as usize * 3];
            let surface = Surface::new(&b.positions, &b.normals, tris).unwrap();
            assert!(
                surface.is_closed(),
                "{name}: {:?} shell {s} is open",
                part.size
            );
            let volume = signed_volume(b, tris);
            assert!(
                volume > 0.0,
                "{name}: {:?} shell {s} volume {volume}",
                part.size
            );
            covered += shell.end - shell.start;
        }
        assert_eq!(covered as usize, part_indices.len() / 3, "{name}");
    }
}

/// Checks buffer lengths, index ranges, finite positions and unit normals.
fn assert_well_formed(b: &MeshBuffers, name: &str) {
    let n = b.vertex_count();
    assert_eq!(b.normals.len(), n, "{name}");
    assert_eq!(b.uvs.len(), n, "{name}");
    assert_eq!(b.tangents.len(), n, "{name}");
    assert_eq!(b.colors.len(), n, "{name}");
    assert_eq!(b.wood.len(), n, "{name}");
    assert!(b.indices.iter().all(|&i| (i as usize) < n), "{name}");
    for (p, nrm) in b.positions.iter().zip(&b.normals) {
        assert!(p.iter().all(|c| c.is_finite()), "{name}: {p:?}");
        let len = Vec3::from(*nrm).length();
        assert!((len - 1.0).abs() < 1e-3, "{name}: normal length {len}");
    }
}

#[test]
fn every_preset_meshes_cleanly_at_every_detail() {
    for preset in DeadwoodPreset::ALL {
        let piece = Piece::from_preset(preset);
        for detail in details() {
            let mesh = piece.mesh_with_detail(&detail);
            let name = format!("{} {}", preset.name(), detail.sides);
            assert!(mesh.surfaces.triangle_count() > 0, "{name}");
            assert_well_formed(&mesh.surfaces, &name);
            if let Some(r) = &mesh.ribbons {
                assert_well_formed(&r.strip, &name);
                assert_well_formed(&r.cull, &name);
                assert_eq!(r.billboards.len(), r.strip.vertex_count(), "{name}");
            }
            assert_shells_closed(&mesh, &name);
        }
    }
}

#[test]
fn every_end_kind_closes_solid_and_hollow() {
    let kinds = [
        EndKind::Saw { tilt: 25.0 },
        EndKind::Chainsaw {
            tilt: 10.0,
            step: 0.2,
        },
        EndKind::Axe { angle: 60.0 },
        EndKind::Snapped {
            jaggedness: 0.6,
            splinters: 10,
            one_sided: 0.8,
        },
        EndKind::Taper,
    ];
    for (k, &tip) in kinds.iter().enumerate() {
        for hollow in [
            None,
            Some([true, false]),
            Some([false, true]),
            Some([true, true]),
        ] {
            let mut o = DeadwoodOptions {
                seed: 40 + k as u32,
                ends: EndsOptions {
                    base: kinds[(k + 2) % kinds.len()],
                    tip,
                },
                ..Default::default()
            };
            if let Some([open_base, open_tip]) = hollow {
                o.hollow = HollowOptions {
                    enabled: true,
                    open_base,
                    open_tip,
                    ..Default::default()
                };
            }
            let piece = Piece::new(o).unwrap();
            for detail in details() {
                let name = format!("tip {tip:?} hollow {hollow:?} sides {}", detail.sides);
                let mesh = piece.mesh_with_detail(&detail);
                assert_well_formed(&mesh.surfaces, &name);
                assert_shells_closed(&mesh, &name);
            }
        }
    }
}

#[test]
fn blind_hollow_never_breaks_through_the_far_end() {
    // A straight log lying along +X, hollow from the base as deep as allowed.
    for tip in [
        EndKind::Saw { tilt: 0.0 },
        EndKind::Snapped {
            jaggedness: 0.6,
            splinters: 4,
            one_sided: 0.5,
        },
    ] {
        let o = DeadwoodOptions {
            decay: 0.0,
            ends: EndsOptions {
                base: EndKind::Saw { tilt: 0.0 },
                tip,
            },
            shape: procgena_deadwood::ShapeOptions {
                bend: 0.0,
                wobble: 0.0,
                ..Default::default()
            },
            hollow: HollowOptions {
                enabled: true,
                open_base: true,
                open_tip: false,
                depth: 1.0,
                ..Default::default()
            },
            settle: procgena_deadwood::SettleOptions {
                settle: false,
                ..Default::default()
            },
            stubs: procgena_deadwood::StubOptions {
                per_metre: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mesh = Piece::new(o)
            .unwrap()
            .mesh_with_detail(&DeadwoodDetail::FULL);
        let b = &mesh.surfaces;
        let deepest_hollow = (0..b.vertex_count())
            .filter(|&i| b.wood[i][0] == procgena_deadwood::Zone::Hollow as u32 as f32)
            .map(|i| b.positions[i][0])
            .fold(f32::MIN, f32::max);
        let nearest_tip = (0..b.vertex_count())
            .filter(|&i| b.wood[i][0] == procgena_deadwood::Zone::End as u32 as f32)
            .map(|i| b.positions[i][0])
            .filter(|&x| x > 1.5)
            .fold(f32::MAX, f32::min);
        assert!(
            deepest_hollow < nearest_tip,
            "{tip:?}: hollow reaches {deepest_hollow}, tip end starts at {nearest_tip}"
        );
    }
}

#[test]
fn same_options_give_the_same_mesh() {
    let a = Piece::from_preset(DeadwoodPreset::FallenLimb).mesh();
    let b = Piece::from_preset(DeadwoodPreset::FallenLimb).mesh();
    assert_eq!(a, b);
    let mut o = DeadwoodPreset::FallenLimb.options();
    o.seed += 1;
    let c = Piece::new(o).unwrap().mesh();
    assert_ne!(a, c);
}

#[test]
fn coarser_levels_have_fewer_triangles() {
    for preset in DeadwoodPreset::ALL {
        let piece = Piece::from_preset(preset);
        let lod = DeadwoodLod::for_options(piece.options());
        let counts: Vec<usize> = piece
            .lod_meshes(&lod)
            .unwrap()
            .iter()
            .map(|(_, m)| m.triangle_count())
            .collect();
        assert!(
            counts.windows(2).all(|w| w[1] < w[0]),
            "{}: {counts:?}",
            preset.name()
        );
    }
}

#[test]
fn coarse_levels_drop_and_flatten_thin_stems() {
    let piece = Piece::from_preset(DeadwoodPreset::FallenLimb);
    let full = piece.mesh_with_detail(&DeadwoodDetail::FULL);
    let low = piece.mesh_with_detail(&DeadwoodDetail::LOW);
    assert!(full.ribbons.is_none());
    let shells = |m: &DeadwoodMesh| m.parts.iter().map(|p| p.shells.len()).sum::<usize>();
    assert!(shells(&low) < shells(&full));
}

#[test]
fn branches_have_side_branches_and_logs_do_not() {
    let limb = Piece::from_preset(DeadwoodPreset::FallenLimb);
    assert_eq!(limb.options().kind, PieceKind::Branch);
    assert!(limb.stem_count() > 5);
    let log = Piece::new(DeadwoodOptions {
        stubs: procgena_deadwood::StubOptions {
            per_metre: 0.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    assert_eq!(log.stem_count(), 1);
}
