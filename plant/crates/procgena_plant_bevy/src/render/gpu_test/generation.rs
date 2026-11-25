use super::*;
use crate::{PlantAccent, PlantParams, PlantScatterOptions};

/// Checks plant and accent proportions, and that accents follow their plant's tufts.
/// Untufted clover provides a comparison with tufted grass and its 20% accent.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_plants_and_accents_take_their_shares_and_accents_grow_in_their_tufts() {
    const TUFT_SIZE: f32 = 1.0;
    let grass = PlantParams {
        tufts: 0.8,
        tuft_size: TUFT_SIZE,
        accent: PlantAccent {
            share: 0.2,
            height: Vec2::new(0.8, 0.9),
            ..PlantAccent::NONE
        },
        ..PlantParams::MEADOW_GRASS
    };
    let mut app = app_with_field(PlantScatter::new(
        Vec2::ZERO,
        vec![
            // 18 clover and 42 grass plants per square metre: a 30% share.
            ScatterPlant::new(PlantParams::CLOVER).scaled(18.0 / PlantParams::CLOVER.density),
            ScatterPlant::new(grass).scaled(42.0 / grass.density),
        ],
    ));
    wait_for_plants(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let (view, words) = gpu_words(&mut app).expect("the view's blades");
    let origin = view.instance_origin;
    let position = |q: u32, center: f32| ((q as f32 / 65535.0) - 0.5) * 2.0 * origin.w + center;

    // Each blade's entry: 0 clover, 1 grass, 2 the grass's accent.
    let mut counts = [0usize; 3];
    let mut to_tuft = [0.0f32; 3];
    for w in &words {
        let entry = ((w[2] >> 16) & 0xff) as usize;
        assert!(entry < 3, "entry {entry}");
        counts[entry] += 1;
        let root = Vec2::new(
            position(w[0] & 0xffff, origin.x),
            position(w[0] >> 16, origin.z),
        );
        let (centre, _) = mirror::tuft_at(root / TUFT_SIZE);
        to_tuft[entry] += root.distance(centre * TUFT_SIZE);
    }
    let total = counts.iter().sum::<usize>() as f32;
    assert!(total > 5_000.0, "{counts:?}");
    let clover = counts[0] as f32 / total;
    assert!((clover - 0.3).abs() < 0.03, "{counts:?}");
    let accent = counts[2] as f32 / (counts[1] + counts[2]) as f32;
    assert!((accent - 0.2).abs() < 0.03, "{counts:?}");

    // Mean distance to the nearest grass tuft centre.
    let mean = |entry: usize| to_tuft[entry] / counts[entry] as f32;
    assert!(
        mean(2) < 0.6 * mean(0),
        "accent {} against clover {}",
        mean(2),
        mean(0)
    );
    assert!(
        mean(1) < 0.6 * mean(0),
        "grass {} against clover {}",
        mean(1),
        mean(0)
    );
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_generation_matches_the_cpu_mirror() {
    generation_matches_the_cpu_mirror(crate::PlantSettings::HIGH);
}

/// Every distance, density and width a game can set reaches the shaders: the
/// GPU and the CPU mirror, which reads them back from the view uniform, still
/// agree blade for blade.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_generation_matches_the_cpu_mirror_with_other_distances_and_densities() {
    generation_matches_the_cpu_mirror(crate::PlantSettings {
        radius: 80.0,
        lod1_start: 0.1,
        lod2_start: 0.35,
        fade_start: 0.7,
        lod_blend: 9.0,
        lod2_density: 0.7,
        fade_density: 0.3,
        mid_range_start: 0.15,
        mid_range_density: 0.8,
        max_widening: 1.6,
        ..crate::PlantSettings::HIGH
    });
}

/// Every blade's root in world space, decoded from the first grass view's
/// instances.
fn gpu_roots(app: &mut App) -> Vec<Vec3> {
    let Some((view, words)) = gpu_words(app) else {
        return Vec::new();
    };
    let origin = view.instance_origin;
    let position =
        |q: u32, center: f32, range: f32| ((q as f32 / 65535.0) - 0.5) * 2.0 * range + center;
    words
        .iter()
        .map(|w| {
            Vec3::new(
                position(w[0] & 0xffff, origin.x, origin.w),
                position(w[1] & 0xffff, origin.y, view.limits.w),
                position(w[0] >> 16, origin.z, origin.w),
            )
        })
        .collect()
}

