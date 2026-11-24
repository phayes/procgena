//! Baking impostors: drawing an object from every view of the atlas, once,
//! and keeping the result by key.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use bevy::asset::{LoadState, UntypedAssetId};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, RenderLayers};
use bevy::camera::{Hdr, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::ecs::system::EntityCommands;
use bevy::image::CompressedImageFormatSupport;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::VertexAttributeValues;
use bevy::pbr::SpecializedMaterialPipelineCache;
use bevy::prelude::*;
use bevy::render::render_resource::{PipelineCache, TextureFormat, TextureUsages};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::RetainedViewEntity;
use bevy::render::{Extract, RenderApp};

use crate::ImpostorSettings;
use crate::cache::{self, DiskReads, ImpostorWrites};
use crate::key::{BakeLayout, ImpostorKey};
use crate::layout::{elevation_basis, elevation_direction, view_basis, view_direction};
use crate::material::{
    BakeOutput, ImpostorExtension, ImpostorMaterial, ImpostorUniform, impostor_base,
};

/// One mesh of an object to bake, with the materials its bake copies draw
/// with: one drawing base colour and one drawing normals (see
/// [`BakeOutput`]).
#[derive(Clone)]
pub struct BakePart {
    mesh: Handle<Mesh>,
    insert_material: InsertMaterial,
}

/// Inserts a part's bake material for an output on one of its copies.
type InsertMaterial = Arc<dyn Fn(&mut EntityCommands, BakeOutput) + Send + Sync>;

impl BakePart {
    /// A part drawing `mesh`, in the object's own coordinates, with
    /// `albedo` in the colour atlas and `normal` in the normal atlas. Both
    /// materials must be bake copies, such as
    /// [`bake_standard`](crate::bake_standard) makes.
    pub fn new<M: Material>(mesh: Handle<Mesh>, albedo: Handle<M>, normal: Handle<M>) -> Self {
        Self {
            mesh,
            insert_material: Arc::new(move |entity, output| {
                match output {
                    BakeOutput::Albedo => entity.insert(MeshMaterial3d(albedo.clone())),
                    BakeOutput::Normal => entity.insert(MeshMaterial3d(normal.clone())),
                    BakeOutput::None => entity,
                };
            }),
        }
    }
}

impl fmt::Debug for BakePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BakePart")
            .field("mesh", &self.mesh)
            .finish_non_exhaustive()
    }
}

/// Asks for the impostor of an object. Spawn it on an entity of your own.
///
/// If an impostor with the same key is already baked, the entity gets it
/// at once. Otherwise it is baked in the background once every part's mesh
/// and every asset in `wait_for` has loaded, without stopping the app. Then
/// the entity gets a [`BakedImpostor`], and [`ImpostorReady`] is triggered
/// on it. The impostor is also kept in [`Impostors`], so later requests with
/// the same key get it at once, even after this entity is gone.
#[derive(Component, Clone, Debug)]
pub struct ImpostorRequest {
    /// Names the impostor; see [`ImpostorKey`].
    pub key: ImpostorKey,
    /// The object's meshes, in its own coordinates, and their bake materials.
    pub parts: Vec<BakePart>,
    /// Assets the bake waits for, such as the textures the bake materials
    /// use, so it doesn't capture them half-loaded.
    pub wait_for: Vec<UntypedAssetId>,
}

/// A baked impostor: its two atlases and the bounds they were drawn over.
#[derive(Clone, Debug, PartialEq)]
pub struct Impostor {
    /// Base colour and coverage of every view.
    pub albedo: Handle<Image>,
    /// Normals of every view, each in its own view's camera space.
    pub normal: Handle<Image>,
    /// Views along each side of the atlases.
    pub frames: u32,
    /// Centre of the object's bounding sphere, in its own coordinates.
    pub center: Vec3,
    /// Radius of the bounding sphere.
    pub radius: f32,
}

