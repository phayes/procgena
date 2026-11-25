//! Turning single grove trees into entities, and finding grove trees by ray
//! or by distance.

use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::shape::{Aabb3d, RayCast3d};
use procgena_core::chain::Step;
use procgena_specimen::{Specimen, TagFilter};
use procgena_tree::CopyAddress;

use super::draw::wind_phase;
use super::{PlacedTree, TreeScatterAssets, TreeScatterInstances};
use crate::spawn::{TreeParams, TreeWindPhase};

/// Gives one tree of a grove an entity of its own: an ordinary tree with a
/// [`TreeParams`], standing exactly where the grove drew it.
///
/// Spawn it with whatever the tree needs, such as a collider:
///
/// ```ignore
/// commands.spawn((PromotedTree::new(grove, address), MyCollider));
/// ```
///
/// The plugin then makes the entity a child of the grove and adds the
/// tree's [`TreeParams`] and `Transform`. The tree is drawn with the same
/// meshes, materials, drawing steps and wind as the grove's copy, which
/// stops drawing it once the entity's parts are spawned, so nothing visibly
/// changes.
///
/// To give the tree back to the grove, despawn the entity, or remove this
/// component, which also removes the tree's [`TreeParams`] and leaves the
/// rest of the entity.
///
/// The entity follows the grove: if an edit leaves no tree on the spot, the
/// entity is despawned; if the spot's tree changes, so do its
/// [`TreeParams`] and `Transform`. An address the grove doesn't have is
/// logged and the component is removed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PromotedTree {
    /// The grove entity.
    pub grove: Entity,
    /// The spot the tree stands on; see [`PlacedTree::address`].
    pub address: CopyAddress,
}

impl PromotedTree {
    /// Promotes the tree on the spot at `address` of `grove`.
    pub fn new(grove: Entity, address: CopyAddress) -> Self {
        Self { grove, address }
    }
}

/// The tree a [`PromotedTree`] entity was last given, to notice changes.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct PromotedAs {
    options: Arc<procgena_tree::TreeOptions>,
    transform: Transform,
    wind_phase: TreeWindPhase,
}

/// Gives each new or changed [`PromotedTree`] its tree, and keeps every
/// promoted tree in step with its grove.
pub(crate) fn sync_promoted_trees(
    mut commands: Commands,
    promoted: Query<(Entity, Ref<PromotedTree>, Option<&PromotedAs>)>,
    groves: Query<(Ref<TreeScatterInstances>, Ref<TreeScatterAssets>)>,
) {
    for (entity, promotion, given) in &promoted {
        let Ok((instances, assets)) = groves.get(promotion.grove) else {
            // The grove has no trees yet, or isn't a grove.
            continue;
        };
        let changed = promotion.is_changed() || instances.is_changed() || assets.is_changed();
        if !changed && given.is_some() {
            continue;
        }

        // The spot has no tree: a new promotion is wrong; an old one lost
        // its tree to an edit.
        let Some(tree) = instances.get(&promotion.address) else {
            if given.is_some() {
                commands.entity(entity).despawn();
            } else {
                warn!(
                    "procgena_tree_bevy: Scatter {} has no tree on spot {:?}",
                    promotion.grove, promotion.address
                );
                commands.entity(entity).remove::<PromotedTree>();
            }
            continue;
        };
        let wanted = PromotedAs {
            options: assets.of(tree).options().clone(),
            transform: tree.transform,
            wind_phase: TreeWindPhase(wind_phase(&promotion.address)),
        };
        if given == Some(&wanted) {
            continue;
        }

        // The parts carry the tree's detail distances, scaled by its size,
        // and its wind phase, so they are rebuilt when either changes.
        let mut entity = commands.entity(entity);
        let rebuild = given.is_none_or(|g| {
            g.options != wanted.options
                || g.transform.scale != wanted.transform.scale
                || g.wind_phase != wanted.wind_phase
        });
        if rebuild {
            entity.insert(TreeParams::Options(wanted.options.clone()));
        }
        entity.insert((
            wanted.transform,
            wanted.wind_phase,
            ChildOf(promotion.grove),
            wanted,
        ));
    }
}

/// Removes the tree from entities that lost their [`PromotedTree`].
pub(crate) fn demote_trees(
    mut commands: Commands,
    mut removed: RemovedComponents<PromotedTree>,
    given: Query<(), With<PromotedAs>>,
) {
    for entity in removed.read() {
        if given.contains(entity) {
            commands
                .entity(entity)
                .remove::<(TreeParams, PromotedAs, TreeWindPhase)>();
        }
    }
}

/// Where a ray hit a grove tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeScatterHit {
    /// The grove entity.
    pub grove: Entity,
    /// The tree's spot; see [`PlacedTree::address`].
    pub address: CopyAddress,
    /// Distance along the ray, in world units.
    pub distance: f32,
    /// The hit point in the world.
    pub point: Vec3,
}

/// Finds grove trees by ray, for picking them with the mouse, or near a
/// point, for promoting the trees near a player.
///
/// Rays are tested against the triangles of each tree's game-level mesh,
/// at rest: a ray through a gap in a crown passes on, and wind sway
/// isn't taken into account. Trees whose variant hasn't finished generating
/// are skipped.
#[derive(SystemParam)]
pub struct TreeScatterTrees<'w, 's> {
    groves: Query<
        'w,
        's,
        (
            Entity,
            &'static TreeScatterInstances,
            &'static TreeScatterAssets,
            &'static GlobalTransform,
        ),
    >,
}

