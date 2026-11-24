//! Every optional look term compiled into one grass pipeline, on a real GPU.
//!
//! The unit tests next to `LookFeatures` check that each term has a
//! definition and a setting that asks for it. They cannot check that the
//! branches those definitions guard compile: a term is only compiled into a
//! pipeline when some field uses it, so a definition no field enables is
//! never seen by the shader compiler at all, and a mistake inside its `@if`
//! would ship unnoticed. This draws one field that uses every term at
//! once, which compiles them all in, and fails if any grass pipeline does.

use bevy::render::render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor};

use super::*;
use crate::scatters::{FieldTables, LookFeatures};
use crate::{
    GroundTint, PlantAccent, PlantParams, PlantScatterOptions, PlantSegment, PlantVariation,
};

/// A plant enabling every optional plant appearance feature, including an accent.
fn every_term_plant() -> PlantParams {
    PlantParams {
        specular: 0.5,
        head_length: 0.3,
        head_width: 0.03,
        tufts: 0.5,
        segment: PlantSegment {
            twist: 0.5,
            droop: 0.5,
            fold: 0.5,
            ..PlantParams::MEADOW_GRASS.segment
        },
        accent: PlantAccent {
            share: 0.2,
            head_length: 0.2,
            ..PlantAccent::NONE
        },
        ..PlantParams::MEADOW_GRASS
    }
}

/// A field enabling every optional field appearance feature for the given plants.
fn every_term(plants: Vec<ScatterPlant>) -> PlantScatter {
    let meadow = PlantScatterPreset::MEADOW.plant_scatter;
    let options = PlantScatterOptions {
        toon_bands: 3,
        ground_tint: GroundTint {
            amount: 0.3,
            ..meadow.ground_tint
        },
        variation: PlantVariation {
            patch_amount: 0.5,
            dry_tips: 0.5,
            dead_share: 0.1,
            ..meadow.variation
        },
        ..PlantScatterOptions::default().with_plants(plants)
    };
    PlantScatter::with_options(Vec2::ZERO, options)
}

/// How many grass pipelines are compiled, panicking on any that failed: a
/// shader that does not compile draws no grass, logs an error and otherwise
/// looks like a fast frame.
fn compiled_plant_pipelines(app: &mut App) -> usize {
    let world = app.sub_app_mut(RenderApp).world();
    let cache = world.resource::<PipelineCache>();
    let mut compiled = 0;
    for pipeline in cache.pipelines() {
        let PipelineDescriptor::RenderPipelineDescriptor(descriptor) = &pipeline.descriptor else {
            continue;
        };
        let label = descriptor.label.as_deref().unwrap_or_default();
        if !label.starts_with("plant") {
            continue;
        }
        match &pipeline.state {
            CachedPipelineState::Err(error) => {
                panic!("{label} failed to compile with every look term on: {error}");
            }
            CachedPipelineState::Ok(_) => compiled += 1,
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => {}
        }
    }
    compiled
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_a_field_using_every_look_term_compiles_and_draws() {
    // Enable all appearance features together to exercise their shader combinations.
    let mut app = app_with_field(every_term(vec![ScatterPlant::new(every_term_plant())]));

    // The tables agree the style asks for every term, so the pipelines below
    // are built with all of them compiled in; then every branch is real.
    app.update();
    let features = app.world().resource::<FieldTables>().features;
    for (term, definition) in LookFeatures::TERMS {
        assert!(
            features.contains(term),
            "{definition} is not asked for, so its shader branch is not compiled at all"
        );
    }

    // Give the pipelines time to compile, which with the other GPU tests
    // running at once can take a few hundred frames; none may fail.
    let mut compiled = 0;
    for _ in 0..600 {
        app.update();
        compiled = compiled_plant_pipelines(&mut app);
        if compiled > 0 && blades_after(&mut app, 1) > 0 {
            break;
        }
    }
    assert!(
        compiled > 0,
        "no grass pipeline compiled, so nothing was checked"
    );
    assert!(
        blades_after(&mut app, 4) > 0,
        "the pass should place blades with every look term on"
    );
}

/// Pipeline labels for each rendering pass that supports branched plants.
fn compiled_branched_pipelines(app: &mut App) -> Vec<String> {
    let world = app.sub_app_mut(RenderApp).world();
    let cache = world.resource::<PipelineCache>();
    let mut labels = Vec::new();
    for pipeline in cache.pipelines() {
        let PipelineDescriptor::RenderPipelineDescriptor(descriptor) = &pipeline.descriptor else {
            continue;
        };
        let label = descriptor.label.as_deref().unwrap_or_default();
        if !label.starts_with("plant") {
            continue;
        }
        match &pipeline.state {
            CachedPipelineState::Err(error) => panic!("{label} failed to compile: {error}"),
            CachedPipelineState::Ok(_) if label.ends_with("branched") => {
                labels.push(label.to_owned());
            }
            _ => {}
        }
    }
    labels.sort();
    labels.dedup();
    labels
}

/// Checks all optional look features on branched main, motion-vector prepass and shadow
/// pipelines.
#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_branched_plants_with_every_look_term_compile_in_every_pass() {
    use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, NormalPrepass};

    use crate::PlantBranches;

    let mut app = headless_app();
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .expect("the harness's camera");
    app.world_mut()
        .entity_mut(camera)
        .insert((DepthPrepass, NormalPrepass, MotionVectorPrepass));
    // A shadow-casting sun ensures the shadow pipeline is exercised too.
    app.world_mut().spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // As dense as the other plant, so the 20% share below is a share of plants.
    let mut fern = PlantParams::FERN;
    fern.density = every_term_plant().density;
    fern.levels[1] = PlantBranches {
        count: 3,
        length: 0.4,
        width: 0.3,
        ..PlantBranches::NONE
    };
    set_harness_field(
        &mut app,
        every_term(vec![
            ScatterPlant::new(fern).scaled(0.2),
            ScatterPlant::new(every_term_plant()).scaled(0.8),
        ]),
    );
    let mut labels = Vec::new();
    for _ in 0..600 {
        app.update();
        labels = compiled_branched_pipelines(&mut app);
        if labels.len() >= 3 {
            break;
        }
    }
    assert_eq!(
        labels,
        [
            "plant_main_branched",
            "plant_prepass_branched",
            "plant_shadow_branched"
        ],
        "every pass's branched pipeline compiled"
    );
}
