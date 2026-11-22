//! Clump mode: growing a clump on the shown tree or rock in the background.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use procgena_clump::{Clump, ClumpError, ClumpMesh, ClumpOptions, Surface};
use procgena_stone_bevy::{StoneGenerating, StonePart};
use procgena_tree_bevy::{TreeGenerating, TreePart, TreePartKind};

use super::protocol::{BuildStats, ClumpBuildError, MeshSize, SceneMode, ViewerEvent};
use super::subject::SceneState;
use super::viewport::ViewportActivity;

/// How rough the clump's surface looks, from 0 (glossy) to 1 (matte).
const CLUMP_ROUGHNESS: f32 = 0.9;

/// The triangles a clump grows on, copied from the subject's meshes.
#[derive(Debug, Default)]
struct SurfaceBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl SurfaceBuffers {
    /// Appends `mesh`'s triangles. Meshes without positions, normals or
    /// indices are skipped.
    fn append(&mut self, mesh: &Mesh) {
        let (
            Some(VertexAttributeValues::Float32x3(positions)),
            Some(VertexAttributeValues::Float32x3(normals)),
            Some(indices),
        ) = (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            mesh.indices(),
        )
        else {
            return;
        };
        let offset = self.positions.len() as u32;
        self.positions.extend_from_slice(positions);
        self.normals.extend_from_slice(normals);
        self.indices
            .extend(indices.iter().map(|i| i as u32 + offset));
    }
}

/// A finished clump build, or why it failed.
type Grown = Result<(ClumpMesh, Duration), ClumpError>;

/// A clump build running in the background.
struct Growing {
    /// The subject entity the clump grows on.
    host: Entity,
    /// The settings it was started with.
    options: ClumpOptions,
    task: Task<Grown>,
}

/// The clump mode's surface, background build and shown clump.
#[derive(Resource, Default)]
pub(crate) struct ClumpJob {
    /// The subject entity whose surface is in `surface`.
    host: Option<Entity>,
    surface: Option<Arc<SurfaceBuffers>>,
    /// The settings of the clump last grown on `host`, whether it succeeded or not.
    grown: Option<ClumpOptions>,
    growing: Option<Growing>,
    /// The shown clump's entity, a child of `host`.
    shown: Option<Entity>,
    /// The material every clump is drawn with.
    material: Option<Handle<StandardMaterial>>,
}

impl ClumpJob {
    /// Forgets the surface and any build in progress. The shown clump goes
    /// when its subject is removed.
    fn reset(&mut self) {
        self.host = None;
        self.surface = None;
        self.grown = None;
        self.growing = None;
        self.shown = None;
    }
}

/// Matches tree or rock entities whose meshes are still being built.
type Generating = Or<(With<TreeGenerating>, With<StoneGenerating>)>;

