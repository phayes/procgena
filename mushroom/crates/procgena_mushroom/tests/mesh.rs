//! Mesh checks across presets and detail levels.

use glam::Vec3;
use procgena_mushroom::{
    BodyShape, MeshBuffers, Mushroom, MushroomDetail, MushroomLod, MushroomOptions, MushroomPreset,
    PartKind,
};

/// Every named detail level.
fn details() -> impl Iterator<Item = MushroomDetail> {
    MushroomDetail::PRESETS.into_iter().map(|(_, d)| d)
}

/// Signed volume enclosed by triangles `range` of `buffers`; positive when
/// faces point outward.
fn signed_volume(buffers: &MeshBuffers, range: std::ops::Range<u32>) -> f32 {
    buffers.indices[range.start as usize..range.end as usize]
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(buffers.positions[i as usize]));
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

#[test]
fn every_preset_meshes_cleanly_at_every_detail() {
    for preset in MushroomPreset::ALL {
        let mushroom = Mushroom::from_preset(preset);
        for detail in details() {
            let mesh = mushroom.mesh_with_detail(&detail).unwrap();
            let b = &mesh.surfaces;
            let name = format!("{} {:?}", preset.name(), detail.shape);
            assert!(b.triangle_count() > 0, "{name}");
            assert_eq!(b.normals.len(), b.vertex_count(), "{name}");
            assert_eq!(b.uvs.len(), b.vertex_count(), "{name}");
            assert_eq!(b.tangents.len(), b.vertex_count(), "{name}");
            assert_eq!(b.colors.len(), b.vertex_count(), "{name}");
            assert_eq!(b.zones.len(), b.vertex_count(), "{name}");
            assert!(
                b.indices.iter().all(|&i| (i as usize) < b.vertex_count()),
                "{name}"
            );
            for (p, n) in b.positions.iter().zip(&b.normals) {
                assert!(p.iter().all(|c| c.is_finite()), "{name}: {p:?}");
                let len = Vec3::from(*n).length();
                assert!((len - 1.0).abs() < 1e-3, "{name}: normal length {len}");
            }
        }
    }
}

#[test]
fn solid_parts_are_closed_and_face_outward() {
    let solid = [MushroomDetail::FULL, MushroomDetail::HIGH];
    for (preset, detail) in MushroomPreset::ALL
        .into_iter()
        .flat_map(|p| solid.map(|d| (p, d)))
    {
        let mushroom = Mushroom::from_preset(preset);
        let mesh = mushroom.mesh_with_detail(&detail).unwrap();
        assert!(!mesh.double_sided);
        for (range, part) in mesh.parts.iter().zip(mesh.object_parts()) {
            let surface = part.surface().unwrap();
            assert_eq!(
                surface.is_closed(),
                range.closed,
                "{} {:?}",
                preset.name(),
                range.kind
            );
            if range.closed {
                for shell in &range.shells {
                    let first = range.indices.start + shell.start * 3;
                    let volume =
                        signed_volume(&mesh.surfaces, first..range.indices.start + shell.end * 3);
                    assert!(
                        volume > 0.0,
                        "{} {:?} volume {volume}",
                        preset.name(),
                        range.kind
                    );
                }
            }
        }
        let cap = mesh.part(PartKind::Cap).unwrap();
        assert!(cap.closed);
        assert!(mesh.part(PartKind::Stem).unwrap().closed);
    }
}

#[test]
fn shell_and_flat_levels_are_open_and_double_sided() {
    let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
    for detail in [
        MushroomDetail::MEDIUM,
        MushroomDetail::LOW,
        MushroomDetail::FLAT,
    ] {
        let mesh = mushroom.mesh_with_detail(&detail).unwrap();
        assert!(mesh.double_sided);
        for part in mesh.object_parts() {
            assert!(!part.closed);
            assert!(!part.surface().unwrap().is_closed());
        }
    }
}

#[test]
fn only_flat_meshes_have_a_stem_ribbon() {
    let mushroom = Mushroom::from_preset(MushroomPreset::Porcini);
    for detail in details() {
        let mesh = mushroom.mesh_with_detail(&detail).unwrap();
        let flat = detail.shape == BodyShape::Flat;
        assert_eq!(mesh.stem_ribbon.is_some(), flat);
        assert_eq!(mesh.part(PartKind::Stem).is_none(), flat);
        if let Some(ribbon) = &mesh.stem_ribbon {
            assert_eq!(ribbon.billboards.len(), ribbon.strip.vertex_count());
            assert_eq!(ribbon.strip.vertex_count(), ribbon.cull.vertex_count());
            assert_eq!(ribbon.strip.indices, ribbon.cull.indices);
            for b in &ribbon.billboards {
                assert!((Vec3::from(b.axis).length() - 1.0).abs() < 1e-4);
            }
        }
    }
}

