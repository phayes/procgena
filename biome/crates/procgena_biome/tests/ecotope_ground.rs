//! An Ecotope's ground layers: litter under its trees and dirt round its
//! boulders, chunk weights that agree along edges, files, and that the
//! ground never changes what is placed.

use glam::{DVec2, Vec2, Vec3Swizzles};
use procgena_biome::{Ecotope, EcotopeError, EcotopePlacement, EcotopePreset, ScatterCopies};
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_core::{ChildKey, HeightGrid};
use procgena_terrain::ground::{
    GROUND_TEXELS, GroundError, GroundLayer, GroundPainter, GroundSource, GroundStack,
    GroundWeights, MaterialCatalogue, MaterialId, OnGround,
};
use procgena_terrain::{ChunkKey, FeatureTier};

fn key(key: &'static str) -> ChildKey {
    ChildKey::from_static(key)
}

fn pine_floor() -> Ecotope {
    EcotopePreset::pine_forest_floor().ecotope.clone()
}

/// Gently rolling ground over `lo` to `hi`, with a margin.
fn ground(lo: DVec2, hi: DVec2) -> HeightGrid {
    HeightGrid::from_fn(lo - 4.0, hi + 4.0, 0.5, |p| {
        let (x, z) = (p.x as f32, p.y as f32);
        0.6 * (0.21 * x).sin() + 0.4 * (0.17 * z + 0.1 * x).cos()
    })
}

/// The pine floor placed over the two level 0 chunks `(-1, 0)` and
/// `(0, 0)`, 128 by 64 m, and the ground it was placed on.
fn placed(ecotope: &Ecotope) -> (EcotopePlacement, HeightGrid) {
    let area = ScatterArea::new(DVec2::new(0.0, 32.0), Vec2::new(128.0, 64.0));
    let grid = ground(DVec2::new(-64.0, 0.0), DVec2::new(64.0, 64.0));
    let placement = ecotope
        .place(&ScatterInput::new(&grid, area, 5))
        .expect("the pine floor places");
    (placement, grid)
}

fn id(catalogue: &MaterialCatalogue, key: &str) -> MaterialId {
    catalogue.id(key).unwrap()
}

/// World positions of the placed copies of the Scatter keyed `scatter`.
fn positions(placement: &EcotopePlacement, scatter: &str) -> Vec<DVec2> {
    let centre = DVec2::new(0.0, 32.0);
    match placement.get(scatter).unwrap().copies() {
        ScatterCopies::Tree(placed) => placed
            .instances()
            .iter()
            .map(|c| centre + c.position.xz().as_dvec2())
            .collect(),
        ScatterCopies::Stone(placed) => placed
            .instances()
            .iter()
            .map(|c| centre + c.position.xz().as_dvec2())
            .collect(),
        _ => unreachable!("only trees and stones are looked at"),
    }
}

#[test]
fn litter_lies_under_pines_and_dirt_round_boulders() {
    let ecotope = pine_floor();
    let (placement, grid) = placed(&ecotope);
    let bare = GroundStack::bare(1, 1.0);
    let painter = ecotope.ground(&bare, &placement).unwrap();
    let catalogue = bare.catalogue();
    let at = |p: DVec2, material: &str| {
        let site = OnGround(&grid).site(p, FeatureTier::Fine);
        painter.paint(p, &site).weight(id(catalogue, material))
    };

    // Litter, broken up by noise, a metre or two from each pine's trunk.
    let pines = positions(&placement, "pines");
    assert!(pines.len() > 5, "{} pines", pines.len());
    let near: Vec<f32> = pines
        .iter()
        .map(|&p| at(p + DVec2::new(1.5, 0.0), "needle_litter"))
        .collect();
    let mean = near.iter().sum::<f32>() / near.len() as f32;
    assert!(mean > 0.5, "mean litter beside pines {mean}");

    // None well away from every pine.
    let mut away = 0;
    for x in (-60..60).step_by(3) {
        for z in (2..62).step_by(3) {
            let p = DVec2::new(f64::from(x), f64::from(z));
            // Beyond a 1 m trunk footprint and its 3 m reach.
            if pines.iter().all(|q| q.distance(p) > 5.0) {
                assert_eq!(at(p, "needle_litter"), 0.0, "at {p}");
                away += 1;
            }
        }
    }
    assert!(away > 20, "{away} points away from pines");

    // Bare dirt at the edge of every boulder.
    let stones = placement.get("boulders").unwrap();
    let ScatterCopies::Stone(placed_stones) = stones.copies() else {
        panic!("boulders are stones");
    };
    assert!(placed_stones.instances().len() > 5);
    for stone in placed_stones.instances() {
        let centre = DVec2::new(0.0, 32.0) + stone.position.xz().as_dvec2();
        let edge = centre + DVec2::new(f64::from(stone.radius), 0.0);
        assert!(
            at(edge, "bare_dirt") > 0.5,
            "dirt beside the stone at {centre}"
        );
    }

    // The forest soil covers the grass everywhere.
    assert_eq!(at(DVec2::new(3.0, 40.0), "grass"), 0.0);
}

