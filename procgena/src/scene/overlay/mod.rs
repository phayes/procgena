//! The segmentation overlay: colours over the terrain that show how the
//! land is divided and labelled, such as its regions, its ground materials
//! or how steep it is.
//!
//! Choose one with [`ViewerCommand::SetOverlay`](super::ViewerCommand::SetOverlay).
//! Subjects with ground offer overlays by putting an [`OverlaySource`] on
//! their entity; [`OverlayStatus`] says which they offer, which is drawn,
//! what it has cost so far and what lies under the pointer.
//!
//! Each piece of ground drawn (a terrain chunk, an Ecotope's ground patch)
//! is sampled on the CPU in the background, at a coarse texel, and kept per
//! piece and family of values, so switching between overlays of one family
//! only recolours. The samples are dropped when the ground changes.

pub mod sample;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::math::DVec2;
use bevy::picking::events::{PointerLeave, PointerMove};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use procgena_terrain::{ChunkKey, FeatureTier};
use procgena_terrain_bevy::{TerrainChunk, TerrainOverlay};

pub use sample::{AreaValues, Family, Legend, Overlay, OverlayGround, Value};

use super::subject::SceneState;
use super::viewport::{SceneViewport, ViewportActivity, ViewportCamera};

/// How strongly the overlay's colours cover the ground's own, from 0 to 1.
const STRENGTH: f32 = 0.8;

/// The most pieces of ground sampled at once.
const MAX_TASKS: usize = 4;

/// The farthest the hover readout looks along the pointer's ray, in
/// metres.
const PROBE_REACH: f64 = 2.0 * super::terrain::VIEW_DISTANCE as f64;

/// The overlay asked for, whether or not the shown subject offers it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChosenOverlay(pub Option<Overlay>);

/// Ground a subject offers overlays for, on the subject's entity.
///
/// Replace it (with a new one from [`OverlaySource::new`]) whenever the
/// ground changes: the samples of the old one are dropped.
#[derive(Component, Clone)]
pub struct OverlaySource {
    /// Where regions, heights and measures come from.
    pub ground: Arc<dyn OverlayGround + Send + Sync>,
    /// The overlays this ground can show.
    pub offered: &'static [Overlay],
    /// Which ground this is, so samples of other ground are never mixed in.
    id: u64,
}

impl OverlaySource {
    /// A source of `ground`, offering `offered`.
    pub fn new(ground: Arc<dyn OverlayGround + Send + Sync>, offered: &'static [Overlay]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            ground,
            offered,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }
}

/// Marks a piece of ground the overlay is drawn over, other than a terrain
/// chunk: the square from `min` with side `side` (x and z, metres), read at
/// `tier`. Its entity needs a terrain material and UVs across that square,
/// and is a descendant of the entity with the [`OverlaySource`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OverlayArea {
    /// The smallest corner.
    pub min: DVec2,
    /// The side, in metres.
    pub side: f64,
    /// The tier its ground is read at.
    pub tier: FeatureTier,
}

/// What the overlay is doing, for the view frame: what the shown subject
/// offers, what is drawn, what it costs, and what lies under the pointer.
#[derive(Resource, Debug, Default, Clone)]
pub struct OverlayStatus {
    /// The overlays the shown subject offers; empty for none.
    pub offered: Vec<Overlay>,
    /// The overlay drawn: the one chosen, if the shown subject offers it.
    pub drawn: Option<Overlay>,
    /// What sampling has cost so far for the shown ground, per family.
    pub cost: HashMap<Family, FamilyCost>,
    /// Pieces of ground being sampled now.
    pub pending: usize,
    /// What lies under the pointer, for the drawn overlay.
    pub readout: Option<Readout>,
}

/// What sampling one family of values has cost.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FamilyCost {
    /// Pieces of ground sampled.
    pub areas: usize,
    /// Their sampling time added up.
    pub total: Duration,
}

impl FamilyCost {
    /// The mean time per piece of ground, zero before any.
    pub fn mean(&self) -> Duration {
        if self.areas == 0 {
            Duration::ZERO
        } else {
            self.total / self.areas as u32
        }
    }
}