/// Grows a clump on the clump mode's subject once it is built, and again
/// whenever the clump settings change, then shows it with the chosen colour.
///
/// Builds run one at a time in the background. When the settings change
/// during a build, the build's result is dropped and a new build starts with
/// the latest settings.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grow_clump(
    mut job: ResMut<ClumpJob>,
    state: Res<SceneState>,
    mut commands: Commands,
    generating: Query<(), Generating>,
    children: Query<&Children>,
    tree_parts: Query<(&TreePart, &Mesh3d)>,
    stone_parts: Query<(&StonePart, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut events: MessageWriter<ViewerEvent>,
    mut activity: ResMut<ViewportActivity>,
) {
    if state.mode() != SceneMode::Clump {
        if job.host.is_some() || job.growing.is_some() {
            job.reset();
        }
        return;
    }
    let settings = state.clump();

    // Copy the subject's surface once it has finished building.
    let subject = state.subject();
    if job.host != Some(subject) && !generating.contains(subject) {
        let mut surface = SurfaceBuffers::default();
        let kids = children.get(subject).into_iter().flatten();
        for kid in kids {
            let mesh = match (tree_parts.get(*kid), stone_parts.get(*kid)) {
                (Ok((part, mesh)), _) if part.lod == 0 && part.kind == TreePartKind::Bark => mesh,
                (_, Ok((part, mesh))) if part.lod == 0 => mesh,
                _ => continue,
            };
            if let Some(mesh) = meshes.get(&mesh.0) {
                surface.append(mesh);
            }
        }
        // A subject with no parts yet has not been built, or failed to build.
        if !surface.indices.is_empty() {
            job.reset();
            job.host = Some(subject);
            job.surface = Some(Arc::new(surface));
        }
    }

    // Show a finished build if its settings are still wanted.
    if let Some(growing) = job.growing.as_mut()
        && let Some(result) = check_ready(&mut growing.task)
    {
        let Growing { host, options, .. } = job.growing.take().expect("checked above");
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
        if job.host == Some(host) && options == settings.clump {
            if let Some(old) = job.shown.take() {
                commands.entity(old).despawn();
            }
            job.grown = Some(options);
            let result = result.map(|(mesh, build_time)| {
                let size = MeshSize {
                    vertices: mesh.vertex_count(),
                    triangles: mesh.triangle_count(),
                };
                if !mesh.is_empty() {
                    let material = job
                        .material
                        .get_or_insert_with(|| {
                            materials.add(StandardMaterial {
                                base_color: settings.color,
                                perceptual_roughness: CLUMP_ROUGHNESS,
                                ..default()
                            })
                        })
                        .clone();
                    let shown = commands
                        .spawn((
                            Mesh3d(meshes.add(bevy_mesh(mesh))),
                            MeshMaterial3d(material),
                            ChildOf(host),
                        ))
                        .id();
                    job.shown = Some(shown);
                }
                BuildStats {
                    levels: vec![size],
                    build_time,
                }
            });
            events.write(ViewerEvent::ClumpBuilt(
                result.map_err(ClumpBuildError::Clump),
            ));
        }
    }

    // Start a build when the shown clump's settings are out of date.
    if job.growing.is_none()
        && job.grown.as_ref() != Some(&settings.clump)
        && let (Some(host), Some(surface)) = (job.host, job.surface.clone())
    {
        let options = settings.clump.clone();
        let task_options = options.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { grow(&surface, &task_options) });
        job.growing = Some(Growing {
            host,
            options,
            task,
        });
    }
    if job.growing.is_some() {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }

    // Keep the clump's colour in step with the settings.
    if let Some(mut material) = job.material.as_ref().and_then(|m| materials.get_mut(m))
        && material.base_color != settings.color
    {
        material.base_color = settings.color;
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// Grows a clump with `options` on `surface`, and measures how long it took.
fn grow(surface: &SurfaceBuffers, options: &ClumpOptions) -> Grown {
    let started = Instant::now();
    let surface = Surface::new(&surface.positions, &surface.normals, &surface.indices)?;
    let clump = Clump::new(surface, options)?;
    Ok((clump.mesh().clone(), started.elapsed()))
}

/// Converts a clump mesh into a Bevy mesh with positions, normals and indices.
fn bevy_mesh(mesh: ClumpMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals)
    .with_inserted_indices(Indices::U32(mesh.indices))
}

#[cfg(test)]
mod tests {
    use procgena_clump::SurfaceMesh;

    use super::*;

    #[test]
    fn meshes_are_appended_with_their_indices_offset() {
        let triangle = || {
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 3])
            .with_inserted_indices(Indices::U16(vec![0, 1, 2]))
        };
        let mut surface = SurfaceBuffers::default();
        surface.append(&triangle());
        surface.append(&triangle());
        assert_eq!(surface.positions.len(), 6);
        assert_eq!(surface.normals.len(), 6);
        assert_eq!(surface.indices, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_clump_grows_on_a_copied_surface() {
        let ground = SurfaceMesh::ground(1.0, 4);
        let ground = ground.surface().unwrap();
        let surface = SurfaceBuffers {
            positions: ground.positions().to_vec(),
            normals: ground.normals().to_vec(),
            indices: ground.indices().to_vec(),
        };
        let (mesh, _) = grow(&surface, &ClumpOptions::default()).unwrap();
        assert!(!mesh.is_empty());
        let mesh = bevy_mesh(mesh);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());

        let mut bad = ClumpOptions::default();
        bad.shape.thickness = 0.0;
        assert!(matches!(
            grow(&surface, &bad),
            Err(ClumpError::Options(e)) if e.field == "shape.thickness"
        ));
    }
}
