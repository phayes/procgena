//! Mesh checks across presets and detail levels.

use glam::Vec3;
use procgena_core::chain::Step;
use procgena_mushroom::{
    MeshBuffers, Mushroom, MushroomDetail, MushroomOptions, MushroomPreset, PartKind,
};

/// Every named detail.
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

/// Checks that every list of `b` has one entry per vertex, every triangle
/// refers to a real vertex, and positions and normals are sound.
fn check_buffers(b: &MeshBuffers, name: &str) {
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

#[test]
fn every_preset_meshes_cleanly_at_every_detail() {
    for preset in MushroomPreset::all() {
        let mushroom = Mushroom::from_preset(preset).unwrap();
        for detail in details() {
            let mesh = mushroom.mesh_with_detail(&detail).unwrap();
            let name = format!("{} {:?}", preset.name, detail.shape);
            assert!(mesh.triangle_count() > 0, "{name}");
            check_buffers(&mesh.surfaces, &name);
        }
    }
}

#[test]
fn solid_parts_are_closed_and_face_outward() {
    let solid = [MushroomDetail::FULL, MushroomDetail::HIGH];
    for (preset, detail) in MushroomPreset::all().flat_map(|p| solid.map(|d| (p, d))) {
        let mushroom = Mushroom::from_preset(preset).unwrap();
        let mesh = mushroom.mesh_with_detail(&detail).unwrap();
        assert!(!mesh.double_sided);
        for (range, part) in mesh.parts.iter().zip(mesh.specimen_parts()) {
            let surface = part.surface().unwrap();
            assert_eq!(
                surface.is_closed(),
                range.closed,
                "{} {:?}",
                preset.name,
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
                        preset.name,
                        range.kind
                    );
                }
            }
        }

        // The stem is closed exactly when its bottom is.
        let cap = mesh.part(PartKind::Cap).unwrap();
        assert!(cap.closed);
        let closed_bottom = preset.mushroom.bulb.closed;
        assert_eq!(
            mesh.part(PartKind::Stem).unwrap().closed,
            closed_bottom,
            "{}",
            preset.name
        );
    }
}

#[test]
fn shell_details_are_open_and_double_sided() {
    let mushroom = Mushroom::from_preset(MushroomPreset::fly_agaric()).unwrap();
    for detail in [MushroomDetail::MEDIUM, MushroomDetail::LOW] {
        let mesh = mushroom.mesh_with_detail(&detail).unwrap();
        assert!(mesh.double_sided);
        for part in mesh.specimen_parts() {
            assert!(!part.closed);
            assert!(!part.surface().unwrap().is_closed());
        }
    }
}

#[test]
fn triangle_counts_fall_along_the_named_details() {
    for preset in MushroomPreset::all() {
        let mushroom = Mushroom::from_preset(preset).unwrap();
        let counts: Vec<usize> = details()
            .map(|d| mushroom.mesh_with_detail(&d).unwrap().triangle_count())
            .collect();
        assert!(
            counts.windows(2).all(|w| w[1] < w[0]),
            "{}: {counts:?}",
            preset.name
        );
    }
}

#[test]
fn the_lower_mesh_is_open_and_cheaper_than_the_game_level() {
    for preset in MushroomPreset::all() {
        let mushroom = Mushroom::from_preset(preset).unwrap();
        let [(game_step, game), (lower_step, lower)] = mushroom.lod_meshes();
        assert_eq!((game_step, lower_step), (Step::Game, Step::Lower));
        assert_eq!(game, mushroom.mesh(), "{}", preset.name);
        assert!(lower.double_sided, "{}", preset.name);
        assert!(lower.part(PartKind::Annulus).is_none(), "{}", preset.name);
        assert!(
            lower.triangle_count() * 3 < game.triangle_count(),
            "{}: {} against {}",
            preset.name,
            lower.triangle_count(),
            game.triangle_count()
        );
    }
}

#[test]
fn meshing_is_repeatable_and_depends_on_the_seed() {
    let a = Mushroom::from_preset(MushroomPreset::fly_agaric())
        .unwrap()
        .mesh();
    let b = Mushroom::from_preset(MushroomPreset::fly_agaric())
        .unwrap()
        .mesh();
    assert_eq!(a, b);

    let mut options = MushroomPreset::fly_agaric().mushroom.clone();
    options.default_seed += 1;
    let c = Mushroom::new(options).unwrap().mesh();
    assert_ne!(a.surfaces.positions, c.surfaces.positions);
}

