//! Checks that GPU storage follows plant demand and supports every active bucket.
//!
//! Allocations must match `mirror::plan_buckets`. Scenes with more buckets than
//! the 64 threads in a workgroup must still count and draw all their shapes.

use super::*;
use crate::PlantSettings;
use crate::api::{PlantArrangement, PlantBranches, PlantParams};
use crate::gpu_layout::MAX_BRANCHED_CLASSES;
use crate::render::view::{DRAW_ARGS_COUNT, bucket_count};

/// Creates a small plant with a distinct shape class for each leaflet count.
///
/// Alternate leaflets preserve odd counts instead of rounding them into pairs.
fn leaflets(leaflets: u32) -> PlantParams {
    PlantParams {
        height: Vec2::new(0.3, 0.5),
        width: 0.01,
        levels: [
            PlantBranches {
                count: leaflets,
                arrangement: PlantArrangement::Alternate,
                length: 0.3,
                width: 0.3,
                ..PlantBranches::NONE
            },
            PlantBranches::NONE,
        ],
        ..PlantParams::MEADOW_GRASS
    }
}

/// Reads demand and allocated slots for branched buckets, skipping the three blade buckets.
fn branched(table: &[u32; BUCKET_TABLE_WORDS], buckets: usize) -> Vec<(u32, u32)> {
    (3..buckets)
        .map(|b| (table[b], table[CAPACITY_AT + b]))
        .collect()
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_busy_shape_gets_the_slots_an_even_split_would_withhold() {
    // Mix a common plant shape with a rare one to create uneven storage demand.
    let field = PlantScatter::new(
        Vec2::ZERO,
        vec![
            ScatterPlant::new(leaflets(6)).scaled(0.05),
            ScatterPlant::new(leaflets(12)).scaled(0.95),
        ],
    )
    .with_density(3.0);
    let mut app = app_with_field(field);
    wait_for_plants(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let buckets = bucket_count(2) as usize;
    let demand: Vec<u32> = branched(&bucket_table(&mut app), buckets)
        .iter()
        .map(|&(count, _)| count)
        .collect();

    // Supply enough storage for every bucket's demand and growth allowance.
    // An equal split would still leave the busiest bucket short.
    let budget: u32 = demand
        .iter()
        .map(|&count| count + count / 4 + mirror::MIN_SLACK)
        .sum();
    let busiest = *demand.iter().max().expect("branched buckets");
    assert!(
        busiest > budget / demand.len() as u32,
        "an even split of {budget} would fit {demand:?}"
    );
    app.insert_resource(PlantSettings {
        plant_capacity: budget,
        ..PlantSettings::HIGH
    });
    for _ in 0..4 {
        app.update();
    }
    let table = bucket_table(&mut app);
    for (b, (count, capacity)) in branched(&table, buckets).into_iter().enumerate() {
        assert!(
            count <= capacity,
            "bucket {}: {count} stems for {capacity} slots; demand {demand:?}",
            b + 3
        );
    }

    // Compare the next frame's GPU allocation with the CPU twin using these counts.
    let (view, _) = view_after(&mut app, 1);
    let after = bucket_table(&mut app);
    let counts: [u32; MAX_BUCKETS] = table[..MAX_BUCKETS].try_into().expect("the counts");
    let (capacity, base) = mirror::plan_buckets(
        &counts,
        view.blade_capacity,
        view.plant_capacity,
        view.bucket_count,
    );
    assert_eq!(after[CAPACITY_AT..CAPACITY_AT + MAX_BUCKETS], capacity);
    assert_eq!(after[BASE_AT..BASE_AT + MAX_BUCKETS], base);
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_more_buckets_than_lanes_are_all_counted_and_drawn() {
    // Add 24 sparse plant shapes to the meadow, requiring 75 buckets in total.
    // This exceeds the 64 threads in a workgroup. Use a separate field for each
    // shape because the CPU placement model does not handle mixed plants.
    const SHAPES: u32 = 24;
    let mut app = headless_app();
    let world = app.world_mut();
    for n in 1..=SHAPES {
        world.spawn((
            PlantScatter::new(Vec2::new(70.0, 50.0), vec![ScatterPlant::new(leaflets(n))])
                .with_density(0.1),
            Transform::from_xyz(3.0, 0.5, -4.0),
        ));
    }
    wait_for_plants(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let tables = app.world().resource::<FieldTables>().clone();
    assert_eq!(tables.shape_classes.len(), SHAPES as usize);
    assert!(SHAPES as usize <= MAX_BRANCHED_CLASSES);
    let buckets = bucket_count(SHAPES as usize) as usize;
    assert!(buckets > 64, "{buckets} buckets");

    // Shapes beyond the first 64 buckets must also be counted and drawn up to capacity.
    let table = bucket_table(&mut app);
    let args = {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let device = world.resource::<RenderDevice>().clone();
        let queue = world.resource::<RenderQueue>().clone();
        let mut views = world.query::<&PlantViewBuffers>();
        let buffers = views.iter(world).next().expect("a grass view");
        read_buffer(
            &device,
            &queue,
            &buffers.draw_args,
            DRAW_ARGS_COUNT as u64 * 20,
        )
    };
    for class in 22..=SHAPES as usize {
        let first = 3 * class;
        let counted: u32 = table[first..first + 3].iter().sum();
        assert!(counted > 0, "class {class} has no stems: {table:?}");
        for b in first..first + 3 {
            // The second word of each draw command gives the number of instances drawn.
            let drawn = args[b * 5 + 1];
            assert_eq!(drawn, table[b].min(table[CAPACITY_AT + b]), "bucket {b}");
        }
    }
    assert!(table[buckets..MAX_BUCKETS].iter().all(|&count| count == 0));

    // Each class's shadow draw must use the same near-detail stem instances
    // as the main draw, but fewer indices because each leaf is one triangle.
    for class in 1..=SHAPES as usize {
        let main = &args[3 * class * 5..3 * class * 5 + 5];
        let shadow = &args[(MAX_BUCKETS + class) * 5..(MAX_BUCKETS + class) * 5 + 5];
        let shape = tables.shape_classes[class - 1];
        assert_eq!(shadow[1], main[1], "class {class} instances");
        assert_eq!(shadow[3], main[3], "class {class} base vertex");
        assert_eq!(
            shadow[0] as usize,
            shape.shadow_indices().len(),
            "class {class}"
        );
        assert!(shadow[0] < main[0], "class {class} shadow must be cheaper");
    }

    // Check generated instances in every bucket against the CPU placement model.
    generation_matches_the_cpu_mirror_in(app, PlantSettings::HIGH);
}
