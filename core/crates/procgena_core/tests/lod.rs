//! Level selection, fade bands, size and quality scaling, and validation of
//! the shared level-of-detail type. Expected bands are taken from the asset
//! crates' own LOD code, which this type replaces.

use procgena_core::{FarForm, LevelForm, Lod, LodError, LodLevel, LodRange, Quality};

/// Fade bands as `(fade_in, fade_out)` pairs, which compare and print well.
fn bands(ranges: &[LodRange]) -> Vec<(std::ops::Range<f32>, std::ops::Range<f32>)> {
    ranges
        .iter()
        .map(|r| (r.fade_in.clone(), r.fade_out.clone()))
        .collect()
}

const INF: std::ops::Range<f32> = f32::INFINITY..f32::INFINITY;

/// Tree's grove test: a middle level faded in over 8–12 m and out over
/// 18–22 m.
fn tree_like() -> Lod {
    Lod::new(
        1.0,
        vec![
            LodLevel::new(0.0, 0.0, ()),
            LodLevel::new(10.0, 0.2, ()),
            LodLevel::new(20.0, 0.1, ()),
        ],
    )
}

/// Mushroom's default levels for an 8 cm cap, ending at 40 m.
fn mushroom_like() -> Lod {
    let level = |distance| LodLevel::new(distance, 0.1, ());
    Lod::new(
        0.08,
        vec![
            level(0.0),
            level(1.5),
            level(4.0),
            level(10.0),
            level(18.0)
                .with_form(LevelForm::Far(FarForm::Card))
                .without_shadows(),
        ],
    )
    .with_cull(40.0)
}

#[test]
fn neighbouring_levels_share_their_fade_bands() {
    let ranges = tree_like().ranges(1.0);
    assert_eq!(
        bands(&ranges),
        vec![
            (0.0..0.0, 8.0..12.0),
            (8.0..12.0, 18.0..22.0),
            (18.0..22.0, INF),
        ]
    );
}

#[test]
fn the_last_level_fades_out_over_the_cull_band() {
    // Deadwood and mushroom fade the last level out over the last level's
    // hysteresis around the cull distance.
    let ranges = mushroom_like().ranges(1.0);
    let last = ranges.last().unwrap();
    assert_eq!(last.fade_out, 36.0..44.0);
    assert!(last.fade_out.contains(&40.0));
}

#[test]
fn an_impostor_level_takes_over_from_the_farthest_mesh() {
    // Tree's impostor: from 120 m with hysteresis 0.1, after its four
    // fixed mesh levels.
    let mut levels: Vec<LodLevel> = [0.0, 10.0, 25.0, 50.0]
        .into_iter()
        .map(|d| LodLevel::new(d, 0.05, ()))
        .collect();
    levels[3].form = LevelForm::Far(FarForm::BillboardLeaves);
    levels.push(LodLevel::new(120.0, 0.1, ()).with_form(LevelForm::Impostor));
    let lod = Lod::new(1.0, levels);
    lod.validate().unwrap();

    let ranges = lod.ranges(1.0);
    assert_eq!(ranges[3].fade_out, 108.0..132.0);
    assert_eq!(ranges[4].fade_in, 108.0..132.0);
    assert_eq!(ranges[4].fade_out, INF);
}

#[test]
fn ranges_scale_with_copy_size() {
    // Stone's levels are written for its 2.5 m default rock; a 5 m rock
    // switches twice as far away.
    let lod = Lod::new(
        2.5,
        vec![
            LodLevel::new(0.0, 0.05, 96),
            LodLevel::new(15.0, 0.05, 48),
            LodLevel::new(40.0, 0.05, 24),
        ],
    );
    let factor = lod.factor(5.0, Quality::High);
    assert_eq!(factor, 2.0);
    let ranges = lod.ranges(factor);
    assert_eq!(ranges[1].fade_in, 28.5..31.5);
    assert_eq!(ranges[2].fade_in, 76.0..84.0);
}

