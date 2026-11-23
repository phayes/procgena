//! Resting on the ground: logs and branches lie on y = 0, stumps stand in
//! it, and every level of detail shares one position.

use procgena_deadwood::{
    DeadwoodDetail, DeadwoodMesh, DeadwoodOptions, DeadwoodPreset, Piece, PieceKind, SettleOptions,
    SizeClass, StubOptions,
};

/// Lowest and highest Y of `mesh`'s part of `size`.
fn y_range(mesh: &DeadwoodMesh, size: SizeClass) -> (f32, f32) {
    let part = mesh.parts.iter().find(|p| p.size == size).expect("part");
    mesh.surfaces.positions[part.vertices.start as usize..part.vertices.end as usize]
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[1]), hi.max(p[1]))
        })
}

/// Lowest Y of every round stem in `mesh`.
fn lowest(mesh: &DeadwoodMesh) -> f32 {
    mesh.surfaces
        .positions
        .iter()
        .fold(f32::MAX, |lo, p| lo.min(p[1]))
}

/// Size class of `piece`'s main stem: a limb for a branch, a trunk
/// otherwise.
fn main_size(piece: &Piece) -> SizeClass {
    if piece.options().kind == PieceKind::Branch {
        SizeClass::Limb
    } else {
        SizeClass::Trunk
    }
}

#[test]
fn logs_and_branches_rest_on_the_ground() {
    for preset in DeadwoodPreset::ALL {
        let piece = Piece::from_preset(preset);
        if piece.options().kind == PieceKind::Stump {
            continue;
        }
        // Stiff wood rests on whatever touches first; soft wood sags until
        // its main stem lies on the ground, and side stems may sink.
        let mesh = piece.mesh_with_detail(&DeadwoodDetail::FULL);
        let (main_lo, _) = y_range(&mesh, main_size(&piece));
        let all_lo = lowest(&mesh);
        let tolerance = 0.02 * piece.options().diameter.max(0.05);
        assert!(
            main_lo > -tolerance,
            "{}: main stem lowest {main_lo}",
            preset.name()
        );
        assert!(
            main_lo.abs() < tolerance || all_lo.abs() < tolerance,
            "{}: floats, main {main_lo}, all {all_lo}",
            preset.name()
        );
        assert!(piece.placement().burial >= 0.0);
    }
}

#[test]
fn stiff_wood_stays_above_the_ground() {
    let mut o = DeadwoodPreset::WindthrowLog.options();
    o.decay = 0.0;
    let piece = Piece::new(o).unwrap();
    let low = lowest(&piece.mesh_with_detail(&DeadwoodDetail::FULL));
    assert!(low > -0.01 * piece.options().diameter, "lowest {low}");
}

