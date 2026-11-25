use bevy::MinimalPlugins;
use bevy::app::{App, PostUpdate};
use bevy::asset::{AssetApp, AssetPlugin, Handle};
use bevy::camera::visibility::{Visibility, VisibilitySystems};
use bevy::math::Vec3Swizzles;
use bevy::transform::components::Transform;

use super::*;
use crate::PlantScatterPreset;
use crate::{PlantAccent, PlantScatterOptions, Srgb};

/// Stands in for Bevy's visibility propagation, which needs the whole
/// renderer: an entity is visible unless it or an ancestor up to the
/// nearest `Visible` is `Hidden`. The full-app GPU test runs the real one.
fn propagate(
    entities: Query<Entity, With<Visibility>>,
    visibilities: Query<(&Visibility, Option<&ChildOf>)>,
    mut inherited: Query<&mut InheritedVisibility>,
) {
    for entity in &entities {
        let mut visible = true;
        let mut current = entity;
        while let Ok((visibility, parent)) = visibilities.get(current) {
            match (visibility, parent) {
                (Visibility::Hidden, _) => visible = false,
                (Visibility::Inherited, Some(parent)) => {
                    current = parent.parent();
                    continue;
                }
                _ => {}
            }
            break;
        }
        let mut inherited = inherited.get_mut(entity).unwrap();
        if inherited.get() != visible {
            *inherited = if visible {
                InheritedVisibility::VISIBLE
            } else {
                InheritedVisibility::HIDDEN
            };
        }
    }
}

/// The systems exactly as the plugin registers and orders them.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<bevy::mesh::Mesh>()
        .add_systems(
            PostUpdate,
            propagate.in_set(VisibilitySystems::VisibilityPropagate),
        )
        .add_plugins(plugin);
    app
}

/// A 32 m meadow field, and the app's tables after it built.
fn spawn_field(app: &mut App, extra: impl Bundle) -> Entity {
    app.world_mut()
        .spawn((
            PlantScatter::from_preset(&PlantScatterPreset::MEADOW, Vec2::splat(32.0)),
            GlobalTransform::IDENTITY,
            extra,
        ))
        .id()
}

/// Runs a frame past the first build, so the tables are quiet.
fn settle(app: &mut App) {
    app.update();
    app.update();
}

/// What the render world would draw the first field with.
fn access_of(app: &App) -> u32 {
    app.world().resource::<FieldTables>().fields[0].access.x
}

#[test]
fn only_a_field_moving_rebuilds_the_tables() {
    let mut app = app();
    let field = spawn_field(&mut app, ());
    let camera = app
        .world_mut()
        .spawn((Transform::IDENTITY, GlobalTransform::IDENTITY))
        .id();
    app.update();
    app.update();
    let built = app.world().resource::<FieldTables>().version;
    assert!(built > 0, "the first field builds the tables");
    assert_eq!(app.world().resource::<FieldTables>().chunks.len(), 4);

    for step in 1..5 {
        *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
            GlobalTransform::from_xyz(step as f32, 0.0, 0.0);
        app.update();
    }
    assert_eq!(
        app.world().resource::<FieldTables>().version,
        built,
        "a moving camera rebuilt the tables"
    );

    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() =
        GlobalTransform::from_xyz(5.0, 0.0, 0.0);
    app.update();
    assert_eq!(app.world().resource::<FieldTables>().version, built + 1);
}

/// Builds a field mix of clover entries, each at the supplied density scale,
/// so the scales are the clovers' relative densities.
fn clover_weighing(weights: &[f32]) -> Vec<ScatterPlant> {
    weights
        .iter()
        .map(|weight| ScatterPlant::new(PlantParams::CLOVER).scaled(*weight))
        .collect()
}

#[test]
fn plant_chances_give_each_its_share_of_the_density() {
    // A conditional selection chance times the remaining share must equal
    // the plant's density divided by the total density.
    let weights = [0.1, 0.06, 0.03, 0.81];
    let chances = plant_chances(&clover_weighing(&weights));
    let mut left = 1.0;
    for (weight, chance) in weights.iter().zip(&chances) {
        assert!((left * chance - weight).abs() < 1e-6);
        left *= 1.0 - chance;
    }
    // Weights need not add up to 1: only their proportions count.
    let chances = plant_chances(&clover_weighing(&[2.0, 2.0]));
    assert!((chances[0] - 0.5).abs() < 1e-6, "{chances:?}");
    let chances = plant_chances(&clover_weighing(&[3.0, 1.0]));
    assert!((chances[0] - 0.75).abs() < 1e-6, "{chances:?}");
}

#[test]
fn a_plants_own_density_and_its_scale_both_set_its_share() {
    // 30 sparse plants and 90 dense ones per square metre: a quarter sparse.
    let sparse = PlantParams {
        density: 30.0,
        ..PlantParams::CLOVER
    };
    let dense = PlantParams {
        density: 90.0,
        ..PlantParams::CLOVER
    };
    let chances = plant_chances(&[ScatterPlant::new(sparse), ScatterPlant::new(dense)]);
    assert!((chances[0] - 0.25).abs() < 1e-6, "{chances:?}");
    // Scaling the dense plant down to 30 evens them out.
    let chances = plant_chances(&[
        ScatterPlant::new(sparse),
        ScatterPlant::new(dense).scaled(1.0 / 3.0),
    ]);
    assert!((chances[0] - 0.5).abs() < 1e-6, "{chances:?}");
}

#[test]
fn the_last_growing_plant_takes_what_is_left_and_a_zero_density_draws_nothing() {
    // The last plant with a density takes the rest outright, whatever
    // rounding the shares before it left behind.
    let chances = plant_chances(&clover_weighing(&[0.1, 0.3]));
    assert_eq!(chances[1], 1.0);
    // A plant scaled to 0 draws nothing, even when it is last.
    let chances = plant_chances(&clover_weighing(&[0.5, 0.0, 0.5, 0.0]));
    assert_eq!(chances, vec![0.5, 0.0, 1.0, 0.0]);
    // A field of nothing but zero densities draws nothing at all.
    let chances = plant_chances(&clover_weighing(&[0.0, 0.0]));
    assert_eq!(chances, vec![0.0, 0.0]);
}

#[test]
fn a_field_with_nothing_to_grow_has_no_entries_and_no_chunks() {
    for plants in [vec![], clover_weighing(&[0.0, 0.0])] {
        let mut app = app();
        spawn_with(
            &mut app,
            PlantScatter::new(Vec2::splat(32.0), plants.clone()),
        );
        let meadow = spawn_field(&mut app, ());
        settle(&mut app);
        let tables = tables_now(&app);
        assert_eq!(tables.fields.len(), 1, "{plants:?}: only the meadow");
        assert!(
            tables.chunks.iter().all(|chunk| chunk.ids.x == 0),
            "{plants:?}: every chunk is the meadow's"
        );
        assert_eq!(tables.chunks.len(), 4, "{plants:?}");
        app.world_mut().despawn(meadow);
        settle(&mut app);
        let tables = tables_now(&app);
        assert!(
            tables.fields.is_empty() && tables.chunks.is_empty(),
            "{plants:?}"
        );
    }
}