#[test]
fn turned_stem_strips_face_the_viewer() {
    let mushroom = Mushroom::from_preset(MushroomPreset::LibertyCap);
    let mesh = mushroom.mesh_with_detail(&MushroomDetail::FLAT).unwrap();
    let ribbon = mesh.stem_ribbon.unwrap();
    for view in [Vec3::X, Vec3::NEG_Z, Vec3::new(0.6, 0.2, 0.8).normalize()] {
        // Turn every vertex as a renderer would for a viewer in direction `view`.
        let turned: Vec<Vec3> = ribbon
            .billboards
            .iter()
            .map(|b| {
                let axis = Vec3::from(b.axis);
                let right = axis.cross(view).normalize();
                Vec3::from(b.pivot) + right * b.offset[0] + axis * b.offset[1]
            })
            .collect();
        for t in ribbon.strip.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| turned[i as usize]);
            let facing = (b - a).cross(c - a).dot(view);
            assert!(facing > 0.0, "a turned triangle faces away from {view}");
        }
    }
}

#[test]
fn triangle_counts_fall_along_the_default_ladder() {
    for preset in MushroomPreset::ALL {
        let mushroom = Mushroom::from_preset(preset);
        let lod = MushroomLod::for_size(mushroom.options().size);
        let counts: Vec<usize> = mushroom
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
        assert!(
            *counts.last().unwrap() < 200,
            "{}: {counts:?}",
            preset.name()
        );
    }
}

#[test]
fn meshing_is_repeatable_and_depends_on_the_seed() {
    let a = Mushroom::from_preset(MushroomPreset::FlyAgaric).mesh();
    let b = Mushroom::from_preset(MushroomPreset::FlyAgaric).mesh();
    assert_eq!(a, b);

    let mut options = MushroomPreset::FlyAgaric.options();
    options.seed += 1;
    let c = Mushroom::new(options).unwrap().mesh();
    assert_ne!(a.surfaces.positions, c.surfaces.positions);
}

#[test]
fn detail_levels_share_the_silhouette() {
    // Every level's bounding box stays close to the full mesh's, so the
    // overall shape doesn't jump when levels switch.
    let mushroom = Mushroom::from_preset(MushroomPreset::Parasol);
    let bounds = |detail: &MushroomDetail| {
        let mesh = mushroom.mesh_with_detail(detail).unwrap();
        let mut points: Vec<Vec3> = mesh.surfaces.positions.iter().map(|&p| p.into()).collect();
        if let Some(r) = &mesh.stem_ribbon {
            points.extend(r.strip.positions.iter().map(|&p| Vec3::from(p)));
        }
        let min = points.iter().copied().fold(Vec3::MAX, Vec3::min);
        let max = points.iter().copied().fold(Vec3::MIN, Vec3::max);
        (min, max)
    };
    let (full_min, full_max) = bounds(&MushroomDetail::FULL);
    let size = mushroom.options().size;
    for detail in details() {
        let (min, max) = bounds(&detail);
        assert!(min.distance(full_min) < size * 0.25, "{:?}", detail.shape);
        assert!(max.distance(full_max) < size * 0.25, "{:?}", detail.shape);
    }
}

#[test]
fn upturn_brings_the_cap_back_upright() {
    let leaning = |upturn: f32| {
        let mut o = MushroomOptions::default();
        o.cap.tilt = 0.0;
        o.stem.lean = 0.5;
        o.stem.curve = 2.0;
        o.stem.upturn = upturn;
        Mushroom::new(o).unwrap()
    };
    let (bent, upturned) = (leaning(0.0), leaning(1.0));

    // Both reach the same sideways offset, but only the upturned stem
    // arrives at the cap pointing straight up.
    assert!((bent.cap_centre() - upturned.cap_centre()).length() < 1e-5);
    assert!(bent.cap_axis().y < 0.8, "{}", bent.cap_axis());
    assert!(upturned.cap_axis().y > 0.999, "{}", upturned.cap_axis());

    // The mesh stays clean.
    for detail in details() {
        let mesh = upturned.mesh_with_detail(&detail).unwrap();
        assert!(
            mesh.surfaces
                .positions
                .iter()
                .flatten()
                .all(|c| c.is_finite())
        );
    }
}

#[test]
fn stem_base_sits_at_the_origin_and_cap_on_top() {
    let mushroom = Mushroom::new(MushroomOptions::default()).unwrap();
    let mesh = mushroom.mesh();
    let min_y = mesh
        .surfaces
        .positions
        .iter()
        .map(|p| p[1])
        .fold(f32::MAX, f32::min);
    assert!(min_y.abs() < 1e-3, "lowest point {min_y}");
    assert!(mushroom.apex().y > mushroom.cap_centre().y);
}

#[test]
fn invalid_detail_is_rejected() {
    let mushroom = Mushroom::from_preset(MushroomPreset::Button);
    let detail = MushroomDetail {
        cap_sides: 2,
        ..MushroomDetail::FULL
    };
    assert!(mushroom.mesh_with_detail(&detail).is_err());
}