impl Impostor {
    /// A material drawing this impostor, with `base`'s lighting settings and
    /// colour tint. Start from [`ImpostorMaterial`]'s usual base with
    /// [`Impostor::material`].
    pub fn material_with(&self, base: StandardMaterial) -> ImpostorMaterial {
        ImpostorMaterial {
            base,
            extension: ImpostorExtension {
                params: ImpostorUniform {
                    center: self.center,
                    radius: self.radius,
                    frames: self.frames,
                },
                albedo: self.albedo.clone(),
                normal: self.normal.clone(),
            },
        }
    }

    /// A material drawing this impostor: untinted, alpha-tested, rough.
    pub fn material(&self) -> ImpostorMaterial {
        self.material_with(impostor_base())
    }

    /// Bounds covering the impostor's quad from every view, in the object's
    /// coordinates. Insert them with `NoAutoAabb` on the entity drawing it,
    /// since the quad mesh itself is only a placeholder.
    pub fn aabb(&self) -> Aabb {
        Aabb::from_min_max(
            self.center - Vec3::splat(self.radius),
            self.center + Vec3::splat(self.radius),
        )
    }

    /// The components that draw this impostor on an entity: the shared
    /// quad, `material`, and the impostor's bounds.
    pub fn bundle(&self, quad: &ImpostorQuad, material: Handle<ImpostorMaterial>) -> impl Bundle {
        (
            Mesh3d(quad.0.clone()),
            MeshMaterial3d(material),
            self.aabb(),
            NoAutoAabb,
        )
    }
}

/// The quad mesh every impostor is drawn with. Its corners are placed by the
/// impostor's shaders.
#[derive(Resource, Clone, Debug)]
pub struct ImpostorQuad(pub Handle<Mesh>);

/// On a request's entity once its impostor is ready.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BakedImpostor(pub Impostor);

/// Triggered on a request's entity when its impostor is ready.
#[derive(EntityEvent, Clone, Debug, PartialEq)]
pub struct ImpostorReady {
    /// The request's entity.
    pub entity: Entity,
    /// The impostor's key.
    pub key: ImpostorKey,
}

/// Every impostor baked or read from disk so far, by key, each with a
/// shared material that draws it.
///
/// Assets ask for their impostors through [`Impostors::request`], which
/// bakes each key once however many assets ask for it.
#[derive(Resource, Default, Debug)]
pub struct Impostors {
    kept: HashMap<ImpostorKey, KeptImpostor>,
    /// Keys asked for through [`Impostors::request`].
    requested: HashSet<ImpostorKey>,
}

/// An impostor in [`Impostors`], with its shared material.
#[derive(Debug)]
struct KeptImpostor {
    impostor: Impostor,
    material: Handle<ImpostorMaterial>,
}

/// On request entities spawned by [`Impostors::request`], which are
/// despawned once answered.
#[derive(Component)]
pub(crate) struct SharedRequest;

impl Impostors {
    /// The impostor baked for `key`, if any.
    pub fn get(&self, key: ImpostorKey) -> Option<&Impostor> {
        self.kept.get(&key).map(|kept| &kept.impostor)
    }

    /// The untinted material ([`Impostor::material`]) drawing the impostor
    /// baked for `key`, shared by everything that draws it; `None` until
    /// it is baked.
    pub fn material(&self, key: ImpostorKey) -> Option<&Handle<ImpostorMaterial>> {
        self.kept.get(&key).map(|kept| &kept.material)
    }

    /// The impostor for `key` once it is baked. Until then, starts its bake
    /// the first time `key` is asked for, with the request `make` returns,
    /// and returns `None`; `make` isn't called again for that key, so call
    /// this every frame until the impostor arrives.
    ///
    /// `make` must return a request for `key`. Its entity is the plugin's
    /// own: [`ImpostorReady`] is triggered on it when the bake is done, and
    /// it is then despawned.
    pub fn request(
        &mut self,
        commands: &mut Commands,
        key: ImpostorKey,
        make: impl FnOnce() -> ImpostorRequest,
    ) -> Option<&Impostor> {
        if !self.kept.contains_key(&key) && self.requested.insert(key) {
            let request = make();
            debug_assert_eq!(request.key, key, "the request is for another key");
            commands.spawn((request, SharedRequest));
        }
        self.get(key)
    }