#[test]
fn ground_layers_never_change_what_is_placed() {
    let with = pine_floor();
    let mut without = with.clone();
    without.ground.clear();
    let (a, _) = placed(&with);
    let (b, _) = placed(&without);
    assert_eq!(a.layers().len(), b.layers().len());
    for (a, b) in a.layers().iter().zip(b.layers()) {
        assert_eq!(a.footprints(), b.footprints(), "{}", a.key());
    }
}

#[test]
fn chunks_of_an_ecotope_agree_along_their_edge_in_any_order() {
    let ecotope = pine_floor();
    let (placement, grid) = placed(&ecotope);
    let bare = GroundStack::bare(1, 1.0);
    let painter = ecotope.ground(&bare, &placement).unwrap();
    let source = OnGround(&grid);
    let (west, east) = (ChunkKey::new(-1, 0, 0), ChunkKey::new(0, 0, 0));
    let build = |k| GroundWeights::chunk(&painter, &source, k, FeatureTier::Fine).unwrap();
    let (a, b) = (build(west), build(east));
    let (b_first, a_second) = (build(east), build(west));
    assert_eq!((&a, &b), (&a_second, &b_first));
    assert_eq!((a.dropped(), b.dropped()), (0, 0));
    assert!(a.palette().len() >= 4, "{:?}", a.palette());
    for j in 0..GROUND_TEXELS {
        for (material, _, _) in bare.catalogue().iter() {
            assert_eq!(
                a.weight(GROUND_TEXELS - 1, j, material),
                b.weight(0, j, material),
                "texel {j}"
            );
        }
    }

    // A single point matches the painter.
    let p = GroundWeights::chunk_point(east, 30, 40);
    let site = source.site(p, FeatureTier::Fine);
    assert_eq!(
        ecotope.ground_at(p, &site, &bare, &placement).unwrap(),
        painter.paint(p, &site)
    );
}

#[test]
fn a_file_changes_adds_and_removes_ground_layers() {
    let text = "\
base: pine_forest_floor
ecotope:
  ground:
    litter:
      opacity: 0.5
    mud:
      material: mud
      order: 330
      when:
        - slope: { fall: [0.05, 0.1] }
    moss: null
";
    let base = pine_floor();
    let read = EcotopePreset::from_yaml("mine", text).expect("the file reads");
    let ecotope = &read.ecotope;
    assert_eq!(ecotope.scatters, base.scatters);
    assert!(!ecotope.ground.contains_key(&key("moss")));
    assert_eq!(ecotope.ground[&key("soil")], base.ground[&key("soil")]);
    let mut litter = base.ground[&key("litter")].clone();
    litter.opacity = 0.5;
    assert_eq!(ecotope.ground[&key("litter")], litter);
    assert_eq!(ecotope.ground[&key("mud")].order, 330);
    assert_eq!(ecotope.ground[&key("mud")].under, []);
    ecotope.validate().expect("the merged Ecotope is valid");

    // Written over its base, it reads back the same, with the removed layer
    // as null.
    let written = read.to_file(Some(EcotopePreset::pine_forest_floor()));
    assert!(written.contains("moss: null"), "{written}");
    assert!(!written.contains("pines"), "{written}");
    let again = EcotopePreset::from_yaml("mine", &written).expect(&written);
    assert_eq!(&again.ecotope, ecotope);

    // Written without a base, layers leave out their default settings.
    let whole = read.to_file(None);
    assert!(!whole.contains("under: []"), "{whole}");
    let again = EcotopePreset::from_yaml("mine", &whole).expect(&whole);
    assert_eq!(&again.ecotope, ecotope);
}