#[test]
fn every_plant_is_an_entry_after_its_field() {
    let mut app = app();
    app.world_mut().spawn((
        PlantScatter::from_preset(&PlantScatterPreset::SUMMER_WILD, Vec2::splat(32.0)),
        GlobalTransform::IDENTITY,
    ));
    app.update();
    let tables = app.world().resource::<FieldTables>();
    let plants = &PlantScatterPreset::SUMMER_WILD.plant_scatter.plants;
    let count = plants.len();
    assert_eq!(tables.fields.len(), count);
    assert!(tables.chunks.iter().all(|chunk| chunk.ids.x == 0));
    let first = tables.fields[0];
    assert_eq!(first.mix.z, count as f32);
    assert_eq!(first.mix.w, PlantParams::SEED_HEADS.height.y);
    // The seed heads' entry has their look and the field's placement.
    let heads = tables.fields[0];
    assert_eq!(heads.shape.y, PlantParams::SEED_HEADS.height.y);
    assert_eq!(heads.form.w, PlantParams::SEED_HEADS.head_length);
    // Each plant has its own tufts, and the field its bare patches.
    let grass = tables.fields[count - 1];
    let main = plants[count - 1].plant;
    assert_eq!(heads.placement, grass.placement);
    assert_eq!(
        (heads.tufts.x, heads.tufts.y),
        (0.0, PlantParams::SEED_HEADS.tuft_size)
    );
    assert_eq!((grass.tufts.x, grass.tufts.y), (main.tufts, main.tuft_size));
    assert_eq!(
        (heads.tufts.z, heads.tufts.w),
        (grass.tufts.z, grass.tufts.w)
    );
    // The cull spread covers the widest tufts of any tufted plant.
    assert_eq!(first.variation.w, main.tuft_size * 1.5);
    assert!(tables.features.contains(LookFeatures::SHAPE));
    assert!(tables.features.contains(LookFeatures::VARIATION));
}

#[test]
fn a_chunk_height_range_covers_every_texel_it_touches() {
    // A 4 × 4 heightmap with one tall texel in the corner.
    let mut heights = vec![0.2; 16];
    heights[15] = 0.9;
    let size = UVec2::new(4, 4);
    let (low, high) = height_range(&heights, size, Vec2::new(0.5, 0.5), Vec2::splat(0.5));
    assert_eq!((low, high), (0.2, 0.9));
    let (_, high) = height_range(&heights, size, Vec2::ZERO, Vec2::splat(0.25));
    assert_eq!(high, 0.2);
}

#[test]
fn a_hidden_field_is_not_drawn_and_showing_it_rebuilds_nothing_else() {
    let mut app = app();
    let field = spawn_field(&mut app, ());
    settle(&mut app);
    assert_eq!(access_of(&app), 1, "a visible field is on layer 0");
    let before = app.world().resource::<FieldTables>().clone();

    app.world_mut().entity_mut(field).insert(Visibility::Hidden);
    app.update();
    let hidden = app.world().resource::<FieldTables>().clone();
    assert_eq!(access_of(&app), 0, "a hidden field is not drawn");
    assert_eq!(hidden.version, before.version + 1);
    assert_eq!(
        hidden.structure_version, before.structure_version,
        "hiding a field must not rebuild its chunks and images"
    );
    assert!(Arc::ptr_eq(&hidden.chunks, &before.chunks));
    assert!(Arc::ptr_eq(&hidden.masks, &before.masks));
    assert!(Arc::ptr_eq(&hidden.heights, &before.heights));

    app.world_mut()
        .entity_mut(field)
        .insert(Visibility::Visible);
    app.update();
    assert_eq!(access_of(&app), 1);
    let shown = app.world().resource::<FieldTables>();
    assert_eq!(shown.structure_version, before.structure_version);
    assert_eq!(shown.chunks.len(), 4, "the chunks were there all along");
}

#[test]
fn a_field_under_a_hidden_parent_is_hidden_too() {
    let mut app = app();
    let parent = app.world_mut().spawn(Visibility::Visible).id();
    let field = spawn_field(&mut app, ChildOf(parent));
    app.update();
    assert_eq!(access_of(&app), 1);

    app.world_mut()
        .entity_mut(parent)
        .insert(Visibility::Hidden);
    app.update();
    assert_eq!(access_of(&app), 0, "hiding a level hides the grass in it");
    // The field itself still says `Inherited`.
    assert_eq!(
        app.world().get::<Visibility>(field),
        Some(&Visibility::Inherited)
    );

    app.world_mut()
        .entity_mut(parent)
        .insert(Visibility::Visible);
    app.update();
    assert_eq!(access_of(&app), 1);
}

#[test]
fn a_field_is_drawn_on_the_render_layers_it_names() {
    assert_eq!(layer_mask(None), 0b1, "no component: layer 0, like a mesh");
    assert_eq!(layer_mask(Some(&RenderLayers::layer(3))), 0b1000);
    assert_eq!(
        layer_mask(Some(&RenderLayers::from_layers(&[0, 5, 31]))),
        1 | 1 << 5 | 1 << 31
    );
    assert_eq!(layer_mask(Some(&RenderLayers::none())), 0);
    let beyond = RenderLayers::from_layers(&[2, 40]);
    assert_eq!(
        layer_mask(Some(&beyond)),
        0b100,
        "layers above 31 are left out"
    );
    assert!(has_unsupported_layer(Some(&beyond)));
    assert!(!has_unsupported_layer(Some(&RenderLayers::layer(31))));
    assert!(!has_unsupported_layer(None));

    let mut app = app();
    let field = spawn_field(&mut app, RenderLayers::layer(2));
    settle(&mut app);
    assert_eq!(access_of(&app), 0b100);
    let structure = app.world().resource::<FieldTables>().structure_version;

    app.world_mut()
        .entity_mut(field)
        .insert(RenderLayers::from_layers(&[1, 2]));
    app.update();
    assert_eq!(access_of(&app), 0b110);
    assert_eq!(
        app.world().resource::<FieldTables>().structure_version,
        structure,
        "changing layers must not rebuild chunks and images"
    );

    // Hidden wins over any layer.
    app.world_mut().entity_mut(field).insert(Visibility::Hidden);
    app.update();
    assert_eq!(access_of(&app), 0);
}

#[test]
fn an_accent_is_an_entry_after_the_plants_that_its_plant_points_at() {
    let mut app = app();
    let plumed = PlantParams {
        tufts: 0.8,
        tuft_size: 1.5,
        accent: PlantAccent {
            share: 0.25,
            height: Vec2::new(2.0, 2.5),
            head_length: 0.3,
            ..PlantAccent::NONE
        },
        ..PlantParams::MEADOW_GRASS
    };
    let plants = vec![
        ScatterPlant::new(plumed).scaled(0.5),
        ScatterPlant::new(PlantParams::CLOVER).scaled(0.2),
        ScatterPlant::new(plumed).scaled(0.3),
    ];
    spawn_with(&mut app, PlantScatter::new(Vec2::splat(32.0), plants));
    settle(&mut app);
    let tables = tables_now(&app);
    assert_eq!(tables.fields.len(), 5, "three plants, then two accents");
    let [first, clover, third, accent_1, accent_3] =
        std::array::from_fn(|index| tables.fields[index]);
    // Each plant stores a relative offset to its own accent entry.
    assert_eq!(first.accent, Vec4::new(0.25, 3.0, 0.0, 0.0));
    assert_eq!(clover.accent, Vec4::ZERO, "no accent");
    assert_eq!(third.accent, Vec4::new(0.25, 2.0, 0.0, 0.0));
    // The accent keeps the parent's tufting but uses its own size and head.
    // Plant selection must never choose the accent entry directly.
    for accent in [accent_1, accent_3] {
        assert_eq!((accent.shape.x, accent.shape.y), (2.0, 2.5));
        assert_eq!(accent.form.w, 0.3);
        assert_eq!((accent.tufts.x, accent.tufts.y), (0.8, 1.5));
        assert_eq!(accent.mix.x, 0.0);
        assert_eq!(accent.accent, Vec4::ZERO);
    }
    // Every entry repeats the plant count (excluding accents) and maximum height.
    for entry in tables.fields.iter() {
        assert_eq!(entry.mix.z, 3.0);
        assert_eq!(entry.mix.w, 2.5);
    }
    assert_eq!(tables.slots[0].entries, 5);
    assert!(tables.chunks.iter().all(|chunk| chunk.ids.x == 0));
}

