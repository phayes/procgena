//! The drawing chain: steps by size on screen, fade bands, quality and view
//! scaling, the impostor's largest size, and validation.

use std::ops::Range;

use procgena_core::chain::{
    ChainError, DistanceChain, Lod, LowerDetail, Step, TakesOver, ViewScale,
};
use procgena_core::{Quality, content_hash};

/// Fade bands as `(fade_in, fade_out)` pairs, which compare and print well.
fn bands(ranges: &[procgena_core::LodRange]) -> Vec<(Range<f32>, Range<f32>)> {
    ranges
        .iter()
        .map(|r| (r.fade_in.clone(), r.fade_out.clone()))
        .collect()
}

/// Whether `a` and `b` match to within a millionth of `b`.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= b.abs() * 1e-6
}

const HIGH: Quality = Quality::High;

/// A chain with round numbers: steps at 10 m, 25 m and 500 m for a factor
/// of 1, fading over ±10%.
const ROUND: DistanceChain = DistanceChain {
    lower_below: 0.1,
    impostor_below: 0.04,
    cull_below: 0.002,
    hysteresis: 0.1,
};

#[test]
fn the_default_chain_is_valid() {
    DistanceChain::DEFAULT.validate().unwrap();
    assert_eq!(DistanceChain::default(), DistanceChain::DEFAULT);
}

#[test]
fn each_step_fades_in_where_the_one_before_fades_out() {
    let ranges = ROUND.ranges(1.0, HIGH);
    let bands = bands(&ranges);
    let expect = [
        (0.0..0.0, 9.0..11.0),
        (9.0..11.0, 22.5..27.5),
        (22.5..27.5, 450.0..550.0),
    ];
    for (got, want) in bands.iter().zip(expect) {
        assert!(
            close(got.0.start, want.0.start)
                && close(got.0.end, want.0.end)
                && close(got.1.start, want.1.start)
                && close(got.1.end, want.1.end),
            "{bands:?}"
        );
    }
}

/// A copy twice as large, or a camera that makes it look twice as large,
/// doubles every distance; a quality that reaches 0.4 times as far shrinks
/// every distance to 0.4.
#[test]
fn size_view_and_quality_multiply_the_distances() {
    let one = DistanceChain::factor(1.0, ViewScale::DEFAULT);
    let big = DistanceChain::factor(2.0, ViewScale::DEFAULT);
    assert!(close(big, 2.0 * one));
    let ranges = ROUND.ranges(big, HIGH);
    assert!(close(ranges[1].fade_in.start, 2.0 * 9.0 * one));
    let low = ROUND.ranges(one, Quality::Low);
    for step in Step::ALL {
        let high = &ROUND.ranges(one, HIGH)[step.index()];
        let low = &low[step.index()];
        assert!(
            close(low.fade_out.start, 0.4 * high.fade_out.start),
            "{step:?}"
        );
    }

    // A narrower field of view makes everything look larger.
    let wide = ViewScale::perspective(1.0).get();
    let narrow = ViewScale::perspective(2.0 * (0.25f32).atan()).get();
    let ratio = (0.5f32).tan() / 0.25;
    assert!(close(narrow, wide * ratio), "{narrow} {wide}");
}

/// The thresholds are sizes on screen: at the switching distance, a copy
/// covers exactly the threshold's share of the screen at High quality, and
/// `threshold / distance_scale` at other qualities.
#[test]
fn a_step_takes_over_at_its_size_on_screen() {
    let chain = DistanceChain::DEFAULT;
    let view = ViewScale::DEFAULT;
    let size = 3.0;
    let factor = DistanceChain::factor(size, view);
    for quality in Quality::ALL {
        let [_, impostor, _] = chain.thresholds(quality);
        let switch = factor / impostor;
        let on_screen = size * view.get() / switch;
        assert!(
            close(on_screen, chain.impostor_below / quality.distance_scale()),
            "{quality:?}"
        );
        let step = |distance| chain.step_at(distance, factor, quality);
        assert_eq!(step(switch * 0.999), Some(Step::Lower));
        assert_eq!(step(switch * 1.001), Some(Step::Impostor));
    }
}

#[test]
fn step_at_ignores_fade_bands_and_ends_at_the_cull() {
    let step = |distance| ROUND.step_at(distance, 1.0, HIGH);
    assert_eq!(step(0.0), Some(Step::Game));
    assert_eq!(step(9.99), Some(Step::Game));
    assert_eq!(step(10.0), Some(Step::Lower));
    assert_eq!(step(25.0), Some(Step::Impostor));
    assert_eq!(step(499.0), Some(Step::Impostor));
    assert_eq!(step(500.0), None);
}

