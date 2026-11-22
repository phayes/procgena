use proptest::prelude::*;

use super::*;
use crate::gpu_layout::MAX_BRANCHED_CLASSES;

const CHUNK: f32 = 16.0;

proptest! {
    #[test]
    fn turning_by_zero_matches_the_full_rotation_exactly(
        v in prop::array::uniform3(-10.0f32..10.0),
        axis in prop::array::uniform3(-1.0f32..1.0),
    ) {
        // Skipping a zero rotation must give the same vertex position as
        // evaluating the full rotation formula.
        let (v, axis) = (Vec3::from(v), Vec3::from(axis).normalize_or(Vec3::Y));
        let full = v * 0.0f32.cos() + axis.cross(v) * 0.0f32.sin()
            + axis * axis.dot(v) * (1.0 - 0.0f32.cos());
        prop_assert_eq!(turn_about(v, axis, 0.0), full);
    }
}

#[test]
fn hash_values_are_pinned() {
    // Pinned so that any change to the hash, which would move every blade,
    // is deliberate; the WESL `hash` must produce the same numbers.
    assert_eq!(hash(0), 129_708_002);
    assert_eq!(hash(1), 2_831_084_092);
}

#[test]
fn the_tile_is_identical_on_every_call() {
    let a = best_candidate_tile(256, 12, 7);
    let b = best_candidate_tile(256, 12, 7);
    assert_eq!(a, b);
}

#[test]
fn every_prefix_of_the_tile_is_evenly_spread() {
    let tile = blue_noise_tile();
    for prefix in [64usize, 256, 1024] {
        let points = &tile[..prefix];
        // For n points evenly spread over a unit torus the nearest-neighbour
        // distance is about 0.9 / sqrt(n); white noise has many pairs far
        // closer. Require every point to keep at least 40% of that spacing.
        let floor = 0.4 * 0.9 / (prefix as f32).sqrt();
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                assert!(
                    toroidal_distance_squared(*a, *b).sqrt() >= floor,
                    "two of the first {prefix} points are closer than {floor}"
                );
            }
        }
    }
}

#[test]
fn the_same_inputs_give_the_same_blade() {
    let layout = ChunkLayout::for_density(CHUNK, 60.0);
    let all = layout.cells() * TILE_POINTS;
    for index in [0, 1, 999, all - 1] {
        assert_eq!(
            candidate([3, 9], 42, &layout, index),
            candidate([3, 9], 42, &layout, index)
        );
    }
}

#[test]
fn candidates_stay_inside_their_chunk() {
    let layout = ChunkLayout::for_density(CHUNK, 60.0);
    for index in 0..layout.cells() * TILE_POINTS {
        let c = candidate([1, 2], 5, &layout, index);
        assert!(c.local.cmpge(Vec2::ZERO).all() && c.local.cmplt(Vec2::splat(CHUNK)).all());
    }
}