#[test]
fn every_plant_of_a_hidden_field_is_hidden() {
    let mut app = app();
    app.world_mut().spawn((
        PlantScatter::from_preset(&PlantScatterPreset::SUMMER_WILD, Vec2::splat(32.0)),
        GlobalTransform::IDENTITY,
        Visibility::Hidden,
    ));
    app.update();
    let tables = app.world().resource::<FieldTables>();
    assert!(tables.fields.len() > 1);
    assert!(tables.fields.iter().all(|entry| entry.access.x == 0));
}

#[test]
fn a_field_is_as_big_as_its_size_times_its_scale() {
    let mut app = app();
    let field = spawn_field(&mut app, ());
    app.update();
    let tables = app.world().resource::<FieldTables>();
    assert_eq!(
        tables.fields[0].extent.truncate().truncate(),
        Vec2::splat(32.0)
    );
    assert_eq!(tables.chunks.len(), 4);

    // A scaled `Plane3d` mesh is bigger; so is a scaled field. It stays
    // centred on its translation, and Y scale does nothing.
    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() = GlobalTransform::from(
        Transform::from_xyz(10.0, 2.0, -6.0).with_scale(Vec3::new(2.0, 5.0, 0.5)),
    );
    app.update();
    let tables = app.world().resource::<FieldTables>();
    let entry = tables.fields[0];
    assert_eq!(entry.extent.truncate().truncate(), Vec2::new(64.0, 16.0));
    assert_eq!(
        entry.min_corner.truncate(),
        Vec3::new(10.0 - 32.0, 2.0, -6.0 - 8.0)
    );
    assert_eq!(tables.chunks.len(), 4, "four chunks across, one deep");
    assert!(tables.chunks.iter().all(|chunk| chunk.ids.z == 0));
}

#[test]
fn a_turn_about_y_is_followed_and_a_tilt_is_noticed() {
    let turned = |rotation: Quat| GlobalTransform::from(Transform::from_rotation(rotation));
    // Unturned is exactly X, so unturned fields are placed exactly as before.
    assert_eq!(field_axis(&GlobalTransform::IDENTITY), Vec2::X);
    assert_eq!(
        field_axis(&GlobalTransform::from(
            Transform::from_xyz(3.0, 1.0, 2.0).with_scale(Vec3::new(2.0, 1.0, 3.0))
        )),
        Vec2::X
    );
    // The field's X axis goes where the rotation takes X.
    for yaw in [0.3, -1.2, 2.5, std::f32::consts::PI] {
        let rotation = Quat::from_rotation_y(yaw);
        let expected = (rotation * Vec3::X).xz();
        assert!(field_axis(&turned(rotation)).abs_diff_eq(expected, 1e-6));
        assert!(!is_tilted(&turned(rotation)));
    }
    // A tilt is left out of the axis, and noticed.
    let tilted = Quat::from_rotation_y(0.4) * Quat::from_rotation_x(0.2);
    assert!(is_tilted(&turned(tilted)));
    assert!(
        field_axis(&turned(tilted)).abs_diff_eq((Quat::from_rotation_y(0.4) * Vec3::X).xz(), 1e-5)
    );
    assert!(!is_tilted(&GlobalTransform::IDENTITY));
    // Scale alone is not a tilt, even when it is not uniform.
    assert!(!is_tilted(&GlobalTransform::from(Transform::from_scale(
        Vec3::new(1.0, 4.0, 0.1)
    ))));

    let mut world = World::new();
    let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
    let mut seen = HashSet::new();
    assert!(first_time(&mut seen, a, true));
    assert!(!first_time(&mut seen, a, true), "once per field");
    assert!(first_time(&mut seen, b, true), "each field has its own");
    assert!(
        !first_time(&mut seen, a, false),
        "nothing wrong, nothing to say"
    );
    assert!(first_time(&mut seen, a, true), "broken again after a fix");
}
/// The version of the tables, now.
fn version(app: &App) -> u64 {
    app.world().resource::<FieldTables>().version
}

/// Moves the first field once, when run.
fn move_the_field(
    mut moved: Local<bool>,
    mut fields: Query<&mut GlobalTransform, With<PlantScatter>>,
) {
    if !*moved {
        *moved = true;
        for mut transform in &mut fields {
            *transform = GlobalTransform::from_xyz(7.0, 0.0, 0.0);
        }
    }
}

#[test]
fn a_system_ordered_before_the_plugins_sets_is_seen_the_same_frame() {
    let mut app = app();
    spawn_field(&mut app, ());
    settle(&mut app);
    let before = version(&app);
    app.add_systems(
        PostUpdate,
        move_the_field.before(PlantSystems::DetectChanges),
    );
    app.update();
    assert_eq!(
        version(&app),
        before + 1,
        "the move was in the tables in the frame it happened"
    );
    assert_eq!(
        app.world().resource::<FieldTables>().fields[0].min_corner.x,
        7.0 - 16.0
    );
}

#[test]
fn a_system_ordered_after_the_plugins_sets_is_seen_the_next_frame() {
    let mut app = app();
    spawn_field(&mut app, ());
    settle(&mut app);
    let before = version(&app);
    app.add_systems(PostUpdate, move_the_field.after(PlantSystems::Rebuild));
    app.update();
    assert_eq!(version(&app), before, "too late for this frame");
    app.update();
    assert_eq!(version(&app), before + 1, "and in the next");
}

#[test]
fn a_definition_edited_before_the_plugins_sets_is_rebuilt_the_same_frame() {
    fn thin_the_plants(mut fields: Query<&mut PlantScatter>) {
        for mut field in &mut fields {
            if field.options.density_scale != 0.5 {
                field.options.density_scale = 0.5;
            }
        }
    }

    let mut app = app();
    spawn_field(&mut app, ());
    settle(&mut app);
    assert_eq!(
        app.world().resource::<FieldTables>().fields[0].shape.w,
        60.0
    );
    app.add_systems(
        PostUpdate,
        thin_the_plants.before(PlantSystems::DetectChanges),
    );
    app.update();
    assert_eq!(
        app.world().resource::<FieldTables>().fields[0].shape.w,
        30.0,
        "an edit made before the plugin's sets must not wait a frame"
    );
}
fn stats(app: &App) -> TableStats {
    app.world().resource::<PlantStats>().tables
}

#[test]
fn rebuilds_are_counted_and_timed_and_showing_a_field_is_not_one() {
    let mut app = app();
    assert_eq!(stats(&app).rebuilds, 0);
    let field = spawn_field(&mut app, ());
    settle(&mut app);
    let built = stats(&app);
    assert!(built.rebuilds >= 1, "{built:?}");
    assert_eq!((built.fields, built.chunks), (1, 4));
    assert_eq!((built.fields_dropped, built.plants_dropped), (0, 0));
    assert!(built.last_rebuild > std::time::Duration::ZERO);

    // Moving the field is a rebuild; hiding it only patches the tables.
    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() =
        GlobalTransform::from_xyz(5.0, 0.0, 0.0);
    app.update();
    assert_eq!(stats(&app).rebuilds, built.rebuilds + 1);
    app.world_mut().entity_mut(field).insert(Visibility::Hidden);
    app.update();
    assert_eq!(
        stats(&app).rebuilds,
        built.rebuilds + 1,
        "hiding is not a rebuild"
    );
    assert_eq!(
        stats(&app).fields,
        1,
        "a hidden field is still in the tables"
    );
}