/// What lies under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct Readout {
    /// Where the pointer's ray meets the ground (x and z, metres).
    pub point: DVec2,
    /// The drawn overlay's value there.
    pub value: Value,
    /// That value in words, such as "slope 0.42 (25°)".
    pub text: String,
}

/// Where the pointer is over the 3D view, from 0 to 1 across and down;
/// `None` when it is elsewhere.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct OverlayPointer(pub Option<Vec2>);

/// A piece of ground: where it is and the tier it is read at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AreaKey {
    min: [u64; 2],
    side: u64,
    tier: FeatureTier,
}

impl AreaKey {
    fn new(min: DVec2, side: f64, tier: FeatureTier) -> Self {
        Self {
            min: [min.x.to_bits(), min.y.to_bits()],
            side: side.to_bits(),
            tier,
        }
    }

    fn min(&self) -> DVec2 {
        DVec2::new(f64::from_bits(self.min[0]), f64::from_bits(self.min[1]))
    }

    fn side(&self) -> f64 {
        f64::from_bits(self.side)
    }

    fn contains(&self, p: DVec2) -> bool {
        let d = p - self.min();
        d.x >= 0.0 && d.y >= 0.0 && d.x <= self.side() && d.y <= self.side()
    }
}

/// Sampling one piece of ground in the background.
type SampleTask = Task<AreaValues>;

/// Samples and images of the shown ground.
#[derive(Resource, Default)]
struct OverlayCache {
    /// The [`OverlaySource`] they are from.
    source: u64,
    values: HashMap<(AreaKey, Family), Arc<AreaValues>>,
    images: HashMap<(AreaKey, Overlay), Handle<Image>>,
    tasks: Vec<((AreaKey, Family), SampleTask)>,
    /// Every sample taken, for tests.
    sampled: usize,
}

/// The entity of the subject whose ground the overlay is drawn over: the
/// shown subject's.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
struct OverlayRoot(Option<Entity>);

/// Adds the overlay to the scene.
pub(crate) fn plugin(app: &mut App) {
    add_drawing(app);
    app.init_resource::<OverlayPointer>()
        .add_systems(Update, follow_shown.before(draw_overlay))
        .add_systems(Update, probe.after(draw_overlay))
        .add_observer(track_pointer)
        .add_observer(lose_pointer);
}

/// Adds what samples and draws the overlay over [`OverlayRoot`]'s ground.
fn add_drawing(app: &mut App) {
    app.init_resource::<ChosenOverlay>()
        .init_resource::<OverlayStatus>()
        .init_resource::<OverlayRoot>()
        .init_resource::<OverlayCache>()
        .init_resource::<ViewportActivity>()
        .add_systems(Update, draw_overlay);
}

/// Draws the overlay over the shown subject.
fn follow_shown(state: Res<SceneState>, mut root: ResMut<OverlayRoot>) {
    root.set_if_neq(OverlayRoot(state.shown_entity()));
}

/// The overlay's subject entity and its source, if it has one.
fn shown_source<'a>(
    root: &OverlayRoot,
    sources: &'a Query<&OverlaySource>,
) -> Option<(Entity, &'a OverlaySource)> {
    let entity = root.0?;
    Some((entity, sources.get(entity).ok()?))
}

/// The pieces of ground under `root`: terrain chunks and marked areas, with
/// their entities.
fn areas(
    root: Entity,
    children: &Query<&Children>,
    chunks: &Query<&TerrainChunk>,
    marked: &Query<&OverlayArea>,
) -> Vec<(Entity, AreaKey)> {
    children
        .iter_descendants(root)
        .filter_map(|entity| {
            if let Ok(chunk) = chunks.get(entity) {
                let key: ChunkKey = chunk.key;
                Some((
                    entity,
                    AreaKey::new(key.min_corner(), key.size(), chunk.tier),
                ))
            } else {
                marked
                    .get(entity)
                    .ok()
                    .map(|a| (entity, AreaKey::new(a.min, a.side, a.tier)))
            }
        })
        .collect()
}

