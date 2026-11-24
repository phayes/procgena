//! Specimens, clusters and scatter.

use glam::Vec3;
use procgena_mushroom::{
    ClusterLayout, ClusterOptions, Mushroom, MushroomInstance, MushroomLod, MushroomOptions,
    MushroomPreset, MushroomScatter, ScatterError, ScatterOptions,
};
use procgena_specimen::{Part, Specimen, SurfaceMesh, TagFilter, tags};

#[test]
fn specimen_has_one_level_per_lod_level_with_tags() {
    let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
    let lod = MushroomLod::for_size(mushroom.options().size);
    let specimen = mushroom.specimen(&lod).unwrap();
    specimen.validate().unwrap();
    assert_eq!(specimen.levels.len(), lod.levels.len());
    for (level, lod_level) in specimen.levels.iter().zip(&lod.levels) {
        assert_eq!(level.start, lod_level.distance);
        for part in &level.parts {
            assert!(part.tags.contains(&tags::MUSHROOM));
        }
    }

    // The full level has every kind; caps can be picked by tag.
    let full = &specimen.levels[0];
    for tag in [tags::CAP, tags::STEM, tags::ANNULUS] {
        assert!(full.parts.iter().any(|p| p.tags.contains(&tag)), "{tag}");
    }
    let caps = specimen.merged(0, &TagFilter::any_of([tags::CAP])).unwrap();
    assert!(caps.closed);
}

/// Returns `base` grown as a cluster of `stems` with `layout`.
fn cluster(base: MushroomPreset, stems: u32, layout: ClusterLayout) -> Mushroom {
    let mut options = base.options();
    options.cluster = ClusterOptions {
        stems,
        layout,
        ..options.cluster
    };
    Mushroom::new(options).unwrap()
}

/// Smallest distance between two members' stem bases.
fn min_base_gap(members: &[MushroomInstance]) -> f32 {
    let mut best = f32::MAX;
    for (i, a) in members.iter().enumerate() {
        for b in &members[i + 1..] {
            best = best.min(a.translation.distance(b.translation));
        }
    }
    best
}

#[test]
fn one_stem_is_exactly_the_mushroom_its_options_describe() {
    let mut options = MushroomPreset::Porcini.options();
    options.cluster.layout = ClusterLayout::FairyRing;
    let mushroom = Mushroom::new(options.clone()).unwrap();
    assert_eq!(mushroom.bodies().len(), 1);
    assert_eq!(mushroom.members().len(), 1);
    // The body keeps every setting but the cluster's and the seed, which
    // comes from the mushroom's specimen id.
    let body = mushroom.bodies()[0].options();
    assert_eq!(body.cluster, ClusterOptions::default());
    assert_eq!(
        *body,
        MushroomOptions {
            seed: body.seed,
            cluster: ClusterOptions::default(),
            ..options.clone()
        }
    );
    assert_eq!(
        mushroom.mesh(),
        mushroom.bodies()[0]
            .mesh_with_detail(&options.detail)
            .unwrap()
    );
}

#[test]
fn clusters_spread_their_members() {
    for layout in [
        ClusterLayout::Tuft,
        ClusterLayout::Line,
        ClusterLayout::FairyRing,
    ] {
        let mushroom = cluster(MushroomPreset::Button, 5, layout);
        let members = mushroom.members();
        assert!(members.len() >= 3, "{layout:?}: {}", members.len());
        assert!(
            members
                .iter()
                .all(|m| (m.variant as usize) < mushroom.bodies().len())
        );
        assert!(min_base_gap(members) > 0.0, "{layout:?}");

        // The combined specimen keeps one closed cap piece per member.
        let specimen = mushroom.specimen(&MushroomLod::default()).unwrap();
        specimen.validate().unwrap();
        let caps = specimen.merged(0, &TagFilter::any_of([tags::CAP])).unwrap();
        assert!(caps.closed);
        assert_eq!(caps.shells.len(), members.len());

        // The combined mesh holds every member's cap.
        let one = mushroom.bodies()[0].mesh_with_detail(&MushroomOptions::default().detail);
        let single_caps = one
            .unwrap()
            .part(procgena_mushroom::PartKind::Cap)
            .unwrap()
            .clone();
        let all_caps = mushroom
            .mesh()
            .part(procgena_mushroom::PartKind::Cap)
            .unwrap()
            .clone();
        assert!(all_caps.indices.len() > single_caps.indices.len() * 2);
    }
}

#[test]
fn bodies_are_never_more_than_stems() {
    let mushroom = cluster(MushroomPreset::Button, 2, ClusterLayout::Tuft);
    assert!(mushroom.bodies().len() <= 2);
    for body in mushroom.bodies() {
        assert_eq!(body.options().cluster.stems, 1);
    }
}

#[test]
fn fairy_rings_stand_on_a_wide_circle() {
    let mut options = MushroomPreset::Button.options();
    options.cluster.stems = 24;
    options.cluster.layout = ClusterLayout::FairyRing;
    options.cluster.fairy_ring.radius = 20.0;
    options.cluster.fairy_ring.width = 0.05;
    let mushroom = Mushroom::new(options.clone()).unwrap();
    let radius = options.cluster.fairy_ring.radius * options.size;
    assert!(mushroom.members().len() >= 20);
    for m in mushroom.members() {
        let distance = m.translation.x.hypot(m.translation.z);
        assert!(
            (distance - radius).abs() < radius * 0.15,
            "a member stands {distance} m from the centre of a {radius} m ring"
        );
    }

    // The bounds hold the whole ring.
    let (min, max) = mushroom.bounds();
    assert!(min.x < -radius * 0.8 && max.x > radius * 0.8, "{min} {max}");
}