#[test]
fn quality_scales_every_distance_together() {
    let lod = mushroom_like();
    let high = lod.ranges(lod.factor(0.08, Quality::High));
    for quality in Quality::ALL {
        let scale = quality.distance_scale();
        let ranges = lod.ranges(lod.factor(0.08, quality));
        for (scaled, reference) in ranges.iter().zip(&high) {
            for (a, b) in [
                (scaled.fade_in.start, reference.fade_in.start),
                (scaled.fade_in.end, reference.fade_in.end),
                (scaled.fade_out.start, reference.fade_out.start),
                (scaled.fade_out.end, reference.fade_out.end),
            ] {
                assert!(
                    (a - b * scale).abs() <= 1e-5 * b.max(1.0),
                    "{quality:?}: {a} vs {b}"
                );
            }
        }
    }
}

#[test]
fn quality_tiers_increase_and_high_is_the_reference() {
    let scales = Quality::ALL.map(Quality::distance_scale);
    assert!(scales.windows(2).all(|w| w[0] < w[1]), "{scales:?}");
    assert_eq!(Quality::default(), Quality::High);
    assert_eq!(Quality::High.distance_scale(), 1.0);
}

#[test]
fn level_at_switches_at_the_middle_of_each_band() {
    let lod = mushroom_like();
    assert_eq!(lod.level_at(0.0, 1.0), Some(0));
    assert_eq!(lod.level_at(1.49, 1.0), Some(0));
    assert_eq!(lod.level_at(1.5, 1.0), Some(1));
    assert_eq!(lod.level_at(17.9, 1.0), Some(3));
    assert_eq!(lod.level_at(18.0, 1.0), Some(4));
    assert_eq!(lod.level_at(39.9, 1.0), Some(4));
    assert_eq!(lod.level_at(40.0, 1.0), None);
    // Twice the size, twice the distances.
    assert_eq!(lod.level_at(3.0, 2.0), Some(1));
    assert_eq!(lod.level_at(79.0, 2.0), Some(4));
    // With no cull distance, the last level is drawn forever.
    assert_eq!(tree_like().level_at(1e9, 1.0), Some(2));
}

#[test]
fn shadow_levels_skip_levels_without_shadows() {
    let lod = mushroom_like();
    assert_eq!(lod.shadow_level_at(12.0, 1.0), Some(3));
    assert_eq!(lod.shadow_level_at(20.0, 1.0), None);
}

#[test]
fn dither_levels_match_the_grove_and_pool_renderers() {
    // The middle level of tree's grove test, faded in over 8–12 m and out
    // over 18–22 m.
    let range = &tree_like().ranges(1.0)[1];
    let cases = [
        (7.9, -16),
        (8.0, -16),
        (10.0, -8),
        (11.99, 0),
        (12.0, 0),
        (17.99, 0),
        (18.0, 0),
        (20.0, 8),
        (21.99, 16),
        (22.0, 16),
        (30.0, 16),
    ];
    for (distance, level) in cases {
        assert_eq!(range.dither_level(distance), level, "at {distance} m");
    }
    assert!(!range.is_drawn(8.0));
    assert!(range.is_drawn(8.2));
    assert!(range.is_drawn(21.0));
    assert!(!range.is_drawn(22.0));

    // The first level is fully drawn from the camera on.
    assert_eq!(LodRange::ALWAYS.dither_level(0.0), 0);
    assert_eq!(LodRange::ALWAYS.dither_level(1e9), 0);
    assert!(LodRange::ALWAYS.is_always());
    assert!(!range.is_always());
}

#[test]
fn dither_levels_cross_fade_neighbouring_levels() {
    // Over a shared band, a level fading out at +n covers exactly the pixels
    // its neighbour fading in at n - 16 leaves.
    let ranges = tree_like().ranges(1.0);
    for step in 0..=40 {
        let distance = 8.0 + step as f32 * 0.1;
        let out = ranges[0].dither_level(distance);
        let into = ranges[1].dither_level(distance);
        assert_eq!(into, out - 16, "at {distance} m");
    }
}

#[test]
fn zero_hysteresis_swaps_in_one_step() {
    let lod = Lod::new(
        1.0,
        vec![LodLevel::new(0.0, 0.0, ()), LodLevel::new(10.0, 0.0, ())],
    );
    let ranges = lod.ranges(1.0);
    assert_eq!(ranges[0].dither_level(9.99), 0);
    assert_eq!(ranges[0].dither_level(10.0), 16);
    assert_eq!(ranges[1].dither_level(9.99), -16);
    assert_eq!(ranges[1].dither_level(10.0), 0);
}