impl TreeScatterTrees<'_, '_> {
    /// The nearest grove tree `ray` hits, if any.
    pub fn ray_cast(&self, ray: Ray3d) -> Option<TreeScatterHit> {
        // Trees whose bounding box the ray meets, nearest box first.
        let mut candidates = Vec::new();
        for (grove, instances, assets, transform) in &self.groves {
            let bounds = variant_bounds(assets);
            for tree in instances.trees() {
                let Some(local_bounds) = bounds[tree.slot as usize] else {
                    continue;
                };
                let world_from_tree = transform.affine() * tree.transform.compute_affine();
                if let Some(entry) = box_entry(ray, &world_from_tree, &local_bounds) {
                    candidates.push((entry, grove, tree, assets, world_from_tree));
                }
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

        // The nearest triangle hit; no farther box can hold a nearer one.
        let mut nearest: Option<TreeScatterHit> = None;
        for (entry, grove, tree, assets, world_from_tree) in candidates {
            if nearest.is_some_and(|n| n.distance <= entry) {
                break;
            }
            let Some(specimen) = assets.of(tree).specimen() else {
                continue;
            };
            let Some((distance, point)) = triangle_hit(ray, &world_from_tree, specimen) else {
                continue;
            };
            if nearest.is_none_or(|n| distance < n.distance) {
                nearest = Some(TreeScatterHit {
                    grove,
                    address: tree.address,
                    distance,
                    point,
                });
            }
        }
        nearest
    }

    /// Every grove tree whose base is within `radius` of `point`, with its
    /// grove entity.
    pub fn within(&self, point: Vec3, radius: f32) -> impl Iterator<Item = (Entity, &PlacedTree)> {
        self.groves
            .iter()
            .flat_map(move |(grove, instances, assets, transform)| {
                let ready: Vec<bool> = assets.variants().iter().map(|v| v.is_ready()).collect();
                instances.trees().iter().filter_map(move |tree| {
                    let base = transform.transform_point(tree.transform.translation);
                    (ready[tree.slot as usize] && base.distance(point) <= radius)
                        .then_some((grove, tree))
                })
            })
    }
}

/// Each variant's bounds at its game-level mesh, in the tree's own
/// coordinates, or `None` if it has none yet.
fn variant_bounds(assets: &TreeScatterAssets) -> Vec<Option<Aabb3d>> {
    assets
        .variants()
        .iter()
        .map(|variant| {
            variant
                .parts()
                .filter(|part| part.step == Step::Game)
                .map(|part| Aabb3d::from(part.bounds))
                .reduce(|a, b| Aabb3d {
                    min: a.min.min(b.min),
                    max: a.max.max(b.max),
                })
        })
        .collect()
}

/// `ray` in a tree's own coordinates: its origin and unit direction.
fn local_ray(ray: Ray3d, world_from_tree: &Affine3A) -> Option<(Vec3, Dir3)> {
    let tree_from_world = world_from_tree.inverse();
    let origin = tree_from_world.transform_point3(ray.origin);
    let direction = Dir3::new(tree_from_world.transform_vector3(*ray.direction)).ok()?;
    Some((origin, direction))
}

/// How far along `ray`, in world units, it enters `bounds` placed by
/// `world_from_tree`: 0 if it starts inside, `None` if it misses.
fn box_entry(ray: Ray3d, world_from_tree: &Affine3A, bounds: &Aabb3d) -> Option<f32> {
    let (origin, direction) = local_ray(ray, world_from_tree)?;
    let local = RayCast3d::new(origin, direction, f32::MAX).aabb_intersection_at(bounds)?;
    let point = world_from_tree.transform_point3(origin + *direction * local);
    Some(point.distance(ray.origin))
}

/// Where `ray` first meets a triangle of `specimen`'s game-level mesh,
/// placed by `world_from_tree`, as the distance along the ray and the point
/// in the world.
fn triangle_hit(
    ray: Ray3d,
    world_from_tree: &Affine3A,
    specimen: &Specimen,
) -> Option<(f32, Vec3)> {
    let (origin, direction) = local_ray(ray, world_from_tree)?;
    let mut nearest = f32::INFINITY;
    for part in specimen.parts(0, &TagFilter::default()) {
        let mesh = &part.mesh;
        let (triangles, _) = mesh.indices.as_chunks::<3>();
        for triangle in triangles {
            let corner = |i: usize| Vec3::from(mesh.positions[triangle[i] as usize]);
            if let Some(t) = ray_triangle(origin, *direction, corner(0), corner(1), corner(2)) {
                nearest = nearest.min(t);
            }
        }
    }
    if !nearest.is_finite() {
        return None;
    }
    let point = world_from_tree.transform_point3(origin + *direction * nearest);
    Some((point.distance(ray.origin), point))
}

/// How far along the ray from `origin` in `direction` it crosses triangle
/// `a`, `b`, `c` from either side, if it does (Möller–Trumbore).
fn ray_triangle(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (ab, ac) = (b - a, c - a);
    let p = direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let to_origin = origin - a;
    let u = to_origin.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = to_origin.cross(ab);
    let v = direction.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = ac.dot(q) / det;
    (t >= 0.0).then_some(t)
}
