//! Objects, clusters and scatter.

use glam::Vec3;
use procgena_mesh::{Object, Part, SurfaceMesh, TagFilter, tags as mesh_tags};
use procgena_mushroom::{
    ClusterLayout, ClusterOptions, Mushroom, MushroomCluster, MushroomLod, MushroomPreset,
    MushroomScatter, ScatterError, ScatterOptions, tags,
};

#[test]
fn object_has_one_level_per_lod_level_with_tags() {
    let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
    let lod = MushroomLod::for_size(mushroom.options().size);
    let object = mushroom.object(&lod).unwrap();
    object.validate().unwrap();
    assert_eq!(object.levels.len(), lod.levels.len());
    for (level, lod_level) in object.levels.iter().zip(&lod.levels) {
        assert_eq!(level.start, lod_level.distance);
        for part in &level.parts {
            assert!(part.tags.contains(&tags::MUSHROOM));
        }
    }

    // The full level has every kind; caps can be picked by tag.
    let full = &object.levels[0];
    for tag in [tags::CAP, tags::STEM, tags::RING, tags::WART] {
        assert!(full.parts.iter().any(|p| p.tags.contains(&tag)), "{tag}");
    }
    let caps = object.merged(0, &TagFilter::any_of([tags::CAP])).unwrap();
    assert!(caps.closed);
}

#[test]
fn warts_part_has_one_shell_per_wart() {
    let mushroom = Mushroom::from_preset(MushroomPreset::FlyAgaric);
    let mesh = mushroom.mesh();
    let warts = mesh.part(procgena_mushroom::PartKind::Warts).unwrap();
    assert!(warts.shells.len() > 10, "{} warts", warts.shells.len());
    assert!(warts.closed);
}

/// Smallest distance between two members' stem bases.
fn min_base_gap(members: &[procgena_mushroom::MushroomInstance]) -> f32 {
    let mut best = f32::MAX;
    for (i, a) in members.iter().enumerate() {
        for b in &members[i + 1..] {
            best = best.min(a.translation.distance(b.translation));
        }
    }
    best
}

#[test]
fn clusters_spread_their_members() {
    for layout in [ClusterLayout::Tuft, ClusterLayout::Line] {
        let options = ClusterOptions {
            count_min: 5,
            count_max: 5,
            layout,
            ..ClusterOptions::default()
        };
        let base = MushroomPreset::Button.options();
        let cluster = MushroomCluster::new(&base, &options).unwrap();
        let members = cluster.members();
        assert!(members.len() >= 3, "{layout:?}: {}", members.len());
        assert!(
            members
                .iter()
                .all(|m| (m.variant as usize) < cluster.variants().len())
        );
        assert!(min_base_gap(members) > 0.0, "{layout:?}");

        // The combined object keeps one closed cap piece per member.
        let object = cluster.object(&MushroomLod::default()).unwrap();
        object.validate().unwrap();
        let caps = object.merged(0, &TagFilter::any_of([tags::CAP])).unwrap();
        assert_eq!(caps.shells.len(), members.len());
    }
}

#[test]
fn swept_clusters_fan_outward() {
    let options = ClusterOptions {
        count_min: 8,
        count_max: 8,
        lean: 0.0,
        sweep: 0.4,
        ..ClusterOptions::default()
    };
    let base = MushroomPreset::Enoki.options();
    let cluster = MushroomCluster::new(&base, &options).unwrap();
    assert!(cluster.members().len() >= 4);

    // Every cap sits further from the cluster's centre line than its stem
    // base, and well outside the base when the base is near the centre.
    let centre_distance = |p: Vec3| p.x.hypot(p.z);
    for m in cluster.members() {
        let variant = &cluster.variants()[m.variant as usize];
        let cap = m.transform().transform_point3(variant.cap_centre());
        let base = m.translation;
        let reach = centre_distance(cap) - centre_distance(base);
        let stem = variant.cap_centre().y * m.scale;
        assert!(reach > stem * 0.2, "reach {reach} for a {stem} m stem");
    }

    // Without sweep, nothing is aimed outward: some caps lean back in.
    let unswept = MushroomCluster::new(
        &base,
        &ClusterOptions {
            sweep: 0.0,
            ..options
        },
    )
    .unwrap();
    assert!(unswept.members().iter().any(|m| {
        let cap = m
            .transform()
            .transform_point3(unswept.variants()[m.variant as usize].cap_centre());
        centre_distance(cap) <= centre_distance(m.translation) + 1e-4
    }));
}

#[test]
fn cluster_caps_do_not_overlap() {
    for (preset, layout) in [
        (MushroomPreset::Button, ClusterLayout::Tuft),
        (MushroomPreset::Porcini, ClusterLayout::Line),
        (MushroomPreset::Enoki, ClusterLayout::Tuft),
    ] {
        let options = ClusterOptions {
            count_min: 8,
            count_max: 8,
            layout,
            ..ClusterOptions::default()
        };
        let cluster = MushroomCluster::new(&preset.options(), &options).unwrap();

        // Treat each cap as a ball around its middle with the cap's radius.
        let caps: Vec<(Vec3, f32)> = cluster
            .members()
            .iter()
            .map(|m| {
                let v = &cluster.variants()[m.variant as usize];
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
    let base = MushroomPreset::Enoki.options();
    let a = MushroomCluster::new(&base, &ClusterOptions::default()).unwrap();
    let b = MushroomCluster::new(&base, &ClusterOptions::default()).unwrap();
    assert_eq!(a.members(), b.members());
}

/// A 2 m ground patch.
fn ground() -> Object {
    Object::single(vec![Part::new(
        [mesh_tags::GROUND],
        SurfaceMesh::ground(2.0, 8),
    )])
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
    let object = scatter.object(&MushroomLod::default()).unwrap();
    object.validate().unwrap();
}

#[test]
fn scatter_respects_the_tag_filter_and_slope() {
    let mushroom = MushroomPreset::Button.options();
    let rock_only = ScatterOptions {
        grows_on: TagFilter::any_of([mesh_tags::ROCK]),
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