/// A field turned about Y is placed, culled and thinned blade for blade as
/// the CPU mirror turns it, and its blades fill the turned rectangle.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_turned_field_matches_the_cpu_mirror() {
    let turned = Transform::from_xyz(3.0, 0.5, -4.0).with_rotation(Quat::from_rotation_y(0.6));
    let mut app = headless_app_with(turned, PlantGround::Flat);
    generation_matches_the_cpu_mirror_in(
        {
            wait_for_plants(&mut app);
            let roots = gpu_roots(&mut app);
            let inverse = turned.rotation.inverse();
            let local = |root: Vec3| inverse * (root - turned.translation);
            // Blades lean and tufts pull, but roots stay in the rectangle.
            assert!(roots.iter().all(|&root| {
                let at = local(root);
                at.x.abs() <= 35.05 && at.z.abs() <= 25.05
            }));
            assert!(
                roots
                    .iter()
                    .any(|root| (root.x - 3.0).abs() > 35.5 || (root.z + 4.0).abs() > 25.5),
                "some blades lie outside the unturned rectangle"
            );
            app
        },
        crate::PlantSettings::HIGH,
    );
}

/// A mesh ground is baked to a heightmap: blades stand on the mesh, at its
/// height, and none grow where it does not reach.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_blades_grow_on_a_mesh_ground_and_only_on_it() {
    let mut app = headless_app();
    // A ledge 2 m up under the field's +X half; the −X half has no ground.
    let ledge = Mesh::from(Plane3d::default().mesh().size(35.0, 50.0))
        .translated_by(Vec3::new(17.5, 2.0, 0.0));
    let ledge = app.world_mut().resource_mut::<Assets<Mesh>>().add(ledge);
    let mut fields = app.world_mut().query::<&mut PlantScatter>();
    fields.single_mut(app.world_mut()).unwrap().ground = PlantGround::mesh(ledge);
    wait_for_plants(&mut app);
    // The tables are rebuilt from the bake within a frame or two.
    for _ in 0..3 {
        app.update();
    }

    let roots = gpu_roots(&mut app);
    assert!(roots.len() > 1_000, "{} blades", roots.len());
    // The field's centre is at x = 3; the bake has a half-cell (0.25 m)
    // seam where ground meets hole.
    assert!(
        roots.iter().all(|root| root.x > 3.0 - 0.3),
        "no blade grows past the mesh's edge"
    );
    assert!(
        roots.iter().all(|root| (root.y - 2.5).abs() < 0.02),
        "every blade stands on the ledge, 2 m above the field's 0.5"
    );
}

/// The heightmaps follow the masks in one GPU buffer: a field with both
/// reads its heights past the mask words, not the mask itself.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_field_with_a_mask_and_a_heightmap_reads_its_heights() {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    let mut app = headless_app();
    let ledge = Mesh::from(Plane3d::default().mesh().size(70.0, 50.0))
        .translated_by(Vec3::new(0.0, 2.0, 0.0));
    let ledge = app.world_mut().resource_mut::<Assets<Mesh>>().add(ledge);
    // Full density and height everywhere: as mask words these texels are
    // all ones, which read as heights would be NaN.
    let mask = Image::new_fill(
        Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    let mask = app.world_mut().resource_mut::<Assets<Image>>().add(mask);
    let mut fields = app.world_mut().query::<&mut PlantScatter>();
    let mut field = fields.single_mut(app.world_mut()).unwrap();
    field.ground = PlantGround::mesh(ledge);
    field.density_map = Some(mask);
    wait_for_plants(&mut app);
    for _ in 0..3 {
        app.update();
    }

    let tables = app.world().resource::<FieldTables>();
    assert!(!tables.masks.is_empty() && !tables.heights.is_empty());
    let roots = gpu_roots(&mut app);
    assert!(roots.len() > 1_000, "{} blades", roots.len());
    assert!(
        roots.iter().all(|root| (root.y - 2.5).abs() < 0.02),
        "every blade stands on the ground, 2 m above the field's 0.5"
    );
}

