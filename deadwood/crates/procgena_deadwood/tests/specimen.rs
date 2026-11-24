//! Conversion to `procgena_specimen::Specimen`, with tags other generators can
//! grow on.

use procgena_deadwood::{DeadwoodPreset, Piece, default_lod};
use procgena_specimen::{TagFilter, tags};

/// The surfaces procgena_clump's moss presets grow on.
fn moss_filter() -> TagFilter {
    TagFilter::any_of([tags::ROCK, tags::BARK, tags::WOOD, tags::GROUND]).excluding([tags::CARD])
}

#[test]
fn specimens_are_valid_closed_and_tagged() {
    for preset in DeadwoodPreset::all() {
        let piece = Piece::from_preset(preset).unwrap();
        let lod = default_lod();
        let specimen = piece.specimen(&lod).unwrap();
        specimen.validate().unwrap();
        assert_eq!(specimen.levels.len(), lod.levels.len());
        for part in &specimen.levels[0].parts {
            assert!(part.closed, "{}", preset.name);
            assert!(!part.shells.is_empty());
            assert!(part.tags.contains(&tags::DEADWOOD));
            assert!(part.tags.contains(&tags::WOOD));
            assert!(
                [tags::TRUNK, tags::LIMB, tags::BRANCH]
                    .iter()
                    .any(|t| part.tags.contains(t))
            );
            assert!(part.surface().unwrap().is_closed(), "{}", preset.name);
        }
    }
}

#[test]
fn bark_tag_follows_the_bark() {
    let fresh = Piece::from_preset(DeadwoodPreset::fresh_cut_log()).unwrap();
    let rotten = Piece::from_preset(DeadwoodPreset::rotten_log()).unwrap();
    let lod = default_lod();
    let has_bark = |p: &Piece| {
        p.specimen(&lod).unwrap().levels[0]
            .parts
            .iter()
            .any(|part| part.tags.contains(&tags::BARK))
    };
    assert!(has_bark(&fresh));
    assert!(!has_bark(&rotten));
}

#[test]
fn moss_can_grow_on_bare_rotten_logs() {
    for preset in [DeadwoodPreset::rotten_log(), DeadwoodPreset::mossy_log()] {
        let piece = Piece::from_preset(preset).unwrap();
        let specimen = piece.specimen(&default_lod()).unwrap();
        let merged = specimen
            .merged(0, &moss_filter())
            .expect("moss finds a surface");
        assert!(merged.closed);
        assert!(merged.triangle_count() > 0);
    }
}

#[test]
fn trunk_moss_grows_on_stumps_but_not_roots() {
    let piece = Piece::from_preset(DeadwoodPreset::notched_stump()).unwrap();
    let specimen = piece.specimen(&default_lod()).unwrap();
    let trunk = TagFilter::any_of([tags::TRUNK, tags::LIMB]);
    let merged = specimen.merged(0, &trunk).unwrap();
    assert!(!merged.tags.contains(&tags::BRANCH));
}