#[test]
fn swept_clusters_fan_outward() {
    let mut options = MushroomPreset::Enoki.options();
    options.cluster = ClusterOptions {
        stems: 8,
        lean: 0.0,
        sweep: 0.4,
        ..options.cluster
    };
    let swept = Mushroom::new(options.clone()).unwrap();
    assert!(swept.members().len() >= 4);

    // Every cap sits further from the cluster's centre line than its stem
    // base, and well outside the base when the base is near the centre.
    let centre_distance = |p: Vec3| p.x.hypot(p.z);
    for m in swept.members() {
        let body = &swept.bodies()[m.variant as usize];
        let cap = m.transform().transform_point3(body.cap_centre());
        let reach = centre_distance(cap) - centre_distance(m.translation);
        let stem = body.cap_centre().y * m.scale;
        assert!(reach > stem * 0.2, "reach {reach} for a {stem} m stem");
    }

    // Without sweep, nothing is aimed outward: some caps lean back in.
    options.cluster.sweep = 0.0;
    let unswept = Mushroom::new(options).unwrap();
    assert!(unswept.members().iter().any(|m| {
        let cap = m
            .transform()
            .transform_point3(unswept.bodies()[m.variant as usize].cap_centre());
        centre_distance(cap) <= centre_distance(m.translation) + 1e-4
    }));
}

#[test]
fn cluster_caps_do_not_overlap() {
    for (preset, layout) in [
        (MushroomPreset::Button, ClusterLayout::Tuft),
        (MushroomPreset::Porcini, ClusterLayout::Line),
        (MushroomPreset::Enoki, ClusterLayout::Tuft),
        (MushroomPreset::FairyRing, ClusterLayout::FairyRing),
    ] {
        let mushroom = cluster(preset, 8, layout);

        // Treat each cap as a ball around its middle with the cap's radius.
        let caps: Vec<(Vec3, f32)> = mushroom
            .members()
            .iter()
            .map(|m| {
                let v = &mushroom.bodies()[m.variant as usize];
                let middle = (v.cap_centre() + v.apex()) * 0.5;
                (
                    m.transform().transform_point3(middle),
                    v.cap_radius() * m.scale,
                )
            })
            .collect();
        for (i, (a, ra)) in caps.iter().enumerate() {
            for (b, rb) in &caps[i + 1..] {
                let gap = a.distance(*b) - (ra + rb);
                assert!(gap >= -1e-5, "{} caps overlap by {} m", preset.name(), -gap);
            }
        }
    }
}

#[test]
fn clusters_are_repeatable() {
    let a = Mushroom::from_preset(MushroomPreset::Enoki);
    let b = Mushroom::from_preset(MushroomPreset::Enoki);
    assert_eq!(a.members(), b.members());
}

/// A 2 m ground patch.
fn ground() -> Specimen {
    Specimen::single(vec![Part::new([tags::GROUND], SurfaceMesh::ground(2.0, 8))])
}

#[test]
fn scatter_places_mushrooms_on_the_ground() {
    let options = ScatterOptions {
        density: 10.0,
        ..ScatterOptions::default()
    };
    let scatter =
        MushroomScatter::on(&ground(), &MushroomPreset::Porcini.options(), &options).unwrap();
    assert!(scatter.instances().len() > 10);
    for m in scatter.instances() {
        // Near the patch, just below its surface, growing mostly up.
        assert!(
            m.translation.x.abs() <= 1.5 && m.translation.z.abs() <= 1.5,
            "{:?}",
            m.translation
        );
        assert!(
            m.translation.y <= 1e-4 && m.translation.y > -0.1,
            "{:?}",
            m.translation
        );
        assert!((m.rotation * Vec3::Y).y > 0.5);
    }
    let specimen = scatter.specimen(&MushroomLod::default()).unwrap();
    specimen.validate().unwrap();
}

#[test]
fn scattered_clusters_drop_their_members_onto_the_surface() {
    // A ground patch tilted about X, so members spread across it land at
    // different heights.
    let mut tilted = SurfaceMesh::ground(4.0, 16);
    for p in &mut tilted.positions {
        p[1] = p[2] * 0.2;
    }
    let host = Specimen::single(vec![Part::new([tags::GROUND], tilted)]);
    let mut mushroom = MushroomPreset::FairyRing.options();
    mushroom.cluster.fairy_ring.radius = 10.0;
    let options = ScatterOptions {
        density: 0.3,
        min_spacing: 1.0,
        sink: 0.0,
        ..ScatterOptions::default()
    };
    let scatter = MushroomScatter::on(&host, &mushroom, &options).unwrap();
    assert!(scatter.instances().len() > 10);
    for m in scatter.instances() {
        let surface = m.translation.z * 0.2;
        assert!(
            (m.translation.y - surface).abs() < 1e-3,
            "a member at {} is off the surface at {surface}",
            m.translation
        );
    }
}

#[test]
fn scatter_respects_the_tag_filter_and_slope() {
    let mushroom = MushroomPreset::Button.options();
    let rock_only = ScatterOptions {
        grows_on: TagFilter::any_of([tags::ROCK]),
        ..ScatterOptions::default()
    };
    assert_eq!(
        MushroomScatter::on(&ground(), &mushroom, &rock_only).unwrap_err(),
        ScatterError::NoSurface
    );

    // With "up" sideways, the flat ground is a wall: nothing grows.
    let sideways = ScatterOptions {
        up: Vec3::X,
        ..ScatterOptions::default()
    };
    let scatter = MushroomScatter::on(&ground(), &mushroom, &sideways).unwrap();
    assert!(scatter.instances().is_empty());
}