#[test]
fn packed_ranges_replace_infinity() {
    let packed = LodRange::ALWAYS.to_vec4();
    assert_eq!(packed.to_array(), [0.0, 0.0, f32::MAX, f32::MAX]);
}

#[test]
fn ranges_can_end_where_something_else_takes_over() {
    // As fruit ends where its tree's impostor fades in: levels from the
    // 9–11 m band on are left out, and level 2 fades out over it.
    let lod = Lod {
        cull_distance: None,
        ..mushroom_like()
    };
    let ranges = lod.ranges_ending_at(1.0, 9.0..11.0);
    assert_eq!(ranges.len(), 3, "{:?}", bands(&ranges));
    assert_eq!(ranges[2].fade_in, 3.6..4.4);
    assert_eq!(ranges[2].fade_out, 9.0..11.0);

    // A cull nearer than the end is kept.
    let ranges = mushroom_like().ranges_ending_at(1.0, 100.0..120.0);
    assert_eq!(ranges.len(), 5);
    assert_eq!(ranges[4].fade_out, 36.0..44.0);
}

#[test]
fn levels_touched_covers_distance_and_size_spreads() {
    let lod = mushroom_like();
    // Wholly inside level 2 (fully drawn from 4.4 to 9 m).
    assert_eq!(lod.levels_touched(5.0..=8.0, 1.0..=1.0), Some(2..=2));
    // Across the 10 m switch.
    assert_eq!(lod.levels_touched(5.0..=9.5, 1.0..=1.0), Some(2..=3));
    // The same distances, but the copies range from half to twice the
    // reference size: 2.5 m to 19 m in reference distances.
    assert_eq!(lod.levels_touched(5.0..=9.5, 0.5..=2.0), Some(1..=4));
    // Beyond the cull band.
    assert_eq!(lod.levels_touched(44.0..=50.0, 1.0..=1.0), None);
    // An empty interval.
    assert_eq!(lod.levels_touched(5.0..=4.0, 1.0..=1.0), None);
}

#[test]
fn map_detail_keeps_the_levels() {
    let lod = Lod::new(1.0, vec![LodLevel::new(0.0, 0.0, 3u32)]).with_cull(5.0);
    let mapped = lod.map_detail(|d| d * 2);
    assert_eq!(mapped.levels[0].detail, 6);
    assert_eq!(mapped.cull_distance, Some(5.0));
}

#[test]
fn validate_accepts_the_ported_defaults() {
    tree_like().validate().unwrap();
    mushroom_like().validate().unwrap();
    for quality in Quality::ALL {
        let lod = mushroom_like();
        let factor = lod.factor(0.3, quality);
        assert_eq!(lod.ranges(factor).len(), 5);
    }
}

#[test]
fn validate_rejects_bad_levels() {
    let level = |d| LodLevel::new(d, 0.1, ());
    let check = |lod: Lod| lod.validate().unwrap_err();

    assert_eq!(
        check(Lod::new(0.0, vec![level(0.0)])),
        LodError::ReferenceSize(0.0)
    );
    assert_eq!(check(Lod::new(1.0, vec![])), LodError::NoLevels);
    assert_eq!(
        check(Lod::new(1.0, vec![level(1.0)])),
        LodError::FirstDistance(1.0)
    );
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), level(5.0), level(5.0)])),
        LodError::Distance {
            level: 2,
            distance: 5.0
        }
    );
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), LodLevel::new(5.0, 1.0, ())])),
        LodError::Hysteresis {
            level: 1,
            hysteresis: 1.0
        }
    );
    // A far form before a mesh, and an impostor that isn't last.
    let far = level(5.0).with_form(LevelForm::Far(FarForm::Strips));
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), far, level(10.0)])),
        LodError::FormOrder { level: 2 }
    );
    let impostor = level(5.0).with_form(LevelForm::Impostor);
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), impostor, level(10.0)])),
        LodError::FormOrder { level: 1 }
    );
    // 10 m ± 50% overlaps 12 m ± 50%.
    let wide = |d| LodLevel::new(d, 0.5, ());
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), wide(10.0), wide(12.0)])),
        LodError::BandsOverlap { level: 2 }
    );
    assert_eq!(
        check(Lod::new(1.0, vec![level(0.0), level(10.0)]).with_cull(10.5)),
        LodError::CullDistance(10.5)
    );
}