    /// Keeps `impostor` under `key`, with a new shared material.
    pub(crate) fn keep(
        &mut self,
        key: ImpostorKey,
        impostor: Impostor,
        materials: &mut Assets<ImpostorMaterial>,
    ) {
        let material = materials.add(impostor.material());
        self.kept.insert(key, KeptImpostor { impostor, material });
    }
}

/// A bake in progress.
struct ActiveBake {
    request: Entity,
    key: ImpostorKey,
    /// The pair of render layers it draws on, as a slot number.
    slot: usize,
    /// The bake copies and cameras, despawned when it ends.
    entities: Vec<Entity>,
    /// The albedo and normal cameras.
    cameras: [Entity; 2],
    /// Frames drawn since every copy's pipeline was built.
    frames: u32,
    impostor: Impostor,
}

/// On the temporary 3D cameras a bake spawns, which draw its atlases.
///
/// While impostors bake, the app has more than one [`Camera3d`]. Add
/// `Without<ImpostorBakeCamera>` to queries meant for your own camera, such
/// as `Single<&Transform, With<Camera3d>>`, which otherwise fail to match
/// or move the bake cameras too.
#[derive(Component)]
pub struct ImpostorBakeCamera {
    copies: Vec<Entity>,
}

/// The bake cameras whose copies all had their pipelines built in the
/// render world's latest frame, so whose atlas is being drawn.
///
/// Shared by the app and its render world. `None` without a render world,
/// where nothing is drawn and every bake counts as drawing at once.
#[derive(Resource, Clone, Default)]
pub(crate) struct BakeReadiness(Option<Arc<Mutex<HashSet<Entity>>>>);

impl BakeReadiness {
    /// Readiness shared with `app`'s render world, if it has one.
    pub(crate) fn for_app(app: &App) -> Self {
        Self(app.get_sub_app(RenderApp).map(|_| Arc::default()))
    }

    /// Whether the app renders, so bakes draw on a GPU.
    pub(crate) fn renders(&self) -> bool {
        self.0.is_some()
    }

    /// Whether every one of `cameras` is drawing its atlas.
    fn drawing(&self, cameras: &[Entity]) -> bool {
        let Some(shared) = &self.0 else {
            return true;
        };
        let ready = shared.lock().unwrap_or_else(PoisonError::into_inner);
        cameras.iter().all(|camera| ready.contains(camera))
    }
}

/// The render world's copy of every bake camera and its copies.
#[derive(Resource, Default)]
pub(crate) struct ExtractedBakeCameras(Vec<(Entity, Vec<Entity>)>);

/// Copies every bake camera and its copies into the render world.
pub(crate) fn extract_bake_cameras(
    mut extracted: ResMut<ExtractedBakeCameras>,
    cameras: Extract<Query<(Entity, &ImpostorBakeCamera)>>,
) {
    extracted.0.clear();
    extracted.0.extend(
        cameras
            .iter()
            .map(|(camera, bake)| (camera, bake.copies.clone())),
    );
}

/// Records which bake cameras have a built pipeline for every one of their
/// copies this frame. A copy whose material, textures or mesh aren't ready
/// yet has none.
pub(crate) fn check_bake_pipelines(
    extracted: Res<ExtractedBakeCameras>,
    readiness: Res<BakeReadiness>,
    specialized: Option<Res<SpecializedMaterialPipelineCache>>,
    pipelines: Res<PipelineCache>,
) {
    let (Some(shared), Some(specialized)) = (&readiness.0, specialized) else {
        return;
    };
    let ready = extracted
        .0
        .iter()
        .filter(|(camera, copies)| {
            let view = RetainedViewEntity::new(MainEntity::from(*camera), None, 0);
            specialized.get(&view).is_some_and(|built| {
                copies.iter().all(|copy| {
                    built
                        .get(&MainEntity::from(*copy))
                        .is_some_and(|id| pipelines.get_render_pipeline(*id).is_some())
                })
            })
        })
        .map(|(camera, _)| *camera)
        .collect();
    *shared.lock().unwrap_or_else(PoisonError::into_inner) = ready;
}

