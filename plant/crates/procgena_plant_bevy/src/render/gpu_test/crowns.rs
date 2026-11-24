//! Checks GPU crown generation against the CPU model, including mesh-class routing and
//! whole-plant overflow handling. A crown is the set of stems sharing one root.

use super::*;
use crate::shader_model::ShapeClass;
use crate::{PlantBranches, PlantParams, PlantScatterOptions};

/// Five stems per crown exercise multi-stem expansion and whole-crown accounting.
const STEMS: u32 = 5;

/// A sparse fern field with `STEMS` fronds per plant and 12 leaflets per frond.
fn fern_field() -> PlantScatter {
    let fern = PlantParams {
        height: Vec2::new(0.5, 0.9),
        width: 0.01,
        lean: 0.4,
        stems_per_plant: STEMS,
        levels: [
            PlantBranches {
                count: 12,
                length: 0.3,
                width: 0.3,
                ..PlantBranches::NONE
            },
            PlantBranches::NONE,
        ],
        ..PlantParams::MEADOW_GRASS
    };
    PlantScatter::with_options(
        Vec2::ZERO,
        PlantScatterOptions::default()
            .with_plants(vec![ScatterPlant::new(fern)])
            .with_density(3.0),
    )
}

/// Counts non-zero-height stems by their packed root coordinates. The key keeps word 0
/// and only the root-height bits of word 1, excluding direction and seed.
fn stems_by_root(words: &[[u32; 4]]) -> HashMap<(u32, u32), u32> {
    let mut crowns = HashMap::new();
    for w in words.iter().filter(|w| w[2] & 0xff != 0) {
        *crowns.entry((w[0], w[1] & 0xffff)).or_insert(0) += 1;
    }
    crowns
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_ferns_grow_in_their_own_class_as_whole_crowns_as_the_mirror_places_them() {
    let mut app = app_with_field(fern_field());
    wait_for_plants(&mut app);
    let tables = app.world().resource::<FieldTables>().clone();
    assert_eq!(
        *tables.shape_classes,
        vec![ShapeClass { children: [12, 0] }],
        "one branched class, for 12 leaflets"
    );

    // Fern instances must occupy only the first branched class, leaving blade buckets
    // empty.
    let table = bucket_table(&mut app);
    let counts = &table[..MAX_BUCKETS];
    assert_eq!(counts[..3], [0, 0, 0], "{counts:?}");
    assert!(counts[3..6].iter().sum::<u32>() > 0, "{counts:?}");
    assert_eq!(counts[6..MAX_BUCKETS].iter().sum::<u32>(), 0, "{counts:?}");

    // Every visible root must retain its complete set of stems.
    let (_, words) = gpu_words(&mut app).expect("a grass view");
    let crowns = stems_by_root(&words);
    assert!(crowns.len() > 50, "{} crowns", crowns.len());
    assert!(
        crowns.values().all(|&stems| stems == STEMS),
        "every crown has its {STEMS} stems"
    );

    // Check placement and packed dimensions against the CPU prediction.
    generation_matches_the_cpu_mirror_in(app, crate::PlantSettings::HIGH);
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_crowns_that_do_not_fit_are_dropped_whole() {
    // Limit storage to 304 stems, forcing this dense scene to drop plants.
    let mut app = app_with_field(fern_field());
    app.insert_resource(crate::PlantSettings {
        plant_capacity: 304,
        ..crate::PlantSettings::HIGH
    });
    wait_for_plants(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let table = bucket_table(&mut app);
    let overflowing: Vec<(u32, u32)> = (3..6)
        .map(|b| (table[b], table[CAPACITY_AT + b]))
        .filter(|&(count, capacity)| count > capacity)
        .collect();
    // Ensure a bucket runs out of space partway through a crown's set of stems.
    // That crown must be dropped whole rather than drawn with missing stems.
    assert!(
        overflowing
            .iter()
            .any(|&(_, capacity)| capacity % STEMS != 0),
        "the scene should overflow a bucket mid-crown: {overflowing:?}"
    );
    let (_, words) = gpu_words(&mut app).expect("a grass view");
    let crowns = stems_by_root(&words);
    assert!(!crowns.is_empty());
    assert!(
        crowns.values().all(|&stems| stems == STEMS),
        "no crown is drawn in part: {crowns:?}"
    );
}