#[test]
fn details_share_the_silhouette() {
    // Every detail's bounding box stays close to the full mesh's, so the
    // overall shape doesn't jump when a renderer switches meshes.
    let mushroom = Mushroom::from_preset(MushroomPreset::parasol()).unwrap();
    let bounds = |detail: &MushroomDetail| {
        let mesh = mushroom.mesh_with_detail(detail).unwrap();
        let points: Vec<Vec3> = mesh.surfaces.positions.iter().map(|&p| p.into()).collect();
        let min = points.iter().copied().fold(Vec3::MAX, Vec3::min);
        let max = points.iter().copied().fold(Vec3::MIN, Vec3::max);
        (min, max)
    };
    let (full_min, full_max) = bounds(&MushroomDetail::FULL);
    let size = mushroom.options().size;
    let lower = MushroomDetail::FULL.lod().lower;
    for detail in details().chain([lower]) {
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
    let (bent_body, upturned_body) = (&bent.bodies()[0], &upturned.bodies()[0]);

    // Both reach the same sideways offset, but only the upturned stem
    // arrives at the cap pointing straight up.
    assert!((bent_body.cap_centre() - upturned_body.cap_centre()).length() < 1e-5);
    assert!(bent_body.cap_axis().y < 0.8, "{}", bent_body.cap_axis());
    assert!(
        upturned_body.cap_axis().y > 0.999,
        "{}",
        upturned_body.cap_axis()
    );

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

/// Returns the lowest height of `mushroom`'s full mesh.
fn lowest(mushroom: &Mushroom) -> f32 {
    mushroom
        .mesh()
        .surfaces
        .positions
        .iter()
        .map(|p| p[1])
        .fold(f32::MAX, f32::min)
}

#[test]
fn a_closed_bottom_hangs_below_the_ground() {
    let mut options = MushroomOptions::default();
    options.stem.lean = 0.0;
    options.bulb.depth = 0.5;
    let mushroom = Mushroom::new(options.clone()).unwrap();
    let body = &mushroom.bodies()[0];
    let depth = options.bulb.depth * body.base_radius();
    let low = lowest(&mushroom);
    assert!(
        (low + depth).abs() < depth * 0.1,
        "lowest point {low}, expected {}",
        -depth
    );
    assert!((body.bottom().y + depth).abs() < 1e-4);
    assert!(body.apex().y > body.cap_centre().y);

    // Shell levels close the bottom too.
    let shell = mushroom.mesh_with_detail(&MushroomDetail::MEDIUM).unwrap();
    let shell_low = shell
        .surfaces
        .positions
        .iter()
        .map(|p| p[1])
        .fold(f32::MAX, f32::min);
    assert!(shell_low < -depth * 0.9, "{shell_low}");
}

#[test]
fn an_open_stem_ends_at_the_ground_with_fewer_triangles() {
    let closed = MushroomOptions::default();
    let mut open = closed.clone();
    open.bulb.closed = false;
    let (closed, open) = (Mushroom::new(closed).unwrap(), Mushroom::new(open).unwrap());
    assert!(lowest(&open).abs() < 1e-4, "lowest point {}", lowest(&open));
    assert_eq!(open.bodies()[0].bottom(), Vec3::ZERO);

    // An open stem is not closed, and is cheaper.
    let stem = |m: &Mushroom| m.mesh().part(PartKind::Stem).unwrap().clone();
    assert!(!stem(&open).closed);
    assert!(stem(&closed).closed);
    assert!(stem(&open).indices.len() < stem(&closed).indices.len());
}

#[test]
fn painted_warts_tint_the_cap_only_when_asked() {
    let mushroom = Mushroom::from_preset(MushroomPreset::fly_agaric()).unwrap();
    let painted = mushroom.mesh_with_detail(&MushroomDetail::FULL).unwrap();
    let plain = mushroom
        .mesh_with_detail(&MushroomDetail {
            paint_warts: false,
            ..MushroomDetail::FULL
        })
        .unwrap();
    assert_eq!(painted.surfaces.positions, plain.surfaces.positions);
    assert_ne!(painted.surfaces.colors, plain.surfaces.colors);
}

#[test]
fn invalid_detail_is_rejected() {
    let mushroom = Mushroom::from_preset(MushroomPreset::button()).unwrap();
    let detail = MushroomDetail {
        cap_sides: 2,
        ..MushroomDetail::FULL
    };
    assert!(mushroom.mesh_with_detail(&detail).is_err());
}