/// Samples the shown ground for the drawn overlay, draws it over every
/// piece, and keeps [`OverlayStatus`] up to date.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn draw_overlay(
    mut commands: Commands,
    root: Res<OverlayRoot>,
    chosen: Res<ChosenOverlay>,
    sources: Query<&OverlaySource>,
    children: Query<&Children>,
    chunks: Query<&TerrainChunk>,
    marked: Query<&OverlayArea>,
    drawn: Query<&TerrainOverlay>,
    mut cache: ResMut<OverlayCache>,
    mut status: ResMut<OverlayStatus>,
    mut images: ResMut<Assets<Image>>,
    mut activity: ResMut<ViewportActivity>,
) {
    let cache = &mut *cache;
    let Some((root, source)) = shown_source(&root, &sources) else {
        if !status.offered.is_empty() || status.drawn.is_some() {
            *status = OverlayStatus::default();
        }
        return;
    };

    // New ground drops everything sampled from the old.
    if cache.source != source.id {
        for (_, image) in cache.images.drain() {
            images.remove(&image);
        }
        cache.values.clear();
        cache.tasks.clear();
        cache.source = source.id;
        *status = OverlayStatus::default();
    }
    let overlay = chosen.0.filter(|o| source.offered.contains(o));
    if status.offered != source.offered {
        status.offered = source.offered.to_vec();
    }
    if status.drawn != overlay {
        status.drawn = overlay;
    }

    // Collect finished samples.
    let mut finished = Vec::new();
    cache
        .tasks
        .retain_mut(|(key, task)| match block_on(poll_once(task)) {
            Some(values) => {
                finished.push((*key, values));
                false
            }
            None => true,
        });
    for (key, values) in finished {
        let cost = status.cost.entry(key.1).or_default();
        cost.areas += 1;
        cost.total += values.cost;
        cache.sampled += 1;
        cache.values.insert(key, Arc::new(values));
    }

    // Forget pieces no longer drawn.
    let pieces = areas(root, &children, &chunks, &marked);
    let present: HashSet<AreaKey> = pieces.iter().map(|(_, key)| *key).collect();
    cache.values.retain(|(key, _), _| present.contains(key));
    let mut dropped = Vec::new();
    cache.images.retain(|(key, _), image| {
        let keep = present.contains(key);
        if !keep {
            dropped.push(image.clone());
        }
        keep
    });
    for image in dropped {
        images.remove(&image);
    }

    // Draw each piece, or start sampling it.
    for (entity, key) in pieces {
        let Some(overlay) = overlay else {
            if drawn.contains(entity) {
                commands.entity(entity).remove::<TerrainOverlay>();
            }
            continue;
        };
        let family = overlay.family();
        let image = match cache.images.get(&(key, overlay)) {
            Some(image) => Some(image.clone()),
            None => cache.values.get(&(key, family)).map(|values| {
                let image = images.add(overlay_image(values, overlay));
                cache.images.insert((key, overlay), image.clone());
                image
            }),
        };
        match image {
            Some(image) => {
                if drawn.get(entity).map_or(true, |d| d.image != image) {
                    commands.entity(entity).insert(TerrainOverlay {
                        image,
                        strength: STRENGTH,
                    });
                }
            }
            None => {
                if drawn.contains(entity) {
                    commands.entity(entity).remove::<TerrainOverlay>();
                }
                let busy = cache.tasks.iter().any(|(k, _)| *k == (key, family));
                if !busy && cache.tasks.len() < MAX_TASKS {
                    let ground = Arc::clone(&source.ground);
                    let task = AsyncComputeTaskPool::get().spawn(async move {
                        AreaValues::sample(family, &*ground, key.min(), key.side(), key.tier)
                    });
                    cache.tasks.push(((key, family), task));
                }
            }
        }
    }

    if status.pending != cache.tasks.len() {
        status.pending = cache.tasks.len();
    }
    if !cache.tasks.is_empty() {
        activity.request_frames(ViewportActivity::SETTLE_FRAMES);
    }
}