#[test]
fn rank_never_decreases_with_the_candidate_index() {
    let layout = ChunkLayout::for_density(CHUNK, 60.0);
    let ranks: Vec<f32> = (0..layout.cells() * TILE_POINTS)
        .map(|i| candidate([0, 0], 0, &layout, i).rank)
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn the_needed_prefix_holds_every_kept_blade() {
    let layout = ChunkLayout::for_density(CHUNK, 80.0);
    for keep in [0.0, 0.01, 0.137, 0.5, 0.999, 1.0] {
        let needed = layout.candidates_needed(keep);
        for index in needed..layout.cells() * TILE_POINTS {
            assert!(
                candidate([4, 4], 9, &layout, index).rank >= keep,
                "keep {keep}, index {index}"
            );
        }
    }
}

#[test]
fn lowering_density_only_removes_blades_in_rank_order() {
    let layout = ChunkLayout::for_density(CHUNK, 80.0);
    let kept = |density: f32| -> Vec<u32> {
        let keep = keep_fraction(density, 1.0, &layout);
        (0..layout.cells() * TILE_POINTS)
            .filter(|&i| candidate([0, 0], 1, &layout, i).rank < keep)
            .collect()
    };
    let mut previous = kept(80.0);
    for density in [60.0, 40.0, 20.0, 5.0, 0.0] {
        let now = kept(density);
        assert!(
            now.iter().all(|i| previous.contains(i)),
            "a blade appeared at {density}"
        );
        previous = now;
    }
    assert!(previous.is_empty());
}

#[test]
fn every_style_density_is_reached() {
    for tenth in 1..2000 {
        let density = tenth as f32 / 10.0;
        let layout = ChunkLayout::for_density(CHUNK, density);
        assert!(
            layout.candidate_density >= density * (1.0 - 1e-5),
            "{density}: {layout:?}"
        );
        let drawn = keep_fraction(density, 1.0, &layout) * layout.candidate_density;
        assert!(
            (drawn - density).abs() < 1e-3 * density,
            "{density}: draws {drawn}"
        );
    }
}

#[test]
fn falloff_is_continuous_and_never_increases() {
    let lod = DistanceLod::for_radius(100.0);
    let mut previous = lod.falloff(0.0);
    assert_eq!(previous, 1.0);
    let mut d = 0.0;
    while d <= 110.0 {
        let now = lod.falloff(d);
        assert!(now <= previous + 1e-6, "falloff rose at {d}");
        assert!((previous - now).abs() < 0.02, "falloff jumps at {d}");
        previous = now;
        d += 0.1;
    }
    assert_eq!(lod.falloff(100.0), 0.0);
    // The architecture's table (50–100% across LOD 1, 15–50% across
    // LOD 2), halved from a band around 10 m on, before LOD 1 begins.
    assert!((lod.falloff(7.0) - 1.0).abs() < 1e-6);
    assert!((lod.falloff(17.0) - 0.5).abs() < 1e-6);
    assert!((lod.falloff(50.0) - 0.25).abs() < 1e-6);
    assert!((lod.falloff(90.0) - 0.075).abs() < 1e-6);
}

#[test]
fn branched_plants_keep_their_density_until_the_last_fade() {
    let lod = DistanceLod::for_radius(100.0);
    let mut previous = lod.fade(0.0);
    let mut d = 0.0;
    while d <= 110.0 {
        let now = lod.fade(d);
        // Branched density never falls below blade density, so shared candidates must
        // cover both.
        assert!(now >= lod.falloff(d), "fade below the blades' at {d}");
        assert!(now <= previous + 1e-6, "fade rose at {d}");
        assert!((previous - now).abs() < 0.02, "fade jumps at {d}");
        previous = now;
        d += 0.1;
    }
    // This radius retains all plants through 90 m, half at 95 m and none at 100 m.
    assert_eq!(lod.fade(89.9), 1.0);
    assert!((lod.fade(95.0) - 0.5).abs() < 1e-5);
    assert_eq!(lod.fade(100.0), 0.0);
}

#[test]
fn a_crowns_stems_spread_evenly_around_it() {
    for count in 2..=8 {
        for crown in [0.0, 0.3, 0.97] {
            let facings: Vec<f32> = (0..count)
                .map(|k| stem_facing(crown, k, count, hash(k * 77 + 5)))
                .collect();
            assert_eq!(facings[0], crown, "the first stem faces the crown's way");
            for (k, facing) in facings.iter().enumerate() {
                assert!((0.0..1.0).contains(facing), "{facing}");
                // Jitter must stay within one fifth of each stem's angular sector.
                let ideal = (crown + k as f32 / count as f32).fract();
                let off = (facing - ideal).abs().min(1.0 - (facing - ideal).abs());
                assert!(off <= 0.18 / count as f32, "stem {k} of {count}: {off}");
            }
        }
    }
}

#[test]
fn lod_switches_only_inside_the_dither_band() {
    let lod = DistanceLod::for_radius(100.0);
    for dither in [0.0, 0.3, 0.7, 0.999] {
        assert_eq!(lod.lod(17.9, dither), 0);
        assert_eq!(lod.lod(22.1, dither), 1);
        assert_eq!(lod.lod(47.9, dither), 1);
        assert_eq!(lod.lod(52.1, dither), 2);
    }
    // Inside the band, a blade's LOD never goes back down as it recedes.
    for dither in [0.1, 0.5, 0.9] {
        let lods: Vec<u32> = (0..100)
            .map(|i| lod.lod(18.0 + i as f32 * 0.04, dither))
            .collect();
        assert!(lods.windows(2).all(|w| w[0] <= w[1]));
    }
}

#[test]
fn coverage_widening_matches_the_architecture() {
    assert_eq!(coverage_width(1.0, 2.5), 1.0);
    assert!((coverage_width(0.5, 2.5) - 2f32.sqrt()).abs() < 1e-6);
    assert_eq!(coverage_width(0.01, 2.5), 2.5);
    // A game can allow more or less widening.
    assert_eq!(coverage_width(0.01, 1.0), 1.0);
    assert_eq!(coverage_width(0.01, 3.0), 3.0);
    assert!((coverage_width(0.25, 3.0) - 2.0).abs() < 1e-6);
}

#[test]
fn clumps_are_patches_about_a_metre_across() {
    // Neighbouring roots mostly share a clump; roots metres apart rarely do.
    let same = |gap: f32| {
        (0..2000)
            .filter(|&i| {
                let p = Vec2::new(unit(hash(i)) * 200.0, unit(hash(i ^ 9)) * 200.0);
                clump_of(p) == clump_of(p + Vec2::new(gap, 0.0))
            })
            .count() as f32
            / 2000.0
    };
    assert!(same(0.1) > 0.8, "{}", same(0.1));
    assert!(same(3.0) < 0.1, "{}", same(3.0));
    // Every tint is used.
    let mut seen = [false; 64];
    for i in 0..20_000 {
        seen[clump_of(Vec2::new(unit(hash(i)) * 300.0, unit(hash(i ^ 3)) * 300.0)) as usize] = true;
    }
    assert!(seen.iter().all(|&s| s));
    // The table repeats seamlessly every 64 m.
    for i in 0..2000 {
        let p = Vec2::new(unit(hash(i)) * 64.0, unit(hash(i ^ 5)) * 64.0);
        assert_eq!(clump_of(p), clump_of(p + Vec2::new(64.0, -128.0)));
    }
}

#[test]
fn packing_round_trips_within_quantization() {
    let blade = Blade {
        position: Vec3::new(12.345, -3.21, -101.5),
        facing: 0.3,
        height: 0.75,
        width: 0.2,
        seed: 201,
        lod: 2,
        field: 17,
        clump: 45,
        bend: Vec2::new(-0.5, 0.25),
        previous_bend: Vec2::new(-0.47, 0.21),
    };
    let extent = 128.0;
    let back = unpack(pack(&blade, extent), extent);
    let horizontal = |v: Vec3| Vec2::new(v.x, v.z);
    assert!(
        (horizontal(back.position) - horizontal(blade.position))
            .abs()
            .max_element()
            < 2.0 * extent / 65_535.0
    );
    assert!((back.position.y - blade.position.y).abs() < 2.0 * HEIGHT_RANGE / 65_535.0);
    assert!((back.facing - blade.facing).abs() <= 1.0 / 256.0);
    assert!((back.height - blade.height).abs() <= 0.5 / 255.0);
    assert!((back.width - blade.width).abs() <= 0.5 / 255.0);
    assert_eq!(
        (back.seed, back.lod, back.field, back.clump),
        (blade.seed, blade.lod, blade.field, blade.clump)
    );
    assert!((back.bend - blade.bend).abs().max_element() <= 1.0 / 4095.0 + 1e-6);
    assert!(
        (back.previous_bend - blade.previous_bend)
            .abs()
            .max_element()
            <= 1.0 / 4095.0 + 0.5 / BEND_DELTA_STEPS + 1e-6
    );
}

#[test]
fn a_large_change_in_bend_is_clamped_not_wrapped() {
    for (bend, previous) in [(0.0, 0.9), (0.0, -0.9), (1.0, -1.0), (-1.0, 1.0)] {
        let (now, back) = unpack_bend(pack_bend(bend, previous));
        // Half a 12-bit step, 1 / 4095, and a little for rounding.
        assert!((now - bend).abs() <= 1.0 / 4095.0 + 1e-6);
        // The furthest step toward last frame's bend.
        assert!((back - previous).abs() < (previous - bend).abs());
        assert!((back - now).abs() <= 8.0 / BEND_DELTA_STEPS);
    }
}

fn curve_length(points: &[Vec3; 3]) -> f32 {
    let steps = 256;
    (0..steps)
        .map(|i| {
            let a = bezier(points, i as f32 / steps as f32);
            let b = bezier(points, (i + 1) as f32 / steps as f32);
            a.distance(b)
        })
        .sum()
}

#[test]
fn a_bent_blade_keeps_its_length() {
    for height in [0.1, 0.5, 1.4] {
        for bend in [0.0, 0.3, 0.6, 0.9, 1.5] {
            for lean in [0.0, 0.2] {
                let curve = blade_curve(height, lean, Vec2::new(bend, 0.0), 0.0, 0.0);
                let length = curve_length(&curve);
                assert!(
                    (length - height).abs() <= height * 0.01,
                    "height {height}, bend {bend}, lean {lean}: length {length}"
                );
            }
        }
    }
}

#[test]
fn a_drooping_blade_arches_over_and_keeps_its_length() {
    for height in [0.1, 0.5, 1.4] {
        for droop in [0.05, 0.25, 0.5, 1.0] {
            for (lean, bend) in [(0.02, 0.0), (0.1, 0.0), (0.3, 0.0), (0.1, 0.6), (0.0, -0.9)] {
                let curve = blade_curve(height, lean, Vec2::new(bend, 0.0), droop, 0.0);
                // The length correction assumes a gentle arch; a
                // drooping blade's tighter curve comes out within 6%,
                // which blades varying ±50% in height hide.
                let length = curve_length(&curve);
                assert!(
                    (length - height).abs() <= height * 0.06,
                    "height {height}, droop {droop}: length {length}"
                );
                // From half on, the tip hangs below the blade's highest
                // point.
                let top = (0..=64)
                    .map(|i| bezier(&curve, i as f32 / 64.0).y)
                    .fold(0.0, f32::max);
                assert!(droop < 0.5 || curve[2].y < top - height * 0.05 * droop);
            }
        }
    }
    // A blade with no lean or bend has no side to droop toward: it stays
    // upright.
    assert_eq!(
        blade_curve(1.0, 0.0, Vec2::ZERO, 0.6, 1.0),
        blade_curve(1.0, 0.0, Vec2::ZERO, 0.0, 0.0)
    );
}

#[test]
fn an_arching_stem_leaves_low_and_lands_on_the_ground() {
    for height in [0.5, 1.5, 3.0] {
        for droop in [0.0, 0.5, 1.0] {
            for (lean, bend) in [(0.1, 0.0), (0.4, 0.0), (0.4, 0.3), (0.4, -0.3)] {
                let curve = blade_curve(height, lean, Vec2::new(bend, 0.0), droop, 1.0);
                let case = format!("height {height}, droop {droop}, lean {lean}, bend {bend}");
                // The tip rests on the ground, and the wind barely moves it: the
                // length correction lifts or shifts a swaying arch's tip by a
                // little.
                let still = blade_curve(height, lean, Vec2::ZERO, droop, 1.0);
                assert!(
                    still[2].y.abs() <= 1e-4 * height,
                    "{case}: tip {}",
                    still[2]
                );
                assert!(
                    curve[2].distance(still[2]) <= 0.03 * height,
                    "{case}: tip {} moved from {}",
                    curve[2],
                    still[2]
                );
                // The wind sways the top of the arch its way.
                assert!(curve[1].x * bend >= 0.0, "{case}: top {}", curve[1]);
                let length = curve_length(&curve);
                assert!(
                    (length - height).abs() <= height * 0.06,
                    "{case}: length {length}"
                );
                // A low arch: its top is at most a quarter of the span, well
                // under a half circle, and droop flattens it.
                let top = (0..=64)
                    .map(|i| bezier(&curve, i as f32 / 64.0).y)
                    .fold(0.0, f32::max);
                let span = curve[2].with_y(0.0).length();
                assert!(top <= span * 0.26, "{case}: top {top}, span {span}");
                assert!(top >= span * 0.1, "{case}: top {top}, span {span}");
                // The stem leaves the root at no more than 45°.
                let root = curve[1] - curve[0];
                assert!(
                    root.y <= root.with_y(0.0).length() * 1.01,
                    "{case}: root {root}"
                );
            }
        }
    }
    // Less droop gives a taller arch.
    let top = |droop| blade_curve(2.0, 0.4, Vec2::ZERO, droop, 1.0)[1].y;
    assert!(top(0.0) > top(1.0));
    // Part of the way, the tip is lowered between the drooping blade's and the
    // ground.
    let hanging = blade_curve(2.0, 0.4, Vec2::ZERO, 1.0, 0.0)[2].y;
    let halfway = blade_curve(2.0, 0.4, Vec2::ZERO, 1.0, 0.5)[2].y;
    assert!(halfway > 0.0 && halfway < hanging);
}

#[test]
fn tufts_are_the_nearest_jittered_centres() {
    // Tuft centres sit one per tuft spacing, jittered, and every point
    // belongs to its nearest centre, give or take the table's rounding.
    for i in 0..4000 {
        let q = Vec2::new(unit(hash(i)), unit(hash(i ^ 7))) * 200.0 - 100.0;
        let (centre, _) = tuft_at(q);
        assert!(q.distance(centre) < 1.5, "{q} -> {centre}");
        let entry = (q * TUFT_ENTRIES_PER_TUFT as f32).floor() + 0.5;
        let entry = entry / TUFT_ENTRIES_PER_TUFT as f32;
        let (entry_centre, _) = tuft_at(entry);
        assert!(entry_centre.distance(centre) < 1e-4);
    }
    // The table repeats every 64 tufts.
    let (a, ra) = tuft_at(Vec2::new(3.3, 7.9));
    let (b, rb) = tuft_at(Vec2::new(3.3 + 64.0, 7.9 - 128.0));
    assert!((b - a - Vec2::new(64.0, -128.0)).length() < 1e-3);
    assert_eq!(ra, rb);
}

#[test]
fn bare_patches_cover_their_share() {
    let noise = gust_noise(GUST_SIZE);
    for share in [0.05, 0.2, 0.5] {
        let threshold = bare_threshold(share);
        let bare = (0..20_000)
            .filter(|&i| {
                let at = Vec2::new(unit(hash(i ^ 0x51)), unit(hash(i ^ 0x52))) * 500.0;
                bare_noise(&noise, at, 3.0) < threshold
            })
            .count() as f32
            / 20_000.0;
        assert!((bare - share).abs() < 0.03, "{share}: {bare}");
    }
    assert!(bare_threshold(0.0) <= bare_threshold(0.3));
}

#[test]
fn plants_take_their_share_scattered_or_clustered() {
    // Two plants with target shares of 10% and 3%, followed by the remainder.
    for clustering in [0.0, 0.8, 1.0] {
        let chances = [(0.1, clustering), (0.03 / 0.9, clustering), (1.0, 0.0)];
        let mut counts = [0usize; 3];
        let blades = 200_000;
        for i in 0..blades {
            let clump = hash(i ^ 0x77) & 63;
            counts[pick_plant(hash(i), clump, &chances)] += 1;
        }
        let share = |n: usize| n as f32 / blades as f32;
        assert!(
            (share(counts[0]) - 0.1).abs() < 0.01,
            "{clustering}: {counts:?}"
        );
        assert!(
            (share(counts[1]) - 0.03).abs() < 0.005,
            "{clustering}: {counts:?}"
        );
    }
    // Clustered, a plant fills some clumps and leaves the rest.
    let chances = [(0.1, 1.0), (1.0, 0.0)];
    let mut per_clump = [0usize; 64];
    for i in 0..64_000 {
        let clump = i % 64;
        if pick_plant(hash(i), clump, &chances) == 0 {
            per_clump[clump as usize] += 1;
        }
    }
    let empty = per_clump.iter().filter(|&&n| n == 0).count();
    assert!(empty >= 56, "{per_clump:?}");
}

#[test]
fn the_last_plant_takes_what_is_left_whatever_its_clustering() {
    // A chance of 1 is never missed, clustered or not: `unit` is below 1.
    for clustering in [0.0, 0.5, 1.0] {
        let chances = [(0.3, 0.7), (1.0, clustering)];
        for i in 0..50_000 {
            let clump = hash(i ^ 0x91) & 63;
            assert!(pick_plant(hash(i), clump, &chances) < 2);
        }
        let last = (0..50_000)
            .filter(|&i| pick_plant(hash(i), hash(i ^ 0x91) & 63, &chances) == 1)
            .count() as f32
            / 50_000.0;
        assert!((last - 0.7).abs() < 0.01, "{clustering}: {last}");
    }
    // A plant of chance 0 is never picked, even when it is last.
    let chances = [(1.0, 0.0), (0.0, 0.0)];
    assert!((0..10_000).all(|i| pick_plant(hash(i), 0, &chances) == 0));
}

#[test]
fn an_accent_takes_its_share_of_the_plants_roots() {
    for share in [0.0, 0.06, 0.25, 1.0] {
        let accents =
            (0..100_000).filter(|&i| is_accent(hash(i), share)).count() as f32 / 100_000.0;
        assert!((accents - share).abs() < 0.005, "{share}: {accents}");
    }
    // Accent selection must be independent of plant selection, so the accent
    // share also holds among roots assigned to that plant.
    let chances = [(0.5, 0.0), (1.0, 0.0)];
    let first: Vec<u32> = (0..100_000)
        .map(hash)
        .filter(|&seed| pick_plant(seed, 0, &chances) == 0)
        .collect();
    let accents = first.iter().filter(|&&seed| is_accent(seed, 0.2)).count() as f32;
    assert!((accents / first.len() as f32 - 0.2).abs() < 0.01);
}

#[test]
fn lods_have_the_planned_vertex_and_triangle_counts() {
    let expected = [(15, 13), (7, 5), (3, 1)];
    for (rows, (vertices, triangles)) in GrassSettings::HIGH.blade_rows.iter().zip(expected) {
        let indices = lod_indices(*rows);
        assert_eq!(indices.len() / 3, triangles);
        assert_eq!(indices.iter().max().copied().unwrap() + 1, vertices);
        assert!(vertices < LOD_VERTEX_STRIDE);
    }
    // The most rows a game may ask for still fit the stride.
    let most = lod_indices(GrassSettings::MAX_BLADE_ROWS);
    assert!(most.iter().max().copied().unwrap() < LOD_VERTEX_STRIDE);
}

#[test]
fn width_range_covers_variation_times_widening() {
    assert!(WIDTH_RANGE >= 1.25 * coverage_width(0.0, GrassSettings::MAX_WIDENING));
}
#[test]
fn trample_pushes_round_trip_within_quantization() {
    for i in 0..1000 {
        let angle = unit(hash(i)) * TAU - PI;
        let push = Vec2::from_angle(angle) * unit(hash(i ^ 0x55));
        let back = trample_unpack(trample_pack(push));
        assert!(back.distance(push) < 2e-3, "{push} came back as {back}");
    }
}

#[test]
fn the_larger_packed_word_is_the_stronger_push() {
    for i in 0..1000 {
        let a = Vec2::from_angle(unit(hash(i)) * TAU) * unit(hash(i ^ 1));
        let b = Vec2::from_angle(unit(hash(i ^ 2)) * TAU) * unit(hash(i ^ 3));
        let (pa, pb) = (trample_pack(a), trample_pack(b));
        if (a.length() - b.length()).abs() > 1.0 / 1023.0 {
            assert_eq!(pa > pb, a.length() > b.length(), "{a} vs {b}");
        }
    }
}

#[test]
fn every_cell_of_a_window_has_its_own_slot() {
    for origin in [IVec2::ZERO, IVec2::new(-300, 17), IVec2::new(1000, -1)] {
        let mut seen = vec![false; (TRAMPLE_TEXELS * TRAMPLE_TEXELS) as usize];
        for z in 0..TRAMPLE_TEXELS {
            for x in 0..TRAMPLE_TEXELS {
                let cell = origin + IVec2::new(x, z);
                let slot = trample_slot(cell);
                assert!(!seen[slot as usize], "two cells share slot {slot}");
                seen[slot as usize] = true;
                let slot_xy =
                    IVec2::new(slot as i32 % TRAMPLE_TEXELS, slot as i32 / TRAMPLE_TEXELS);
                assert_eq!(trample_cell(slot_xy, origin), cell);
                assert!(in_trample_window(cell, origin));
            }
        }
    }
    assert!(!in_trample_window(IVec2::new(256, 0), IVec2::ZERO));
    assert!(!in_trample_window(IVec2::new(0, -1), IVec2::ZERO));
}

#[test]
fn a_raised_view_keeps_the_same_number_of_blades_at_any_height() {
    assert_eq!(height_thinning(100.0, 0.0), 1.0, "a view on the ground");
    assert_eq!(height_thinning(100.0, 100.0), 0.25);
    // The ground in view grows with the square of the height over the
    // radius; the share kept falls by as much.
    for offset in [50.0f32, 200.0, 1000.0] {
        let ground = (100.0 + offset) * (100.0 + offset);
        assert!((height_thinning(100.0, offset) * ground - 100.0 * 100.0).abs() < 1e-2);
    }
}

/// A 1 m upright stem with one straight child pointing up from three quarters
/// of the way along it.
fn upright_with_one_child(length: f32) -> GrassPlant {
    use crate::look::{GrassArrangement, GrassBranches, GrassSegment};
    let straight = GrassSegment {
        profile: 0.0,
        droop: 0.0,
        twist: 0.0,
        fold: 0.0,
        ..GrassPlant::FERN.segment
    };
    GrassPlant {
        height: Vec2::new(1.0, 1.0),
        lean: 0.0,
        segment: straight,
        levels: [
            GrassBranches {
                count: 1,
                arrangement: GrassArrangement::Alternate,
                attachment_start: 0.5,
                length,
                length_envelope: 0.5,
                width: 0.1,
                angle: 1.0,
                spread: 0.0,
                segment: straight,
            },
            GrassBranches::NONE,
        ],
        ..GrassPlant::FERN
    }
}

#[test]
fn a_child_along_an_upright_stem_tops_it_by_its_length() {
    // An upright stem's middle control point sits at its tip, so the point at
    // t = 0.75 is at height 2t - t² = 0.9375 m. The child's tip is 0.6 m above.
    let top = branched_top(&upright_with_one_child(0.6));
    assert!((top - 1.5375).abs() < 1e-4, "top {top}");
}

#[test]
fn an_arching_stem_rests_its_tip_on_the_ground_well_below_its_length() {
    let plant = GrassPlant {
        lean: 0.4,
        segment: crate::look::GrassSegment {
            droop: 2.0,
            ..GrassPlant::FERN.segment
        },
        ..upright_with_one_child(0.1)
    };
    for droop_random in [0.0, 0.5, 1.0] {
        let stem = RestingStem {
            height: 1.0,
            width: 1.0,
            droop_random,
            twist_random: 0.5,
            key: 0.0,
        };
        let tip = resting_stem(&plant, &stem).points[2];
        assert!(tip.y.abs() < 0.02, "tip {tip}");
    }
    let top = branched_top(&plant);
    assert!(top < 0.6 * plant.height.y, "top {top}");
}

#[test]
fn stem_droop_varies_up_to_one_and_arches_above_it() {
    assert_eq!(stem_droop(0.6, 1.0, 0.0), (0.3, 0.0));
    assert_eq!(stem_droop(1.5, 0.5, 1.0), (0.5, 0.5));
    assert_eq!(stem_droop(2.5, 0.0, 1.0), (0.0, 1.0));
}

#[test]
fn long_branches_raise_a_plant_above_its_stem_but_within_its_reach() {
    let plant = GrassPlant::BROOM;
    let top = branched_top(&plant);
    let levels = level_entries(&plant);
    assert!(top > plant.height.y, "top {top}");
    assert!(
        top <= plant.height.y * crate::fields::reach_factor(&levels),
        "top {top}"
    );
}

#[test]
fn with_no_demand_yet_the_plant_slots_are_split_evenly() {
    // Two branched shapes share six buckets equally before any demand is recorded.
    let (capacity, base) = plan_buckets(&[0; MAX_BUCKETS], 1_000, 6_000, 9);
    assert_eq!(
        capacity[..3],
        [1_000, 500, 500],
        "blades keep fixed buckets"
    );
    assert_eq!(capacity[3..9], [1_000; 6]);
    assert!(capacity[9..].iter().all(|&slots| slots == 0));
    assert_eq!(base[..4], [0, 1_000, 1_500, 2_000]);
    assert_eq!(
        base[MAX_BUCKETS - 1],
        2_000 + 6_000,
        "unused buckets take no room"
    );
}

#[test]
fn a_bucket_with_more_demand_gets_more_of_the_budget() {
    // This shape needs 4,000 near stems. Dividing the 6,000-slot budget equally
    // between three detail levels would leave the near bucket short by 2,000.
    let mut counts = [0; MAX_BUCKETS];
    counts[3] = 4_000;
    let (capacity, _) = plan_buckets(&counts, 0, 6_000, 6);
    assert!(capacity[3] >= 4_000 + 4_000 / 4, "{:?}", &capacity[3..6]);
    assert!(capacity[4] >= MIN_SLACK && capacity[5] >= MIN_SLACK);
    assert!(capacity[3..6].iter().sum::<u32>() <= 6_000);
}

#[test]
fn demand_beyond_the_budget_is_scaled_to_fit() {
    let mut counts = [0; MAX_BUCKETS];
    counts[3..6].copy_from_slice(&[9_000, 3_000, 1_000]);
    let (capacity, _) = plan_buckets(&counts, 0, 6_000, 6);
    assert!(capacity[3..6].iter().sum::<u32>() <= 6_000);
    // With growth allowances and the per-request cap, requests total 11,512:
    // 6,000, 4,006 and 1,506. Proportional reduction fits them into 6,000 slots.
    assert_eq!(capacity[3..6], [3_125, 2_087, 784]);
}

proptest! {
    #[test]
    fn the_plant_slots_never_exceed_the_budget(
        counts in prop::collection::vec(any::<u32>(), MAX_BUCKETS),
        blade_capacity in 0..=GrassSettings::MAX_INSTANCE_CAPACITY,
        plant_capacity in 0..=GrassSettings::MAX_PLANT_CAPACITY,
        classes in 0..=MAX_BRANCHED_CLASSES as u32,
    ) {
        // Test builds check integer overflow, so these cases also check that the
        // shader's matching u32 calculations stay within range.
        let counts: [u32; MAX_BUCKETS] = counts.try_into().expect("one count per bucket");
        let buckets = 3 * (1 + classes);
        let (capacity, base) = plan_buckets(&counts, blade_capacity, plant_capacity, buckets);
        let branched: u32 = capacity[3..].iter().sum();
        prop_assert!(branched <= plant_capacity);
        prop_assert!(capacity[buckets as usize..].iter().all(|&slots| slots == 0));
        prop_assert_eq!(capacity[0], blade_capacity);
        for b in 1..MAX_BUCKETS {
            prop_assert_eq!(base[b], base[b - 1] + capacity[b - 1]);
        }
    }
}

proptest! {
    #[test]
    fn lerp_stays_within_its_endpoints(
        a in -1e6f32..1e6,
        b in -1e6f32..1e6,
        t in -1.0f32..2.0,
    ) {
        let result = lerp(a, b, t);
        let low = a.min(b);
        let high = a.max(b);
        let tolerance = 1e-5 * high.abs().max(low.abs()).max(1.0);
        prop_assert!(result >= low - tolerance);
        prop_assert!(result <= high + tolerance);
    }

    #[test]
    fn lerp_hits_its_endpoints(
        a in -1e6f32..1e6,
        b in -1e6f32..1e6,
    ) {
        let tolerance = 1e-5 * a.abs().max(b.abs()).max(1.0);
        prop_assert!((lerp(a, b, 0.0) - a).abs() <= tolerance);
        prop_assert!((lerp(a, b, 1.0) - b).abs() <= tolerance);
    }

    #[test]
    fn quantize_dequantize_round_trips(
        value in 0.0f32..=1.0,
        bits in 1u32..=16,
    ) {
        let max = ((1u64 << bits) - 1) as f32;
        let back = dequantize(quantize(value, bits), bits);
        prop_assert!((back - value).abs() <= 0.5 / max + 1e-6);
    }

    #[test]
    fn pack_bend_round_trips(
        bend in -1.0f32..=1.0,
        delta in -0.06f32..=0.05,
    ) {
        let previous = bend + delta;
        let (now, back) = unpack_bend(pack_bend(bend, previous));
        prop_assert!((now - bend).abs() <= 1.0 / 4095.0 + 1e-6);
        prop_assert!((back - previous).abs() <= 0.005);
    }

    #[test]
    fn coverage_width_stays_between_one_and_max(
        falloff in 0.0f32..=1.0,
        max in 1.0f32..=10.0,
    ) {
        let width = coverage_width(falloff, max);
        prop_assert!(width >= 1.0);
        prop_assert!(width <= max);
    }

    #[test]
    fn keep_fraction_is_a_share(
        density in -1000.0f32..=1000.0,
        scale in -10.0f32..=10.0,
        chunk_size in 0.1f32..=1000.0,
        layout_density in 0.0f32..=1000.0,
    ) {
        let layout = ChunkLayout::for_density(chunk_size, layout_density);
        let keep = keep_fraction(density, scale, &layout);
        prop_assert!((0.0..=1.0).contains(&keep));
    }
}