#[test]
fn ranges_ending_at_drops_the_steps_past_the_end() {
    // Ending inside the impostor's band: the lower mesh fades out there,
    // and the impostor is left out.
    let ranges = ROUND.ranges_ending_at(1.0, HIGH, 18.0..20.0);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[1].fade_out, 18.0..20.0);

    // Ending beyond the cull changes nothing.
    let all = ROUND.ranges_ending_at(1.0, HIGH, 1000.0..1100.0);
    assert_eq!(bands(&all), bands(&ROUND.ranges(1.0, HIGH)));
}

#[test]
fn steps_touched_covers_the_bands_reached() {
    let touched = |distances, factors| ROUND.steps_touched(distances, factors, HIGH);
    assert_eq!(touched(0.0..=5.0, 1.0..=1.0), Some(Step::Game..=Step::Game));
    assert_eq!(
        touched(10.0..=10.0, 1.0..=1.0),
        Some(Step::Game..=Step::Lower),
        "inside the first fade band"
    );
    assert_eq!(
        touched(30.0..=40.0, 0.5..=2.0),
        Some(Step::Lower..=Step::Impostor),
        "larger copies are still on their lower mesh"
    );
    assert_eq!(touched(600.0..=700.0, 1.0..=1.0), None);
    assert_eq!(touched(5.0..=1.0, 1.0..=1.0), None);
}

#[test]
fn the_impostor_is_largest_where_it_starts_fading_in() {
    let chain = DistanceChain::DEFAULT;
    let factor = DistanceChain::factor(1.0, ViewScale::DEFAULT);
    for quality in Quality::ALL {
        let fade_start = chain.ranges(factor, quality)[Step::Impostor.index()]
            .fade_in
            .start;
        let on_screen = ViewScale::DEFAULT.get() / fade_start;
        assert!(close(chain.impostor_max_size(quality), on_screen));
    }
    // Lower quality shows the impostor larger, so Low sets the largest.
    let low = chain.impostor_max_size(Quality::Low);
    assert!(low > chain.impostor_max_size(Quality::Ultra));
    assert_eq!(chain.largest_impostor_size(), low);
}

#[test]
fn validate_names_the_first_problem() {
    let bad = DistanceChain {
        impostor_below: 0.0,
        ..ROUND
    };
    assert_eq!(
        bad.validate(),
        Err(ChainError::Threshold {
            takes_over: TakesOver::Impostor,
            threshold: 0.0
        })
    );
    let bad = DistanceChain {
        cull_below: 0.05,
        ..ROUND
    };
    assert_eq!(
        bad.validate(),
        Err(ChainError::Order {
            takes_over: TakesOver::Cull
        })
    );
    let bad = DistanceChain {
        hysteresis: 1.0,
        ..ROUND
    };
    assert_eq!(bad.validate(), Err(ChainError::Hysteresis(1.0)));
    let bad = DistanceChain {
        impostor_below: 0.09,
        ..ROUND
    };
    assert_eq!(
        bad.validate(),
        Err(ChainError::BandsOverlap {
            takes_over: TakesOver::Impostor
        })
    );
    assert!(!bad.validate().unwrap_err().to_string().is_empty());
}

/// A detail whose lower mesh has half the sides.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sides(u32);

impl LowerDetail for Sides {
    fn lower(&self) -> Self {
        Self((self.0 / 2).max(3))
    }
}

#[test]
fn a_lod_derives_its_lower_mesh_from_the_game_level() {
    let lod = Lod::from_game(Sides(16));
    assert_eq!(lod.lower, Sides(8));
    assert_eq!(lod.detail(Step::Game), Some(&Sides(16)));
    assert_eq!(lod.detail(Step::Lower), Some(&Sides(8)));
    assert_eq!(lod.detail(Step::Impostor), None);
    let steps: Vec<_> = lod.meshes().iter().map(|(step, _)| *step).collect();
    assert_eq!(steps, [Step::Game, Step::Lower]);
    assert_eq!(lod.map(|s| s.0), Lod { game: 16, lower: 8 });
}

#[test]
fn steps_are_in_order_and_only_meshes_cast_shadows() {
    for (i, step) in Step::ALL.into_iter().enumerate() {
        assert_eq!(step.index(), i);
    }
    assert!(Step::Game < Step::Lower && Step::Lower < Step::Impostor);
    assert!(Step::Lower.casts_shadows());
    assert!(!Step::Impostor.casts_shadows());
}

#[test]
fn content_hashes_follow_the_values() {
    let a = content_hash(&DistanceChain::DEFAULT);
    let b = content_hash(&DistanceChain {
        hysteresis: 0.05,
        ..DistanceChain::DEFAULT
    });
    assert_ne!(a, b);
    assert_eq!(a, content_hash(&DistanceChain::DEFAULT));
    assert_ne!(
        content_hash(&Lod {
            game: 2u32,
            lower: 1u32
        }),
        content_hash(&Lod {
            game: 1u32,
            lower: 2u32
        })
    );
}