/// An image of `values` coloured by `overlay`: one texel per sample, sharp
/// for overlays of kinds, smooth for ramps.
fn overlay_image(values: &AreaValues, overlay: Overlay) -> Image {
    let data: Vec<u8> = values.colours(overlay).into_iter().flatten().collect();
    let mut image = Image::new(
        Extent3d {
            width: values.size,
            height: values.size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(match overlay.family() {
        Family::Measures => ImageSamplerDescriptor::linear(),
        Family::Regions | Family::Labels => ImageSamplerDescriptor::nearest(),
    });
    image
}

/// Follows the pointer over the 3D view.
fn track_pointer(
    moved: On<PointerMove>,
    viewports: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    mut pointer: ResMut<OverlayPointer>,
) {
    // Moves over the view's own overlays bubble up to it.
    let Ok((node, transform)) = viewports.get(moved.entity) else {
        return;
    };
    let size = node.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let corner = transform.translation - size / 2.0;
    let at = moved.pointer.position / node.inverse_scale_factor();
    let fraction = (at - corner) / size;
    pointer.set_if_neq(OverlayPointer(Some(fraction)));
}

/// Forgets the pointer when it leaves the 3D view.
fn lose_pointer(
    out: On<PointerLeave>,
    viewports: Query<(), With<SceneViewport>>,
    mut pointer: ResMut<OverlayPointer>,
) {
    if viewports.contains(out.entity) {
        pointer.set_if_neq(OverlayPointer(None));
    }
}

/// Finds what lies under the pointer for the drawn overlay after the
/// pointer, the camera or the overlay changes, once the camera is still.
#[expect(clippy::too_many_arguments, reason = "one Bevy system")]
fn probe(
    pointer: Res<OverlayPointer>,
    root: Res<OverlayRoot>,
    sources: Query<&OverlaySource>,
    children: Query<&Children>,
    chunks: Query<&TerrainChunk>,
    marked: Query<&OverlayArea>,
    camera: Query<(&Camera, Ref<GlobalTransform>), With<ViewportCamera>>,
    mut status: ResMut<OverlayStatus>,
    mut stale: Local<bool>,
) {
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    if pointer.is_changed() || transform.is_changed() || status.is_changed() {
        *stale = true;
    }
    // Tracing the ray takes milliseconds, so wait for the camera to stop
    // rather than trace it on every frame of a drag.
    if !*stale || transform.is_changed() {
        return;
    }
    *stale = false;
    let readout = (|| {
        let overlay = status.drawn?;
        let (root, source) = shown_source(&root, &sources)?;
        let fraction = pointer.0?;
        let size = camera.logical_viewport_size()?;
        let ray = camera.viewport_to_world(&transform, fraction * size).ok()?;
        let pieces = areas(root, &children, &chunks, &marked);
        let origin = ray.origin.as_dvec3();
        let direction = ray.direction.as_dvec3();
        let point = hit(&*source.ground, &pieces, origin, direction)?;
        let tier = tier_at(&pieces, point)?;
        let value = sample::sample_point(overlay.family(), &*source.ground, point, tier);
        let text = overlay.describe(&value);
        Some(Readout { point, value, text })
    })();
    if status.readout != readout {
        status.readout = readout;
    }
}

/// The tier of the drawn piece of ground holding `p`: the smallest piece's.
fn tier_at(pieces: &[(Entity, AreaKey)], p: DVec2) -> Option<FeatureTier> {
    pieces
        .iter()
        .filter(|(_, key)| key.contains(p))
        .min_by(|a, b| a.1.side().total_cmp(&b.1.side()))
        .map(|(_, key)| key.tier)
}

/// Where the ray from `origin` along `direction` first meets the ground
/// drawn in `pieces` (x and z, metres), if it does within
/// [`PROBE_REACH`]. Heights are read only inside drawn pieces, at their
/// tier, so nothing new is built.
fn hit(
    ground: &(impl OverlayGround + ?Sized),
    pieces: &[(Entity, AreaKey)],
    origin: bevy::math::DVec3,
    direction: bevy::math::DVec3,
) -> Option<DVec2> {
    // How far the ray's point lies above the ground, if over drawn ground.
    let above = |t: f64| -> Option<f64> {
        let p = origin + direction * t;
        let flat = DVec2::new(p.x, p.z);
        let tier = tier_at(pieces, flat)?;
        let h = ground.height(flat, tier);
        h.is_finite().then(|| p.y - f64::from(h))
    };

    // March in steps that grow with distance until the ray goes under.
    let mut t = 0.0;
    let mut last = above(t);
    while t < PROBE_REACH {
        let step = (t * 0.004).max(0.5);
        let next = t + step;
        let now = above(next);
        if let (Some(a), Some(b)) = (last, now)
            && a >= 0.0
            && b < 0.0
        {
            // Narrow down to where it crosses.
            let (mut lo, mut hi) = (t, next);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                match above(mid) {
                    Some(m) if m >= 0.0 => lo = mid,
                    _ => hi = mid,
                }
            }
            let p = origin + direction * hi;
            return Some(DVec2::new(p.x, p.z));
        }
        t = next;
        last = now;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bevy::asset::AssetPlugin;
    use bevy::math::DVec3;
    use procgena_terrain::{
        Elevation, RegionId, SampleDetail, TerrainPreset, Transitions, WorldTerrain,
        WorldTerrainOptions,
    };

    use super::*;

    /// An endless world of rolling hills.
    fn world() -> Arc<WorldTerrain> {
        let source =
            |_: RegionId, _: DVec2, _: &Elevation| TerrainPreset::rolling_hills().terrain.clone();
        Arc::new(WorldTerrain::new(WorldTerrainOptions::default(), Arc::new(source)).unwrap())
    }

    /// A source of `world`'s ground offering every overlay.
    fn source(world: &Arc<WorldTerrain>) -> OverlaySource {
        OverlaySource::new(world.clone(), &Overlay::ALL)
    }

    /// The two level-0 chunks at the origin and beside it.
    fn chunk_keys() -> [ChunkKey; 2] {
        [ChunkKey::new(0, 0, 0), ChunkKey::new(1, 0, 0)]
    }

    /// An app drawing the overlay over two terrain chunks of `world`, and
    /// the chunks' entities.
    fn app(world: &Arc<WorldTerrain>) -> (App, Entity, [Entity; 2]) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        add_drawing(&mut app);
        let root = app.world_mut().spawn(source(world)).id();
        let chunks = chunk_keys().map(|key| {
            app.world_mut()
                .spawn((
                    TerrainChunk {
                        key,
                        transitions: Transitions::NONE,
                        tier: FeatureTier::Rough,
                    },
                    ChildOf(root),
                ))
                .id()
        });
        app.insert_resource(OverlayRoot(Some(root)));
        (app, root, chunks)
    }

    /// Chooses `overlay` and runs `app` until every chunk draws it, or none
    /// does for `None`; returns the chunks' images.
    fn show(
        app: &mut App,
        chunks: [Entity; 2],
        overlay: Option<Overlay>,
    ) -> [Option<Handle<Image>>; 2] {
        app.insert_resource(ChosenOverlay(overlay));
        let start = Instant::now();
        loop {
            app.update();
            let drawn = chunks.map(|c| {
                app.world()
                    .get::<TerrainOverlay>(c)
                    .map(|o| o.image.clone())
            });
            let status = app.world().resource::<OverlayStatus>();
            let done = status.drawn == overlay
                && status.pending == 0
                && drawn.iter().all(|d| d.is_some() == overlay.is_some());
            if done {
                return drawn;
            }
            assert!(start.elapsed().as_secs() < 120, "the overlay was not drawn");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn sampled(app: &App) -> usize {
        app.world().resource::<OverlayCache>().sampled
    }

    #[test]
    fn overlays_of_one_family_share_their_samples() {
        let world = world();
        let (mut app, _, chunks) = app(&world);

        let slope = show(&mut app, chunks, Some(Overlay::Slope));
        assert_eq!(sampled(&app), 2, "one sample per chunk");
        let cost = app.world().resource::<OverlayStatus>().cost[&Family::Measures];
        assert_eq!(cost.areas, 2);
        assert!(cost.total > Duration::ZERO);

        // Another measure only recolours.
        let curvature = show(&mut app, chunks, Some(Overlay::Curvature));
        assert_eq!(sampled(&app), 2);
        assert_ne!(curvature, slope);

        // Another family samples again, once per chunk.
        show(&mut app, chunks, Some(Overlay::Regions));
        assert_eq!(sampled(&app), 4);

        // Going back reuses the images made before.
        assert_eq!(show(&mut app, chunks, Some(Overlay::Slope)), slope);
        assert_eq!(sampled(&app), 4);

        // Off takes the overlay away.
        assert_eq!(show(&mut app, chunks, None), [None, None]);
        assert_eq!(app.world().resource::<OverlayStatus>().drawn, None);
    }

    #[test]
    fn new_ground_is_sampled_again() {
        let world = world();
        let (mut app, root, chunks) = app(&world);
        show(&mut app, chunks, Some(Overlay::Slope));
        assert_eq!(sampled(&app), 2);

        app.world_mut().entity_mut(root).insert(source(&world));
        app.update();
        assert!(app.world().resource::<OverlayStatus>().cost.is_empty());
        show(&mut app, chunks, Some(Overlay::Slope));
        assert_eq!(sampled(&app), 4);
    }

    #[test]
    fn overlays_not_offered_are_not_drawn() {
        let world = world();
        let (mut app, root, chunks) = app(&world);
        let mut ground = source(&world);
        ground.offered = &[Overlay::Landform];
        app.world_mut().entity_mut(root).insert(ground);
        app.insert_resource(ChosenOverlay(Some(Overlay::Slope)));
        for _ in 0..3 {
            app.update();
        }
        let status = app.world().resource::<OverlayStatus>();
        assert_eq!(status.offered, vec![Overlay::Landform]);
        assert_eq!(status.drawn, None);
        assert!(
            chunks
                .iter()
                .all(|&c| app.world().get::<TerrainOverlay>(c).is_none())
        );
    }

    #[test]
    fn the_pointer_ray_finds_the_ground_and_its_value() {
        let world = world();
        let keys = chunk_keys();
        let pieces: Vec<(Entity, AreaKey)> = keys
            .iter()
            .map(|key| {
                (
                    Entity::PLACEHOLDER,
                    AreaKey::new(key.min_corner(), key.size(), FeatureTier::Rough),
                )
            })
            .collect();
        let p = keys[1].min_corner() + DVec2::new(13.0, 21.0);

        // Looking down at an angle from above `p`, the ray meets the ground
        // where its height is.
        let height = f64::from(OverlayGround::height(&*world, p, FeatureTier::Rough));
        let direction = DVec3::new(0.3, -1.0, 0.2).normalize();
        let origin = DVec3::new(p.x, height, p.y) - direction * 1000.0;
        let found = hit(&*world, &pieces, origin, direction).expect("the ray meets the ground");

        // What one probe costs when the whole ray runs over drawn ground.
        let wide = [(
            Entity::PLACEHOLDER,
            AreaKey::new(p - DVec2::splat(1500.0), 3000.0, FeatureTier::Rough),
        )];
        let start = Instant::now();
        let far = hit(&*world, &wide, origin, direction).expect("the ray meets the ground");
        eprintln!("one probe 1 km along the ray: {:?}", start.elapsed());
        assert!(far.distance(p) < 0.05, "{far} is near {p}");
        assert!(found.distance(p) < 0.05, "{found} is near {p}");

        // A ray over ground that isn't drawn finds nothing.
        let far = DVec3::new(1e5, height + 50.0, 1e5);
        assert_eq!(hit(&*world, &pieces, far, DVec3::NEG_Y), None);

        // The readout's value is the ground's own.
        let tier = tier_at(&pieces, found).unwrap();
        let value = sample::sample_point(Family::Measures, &*world, found, tier);
        let terrain = world.terrain_tier(world.region_at(found), tier).unwrap();
        let expected = terrain.query(found, SampleDetail::Measures).measures;
        assert_eq!(value, Value::Measures(expected));
    }
}