#[test]
fn the_limits_that_drop_content_are_visible_to_the_game() {
    // 256 entries: 260 one-plant fields leave the last four out.
    let mut app = app();
    for _ in 0..260 {
        spawn_field(&mut app, ());
    }
    settle(&mut app);
    let built = stats(&app);
    assert_eq!(built.fields, MAX_ENTRIES);
    assert_eq!(built.fields_dropped, 4);
    assert_eq!(
        app.world().resource::<FieldTables>().fields.len(),
        MAX_ENTRIES
    );

    // 16 plants per field: twenty asked for, sixteen drawn.
    let mut app = self::app();
    spawn_with(
        &mut app,
        PlantScatter::new(Vec2::splat(16.0), clover_weighing(&[0.05; 20])),
    );
    settle(&mut app);
    let built = stats(&app);
    assert_eq!(built.plants_dropped, 20 - MAX_PLANTS);
    assert_eq!(built.fields_dropped, 0);
    assert_eq!(tables_now(&app).fields.len(), MAX_PLANTS);

    // Supply 33 valid branch patterns and one plant with too many vertices per stem.
    // Only the first 32 valid patterns in sorted order should remain. Spread them
    // across fields to stay within each field's plant limit.
    let mut app = self::app();
    // The patterns [1, 0] through [32, 0], plus [1, 1], give 33 valid classes.
    // [1, 1] sorts second. Alternate leaflets preserve odd branch counts, keeping
    // every pattern distinct.
    let alternate = |leaflets| {
        let mut plant = branched(leaflets);
        plant.levels[0].arrangement = PlantArrangement::Alternate;
        plant
    };
    let mut kinds: Vec<PlantParams> = (1..=MAX_BRANCHED_CLASSES as u32).map(alternate).collect();
    let mut pair = alternate(1);
    pair.levels[1] = crate::PlantBranches {
        count: 1,
        arrangement: PlantArrangement::Alternate,
        length: 0.3,
        ..crate::PlantBranches::NONE
    };
    kinds.push(pair);
    let mut huge = branched(32);
    huge.levels[1] = crate::PlantBranches {
        count: 8,
        length: 0.3,
        ..crate::PlantBranches::NONE
    };
    kinds.push(huge);
    for group in kinds.chunks(MAX_PLANTS - 1) {
        let mut plants: Vec<ScatterPlant> = group
            .iter()
            .map(|&plant| ScatterPlant::new(plant).scaled(0.05))
            .collect();
        plants.push(ScatterPlant::new(PlantParams::MEADOW_GRASS).scaled(0.3));
        spawn_with(&mut app, PlantScatter::new(Vec2::splat(16.0), plants));
    }
    settle(&mut app);
    let tables = tables_now(&app);
    assert_eq!(tables.shape_classes.len(), MAX_BRANCHED_CLASSES);
    let last = MAX_BRANCHED_CLASSES as u32 - 1;
    assert_eq!(
        tables.shape_classes[MAX_BRANCHED_CLASSES - 1],
        ShapeClass {
            children: [last, 0]
        },
        "the kind sorted last is the one left out"
    );
    assert_eq!(stats(&app).branched_dropped, 2);
    let skipped = tables
        .fields
        .iter()
        .filter(|entry| entry.access.y == NO_CLASS)
        .count();
    assert_eq!(
        skipped, 2,
        "one kind more than are drawn, and too many vertices"
    );
}
// ---- incremental rebuilds -------------------------------------------

use bevy::asset::RenderAssetUsages;
use bevy::color::Color;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

fn mask_image(size: u32, value: u8) -> Image {
    Image::new_fill(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[value, 255, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    )
}

fn add_image(app: &mut App, size: u32, value: u8) -> Handle<Image> {
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .add(mask_image(size, value))
}

fn spawn_with(app: &mut App, field: PlantScatter) -> Entity {
    app.world_mut()
        .spawn((field, GlobalTransform::IDENTITY))
        .id()
}

fn tables_now(app: &App) -> FieldTables {
    app.world().resource::<FieldTables>().clone()
}

/// Edits a field's component; taking it mutably is what announces the change.
fn edit_field(app: &mut App, field: Entity, edit: impl FnOnce(&mut PlantScatter)) {
    let mut field = app
        .world_mut()
        .get_mut::<PlantScatter>(field)
        .expect("the field");
    edit(&mut field);
}

/// Sets the main (last) plant's tip colour.
fn set_tip_color(field: &mut PlantScatter, color: Srgb) {
    field
        .options
        .plants
        .to_mut()
        .last_mut()
        .expect("a plant")
        .plant
        .segment
        .tip_color = color;
}

/// A meadow field of 32 m with a mask and a heightmap.
fn rich_field(app: &mut App) -> (Entity, Handle<Image>, Handle<Image>) {
    let mask = add_image(app, 64, 200);
    let heightmap = add_image(app, 64, 128);
    let field = spawn_with(
        app,
        PlantScatter {
            size: Vec2::splat(32.0),
            density_map: Some(mask.clone()),
            ground: PlantGround::Heightmap {
                image: heightmap.clone(),
                height_scale: 5.0,
            },
            ..Default::default()
        },
    );
    (field, mask, heightmap)
}

#[test]
fn a_colour_only_field_change_is_a_uniform_update() {
    let mut app = app();
    let (field, ..) = rich_field(&mut app);
    settle(&mut app);
    let before = tables_now(&app);
    let at_start = stats(&app);
    assert_eq!(at_start.images_read, 2, "the mask and the heightmap, once");

    // Retune the colours every frame, as a day/night or season blend does.
    for step in 0..30 {
        let t = step as f32 / 30.0;
        edit_field(&mut app, field, |field| {
            set_tip_color(field, Srgb::new(0.2 + 0.6 * t, 0.6 - 0.3 * t, 0.1));
            field.options.ground_tint.color = Srgb::new(0.1 + 0.2 * t, 0.1, 0.05);
        });
        app.update();
    }
    let after = tables_now(&app);
    let now = stats(&app);

    assert_eq!(after.version, before.version + 30, "every change is shown");
    assert_eq!(
        after.structure_version, before.structure_version,
        "and none of them is a layout rebuild"
    );
    assert!(
        Arc::ptr_eq(&after.chunks, &before.chunks),
        "chunks are kept"
    );
    assert!(Arc::ptr_eq(&after.masks, &before.masks), "masks are kept");
    assert!(
        Arc::ptr_eq(&after.heights, &before.heights),
        "heights are kept"
    );
    assert_eq!(
        now.images_read, at_start.images_read,
        "no image was read again"
    );
    assert_eq!(now.layout_rebuilds, at_start.layout_rebuilds);
    assert_eq!(now.rebuilds, at_start.rebuilds + 30);
    assert_eq!(now.entries_rewritten, at_start.entries_rewritten + 30);

    // And the colour really is the last one.
    let t = 29.0 / 30.0;
    let last = linear(Srgb::new(0.2 + 0.6 * t, 0.6 - 0.3 * t, 0.1));
    assert_eq!(after.fields[0].tip_color.truncate(), last.truncate());
    assert_ne!(after.fields[0].tip_color, before.fields[0].tip_color);
}

#[test]
fn a_big_mask_is_read_once_however_often_the_field_changes() {
    let mut app = app();
    let mask = add_image(&mut app, 2048, 200);
    let field = spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::splat(64.0),
            density_map: Some(mask),
            ..Default::default()
        },
    );
    app.update();
    let first = stats(&app);
    assert_eq!(first.images_read, 1);
    let initial = first.last_rebuild;

    let mut slowest = std::time::Duration::ZERO;
    for step in 0..20 {
        edit_field(&mut app, field, |field| {
            set_tip_color(field, Srgb::new(0.1 + step as f32 * 0.04, 0.5, 0.1));
        });
        app.update();
        slowest = slowest.max(stats(&app).last_rebuild);
    }
    assert_eq!(stats(&app).images_read, 1, "4 million texels, read once");
    assert!(
        slowest * 4 < initial,
        "an update took {slowest:?} against {initial:?} for the first build, which read the mask"
    );
}

