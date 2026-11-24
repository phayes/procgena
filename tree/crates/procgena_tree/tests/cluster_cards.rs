//! Verify cluster cards: the lone cluster a card shows, and the cards that
//! replace cluster geometry at the levels that use them.

use glam::Vec3;
use procgena_tree::{
    ClusterPart, LodDetail, Skeleton, TreeOptions, TreePreset, cluster_card_bounds,
    cluster_card_mesh, default_lod,
};

/// Level 2 of the default levels, drawing clusters as `variants` card variants.
fn carded(variants: u32) -> LodDetail {
    LodDetail {
        cluster_cards: variants,
        ..default_lod().levels[2].detail
    }
}

fn willow() -> TreeOptions {
    TreePreset::spring_pussy_willow().tree.clone()
}

fn dogwood() -> TreeOptions {
    TreePreset::flowering_dogwood().tree.clone()
}

#[test]
fn cards_replace_cluster_geometry_two_triangles_each() {
    for (options, part) in [
        (willow(), ClusterPart::Fruit),
        (dogwood(), ClusterPart::Flowers),
    ] {
        let skeleton = Skeleton::generate(&options).unwrap();
        let detail = carded(1);
        let mesh = skeleton.mesh(&options, &detail);
        let (cards, clusters) = match part {
            ClusterPart::Fruit => (&mesh.fruit_cards, &skeleton.fruit),
            ClusterPart::Flowers => (&mesh.flower_cards, &skeleton.flowers),
        };
        assert_eq!(
            mesh.fruit.triangle_count() + mesh.flowers.triangle_count(),
            0
        );
        assert_eq!(cards.len(), 1);
        let kept = clusters
            .len()
            .div_ceil(detail.cluster_stride.max(1) as usize);
        assert_eq!(cards[0].triangle_count(), 2 * kept);
        assert_eq!(cards[0].billboard.len(), cards[0].vertex_count());
        assert_eq!(cards[0].wind.len(), cards[0].vertex_count());
    }
}

#[test]
fn each_card_sits_on_its_cluster_and_faces_along_its_axis() {
    let options = dogwood();
    let skeleton = Skeleton::generate(&options).unwrap();
    let mesh = skeleton.mesh(&options, &carded(1));
    let cards = &mesh.flower_cards[0];
    for (index, cluster) in skeleton.flowers.iter().enumerate() {
        let corner = &cards.billboard[index * 4];
        let pivot = Vec3::from_array(corner.pivot);
        let radius = corner.offset[1].abs();
        // The picture covers the cluster, so its centre is within it.
        assert!(
            pivot.distance(cluster.origin) <= radius + 1e-4,
            "cluster {index}"
        );
        let axis = Vec3::from_array(corner.axis);
        let expected = cluster.orientation * Vec3::Y;
        assert!(
            axis.dot(expected) > 0.999,
            "cluster {index}: {axis} vs {expected}"
        );
    }
}

#[test]
fn variants_are_spread_over_the_clusters() {
    let options = willow();
    let skeleton = Skeleton::generate(&options).unwrap();
    let mesh = skeleton.mesh(&options, &carded(4));
    assert_eq!(mesh.fruit_cards.len(), 4);
    let counts: Vec<usize> = mesh
        .fruit_cards
        .iter()
        .map(|c| c.triangle_count() / 2)
        .collect();
    assert_eq!(counts.iter().sum::<usize>(), skeleton.fruit.len());
    assert!(
        counts.iter().all(|&c| c > skeleton.fruit.len() / 8),
        "{counts:?}"
    );
}

#[test]
fn the_lone_cluster_is_local_faces_up_and_ignores_the_seed() {
    for (options, part) in [
        (willow(), ClusterPart::Fruit),
        (dogwood(), ClusterPart::Flowers),
    ] {
        let mesh = cluster_card_mesh(&options, part, 0);
        assert!(mesh.triangle_count() > 0, "{}", part.name());
        assert!(mesh.wind.iter().all(|w| *w == [0.0; 4]));
        let (center, radius) = cluster_card_bounds(&mesh);
        // It grows from the origin along +Y.
        assert!(
            center.y > 0.0 && center.length() < radius,
            "{center} {radius}"
        );

        let mut reseeded = options.clone();
        reseeded.default_seed += 1;
        assert_eq!(cluster_card_mesh(&reseeded, part, 0), mesh);
        assert_ne!(
            cluster_card_mesh(&options, part, 1),
            mesh,
            "variants differ"
        );
    }
}