/// A camera raised with `radius_from_ground` thins, culls and picks detail
/// blade for blade as the CPU mirror does: distances less its height for the
/// reach and the thinning, its height thinning on top, and the real distance
/// for the level of detail.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_raised_camera_matches_the_cpu_mirror() {
    use crate::api::PlantCamera;

    let mut app = headless_app();
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(camera).insert((
        Transform::from_xyz(3.0, 40.5, 40.0).looking_at(Vec3::new(3.0, 0.5, -4.0), Vec3::Y),
        PlantCamera {
            radius_from_ground: true,
            ..default()
        },
    ));
    wait_for_plants(&mut app);
    let (view, _) = view_after(&mut app, 1);
    assert_eq!(view.distance_offset, 40.0);
    generation_matches_the_cpu_mirror_in(app, crate::PlantSettings::HIGH);
}

/// The same ground as one field, as two fields that meet, or as four grows
/// the same blades, tufts and all: candidates sit on World cells, and each
/// field owns the candidates in its own rectangle before any tuft pulls
/// them.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_fields_that_meet_grow_the_blades_of_one() {
    // Tufted grass, so roots are pulled across the seams.
    let options = PlantScatterPreset::SUMMER_WILD.plant_scatter.clone();
    assert!(options.plants.iter().any(|entry| entry.plant.tufts > 0.0));

    // One field, 70 by 50 m around (3, -4).
    let mut app = app_with_field(PlantScatter::with_options(Vec2::ZERO, options.clone()));
    wait_for_plants(&mut app);
    let whole = gpu_roots(&mut app);
    assert!(whole.len() > 10_000, "the scene should place many blades");

    // The same rectangle as two fields side by side, then as four.
    for (columns, rows) in [(2, 1), (2, 2)] {
        split_harness(&mut app, &options, columns, rows);
        for _ in 0..10 {
            app.update();
        }
        let split = gpu_roots(&mut app);
        assert_same_roots(&whole, &split, &format!("{columns} by {rows} fields"));
    }
}

/// Cuts the harness's 70 by 50 m field around (3, -4) into `columns` by
/// `rows` equal fields of `options`, replacing any other fields.
fn split_harness(app: &mut App, options: &PlantScatterOptions, columns: u32, rows: u32) {
    let world = app.world_mut();
    let size = Vec2::new(70.0 / columns as f32, 50.0 / rows as f32);
    let centre = |column: u32, row: u32| {
        let low = Vec2::new(3.0 - 35.0, -4.0 - 25.0);
        let middle = low + (Vec2::new(column as f32, row as f32) + 0.5) * size;
        Transform::from_xyz(middle.x, 0.5, middle.y)
    };

    // The harness keeps the first piece; every other field goes.
    let mut fields = world.query_filtered::<Entity, With<PlantScatter>>();
    let all: Vec<Entity> = fields.iter(world).collect();
    for &entity in &all[1..] {
        world.despawn(entity);
    }
    let mut harness = world
        .query::<(&mut PlantScatter, &mut Transform)>()
        .single_mut(world)
        .expect("the harness's field");
    harness.0.size = size;
    *harness.1 = centre(0, 0);

    // The other pieces.
    for row in 0..rows {
        for column in 0..columns {
            if (column, row) != (0, 0) {
                world.spawn((
                    PlantScatter::with_options(size, options.clone()),
                    centre(column, row),
                ));
            }
        }
    }
}

/// Asserts that `split` holds the same roots as `whole`.
fn assert_same_roots(whole: &[Vec3], split: &[Vec3], what: &str) {
    let unmatched = missing_roots(whole, split);
    assert_eq!(split.len(), whole.len(), "{what}: blade count");
    assert!(
        unmatched.is_empty(),
        "{what}: {} roots of the whole field are missing, such as {:?}",
        unmatched.len(),
        &unmatched[..unmatched.len().min(5)]
    );
}