/// The bakes in progress, and which layer slots they hold.
#[derive(Resource, Default)]
pub(crate) struct ActiveBakes {
    bakes: Vec<ActiveBake>,
}

/// Starts baking requests that are ready, up to the settings' limit at a
/// time, and answers requests whose impostor is already baked.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
pub(crate) fn start_bakes(
    mut commands: Commands,
    settings: Res<ImpostorSettings>,
    requests: Query<(Entity, &ImpostorRequest, Has<SharedRequest>), Without<BakedImpostor>>,
    impostors: Res<Impostors>,
    mut active: ResMut<ActiveBakes>,
    mut reads: ResMut<DiskReads>,
    meshes: Res<Assets<Mesh>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    writes: Res<ImpostorWrites>,
) {
    let bc7 = cache::supports_bc7(formats.as_deref());
    for (entity, request, shared) in &requests {
        // Already baked or read from disk: answer at once. The plugin's own
        // requests have no one left to tell.
        if let Some(impostor) = impostors.get(request.key) {
            commands
                .entity(entity)
                .insert(BakedImpostor(impostor.clone()));
            commands.trigger(ImpostorReady {
                entity,
                key: request.key,
            });
            if shared {
                commands.entity(entity).despawn();
            }
            continue;
        }

        // Look on disk first: the shipped folder, then the cache folder.
        if reads.wait_for(request.key, &settings, bc7) {
            continue;
        }

        // Baking or being finished, for this request or another with the
        // same key.
        if writes.finishing(request.key)
            || active
                .bakes
                .iter()
                .any(|b| b.request == entity || b.key == request.key)
        {
            continue;
        }
        let Some(slot) = (0..settings.concurrent_bakes)
            .find(|slot| active.bakes.iter().all(|b| b.slot != *slot))
        else {
            break;
        };

        // Wait for the meshes and the assets the request names.
        let loaded = request
            .wait_for
            .iter()
            .all(|id| match server.get_load_state(*id) {
                None | Some(LoadState::Loaded | LoadState::Failed(_)) => true,
                Some(_) => false,
            });
        let Some((center, radius)) = loaded
            .then(|| bounding_sphere(&request.parts, &meshes))
            .flatten()
        else {
            continue;
        };

        let (impostor, entities, cameras) =
            spawn_bake(&mut commands, request, slot, center, radius, &mut images);
        active.bakes.push(ActiveBake {
            request: entity,
            key: request.key,
            slot,
            entities,
            cameras,
            frames: 0,
            impostor,
        });
    }
}

/// Ends bakes that have drawn for long enough: removes their copies and
/// cameras. On a GPU, the atlases are then read back to be finished (mip
/// levels, files), and the requests are answered once that is done;
/// otherwise the impostor is kept at once, and [`start_bakes`] answers the
/// requests.
pub(crate) fn finish_bakes(
    mut commands: Commands,
    settings: Res<ImpostorSettings>,
    readiness: Res<BakeReadiness>,
    mut active: ResMut<ActiveBakes>,
    mut impostors: ResMut<Impostors>,
    mut writes: ResMut<ImpostorWrites>,
    mut materials: ResMut<Assets<ImpostorMaterial>>,
) {
    // Count only frames drawn with every pipeline built.
    for bake in &mut active.bakes {
        if readiness.drawing(&bake.cameras) {
            bake.frames += 1;
        }
    }
    let (done, waiting): (Vec<_>, Vec<_>) = active
        .bakes
        .drain(..)
        .partition(|bake| bake.frames >= settings.settle_frames);
    active.bakes = waiting;

    for bake in done {
        for entity in bake.entities {
            commands.entity(entity).despawn();
        }
        // Read the atlases back for their mip levels and files; only a
        // GPU has anything to read back.
        if readiness.renders() {
            let size = bake.key.bake().atlas_size();
            cache::save(&mut commands, &mut writes, bake.key, &bake.impostor, size);
            continue;
        }
        impostors.keep(bake.key, bake.impostor, &mut materials);
    }
}

