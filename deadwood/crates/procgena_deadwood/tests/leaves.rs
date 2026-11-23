//! Leaf card checks: placement, thinning by detail, and validation.

use glam::Vec3;
use procgena_deadwood::{
    DeadwoodDetail, DeadwoodOptions, DeadwoodPreset, LeafOptions, MeshBuffers, OptionsError, Piece,
};

/// The seedling log preset with leaves changed by `edit`.
fn seedling_log(edit: impl FnOnce(&mut LeafOptions)) -> DeadwoodOptions {
    let mut o = DeadwoodPreset::SeedlingLog.options();
    edit(&mut o.leaves);
    o
}

/// Returns the leaf buffers of `piece` at `detail`, which must have some.
fn leaves(piece: &Piece, detail: &DeadwoodDetail) -> MeshBuffers {
    piece
        .mesh_with_detail(detail)
        .leaves
        .expect("leaf cards at this detail")
}

/// Returns the middle of each card's bottom edge, where it meets the wood.
/// Each card has four vertices, the second and third at its base.
fn card_bases(b: &MeshBuffers) -> Vec<Vec3> {
    b.positions
        .chunks(4)
        .map(|c| (Vec3::from(c[1]) + Vec3::from(c[2])) / 2.0)
        .collect()
}

#[test]
fn leaves_are_off_by_default() {
    let piece = Piece::new(DeadwoodOptions::default()).unwrap();
    assert!(piece.mesh().leaves.is_none());
    for preset in DeadwoodPreset::ALL {
        let has = preset.options().leaves.per_metre > 0.0;
        let expected = matches!(
            preset,
            DeadwoodPreset::SproutingStump | DeadwoodPreset::SeedlingLog
        );
        assert_eq!(has, expected, "{}", preset.name());
    }
}

#[test]
fn leaf_presets_grow_well_formed_cards() {
    for preset in [DeadwoodPreset::SproutingStump, DeadwoodPreset::SeedlingLog] {
        let piece = Piece::from_preset(preset);
        let b = leaves(&piece, &DeadwoodDetail::FULL);
        let n = b.vertex_count();
        assert!(n >= 4, "{}", preset.name());
        assert_eq!(n % 4, 0);
        for len in [
            b.normals.len(),
            b.uvs.len(),
            b.tangents.len(),
            b.colors.len(),
        ] {
            assert_eq!(len, n);
        }
        assert_eq!(b.wood.len(), n);
        assert!(b.indices.iter().all(|&i| (i as usize) < n));
        for (p, nrm) in b.positions.iter().zip(&b.normals) {
            assert!(p.iter().all(|c| c.is_finite()));
            assert!((Vec3::from(*nrm).length() - 1.0).abs() < 1e-3);
        }
    }
}

#[test]
fn cards_grow_above_the_ground() {
    for preset in [DeadwoodPreset::SproutingStump, DeadwoodPreset::SeedlingLog] {
        let piece = Piece::from_preset(preset);
        for base in card_bases(&leaves(&piece, &DeadwoodDetail::FULL)) {
            assert!(base.y > 0.0, "{}: card base at {base}", preset.name());
        }
    }
}

#[test]
fn leaves_never_change_the_wood() {
    let bare = Piece::new(seedling_log(|l| l.per_metre = 0.0)).unwrap();
    let leafy = Piece::new(seedling_log(|l| l.per_metre = 8.0)).unwrap();
    let (a, b) = (bare.mesh(), leafy.mesh());
    assert_eq!(a.surfaces, b.surfaces);
    assert_eq!(a.ribbons, b.ribbons);
    assert_eq!(bare.placement(), leafy.placement());
    assert!(a.leaves.is_none());
    assert!(b.leaves.is_some());
}

#[test]
fn farther_details_keep_fewer_of_the_same_cards() {
    let piece = Piece::new(seedling_log(|l| l.per_metre = 10.0)).unwrap();
    let full = leaves(&piece, &DeadwoodDetail::FULL);
    let high = leaves(&piece, &DeadwoodDetail::HIGH);
    assert!(high.vertex_count() < full.vertex_count());
    assert!(high.positions.iter().all(|p| full.positions.contains(p)));
    let low = piece.mesh_with_detail(&DeadwoodDetail::LOW).leaves;
    if let Some(low) = low {
        assert!(low.vertex_count() <= high.vertex_count());
        assert!(low.positions.iter().all(|p| high.positions.contains(p)));
    }
    assert!(
        piece
            .mesh_with_detail(&DeadwoodDetail::STRIP)
            .leaves
            .is_none()
    );
}

#[test]
fn top_preference_puts_sprays_on_the_upper_side() {
    let o = seedling_log(|l| {
        l.per_metre = 10.0;
        l.top = 1.0;
    });
    let middle = 0.5 * o.tip_radius();
    let piece = Piece::new(o).unwrap();
    let bases = card_bases(&leaves(&piece, &DeadwoodDetail::FULL));
    let high = bases.iter().filter(|b| b.y > middle).count();
    assert!(
        high * 10 >= bases.len() * 9,
        "{high} of {} cards on top",
        bases.len()
    );
}

#[test]
fn flattened_cards_face_up() {
    let piece = Piece::new(seedling_log(|l| l.flatten = 1.0)).unwrap();
    let b = leaves(&piece, &DeadwoodDetail::FULL);
    for n in &b.normals {
        assert!(n[1] > 0.99, "normal {n:?}");
    }
}

#[test]
fn crossed_cards_double_the_triangles() {
    let single = Piece::new(seedling_log(|_| {})).unwrap();
    let crossed = Piece::new(seedling_log(|l| l.crossed = true)).unwrap();
    assert_eq!(
        leaves(&crossed, &DeadwoodDetail::FULL).triangle_count(),
        2 * leaves(&single, &DeadwoodDetail::FULL).triangle_count()
    );
}

#[test]
fn leaf_settings_are_checked() {
    let bad_texture = seedling_log(|l| l.texture = "oak_tree".into());
    assert!(matches!(
        bad_texture.validate(),
        Err(OptionsError::LeafTexture { .. })
    ));
    let no_cards = seedling_log(|l| l.cards = 0);
    assert!(matches!(
        no_cards.validate(),
        Err(OptionsError::IntOutOfRange {
            field: "leaves.cards",
            ..
        })
    ));
    let bad_tint = seedling_log(|l| l.tint = "green".into());
    assert!(matches!(
        bad_tint.validate(),
        Err(OptionsError::Colour {
            field: "leaves.tint",
            ..
        })
    ));
}