#[test]
fn fresh_log_rests_on_its_stubs() {
    // Average height of the main stem's surface, with and without stubs.
    let mean_height = |per_metre: f32| {
        let o = DeadwoodOptions {
            decay: 0.0,
            stubs: StubOptions {
                per_metre,
                length: 0.8,
                angle: 85.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mesh = Piece::new(o)
            .unwrap()
            .mesh_with_detail(&DeadwoodDetail::LOW);
        let part = &mesh.parts[0];
        let ys = &mesh.surfaces.positions[part.vertices.start as usize..part.vertices.end as usize];
        ys.iter().map(|p| p[1]).sum::<f32>() / ys.len() as f32
    };
    let (bare, stubby) = (mean_height(0.0), mean_height(5.0));
    assert!(stubby > bare + 0.01, "with stubs {stubby}, without {bare}");
}

#[test]
fn decay_flattens_and_lowers() {
    let height = |decay: f32| {
        let o = DeadwoodOptions {
            decay,
            ..Default::default()
        };
        let piece = Piece::new(o).unwrap();
        let (lo, hi) = y_range(
            &piece.mesh_with_detail(&DeadwoodDetail::FULL),
            SizeClass::Trunk,
        );
        (lo, hi - lo)
    };
    let (fresh_lo, fresh_h) = height(0.0);
    let (rotten_lo, rotten_h) = height(5.0);
    assert!(
        rotten_h < fresh_h * 0.85,
        "fresh {fresh_h}, rotten {rotten_h}"
    );
    assert!(rotten_lo.abs() < 0.01 && fresh_lo > -0.01);
    let rotten = Piece::new(DeadwoodOptions {
        decay: 5.0,
        ..Default::default()
    })
    .unwrap();
    assert!(rotten.placement().burial > 0.25 * rotten.options().diameter);
}

#[test]
fn every_level_shares_the_resting_position() {
    for preset in [DeadwoodPreset::WindthrowLog, DeadwoodPreset::FallenLimb] {
        let piece = Piece::from_preset(preset);
        let size = main_size(&piece);
        let (round, strips): (Vec<_>, Vec<_>) = DeadwoodDetail::PRESETS
            .iter()
            .map(|(_, d)| *d)
            .partition(|d| !d.strip_all);
        let ranges: Vec<(f32, f32)> = round
            .iter()
            .map(|d| y_range(&piece.mesh_with_detail(d), size))
            .collect();
        let tolerance = 0.15 * piece.options().diameter;

        // Strips-only levels have no round stems; their strips, as laid
        // out for shadows and export, rest as low as the round stems.
        let full_lowest = lowest(&piece.mesh_with_detail(&round[0]));
        for d in &strips {
            let mesh = piece.mesh_with_detail(d);
            let cull = &mesh.ribbons.as_ref().expect("strips").cull;
            let low = cull.positions.iter().fold(f32::MAX, |lo, p| lo.min(p[1]));
            assert!(
                (low - full_lowest).abs() < tolerance,
                "{}: strips rest at {low}, round stems at {full_lowest}",
                preset.name()
            );
        }
        for r in &ranges[1..] {
            assert!(
                (r.0 - ranges[0].0).abs() < tolerance,
                "{}: {ranges:?}",
                preset.name()
            );
            assert!(
                (r.1 - ranges[0].1).abs() < tolerance,
                "{}: {ranges:?}",
                preset.name()
            );
        }
    }
}

#[test]
fn without_settling_the_log_keeps_its_direction() {
    let o = DeadwoodOptions {
        decay: 0.0,
        ends: procgena_deadwood::EndsOptions {
            base: procgena_deadwood::EndKind::Saw { tilt: 0.0 },
            tip: procgena_deadwood::EndKind::Saw { tilt: 0.0 },
        },
        shape: procgena_deadwood::ShapeOptions {
            bend: 0.0,
            wobble: 0.0,
            ..Default::default()
        },
        settle: SettleOptions {
            settle: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let piece = Piece::new(o).unwrap();
    let mesh = piece.mesh_with_detail(&DeadwoodDetail::LOW);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &mesh.surfaces.positions {
        lo = lo.min(p[0]);
        hi = hi.max(p[0]);
    }
    assert!(
        (hi - lo - piece.options().length).abs() < 0.05,
        "x extent {}",
        hi - lo
    );
    // A coarse mesh can miss the lowest bump the resting analysis found, so
    // allow the bump height (lumps are 6% of the radius by default).
    let low = lowest(&mesh);
    assert!(low.abs() < 0.05 * piece.options().diameter, "lowest {low}");
}

#[test]
fn stumps_stand_in_the_ground() {
    for preset in [
        DeadwoodPreset::FelledStump,
        DeadwoodPreset::NotchedStump,
        DeadwoodPreset::SnappedStump,
        DeadwoodPreset::RottenStump,
    ] {
        let piece = Piece::from_preset(preset);
        let o = piece.options();
        let mesh = piece.mesh_with_detail(&DeadwoodDetail::FULL);
        let (lo, hi) = y_range(&mesh, SizeClass::Trunk);
        assert!(
            lo < -0.5 * o.roots.depth * o.diameter,
            "{}: base {lo}",
            preset.name()
        );
        let reach = o.ends.tip.reach(o.tip_radius());
        assert!(
            (hi - o.length).abs() <= reach + 0.1 * o.length,
            "{}: top {hi}",
            preset.name()
        );
        if o.roots.count > 0 {
            let (root_lo, _) = y_range(&mesh, SizeClass::Branch);
            assert!(
                root_lo < 0.0,
                "{}: roots don't reach the soil",
                preset.name()
            );
        }
        assert_eq!(piece.placement().burial, 0.0);
    }
}

#[test]
fn stump_flare_widens_the_base() {
    let piece = Piece::from_preset(DeadwoodPreset::NotchedStump);
    let o = piece.options();
    let mesh = piece.mesh_with_detail(&DeadwoodDetail::FULL);
    let part = mesh
        .parts
        .iter()
        .find(|p| p.size == SizeClass::Trunk)
        .unwrap();
    let widest_near = |y0: f32, y1: f32| {
        mesh.surfaces.positions[part.vertices.start as usize..part.vertices.end as usize]
            .iter()
            .filter(|p| p[1] >= y0 && p[1] <= y1)
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0f32, f32::max)
    };
    let ground = widest_near(-0.12, 0.12);
    let above = widest_near(o.length * 0.6, o.length * 0.9);
    assert!(
        ground > above * (1.0 + 0.5 * o.roots.flare),
        "ground {ground}, above {above}"
    );
}