/// The centre and radius of a sphere around every vertex of `parts`, or
/// `None` while a mesh hasn't loaded.
fn bounding_sphere(parts: &[BakePart], meshes: &Assets<Mesh>) -> Option<(Vec3, f32)> {
    let mut points = Vec::new();
    for part in parts {
        let mesh = meshes.get(&part.mesh)?;
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            points.extend(positions.iter().map(|p| Vec3::from_array(*p)));
        }
    }
    let (min, max) = points
        .iter()
        .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), p| {
            (min.min(*p), max.max(*p))
        });
    if points.is_empty() {
        return Some((Vec3::ZERO, 1e-3));
    }
    let center = (min + max) / 2.0;
    let radius = points
        .iter()
        .map(|p| p.distance(center))
        .fold(1e-3f32, f32::max);
    Some((center, radius))
}

/// First render layer of the bake slots; slot `n` draws on layers
/// `BAKE_LAYERS + 2n` (colour) and `BAKE_LAYERS + 2n + 1` (normals).
const BAKE_LAYERS: usize = 24;

/// Spawns one copy of every part per view and output, each turned so a
/// camera looking down -Z sees it from that view and moved into the view's
/// atlas cell, and one camera per output drawing its whole atlas. Returns
/// the impostor the bake fills, every entity it spawned, and the albedo and
/// normal cameras.
fn spawn_bake(
    commands: &mut Commands,
    request: &ImpostorRequest,
    slot: usize,
    center: Vec3,
    radius: f32,
    images: &mut Assets<Image>,
) -> (Impostor, Vec<Entity>, [Entity; 2]) {
    let bake = request.key.bake();
    let frames = bake.frames;
    let cell = 2.0 * radius;
    let side = frames as f32 * cell;
    let views: Vec<Transform> = (0..frames * frames)
        .map(|index| {
            let cell_index = UVec2::new(index % frames, index / frames);
            let basis = match bake.layout {
                BakeLayout::Hemisphere => view_basis(view_direction(cell_index, frames)),
                BakeLayout::Elevations => {
                    elevation_basis(elevation_direction(index, frames * frames))
                }
            };
            let cell_center = Vec3::new(
                (cell_index.x as f32 + 0.5) * cell - side / 2.0,
                side / 2.0 - (cell_index.y as f32 + 0.5) * cell,
                0.0,
            );
            Transform::from_translation(cell_center)
                * Transform::from_rotation(Quat::from_mat3(&basis).inverse())
                * Transform::from_translation(-center)
        })
        .collect();

    let atlas_size = bake.atlas_size();
    let mut entities = Vec::new();
    let mut cameras = Vec::new();
    let mut atlases = Vec::new();
    for (index, output) in [BakeOutput::Albedo, BakeOutput::Normal]
        .into_iter()
        .enumerate()
    {
        let layer = RenderLayers::layer(BAKE_LAYERS + 2 * slot + index);

        // Every view's copy of every part.
        let mut copies = Vec::new();
        for part in &request.parts {
            for transform in &views {
                let mut copy = commands.spawn((
                    Mesh3d(part.mesh.clone()),
                    *transform,
                    layer.clone(),
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
                (part.insert_material)(&mut copy, output);
                copies.push(copy.id());
            }
        }
        entities.extend(&copies);

        // The atlas, and the camera drawing it. Colour is stored as sRGB,
        // normals linearly.
        let format = match output {
            BakeOutput::Normal => TextureFormat::Rgba8Unorm,
            _ => TextureFormat::Rgba8UnormSrgb,
        };
        let mut image = Image::new_target_texture(atlas_size, atlas_size, format, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = images.add(image);
        let camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1000 - (2 * slot + index) as isize,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: side,
                    height: side,
                },
                near: 0.0,
                far: 4.0 * radius,
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_xyz(0.0, 0.0, 2.0 * radius).looking_to(Vec3::NEG_Z, Vec3::Y),
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            layer,
            ImpostorBakeCamera { copies },
        ));
        entities.push(camera.id());
        cameras.push(camera.id());
        atlases.push(image);
    }

    let impostor = Impostor {
        albedo: atlases[0].clone(),
        normal: atlases[1].clone(),
        frames,
        center,
        radius,
    };
    (impostor, entities, [cameras[0], cameras[1]])
}