#[test]
fn only_the_image_that_changed_is_read_again() {
    let mut app = app();
    let mask_a = add_image(&mut app, 32, 50);
    let mask_b = add_image(&mut app, 32, 60);
    let heightmap_b = add_image(&mut app, 32, 128);
    spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::splat(16.0),
            density_map: Some(mask_a.clone()),
            ..Default::default()
        },
    );
    spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::splat(16.0),
            density_map: Some(mask_b),
            ground: PlantGround::Heightmap {
                image: heightmap_b,
                height_scale: 3.0,
            },
            ..Default::default()
        },
    );
    settle(&mut app);
    let before = tables_now(&app);
    let at_start = stats(&app);
    assert_eq!(at_start.images_read, 3);

    // Paint one texel of the first mask.
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&mask_a)
        .unwrap()
        .set_color_at(0, 0, Color::linear_rgb(1.0, 1.0, 0.0))
        .unwrap();
    app.update();
    let after = tables_now(&app);
    let now = stats(&app);

    assert_eq!(now.images_read, at_start.images_read + 1, "only that image");
    assert!(
        Arc::ptr_eq(&after.heights, &before.heights),
        "heights untouched"
    );
    assert!(
        Arc::ptr_eq(&after.chunks, &before.chunks),
        "chunks untouched"
    );
    assert!(!Arc::ptr_eq(&after.masks, &before.masks));
    assert_eq!(
        after.structure_version,
        before.structure_version + 1,
        "the masks upload"
    );
    // The painted texel is in the table, and nothing else moved. (Fields
    // are in the tables in entity order, which is not spawn order, so look
    // for what changed rather than where.)
    let differing = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(a, b)| a != b).count();
    assert_eq!(before.masks.len(), after.masks.len());
    assert_eq!(
        differing(&before.masks, &after.masks),
        1,
        "exactly the painted word changed"
    );
    assert_eq!(
        now.entries_rewritten,
        at_start.entries_rewritten + 1,
        "its field only"
    );
    let changed_entries = before
        .fields
        .iter()
        .zip(after.fields.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        changed_entries, 0,
        "the mask kept its size and place, so no entry differs"
    );
}

#[test]
fn only_the_entries_of_the_edited_field_are_rewritten() {
    let mut app = app();
    let one = |app: &mut App, preset: &PlantScatterPreset| {
        spawn_with(app, PlantScatter::from_preset(preset, Vec2::splat(16.0)))
    };
    let plain = one(&mut app, &PlantScatterPreset::MEADOW);
    let wild = one(&mut app, &PlantScatterPreset::SUMMER_WILD);
    one(&mut app, &PlantScatterPreset::MEADOW);
    settle(&mut app);
    let before = tables_now(&app);
    let start = stats(&app).entries_rewritten;
    let wild_entries = PlantScatterPreset::SUMMER_WILD.plant_scatter.plants.len() as u64;
    assert_eq!(before.fields.len() as u64, 2 + wild_entries);

    edit_field(&mut app, plain, |field| {
        field.options.plants.to_mut()[0].plant.segment.stiffness = 0.9;
    });
    app.update();
    let after = tables_now(&app);
    assert_eq!(
        stats(&app).entries_rewritten,
        start + 1,
        "the edited plain field, and not its twin"
    );
    let changed = |a: &FieldTables, b: &FieldTables| {
        a.fields
            .iter()
            .zip(b.fields.iter())
            .filter(|(a, b)| a != b)
            .count()
    };
    assert_eq!(changed(&after, &before), 1, "and only its entry differs");

    edit_field(&mut app, wild, |field| {
        field.options.plants.to_mut()[0].plant.segment.stiffness = 0.9;
    });
    app.update();
    assert_eq!(
        stats(&app).entries_rewritten,
        start + 1 + wild_entries,
        "the wild field and each of its plants"
    );
}

#[test]
fn a_field_touched_without_a_change_uploads_nothing() {
    let mut app = app();
    let field = spawn_field(&mut app, ());
    settle(&mut app);
    let (tables, counted) = (tables_now(&app), stats(&app));

    // Touching the field without changing it, or putting the same field
    // back: the entries are worked out again, find themselves the same, and
    // nothing is uploaded.
    edit_field(&mut app, field, |_| ());
    app.update();
    assert_eq!(version(&app), tables.version);
    assert_eq!(stats(&app).rebuilds, counted.rebuilds);
    assert_eq!(stats(&app).entries_rewritten, counted.entries_rewritten);
    app.world_mut()
        .entity_mut(field)
        .insert(PlantScatter::from_preset(
            &PlantScatterPreset::MEADOW,
            Vec2::splat(32.0),
        ));
    app.update();
    assert_eq!(version(&app), tables.version);
    assert_eq!(stats(&app).rebuilds, counted.rebuilds);
    assert_eq!(stats(&app).entries_rewritten, counted.entries_rewritten);
}

#[test]
fn moving_a_field_reads_no_image_and_a_sideways_move_uploads_no_layout() {
    let mut app = app();
    let (field, ..) = rich_field(&mut app);
    settle(&mut app);
    let before = tables_now(&app);
    let reads = stats(&app).images_read;

    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() =
        GlobalTransform::from_xyz(40.0, 0.0, -10.0);
    app.update();
    let sideways = tables_now(&app);
    assert_eq!(stats(&app).images_read, reads);
    assert_eq!(sideways.version, before.version + 1);
    assert_eq!(
        sideways.structure_version, before.structure_version,
        "chunk bounds are relative to the field, so a sideways move changes only its entries"
    );
    assert!(Arc::ptr_eq(&sideways.masks, &before.masks));
    assert_eq!(sideways.fields[0].min_corner.x, 40.0 - 16.0);

    // Up is different: the chunks hold the ground's height.
    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() =
        GlobalTransform::from_xyz(40.0, 3.0, -10.0);
    app.update();
    let up = tables_now(&app);
    assert_eq!(up.structure_version, sideways.structure_version + 1);
    assert_eq!(stats(&app).images_read, reads, "still no image read");
    assert!(Arc::ptr_eq(&up.masks, &before.masks), "and the masks stay");
    assert!(up.chunks[0].bounds.z > sideways.chunks[0].bounds.z);
}

#[test]
fn a_fields_wind_and_density_are_its_own() {
    let mut app = app();
    let (plain, changed) = (spawn_field(&mut app, ()), spawn_field(&mut app, ()));
    settle(&mut app);
    let entry_of = |app: &App, entity: Entity| {
        let tables = tables_now(app);
        let slot = tables.slots.iter().find(|s| s.entity == entity).unwrap();
        tables.fields[slot.first]
    };
    let before = entry_of(&app, changed);
    assert_eq!(before.shape, entry_of(&app, plain).shape);

    let meadow = PlantScatterPreset::MEADOW.plant_scatter.density();
    edit_field(&mut app, changed, |field| {
        field.options.wind_scale = 0.25;
        field.options.density_scale = 0.5;
    });
    app.update();
    let now = entry_of(&app, changed);
    assert_eq!(before.motion.z, 0.0, "the full wind");
    assert_eq!(now.motion.z, 0.75, "a quarter of the wind");
    assert_eq!(now.shape.w, meadow * 0.5);
    assert_eq!(
        (now.shape.x, now.shape.y, now.shape.z),
        (before.shape.x, before.shape.y, before.shape.z),
        "the plants keep their size"
    );
    assert_eq!(now.root_color, before.root_color);
    assert_eq!(now.tip_color, before.tip_color);
    assert_eq!(
        now.placement.y,
        ChunkLayout::for_density(CHUNK_SIZE, meadow * 0.5).tiles_per_side,
        "fewer blades need fewer tiles"
    );
    assert_eq!(entry_of(&app, plain), before, "the other field");

    // Putting the settings back puts the field back as it was.
    edit_field(&mut app, changed, |field| {
        field.options.wind_scale = 1.0;
        field.options.density_scale = 1.0;
    });
    app.update();
    assert_eq!(entry_of(&app, changed), before);

    // A wind backwards is clamped to no wind.
    edit_field(&mut app, changed, |field| field.options.wind_scale = -2.0);
    app.update();
    assert_eq!(
        entry_of(&app, changed).motion.z,
        1.0,
        "no wind, not a wind backwards"
    );
}

