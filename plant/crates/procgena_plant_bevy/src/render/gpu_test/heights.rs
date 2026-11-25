//! Checks the CPU model of a branched plant's resting height against what the GPU
//! draws: a patch of one plant, seen side-on through an orthographic camera with no
//! wind, must top out where `mirror::branched_top` says.

use std::sync::{Arc, Mutex};

use bevy::camera::{ClearColorConfig, OrthographicProjection, Projection, ScalingMode};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::TextureUsages;

use super::*;
use crate::PlantParams;
use crate::PlantSettings;

/// Side of the square render target, in pixels. 512 × 4 bytes is a whole number of
/// 256-byte rows, so the readback has no row padding.
const SIDE: u32 = 512;

/// Ground below the bottom of the view, in metres, so roots are in frame.
const BELOW: f32 = 0.1;

/// Renders `plant` side-on and returns the height, in metres, of the highest drawn
/// pixel's top edge, with the height of one pixel.
fn drawn_top(plant: &PlantParams, view_height: f32) -> (f32, f32) {
    let mut app = headless_app_with(Transform::IDENTITY, PlantGround::Flat);
    app.insert_resource(PlantWind {
        strength: 0.0,
        ..PlantWind::CALM
    });
    app.insert_resource(PlantSettings {
        orthographic_lod: 0,
        ..PlantSettings::HIGH
    });

    // Centre a narrow, deep patch under the camera so every stem's top is visible.
    // Grow each plant at 15 plants per square metre, regardless of its natural density.
    let world = app.world_mut();
    let mut fields = world.query::<&mut PlantScatter>();
    let mut field = fields.single_mut(world).expect("one field");
    field.size = Vec2::new(0.6, 4.0);
    field.options.plants = vec![ScatterPlant::new(*plant)].into();
    field.options.density_scale = 15.0 / plant.density;

    // An orthographic camera looking along -Z at a flat background.
    let mut image = Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = world.resource_mut::<Assets<Image>>().add(image);
    let mut cameras = world.query_filtered::<Entity, With<Camera3d>>();
    let camera = cameras.single(world).expect("one camera");
    let center = view_height * 0.5 - BELOW;
    world.entity_mut(camera).insert((
        RenderTarget::Image(target.clone().into()),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(1.0, 0.0, 1.0)),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: view_height,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, center, 4.0).looking_to(Vec3::NEG_Z, Vec3::Y),
    ));

    // Read the target back every frame until the drawing settles.
    let pixels: Arc<Mutex<Vec<u8>>> = Arc::default();
    let latest = pixels.clone();
    world
        .spawn(Readback::texture(target))
        .observe(move |event: On<ReadbackComplete>| {
            *latest.lock().expect("unpoisoned") = event.data.clone();
        });
    let mut previous = None;
    let mut settled = 0;
    for _ in 0..600 {
        app.update();
        let data = pixels.lock().expect("unpoisoned").clone();
        let row = top_row(&data);
        if row.is_some() && row == previous {
            settled += 1;
            if settled == 10 {
                break;
            }
        } else {
            settled = 0;
        }
        previous = row;
    }
    assert_eq!(settled, 10, "the plant never drew a steady picture");

    let pixel = view_height / SIDE as f32;
    let row = previous.expect("settled on a drawn row");
    (center + view_height * 0.5 - row as f32 * pixel, pixel)
}

/// The first row from the top with a pixel unlike the top-left corner's, the
/// background's, if there is one.
fn top_row(data: &[u8]) -> Option<u32> {
    let stride = SIDE as usize * 4;
    if data.len() < stride * SIDE as usize {
        return None;
    }
    let background = &data[..4];
    (0..SIDE).find(|&row| {
        data[row as usize * stride..(row as usize + 1) * stride]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| {
                pixel[..3]
                    .iter()
                    .zip(&background[..3])
                    .map(|(a, b)| a.abs_diff(*b) as u32)
                    .sum::<u32>()
                    > 60
            })
    })
}

/// Asserts that the GPU draws `plant` as tall as the CPU model says, within a pixel
/// or two of edge blending and the few per cent by which the least drooping stem in
/// the patch droops more than the plant's least drooping possible stem.
fn tops_match(name: &str, plant: &PlantParams) {
    // Every stem at full height, so only the droop varies between them.
    let plant = PlantParams {
        height: Vec2::splat(plant.height.y),
        ..*plant
    };
    let cpu = mirror::branched_top(&plant);
    let (gpu, pixel) = drawn_top(&plant, cpu * 1.4 + 0.3);
    assert!(
        gpu <= cpu + 2.0 * pixel,
        "{name}: the GPU draws up to {gpu} m, above the CPU top {cpu} m"
    );
    assert!(
        gpu >= cpu * 0.97 - 2.0 * pixel,
        "{name}: the GPU draws up to {gpu} m, well below the CPU top {cpu} m"
    );
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_upright_branches_reach_the_modelled_height() {
    tops_match("BROOM", &PlantParams::BROOM);
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_drooping_fronds_reach_the_modelled_height() {
    tops_match("MATURE_BRACKEN", &PlantParams::MATURE_BRACKEN);
}

#[test]
#[ignore = "needs a GPU: cargo test -- --ignored gpu"]
fn gpu_arching_canes_reach_the_modelled_height() {
    tops_match("BRAMBLE", &PlantParams::BRAMBLE);
}