/// The roots of `wanted` that `found` lacks. Roots are read back packed to
/// 16 bits around the view's instance origin, so the same root may read back
/// a step apart: each root is matched with one within 5 mm.
fn missing_roots(wanted: &[Vec3], found: &[Vec3]) -> Vec<Vec3> {
    let cell = |r: Vec3| (r.xz() / 0.05).floor().as_ivec2();
    let mut buckets: HashMap<IVec2, Vec<Vec3>> = HashMap::default();
    for &root in found {
        buckets.entry(cell(root)).or_default().push(root);
    }
    let mut missing = Vec::new();
    for &root in wanted {
        let near = (-1..=1).flat_map(|dz| (-1..=1).map(move |dx| IVec2::new(dx, dz)));
        let matched = near.into_iter().find_map(|d| {
            let bucket = buckets.get_mut(&(cell(root) + d))?;
            let at = bucket
                .iter()
                .position(|other| other.distance(root) < 0.005)?;
            Some(bucket.swap_remove(at))
        });
        if matched.is_none() {
            missing.push(root);
        }
    }
    missing
}

/// A field turned about Y grows the blades of the World's pattern inside
/// its turned rectangle: every root is one a larger, unturned field with the
/// same seed grows there.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_turned_field_grows_the_world_pattern() {
    // A 120 m square around (3, -4), unturned.
    let mut app = headless_app();
    set_harness_size(&mut app, Vec2::splat(120.0));
    wait_for_plants(&mut app);
    let world_pattern = gpu_roots(&mut app);

    // The 70 by 50 m field, turned.
    set_harness_size(&mut app, Vec2::new(70.0, 50.0));
    let world = app.world_mut();
    let mut transform = world
        .query_filtered::<&mut Transform, With<PlantScatter>>()
        .single_mut(world)
        .expect("the harness's field");
    transform.rotation = Quat::from_rotation_y(0.6);
    for _ in 0..10 {
        app.update();
    }
    let turned = gpu_roots(&mut app);

    assert!(turned.len() > 10_000, "the scene should place many blades");
    let missing = missing_roots(&turned, &world_pattern);
    assert!(
        missing.is_empty(),
        "{} roots of the turned field are not in the World's pattern, such as {:?}",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
}

fn set_harness_size(app: &mut App, size: Vec2) {
    let world = app.world_mut();
    let mut field = world
        .query::<&mut PlantScatter>()
        .single_mut(world)
        .expect("the harness's field");
    field.size = size;
}

/// A masked field places blade for blade as the CPU mirror does, and
/// raising its mask only adds blades: every blade of the sparser mask stays
/// where it was.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_masked_field_matches_the_cpu_mirror_and_more_mask_only_adds_blades() {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    // A 16 by 16 mask whose density rises along X, from `low` to `low + 160`,
    // with full height.
    let gradient = |low: u8| {
        let mut data = Vec::with_capacity(16 * 16 * 4);
        for _z in 0..16 {
            for x in 0..16u8 {
                data.extend_from_slice(&[low + x * 10, 255, 0, 255]);
            }
        }
        Image::new(
            Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        )
    };
    let set_mask = |app: &mut App, image: Image| {
        let mask = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        let world = app.world_mut();
        let mut field = world
            .query::<&mut PlantScatter>()
            .single_mut(world)
            .expect("the harness's field");
        field.density_map = Some(mask);
    };

    // The sparse mask, then a denser one.
    let mut app = headless_app();
    set_mask(&mut app, gradient(20));
    wait_for_plants(&mut app);
    let sparse = gpu_roots(&mut app);
    set_mask(&mut app, gradient(90));
    for _ in 0..10 {
        app.update();
    }
    let dense = gpu_roots(&mut app);

    assert!(sparse.len() > 1_000, "{} blades", sparse.len());
    assert!(
        dense.len() > sparse.len() * 3 / 2,
        "{} against {}",
        dense.len(),
        sparse.len()
    );
    let moved = missing_roots(&sparse, &dense);
    assert!(
        moved.is_empty(),
        "{} blades of the sparse mask are gone or moved, such as {:?}",
        moved.len(),
        &moved[..moved.len().min(5)]
    );
    generation_matches_the_cpu_mirror_in(app, crate::PlantSettings::HIGH);
}