#[test]
fn the_highest_ground_under_a_point_is_found_in_the_right_chunk() {
    let mut app = app();
    let field = |app: &mut App, size: Vec2, x: f32, y: f32| {
        let id = spawn_with(
            app,
            PlantScatter {
                size,
                ..Default::default()
            },
        );
        *app.world_mut().get_mut::<GlobalTransform>(id).unwrap() =
            GlobalTransform::from_xyz(x, y, 0.0);
        id
    };
    // 40 × 20 m (3 × 2 chunks) at height 3, and 16 × 16 m at height 7.
    field(&mut app, Vec2::new(40.0, 20.0), 0.0, 3.0);
    field(&mut app, Vec2::splat(16.0), 100.0, 7.0);
    settle(&mut app);
    let tables = tables_now(&app);
    let at = |x: f32, z: f32| tables.highest_ground_at(Vec2::new(x, z));
    // Which field is first in the tables depends on the entities; either
    // way each point finds its own field's height.
    assert_eq!(at(0.0, 0.0), Some(3.0));
    assert_eq!(
        at(19.9, 9.9),
        Some(3.0),
        "the far corner of the first field"
    );
    assert_eq!(at(-19.9, -9.9), Some(3.0), "and the near one");
    assert_eq!(at(100.0, 0.0), Some(7.0));
    assert_eq!(at(107.9, -7.9), Some(7.0));
    assert_eq!(at(20.1, 0.0), None, "beside the first field");
    assert_eq!(at(0.0, 10.1), None, "past its edge");
    assert_eq!(at(0.0, 50.0), None);
}

/// The tables worked out from nothing, for what the app holds now.
fn from_scratch(app: &mut App) -> FieldTables {
    let kept = tables_now(app);
    *app.world_mut().resource_mut::<FieldTables>() = FieldTables::default();
    *app.world_mut().resource_mut::<PackedImages>() = PackedImages::default();
    app.world_mut().resource_mut::<TablesDirty>().fields = true;
    app.update();
    let fresh = tables_now(app);
    // Carry on from where the incremental tables were, as the caller's
    // next step must continue from them.
    *app.world_mut().resource_mut::<FieldTables>() = kept;
    fresh
}

fn same_tables(incremental: &FieldTables, fresh: &FieldTables, step: &str) {
    assert_eq!(incremental.fields, fresh.fields, "{step}: fields");
    assert_eq!(incremental.chunks, fresh.chunks, "{step}: chunks");
    assert_eq!(incremental.masks, fresh.masks, "{step}: masks");
    assert_eq!(incremental.heights, fresh.heights, "{step}: heights");
    assert_eq!(incremental.slots, fresh.slots, "{step}: slots");
    assert_eq!(incremental.mask_ids, fresh.mask_ids, "{step}: mask ids");
    assert_eq!(
        incremental.height_ids, fresh.height_ids,
        "{step}: height ids"
    );
    assert_eq!(incremental.features, fresh.features, "{step}: features");
    assert_eq!(
        incremental.shape_classes, fresh.shape_classes,
        "{step}: shape classes"
    );
}

/// Adds the requested number of leaflets to a meadow plant.
fn branched(leaflets: u32) -> PlantParams {
    PlantParams {
        stems_per_plant: 3,
        levels: [
            crate::PlantBranches {
                count: leaflets,
                length: 0.4,
                width: 0.2,
                ..crate::PlantBranches::NONE
            },
            crate::PlantBranches::NONE,
        ],
        ..PlantParams::MEADOW_GRASS
    }
}

/// A 16 m field growing `plants`.
fn small_field(app: &mut App, plants: Vec<ScatterPlant>) -> Entity {
    spawn_with(app, PlantScatter::new(Vec2::splat(16.0), plants))
}

#[test]
fn branched_plants_get_shape_classes_and_reach_past_their_height() {
    let mut app = app();
    // Combine a field containing 10-leaflet ferns with a field whose plants have 4
    // branches carrying 3 leaves each.
    let mut twigs = branched(4);
    twigs.levels[1] = crate::PlantBranches {
        count: 3,
        length: 0.5,
        ..crate::PlantBranches::NONE
    };
    let mixed = small_field(
        &mut app,
        vec![
            ScatterPlant::new(branched(10)).scaled(0.2),
            ScatterPlant::new(PlantParams::MEADOW_GRASS).scaled(0.8),
        ],
    );
    let bushy = small_field(&mut app, vec![ScatterPlant::new(twigs)]);
    settle(&mut app);
    let tables = tables_now(&app);
    let first_of = |field: Entity| {
        tables
            .slots
            .iter()
            .find(|slot| slot.entity == field)
            .expect("the field is in the tables")
            .first
    };
    let (mixed, bushy) = (first_of(mixed), first_of(bushy));
    assert_eq!(
        *tables.shape_classes,
        vec![
            ShapeClass { children: [4, 4] },
            ShapeClass { children: [10, 0] }
        ],
        "sorted; opposite pairs round 3 up to 4"
    );
    let [fern, meadow, bush] = [
        tables.fields[mixed],
        tables.fields[mixed + 1],
        tables.fields[bushy],
    ];
    assert_eq!(
        [meadow.access.y, fern.access.y, bush.access.y],
        [0, 2, 1],
        "the blades are class 0"
    );
    // This mixed field must select a plant before applying its distance thinning.
    assert_eq!([meadow.access.z, fern.access.z, bush.access.z], [1, 1, 1]);
    assert_eq!([meadow.access.w, fern.access.w], [1, 3], "stems per plant");
    // The branch bound adds 40% to the stem height for culling. The field's maximum
    // stem height remains unchanged.
    let fern_height = PlantParams::MEADOW_GRASS.height.y;
    assert_eq!(meadow.mix.w, fern_height);
    assert!((meadow.min_corner.w - fern_height * 1.4).abs() < 1e-6);
    assert!((bush.min_corner.w - fern_height * (1.0 + 0.4 * 1.5)).abs() < 1e-6);

    // Without branches, the reach bound equals the maximum stem height.
    let mut app = self::app();
    small_field(
        &mut app,
        PlantScatterPreset::SUMMER_WILD
            .plant_scatter
            .plants
            .to_vec(),
    );
    settle(&mut app);
    let tables = tables_now(&app);
    assert!(tables.shape_classes.is_empty());
    for entry in tables.fields.iter() {
        assert_eq!(entry.min_corner.w, entry.mix.w);
        assert_eq!(entry.access.y, 0);
        assert_eq!(entry.access.z, 0);
        assert_eq!(entry.levels, [LevelGpu::default(); 2]);
    }
}

#[test]
fn a_second_level_on_a_level_that_is_off_grows_nothing() {
    let mut plant = branched(0);
    plant.levels[1] = crate::PlantBranches {
        count: 6,
        length: 0.5,
        ..crate::PlantBranches::NONE
    };
    let mut app = app();
    small_field(&mut app, vec![ScatterPlant::new(plant)]);
    settle(&mut app);
    let tables = tables_now(&app);
    assert!(tables.shape_classes.is_empty(), "it is a plain blade");
    assert_eq!(tables.fields[0].levels, [LevelGpu::default(); 2]);
}