#[test]
fn removing_a_followed_scatter_removes_its_layer_but_never_blocks_placing() {
    let text = "\
base: pine_forest_floor
ecotope:
  scatters:
    pines: null
";
    let read = EcotopePreset::from_yaml("mine", text).expect("the file reads");
    let ecotope = &read.ecotope;
    ecotope
        .validate()
        .expect("a layer may follow a missing Scatter");
    let (placement, grid) = placed(ecotope);
    let bare = GroundStack::bare(1, 1.0);
    let painter = ecotope.ground(&bare, &placement).unwrap();
    let weights = GroundWeights::chunk(
        &painter,
        &OnGround(&grid),
        ChunkKey::new(0, 0, 0),
        FeatureTier::Fine,
    )
    .unwrap();
    let catalogue = bare.catalogue();
    assert!(!weights.palette().contains(&id(catalogue, "needle_litter")));
    assert!(weights.palette().contains(&id(catalogue, "bare_dirt")));

    // A broken ground layer doesn't stop the Scatters placing either.
    let broken = pine_floor().with_ground(key("mud"), GroundLayer::new(key("mud"), 400));
    assert!(broken.validate().is_err());
    let (a, _) = placed(&broken);
    let (b, _) = placed(&pine_floor());
    assert_eq!(a.layers().len(), b.layers().len());
}

#[test]
fn bad_ground_layers_are_refused() {
    let base = pine_floor();

    // Two layers at one order.
    let ecotope = base
        .clone()
        .with_ground(key("mud"), GroundLayer::new(key("mud"), 400));
    assert!(matches!(
        ecotope.validate(),
        Err(EcotopeError::Ground(GroundError::SharedOrder {
            order: 400,
            ..
        }))
    ));

    // A material the World's catalogue lacks.
    let ecotope = base.with_ground(key("lava"), GroundLayer::new(key("lava"), 500));
    ecotope
        .validate()
        .expect("materials are checked against a catalogue");
    let (placement, _) = placed(&pine_floor());
    let bare = GroundStack::bare(1, 1.0);
    assert!(matches!(
        ecotope.ground(&bare, &placement),
        Err(EcotopeError::Ground(GroundError::UnknownMaterial { .. }))
    ));
}

#[test]
fn measure_and_label_conditions_read_and_write_in_a_file() {
    use procgena_terrain::ground::SiteDetail;
    use procgena_terrain::ground::{Condition, FlagRule, Ramp};
    use procgena_terrain::{Flags, Landform};

    let text = "\
base: pine_forest_floor
ecotope:
  ground:
    bank_mud:
      material: mud
      order: 330
      when:
        - water_distance: { fall: [2.0, 5.0] }
        - road_distance: { rise: [3.0, 6.0] }
        - curvature: { fall: [-0.01, 0.0] }
        - landform: [riverbank, shore]
        - flags: { any: [wet], none: [road, road_bank] }
";
    let read = EcotopePreset::from_yaml("mine", text).expect("the file reads");
    let ecotope = &read.ecotope;
    let mud = &ecotope.ground[&key("bank_mud")];
    assert_eq!(
        mud.when,
        [
            Condition::WaterDistance(Ramp::fall(2.0, 5.0)),
            Condition::RoadDistance(Ramp::rise(3.0, 6.0)),
            Condition::Curvature(Ramp::fall(-0.01, 0.0)),
            Condition::Landform([Landform::Riverbank, Landform::Shore].into()),
            Condition::Flags(FlagRule {
                any: Flags::WET,
                none: Flags::ROAD | Flags::ROAD_BANK,
            }),
        ]
    );
    assert_eq!(mud.detail(), SiteDetail::Labels);
    ecotope.validate().expect("the merged Ecotope is valid");

    // The Ecotope painter must request labels to evaluate these conditions.
    let (placement, _) = placed(ecotope);
    let bare = GroundStack::bare(1, 1.0);
    let painter = ecotope.ground(&bare, &placement).unwrap();
    assert_eq!(painter.detail(), SiteDetail::Labels);

    // Saving and loading the preset must preserve the complete Ecotope.
    let written = read.to_file(Some(EcotopePreset::pine_forest_floor()));
    assert!(written.contains("riverbank"), "{written}");
    let again = EcotopePreset::from_yaml("mine", &written).expect(&written);
    assert_eq!(&again.ecotope, ecotope);

    // Invalid landform and flag names should produce errors listing valid names.
    for (bad, known) in [
        ("landform: [beach]", "riverbank"),
        ("flags: { any: [snow] }", "road_bank"),
    ] {
        let text = format!(
            "base: pine_forest_floor\necotope:\n  ground:\n    m:\n      material: mud\n      \
             order: 331\n      when:\n        - {bad}\n"
        );
        let error = EcotopePreset::from_yaml("mine", &text)
            .unwrap_err()
            .to_string();
        assert!(error.contains(known), "{error}");
    }
}