#[test]
fn building_incrementally_gives_exactly_what_building_from_scratch_gives() {
    // Every kind of change, in a world with several fields, masks,
    // heightmaps and species; after each the incremental tables must equal
    // those worked out from nothing. A stale cache would show here.
    let mut app = app();
    let (rich, mask, heightmap) = rich_field(&mut app);
    let small_mask = add_image(&mut app, 16, 90);
    let second = spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::new(48.0, 20.0),
            density_map: Some(small_mask.clone()),
            ..PlantScatter::from_preset(&PlantScatterPreset::SUMMER_WILD, Vec2::ZERO)
        },
    );
    settle(&mut app);
    let check = |app: &mut App, step: &str| {
        app.update();
        app.update();
        let fresh = from_scratch(app);
        same_tables(&tables_now(app), &fresh, step);
    };
    check(&mut app, "start");

    edit_field(&mut app, rich, |field| {
        set_tip_color(field, Srgb::new(0.9, 0.5, 0.1));
    });
    check(&mut app, "colour");
    edit_field(&mut app, rich, |field| field.options.density_scale = 0.5);
    check(&mut app, "density, which changes the tile layout");
    edit_field(&mut app, second, |field| {
        let last = field.options.plants.len() - 1;
        field.options.plants.to_mut().drain(..last);
    });
    check(&mut app, "plants removed: entries shift");
    edit_field(&mut app, second, |field| {
        field.options.plants = PlantScatterPreset::AUTUMN_WILD.plant_scatter.plants;
    });
    check(&mut app, "plants added: entries shift back");
    edit_field(&mut app, second, |field| {
        field.options.plants.to_mut()[0].density_scale = 0.0;
    });
    check(&mut app, "a density scale changed: the chances change");
    edit_field(&mut app, second, |field| {
        for entry in field.options.plants.to_mut() {
            entry.density_scale = 0.0;
        }
    });
    check(&mut app, "nothing left to grow: the field goes");
    edit_field(&mut app, second, |field| {
        field.options.plants = PlantScatterPreset::SUMMER_WILD.plant_scatter.plants;
    });
    check(&mut app, "and comes back");

    *app.world_mut().get_mut::<GlobalTransform>(second).unwrap() =
        GlobalTransform::from_xyz(-30.0, 1.0, 5.0);
    check(&mut app, "moved");
    app.world_mut().get_mut::<PlantScatter>(rich).unwrap().size = Vec2::new(64.0, 16.0);
    check(&mut app, "resized");

    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&mask)
        .unwrap()
        .set_color_at(3, 3, Color::linear_rgb(0.0, 1.0, 0.0))
        .unwrap();
    check(&mut app, "mask painted");
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&heightmap)
        .unwrap()
        .set_color_at(5, 5, Color::linear_rgb(1.0, 0.0, 0.0))
        .unwrap();
    check(&mut app, "heightmap painted");

    // A different mask on a field, with a different size.
    let bigger = add_image(&mut app, 128, 30);
    app.world_mut()
        .get_mut::<PlantScatter>(second)
        .unwrap()
        .density_map = Some(bigger);
    check(&mut app, "mask swapped for a bigger one");
    app.world_mut()
        .get_mut::<PlantScatter>(second)
        .unwrap()
        .density_map = None;
    check(&mut app, "mask removed");

    let third = spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::splat(24.0),
            density_map: Some(small_mask),
            ..Default::default()
        },
    );
    check(&mut app, "a field added, sharing a mask");
    edit_field(&mut app, second, |field| {
        field.options.wind_scale = 0.5;
        field.options.ground_tint.amount = 0.6;
    });
    check(&mut app, "wind and ground tint changed");
    app.world_mut().entity_mut(rich).despawn();
    check(&mut app, "a field removed");
    app.world_mut().entity_mut(third).insert(Visibility::Hidden);
    app.update();
    let fresh = from_scratch(&mut app);
    same_tables(&tables_now(&app), &fresh, "hidden");
}

/// More steps of `building_incrementally_gives_exactly_what_building_from_scratch_gives`:
/// a field turned, and a field on a mesh.
#[test]
fn branched_plants_build_incrementally_exactly_as_from_scratch() {
    // Changing one field can renumber mesh classes across the table. Retained entries
    // must receive the new numbers too.
    let mut app = app();
    let [plain, wild] =
        [PlantScatterPreset::MEADOW, PlantScatterPreset::SUMMER_WILD].map(|preset| {
            spawn_with(
                &mut app,
                PlantScatter::from_preset(&preset, Vec2::new(40.0, 24.0)),
            )
        });
    settle(&mut app);
    let check = |app: &mut App, step: &str| {
        app.update();
        app.update();
        let fresh = from_scratch(app);
        same_tables(&tables_now(app), &fresh, step);
    };
    edit_field(&mut app, wild, |field| {
        field
            .options
            .plants
            .to_mut()
            .insert(0, ScatterPlant::new(branched(10)).scaled(0.1));
    });
    check(&mut app, "a branched plant: a shape class appears");
    assert_eq!(tables_now(&app).shape_classes.len(), 1);
    edit_field(&mut app, plain, |field| {
        field.options.plants.to_mut()[0].plant = branched(4);
    });
    check(
        &mut app,
        "another field branches: a second class, sorted first",
    );
    assert_eq!(tables_now(&app).shape_classes.len(), 2);
    edit_field(&mut app, wild, |field| {
        field.options.plants.to_mut().remove(0);
    });
    check(&mut app, "the first class goes: the other is renumbered");
    assert_eq!(
        *tables_now(&app).shape_classes,
        vec![ShapeClass { children: [4, 0] }]
    );
}

#[test]
fn turned_and_mesh_fields_build_incrementally_exactly_as_from_scratch() {
    let mut app = app();
    let (rich, ..) = rich_field(&mut app);
    settle(&mut app);
    let check = |app: &mut App, step: &str| {
        app.update();
        app.update();
        let fresh = from_scratch(app);
        same_tables(&tables_now(app), &fresh, step);
    };
    check(&mut app, "start");
    *app.world_mut().get_mut::<GlobalTransform>(rich).unwrap() = GlobalTransform::from(
        Transform::from_xyz(-30.0, 1.0, 5.0).with_rotation(Quat::from_rotation_y(0.7)),
    );
    check(&mut app, "turned");
    let hill = app
        .world_mut()
        .resource_mut::<Assets<bevy::mesh::Mesh>>()
        .add(crate::ground::tests::slope(Vec2::splat(20.0), 3.0));
    let on_mesh = spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::splat(20.0),
            ground: PlantGround::mesh(hill.clone()),
            ..Default::default()
        },
    );
    check(&mut app, "a field on a mesh added");
    app.world_mut()
        .resource_mut::<Assets<bevy::mesh::Mesh>>()
        .insert(&hill, crate::ground::tests::slope(Vec2::splat(20.0), 6.0))
        .unwrap();
    check(&mut app, "its mesh changed");
    app.world_mut()
        .get_mut::<PlantScatter>(on_mesh)
        .unwrap()
        .ground = PlantGround::Flat;
    check(&mut app, "the mesh ground taken off");
}

#[test]
fn a_turned_field_holds_the_points_of_its_turned_rectangle() {
    let mut app = app();
    // 40 by 4 m, turned a quarter turn: it lies along Z.
    let field = spawn_with(
        &mut app,
        PlantScatter {
            size: Vec2::new(40.0, 4.0),
            ..Default::default()
        },
    );
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    *app.world_mut().get_mut::<GlobalTransform>(field).unwrap() =
        GlobalTransform::from(Transform::from_xyz(5.0, 2.0, 0.0).with_rotation(turn));
    settle(&mut app);
    let tables = tables_now(&app);
    let entry = tables.fields[0];

    // The minimum corner is the field's own −X, −Z corner, where it lies now.
    let corner = Vec3::new(5.0, 2.0, 0.0) + turn * Vec3::new(-20.0, 0.0, -2.0);
    assert!(entry.min_corner.truncate().abs_diff_eq(corner, 1e-4));
    assert!(entry.x_axis().abs_diff_eq((turn * Vec3::X).xz(), 1e-6));
    let centre = entry.world_of(Vec2::new(20.0, 2.0));
    assert!(centre.abs_diff_eq(Vec2::new(5.0, 0.0), 1e-4));
    let local = Vec2::new(7.0, 3.0);
    assert!(
        entry
            .local_of(entry.world_of(local))
            .abs_diff_eq(local, 1e-4)
    );

    let at = |x: f32, z: f32| tables.highest_ground_at(Vec2::new(x, z));
    assert_eq!(at(5.0, 19.0), Some(2.0), "along Z, where the turn put it");
    assert_eq!(at(5.0, -19.0), Some(2.0));
    assert_eq!(at(6.9, 0.0), Some(2.0));
    assert_eq!(at(20.0, 0.0), None, "where it would lie unturned");
    assert_eq!(at(7.1, 0.0), None);
}

/// The field change that turns one optional look term on.
type TurnOn = fn(&mut PlantScatter, &mut PlantParams);

/// One example setting for each optional shader feature in `LookFeatures::TERMS`.
/// The tests below check that each setting enables its feature.
const TERM_ENABLERS: [(LookFeatures, TurnOn); 5] = [
    (LookFeatures::GROUND_TINT, |field, _| {
        field.options.ground_tint.amount = 0.5;
    }),
    (LookFeatures::SHEEN, |_, plant| plant.specular = 0.5),
    (LookFeatures::TOON, |field, _| field.options.toon_bands = 3),
    (LookFeatures::SHAPE, |_, plant| plant.segment.twist = 0.5),
    (LookFeatures::VARIATION, |field, _| {
        field.options.variation.patch_amount = 0.5;
    }),
];

/// A plant that uses no optional look term, whatever the presets do.
fn plain_plant() -> PlantParams {
    PlantParams {
        specular: 0.0,
        head_length: 0.0,
        segment: crate::PlantSegment {
            twist: 0.0,
            droop: 0.0,
            fold: 0.0,
            ..PlantParams::MEADOW_GRASS.segment
        },
        ..PlantParams::MEADOW_GRASS
    }
}

/// A field that uses no optional look term, whatever the presets do.
fn plain_field() -> PlantScatter {
    let options = PlantScatterOptions {
        toon_bands: 0,
        ground_tint: GroundTint {
            amount: 0.0,
            ..PlantScatterPreset::MEADOW.plant_scatter.ground_tint
        },
        variation: PlantVariation {
            patch_amount: 0.0,
            dry_tips: 0.0,
            dead_share: 0.0,
            ..PlantScatterPreset::MEADOW.plant_scatter.variation
        },
        ..PlantScatterOptions::default().with_plants(vec![ScatterPlant::new(plain_plant())])
    };
    PlantScatter::with_options(Vec2::ZERO, options)
}

/// Every `PLANT_...` name in a shader source, comments included.
fn plant_definitions(source: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("PLANT_") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(tail.len());
        names.push(&tail[..end]);
        rest = &tail[end..];
    }
    names
}

#[test]
fn every_look_term_has_a_definition_a_shader_and_a_setting_that_asks_for_it() {
    // A look term is declared in four places: its bit, the shader definition
    // that compiles it in, the shaders that read that definition, and the
    // style setting that turns it on. Three of those are silent if they are
    // missed (the shader draws a field that quietly looks plain, or a setting
    // does nothing), so they are checked here against the one list,
    // `LookFeatures::TERMS`.
    let definition = |term: LookFeatures| {
        LookFeatures::TERMS
            .iter()
            .find(|(known, _)| *known == term)
            .map_or("(a definition no shader has)", |(_, def)| *def)
    };
    let terms: Vec<&str> = LookFeatures::TERMS
        .iter()
        .map(|(term, _)| definition(*term))
        .collect();
    let enabled: Vec<&str> = TERM_ENABLERS
        .iter()
        .map(|(term, _)| definition(*term))
        .collect();
    assert_eq!(
        terms, enabled,
        "every term needs a row in TERM_ENABLERS that turns it on, and no row may be left over"
    );

    // A field that uses no term asks for none, and every field that uses one
    // asks for it: `LookFeatures::of` covers each term.
    assert_eq!(
        LookFeatures::of(&plain_field(), &plain_plant()),
        LookFeatures::default(),
        "a field that uses no optional term must not compile any of them in"
    );
    for (term, enable) in TERM_ENABLERS {
        let (mut field, mut plant) = (plain_field(), plain_plant());
        enable(&mut field, &mut plant);
        let features = LookFeatures::of(&field, &plant);
        assert!(
            features.contains(term),
            "{} is used by the field but not asked for, so it would draw as a no-op",
            definition(term)
        );
    }

    // Every definition is named by a shader, and every name a shader uses is
    // one of the terms: a definition no shader reads compiles nothing in, and
    // a name no term emits is a branch that can never be turned on.
    let sources = [
        include_str!("../../shaders/blade.wesl"),
        include_str!("../../shaders/common.wesl"),
        include_str!("../../shaders/plant_main.wesl"),
        include_str!("../../shaders/plant_prepass.wesl"),
        include_str!("../../shaders/look.wesl"),
    ];
    let mut definitions: Vec<&str> = LookFeatures::TERMS.iter().map(|(_, def)| *def).collect();
    for definition in &definitions {
        assert!(
            sources.iter().any(|source| source.contains(definition)),
            "{definition} is compiled in by no shader"
        );
    }
    let mut named: Vec<&str> = sources
        .iter()
        .flat_map(|source| plant_definitions(source))
        .collect();
    named.sort_unstable();
    named.dedup();
    definitions.sort_unstable();
    assert_eq!(
        named, definitions,
        "a shader names a PLANT_ definition that no look term emits, or a term has no shader"
    );
}

#[test]
fn a_field_on_a_ground_from_a_function_stands_on_it() {
    let mut app = app();
    let world = app.world_mut();
    let field = PlantScatter {
        size: Vec2::splat(64.0),
        ..Default::default()
    }
    .with_ground_from_fn(&mut world.resource_mut::<Assets<Image>>(), 0.5, |point| {
        if point.x < 0.0 { 1.0 } else { 7.0 }
    });
    world.spawn((field, GlobalTransform::from_xyz(0.0, 2.0, 0.0)));
    settle(&mut app);
    let tables = app.world().resource::<FieldTables>();
    // Ground heights are metres above the field's own height. (A chunk's
    // range reaches a texel into its neighbours, so the chunks beside the
    // step at x = 0 both reach 9.)
    assert_eq!(tables.highest_ground_at(Vec2::new(-24.0, 0.0)), Some(3.0));
    assert_eq!(tables.highest_ground_at(Vec2::new(24.0, 0.0)), Some(9.0));
}

#[test]
fn heights_draw_the_same_in_either_order() {
    // Sort the height limits before passing them to the shaders, which draw
    // the plants, so reversing the limits does not change their appearance.
    let field = PlantScatterPreset::SUMMER_TALL.plant_scatter;
    let plant = PlantParams::SUMMER_TALL_GRASS;
    let swapped = PlantParams {
        height: Vec2::new(plant.height.y, plant.height.x),
        ..plant
    };
    assert_ne!(swapped.height, plant.height);
    let entry = FieldGpu::default();
    let drawn = |plant: &PlantParams| look_entry(&entry, &field, plant, Vec4::ZERO);
    assert_eq!(drawn(&swapped), drawn(&plant));
}

#[test]
fn a_seed_moves_the_blades_and_seed_0_keeps_where_they_were() {
    let mut app = app();
    let field = spawn_field(&mut app, ());
    settle(&mut app);
    let seed_of = |app: &App| tables_now(app).fields[0].placement.x;
    let corner = tables_now(&app).fields[0].min_corner;
    let by_position = shader_model::hash2(corner.x.to_bits(), corner.z.to_bits());
    assert_eq!(seed_of(&app), by_position, "seed 0 is the corner's seed");

    // The field's own seed is mixed in.
    edit_field(&mut app, field, |field| field.seed = Some(5));
    app.update();
    assert_eq!(seed_of(&app), by_position ^ 5);

    // Without one, the options' default seed is used.
    edit_field(&mut app, field, |field| {
        field.seed = None;
        field.options.default_seed = 7;
    });
    app.update();
    assert_eq!(seed_of(&app), by_position ^ 7);
}
