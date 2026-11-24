//! An endless world of terrains: the region graph, the world elevation E,
//! and a terrain for each region, built when it is first asked for.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use glam::{DVec2, IVec2, Vec2};
use procgena_core::{Ground, GroundSample};

use crate::converge::{
    BorderTarget, Convergence, EdgeTarget, blend_weights, relief_meeting_is_valid, relief_target,
};
use crate::elevation::{Elevation, ElevationError, ElevationOptions};
use crate::graph::{RegionGraph, RegionGraphError, RegionGraphOptions, RegionKey};
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::region::{CornerId, EdgeId, Region, RegionId};
use crate::seam::{CornerAgreement, EdgeOffer, SeamError, negotiate, negotiate_corner};
use crate::terrain::{HeightField, RiverTrace, Terrain, TerrainError, TerrainGenerator};

/// The default [`WorldTerrainOptions::cache`]: how many built terrains a
/// [`WorldTerrain`] keeps.
pub const DEFAULT_CACHE: usize = 64;

/// How a [`WorldTerrain`] is laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldTerrainOptions {
    /// The world seed. Every region's terrain, the region graph and the
    /// world elevation are seeded from it, so the same seed and settings
    /// always give the same world.
    pub seed: u32,
    /// The world elevation E that every region stands on.
    pub elevation: ElevationOptions,
    /// How the plane is split into regions.
    pub regions: RegionGraphOptions,
    /// How neighbouring terrains become alike near their shared borders.
    pub convergence: Convergence,
    /// How many built terrains are kept, at least 1. A terrain that was
    /// dropped is built again, identically, when it is next needed. The
    /// smaller stages a build needs are kept in proportion.
    pub cache: usize,
}

impl Default for WorldTerrainOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            elevation: ElevationOptions::default(),
            regions: RegionGraphOptions::default(),
            convergence: Convergence::default(),
            cache: DEFAULT_CACHE,
        }
    }
}

/// Gives each region of a [`WorldTerrain`] its terrain settings.
///
/// It must give the same settings every time it is asked about the same
/// region, from any thread. The settings' `default_seed` is replaced by the
/// world seed, so every region shares the world's noise.
///
/// Any `Fn(RegionId, DVec2, &Elevation) -> TerrainOptions` closure that is
/// `Send + Sync` is a source.
pub trait TerrainSource: Send + Sync {
    /// The settings for `region`, whose site (the point the region is
    /// built around) is `site`. `elevation` is the world elevation, so the
    /// choice can depend on how high the land stands there.
    fn terrain_options(
        &self,
        region: RegionId,
        site: DVec2,
        elevation: &Elevation,
    ) -> TerrainOptions;
}

impl<F> TerrainSource for F
where
    F: Fn(RegionId, DVec2, &Elevation) -> TerrainOptions + Send + Sync,
{
    fn terrain_options(
        &self,
        region: RegionId,
        site: DVec2,
        elevation: &Elevation,
    ) -> TerrainOptions {
        self(region, site, elevation)
    }
}

/// Why a [`WorldTerrain`], or one of its terrains, could not be built.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WorldError {
    /// The world elevation settings aren't valid.
    Elevation(ElevationError),
    /// The region graph settings aren't valid.
    Regions(RegionGraphError),
    /// The relief meeting exponent is above 1 or NaN.
    ReliefMeeting(f32),
    /// The relief blend is outside 0 to 1, or NaN.
    ReliefBlend(f32),
    /// The cache size is 0.
    Cache,
    /// The source gave a region invalid settings.
    Options(RegionId, OptionsError),
    /// A region's terrain could not be built.
    Terrain(RegionId, TerrainError),
    /// A border of a region could not be agreed.
    Seam(RegionId, SeamError),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Elevation(e) => write!(f, "world elevation: {e}"),
            Self::Regions(e) => write!(f, "region graph: {e}"),
            Self::ReliefMeeting(p) => write!(f, "relief meeting exponent {p} is above 1 or NaN"),
            Self::ReliefBlend(b) => write!(f, "relief blend {b} is outside 0 to 1"),
            Self::Cache => write!(f, "the cache must hold at least one terrain"),
            Self::Options(id, e) => write!(f, "region {}: {e}", id.0),
            Self::Terrain(id, e) => write!(f, "region {}: {e}", id.0),
            Self::Seam(id, e) => write!(f, "a border of region {}: {e}", id.0),
        }
    }
}

impl std::error::Error for WorldError {}

/// An endless world: the plane split into regions (see [`RegionGraph`]),
/// all standing on one world elevation E, each with the terrain its
/// [`TerrainSource`] gives it, joined without seams.
///
/// Nothing is built up front. A region's terrain is built the first time
/// it is asked for, from its own settings and its neighbours' offers, and
/// kept for later. Asking for regions in any order, on any number of
/// threads, gives the same terrains bit for bit; several threads asking
/// for one region wait for a single build.
///
/// ```
/// use std::sync::Arc;
/// use glam::DVec2;
/// use procgena_terrain::{
///     Elevation, HeightField, RegionId, TerrainPreset, WorldTerrain, WorldTerrainOptions,
/// };
///
/// // Plains wherever the land stands low, rolling hills elsewhere.
/// let source = |_: RegionId, site: DVec2, elevation: &Elevation| {
///     let preset = if elevation.height(site) < 0.0 {
///         TerrainPreset::plains()
///     } else {
///         TerrainPreset::rolling_hills()
///     };
///     preset.terrain.clone()
/// };
/// let world = WorldTerrain::new(WorldTerrainOptions::default(), Arc::new(source))?;
/// let p = DVec2::new(1200.0, -300.0);
/// let region = world.region_at(p);
/// assert!(world.height(p).is_finite());
/// assert_eq!(world.terrain(region)?.region().id, region);
/// # Ok::<(), procgena_terrain::WorldError>(())
/// ```
pub struct WorldTerrain {
    options: WorldTerrainOptions,
    source: Arc<dyn TerrainSource>,
    elevation: Elevation,
    graph: RegionGraph,
    regions: Cache<Region>,
    settings: Cache<Settings>,
    plans: Cache<Plan>,
    /// Terrains without their roads.
    provisional: Cache<Terrain>,
    terrains: Cache<Terrain>,
    /// Told when each region's stages start and finish.
    progress: Option<ProgressHook>,
}

/// One stage of building a region, in the order a region goes through
/// them. Their costs differ a lot: on the machines measured so far, roads
/// take several times as long as the other two together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegionStage {
    /// The region's own side of its borders: its relief, its rivers'
    /// courses and its border offers. Every region next to a region being
    /// built needs this stage too.
    Plan,
    /// The ground fitted to its borders, with rivers and ponds but no
    /// roads: what [`WorldTerrain::provisional_terrain`] gives.
    Ground,
    /// Routing and cutting in the roads: what [`WorldTerrain::terrain`]
    /// adds to the ground.
    Roads,
}

/// A region stage starting or finishing, as told to a [`ProgressHook`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionProgress {
    /// `region` started `stage`.
    Started {
        /// The region being built.
        region: RegionId,
        /// The stage it started.
        stage: RegionStage,
    },
    /// `region` finished `stage`, which took `elapsed`.
    Finished {
        /// The region being built.
        region: RegionId,
        /// The stage it finished.
        stage: RegionStage,
        /// How long the stage took.
        elapsed: Duration,
        /// Whether the stage succeeded; `false` if the region can't be
        /// built (see [`WorldTerrain::terrain`] for why).
        built: bool,
    },
}

/// Told about every region stage a [`WorldTerrain`] starts and finishes
/// (see [`WorldTerrain::with_progress`]), for example to drive a loading
/// screen.
///
/// It is called on whichever thread builds the region, often several at
/// once, so it should return quickly. It must not ask the same world for
/// terrain: that can wait on the build it is being told about, forever.
pub type ProgressHook = Arc<dyn Fn(&RegionProgress) + Send + Sync>;

impl fmt::Debug for WorldTerrain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorldTerrain")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// One region's settings from the source, ready to build with.
struct Settings {
    /// A generator for the settings, standing on E, without edge targets.
    generator: TerrainGenerator,
    /// The terrain's relief amplitude (see
    /// [`TerrainGenerator::relief_amplitude`]).
    amplitude: f32,
}

/// One region's side of its borders: what it needs from itself before any
/// negotiation.
///
/// The first planning stage: relief, river trace and offers. Rivers and
/// roads are later stages, cached as the provisional and full terrains.
struct Plan {
    region: Arc<Region>,
    /// The generator, converged towards its neighbours.
    generator: TerrainGenerator,
    /// The region's own rivers, traced once for the offers and the build.
    trace: RiverTrace,
    /// The offer for each shared edge.
    offers: HashMap<EdgeId, EdgeOffer>,
}

impl WorldTerrain {
    /// The world for `options`, with each region's terrain settings from
    /// `source`. Builds nothing yet.
    ///
    /// # Errors
    ///
    /// If the elevation or region settings aren't valid, if
    /// `options.convergence` isn't (see
    /// [`relief_meeting_is_valid`](crate::relief_meeting_is_valid); the blend
    /// must be from 0 to 1), or if `options.cache` is 0.
    pub fn new(
        options: WorldTerrainOptions,
        source: Arc<dyn TerrainSource>,
    ) -> Result<Self, WorldError> {
        let convergence = options.convergence;
        if !relief_meeting_is_valid(convergence.meeting) {
            return Err(WorldError::ReliefMeeting(convergence.meeting));
        }
        if !(0.0..=1.0).contains(&convergence.blend) {
            return Err(WorldError::ReliefBlend(convergence.blend));
        }
        if options.cache == 0 {
            return Err(WorldError::Cache);
        }
        let elevation = Elevation::new(options.elevation.clone(), options.seed)
            .map_err(WorldError::Elevation)?;
        let graph = RegionGraph::new(options.regions, options.seed).map_err(WorldError::Regions)?;

        // A terrain build reads the plans of its region and of every region
        // round it, and those read the settings of the next ring out.
        let cache = options.cache;
        Ok(Self {
            regions: Cache::new(cache.saturating_mul(16)),
            settings: Cache::new(cache.saturating_mul(16)),
            plans: Cache::new(cache.saturating_mul(4)),
            provisional: Cache::new(cache),
            terrains: Cache::new(cache),
            progress: None,
            options,
            source,
            elevation,
            graph,
        })
    }

    /// This world, telling `hook` whenever a region stage starts or
    /// finishes. Only stages built after this call are told.
    pub fn with_progress(mut self, hook: ProgressHook) -> Self {
        self.progress = Some(hook);
        self
    }

    /// The settings.
    pub fn options(&self) -> &WorldTerrainOptions {
        &self.options
    }

    /// The world elevation E: the height every region's relief stands on.
    pub fn elevation(&self) -> &Elevation {
        &self.elevation
    }

    /// The region graph: which regions there are and where.
    pub fn graph(&self) -> &RegionGraph {
        &self.graph
    }

    /// The site of `region`: the point its region is built around, and the
    /// one its source is given.
    pub fn site(&self, region: RegionId) -> DVec2 {
        self.graph.site(RegionKey::from_id(region))
    }

    /// The region holding `p`. Points on a border go to one side or the
    /// other, always the same one for the same point.
    pub fn region_at(&self, p: DVec2) -> RegionId {
        self.owner(p).id()
    }

    /// The outline, edges and corners of `region`. Every id is a region.
    pub fn region(&self, region: RegionId) -> Arc<Region> {
        self.region_of(RegionKey::from_id(region))
    }

    /// The terrain of `region`, built now if it isn't kept already.
    ///
    /// # Errors
    ///
    /// If the source gives this region or one around it invalid settings,
    /// or if one of its borders can't be agreed. The same error comes back
    /// every time while it is kept.
    pub fn terrain(&self, region: RegionId) -> Result<Arc<Terrain>, WorldError> {
        let key = RegionKey::from_id(region);
        self.terrains.get(key, || self.build_terrain(key))
    }

    /// The terrain of the region holding `p` (see [`region_at`](Self::region_at)).
    ///
    /// # Errors
    ///
    /// As for [`terrain`](Self::terrain).
    pub fn terrain_at(&self, p: DVec2) -> Result<Arc<Terrain>, WorldError> {
        self.terrain(self.region_at(p))
    }

    /// The terrain of `region` without its roads, built now if it isn't
    /// kept already.
    ///
    /// Routing roads is the slowest part of building a region, so this
    /// comes much sooner than [`terrain`](Self::terrain), which adds the
    /// roads to it. It is the same ground away from the roads and their
    /// border crossings. Use it only to draw far ground while the full
    /// terrain is built; placement and queries need the full terrain.
    ///
    /// # Errors
    ///
    /// As for [`terrain`](Self::terrain).
    pub fn provisional_terrain(&self, region: RegionId) -> Result<Arc<Terrain>, WorldError> {
        let key = RegionKey::from_id(region);
        self.provisional.get(key, || {
            self.reported(key, RegionStage::Ground, || self.build_provisional(key))
        })
    }

    /// The ground height at `p` (x and z, metres): E plus the relief of
    /// the region holding `p`, with its rivers, ponds and roads cut in.
    /// NaN if that region's terrain can't be built (see
    /// [`terrain`](Self::terrain) for why).
    pub fn height(&self, p: DVec2) -> f32 {
        self.terrain_at(p).map_or(f32::NAN, |t| t.height(p))
    }

    /// The water surface height at `p` (metres) if a river or pond covers
    /// it, otherwise `None`. Also `None` if the region's terrain can't be
    /// built.
    pub fn water_level(&self, p: DVec2) -> Option<f32> {
        self.terrain_at(p).ok()?.water_level(p)
    }

    /// The region holding `p`: the first of the nearby regions, nearest
    /// site first, whose outline holds it, or failing that (a point right
    /// on a border) the one whose border is nearest.
    fn owner(&self, p: DVec2) -> RegionKey {
        let near = self.graph.cell_near(p);
        let mut candidates: Vec<(f64, RegionKey)> = (-2..=2)
            .flat_map(|dz| (-2..=2).map(move |dx| RegionKey(near.0 + IVec2::new(dx, dz))))
            .map(|k| (self.graph.site(k).distance_squared(p), k))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut nearest_border: Option<(f64, RegionKey)> = None;
        for (_, key) in candidates {
            let boundary = &self.region_of(key).boundary;
            if boundary.contains(p) {
                return key;
            }
            let d = boundary.distance_to_border(p);
            if nearest_border.is_none_or(|(best, _)| d < best) {
                nearest_border = Some((d, key));
            }
        }
        nearest_border.expect("there are candidates").1
    }

    /// Region `key` from the graph, kept.
    fn region_of(&self, key: RegionKey) -> Arc<Region> {
        self.regions
            .get(key, || Ok(self.graph.region(key)))
            .expect("the region graph can't fail")
    }

    /// Region `key`'s settings from the source, kept.
    fn settings(&self, key: RegionKey) -> Result<Arc<Settings>, WorldError> {
        self.settings.get(key, || {
            let id = key.id();
            let mut options =
                self.source
                    .terrain_options(id, self.graph.site(key), &self.elevation);
            options.default_seed = self.options.seed;
            let generator = TerrainGenerator::new(options)
                .map_err(|e| WorldError::Options(id, e))?
                .with_elevation(self.elevation.clone());
            let amplitude = generator.relief_amplitude();
            Ok(Settings {
                generator,
                amplitude,
            })
        })
    }

    /// Region `key`'s plan, kept.
    fn plan(&self, key: RegionKey) -> Result<Arc<Plan>, WorldError> {
        self.plans.get(key, || {
            self.reported(key, RegionStage::Plan, || self.build_plan(key))
        })
    }

    /// Runs `build`, region `key`'s `stage`, telling the progress hook
    /// when it starts and finishes.
    fn reported<T>(
        &self,
        key: RegionKey,
        stage: RegionStage,
        build: impl FnOnce() -> Result<T, WorldError>,
    ) -> Result<T, WorldError> {
        let Some(hook) = &self.progress else {
            return build();
        };
        let region = key.id();
        hook(&RegionProgress::Started { region, stage });
        let started = Instant::now();
        let result = build();
        hook(&RegionProgress::Finished {
            region,
            stage,
            elapsed: started.elapsed(),
            built: result.is_ok(),
        });
        result
    }

    /// Converges region `key`'s generator towards every region along its
    /// borders and makes its offers.
    fn build_plan(&self, key: RegionKey) -> Result<Plan, WorldError> {
        let region = self.region_of(key);
        let id = region.id;

        // The settings of this region and of every region on its edges and
        // corners.
        let mut touching: BTreeSet<RegionId> = region
            .corners
            .iter()
            .flat_map(|c| c.incident.iter().copied())
            .chain(region.edges.iter().filter_map(|e| e.neighbor))
            .collect();
        touching.insert(id);
        let settings = touching
            .iter()
            .map(|&r| Ok((r, self.settings(RegionKey::from_id(r))?)))
            .collect::<Result<BTreeMap<_, _>, WorldError>>()?;

        // What each edge and corner fades towards.
        let meeting = self.options.convergence.meeting;
        let target = |regions: &[RegionId]| {
            let mut regions = regions.to_vec();
            regions.sort();
            regions.dedup();
            let amplitudes: Vec<f32> = regions.iter().map(|r| settings[r].amplitude).collect();
            let roads = regions
                .iter()
                .map(|r| settings[r].generator.options().features.roads.grades())
                .reduce(RoadGrades::steepest)
                .expect("a border has regions");
            BorderTarget {
                amplitude: relief_target(&amplitudes, meeting),
                blend: regions
                    .iter()
                    .copied()
                    .zip(blend_weights(&amplitudes, meeting))
                    .collect(),
                roads,
            }
        };
        let corners: HashMap<CornerId, BorderTarget> = region
            .corners
            .iter()
            .map(|c| (c.id, target(&c.incident)))
            .collect();
        let targets: Vec<EdgeTarget> = region
            .edges
            .iter()
            .filter_map(|edge| {
                let neighbor = edge.neighbor?;
                Some(EdgeTarget {
                    edge: edge.id,
                    middle: target(&[id, neighbor]),
                    start: corners[&edge.start].clone(),
                    end: corners[&edge.end].clone(),
                })
            })
            .collect();

        // The converged generator and its offers.
        let neighbours: Vec<(RegionId, TerrainOptions)> = settings
            .iter()
            .filter(|(r, _)| **r != id)
            .map(|(r, s)| (*r, s.generator.options().clone()))
            .collect();
        let generator = settings[&id]
            .generator
            .clone()
            .with_edge_targets(
                &region,
                &targets,
                &neighbours,
                self.options.convergence.blend,
            )
            .map_err(|e| WorldError::Terrain(id, e))?;
        let trace = generator.trace_rivers(&region);
        let offers = generator
            .offers_traced(&region, &trace)
            .into_iter()
            .map(|o| (o.edge, o))
            .collect();
        Ok(Plan {
            region,
            generator,
            trace,
            offers,
        })
    }

    /// Region `key`'s terrain: its terrain without roads, with the roads
    /// added.
    fn build_terrain(&self, key: RegionKey) -> Result<Terrain, WorldError> {
        let provisional = self.provisional_terrain(key.id())?;
        self.reported(key, RegionStage::Roads, || {
            Ok(Terrain::clone(&provisional).with_roads())
        })
    }

    /// Agrees every corner and edge of region `key` with the regions
    /// round it, and builds its terrain without roads.
    fn build_provisional(&self, key: RegionKey) -> Result<Terrain, WorldError> {
        let plan = self.plan(key)?;
        let region = &plan.region;
        let id = region.id;
        let seam = |e| WorldError::Seam(id, e);

        // Every corner, from the offers of every region touching it.
        let mut corners: HashMap<CornerId, CornerAgreement> = HashMap::new();
        for corner in &region.corners {
            let offers = corner
                .incident
                .iter()
                .map(|&r| {
                    let other = self.plan(RegionKey::from_id(r))?;
                    Ok(other.generator.corner_offer(&other.region, corner))
                })
                .collect::<Result<Vec<_>, WorldError>>()?;
            corners.insert(corner.id, negotiate_corner(corner, &offers).map_err(seam)?);
        }

        // Every shared edge, from both sides' offers.
        let mut agreements = Vec::new();
        for edge in &region.edges {
            let Some(neighbor) = edge.neighbor else {
                continue;
            };
            let other = self.plan(RegionKey::from_id(neighbor))?;
            let mine = &plan.offers[&edge.id];
            let theirs = other
                .offers
                .get(&edge.id)
                .ok_or(WorldError::Seam(id, SeamError::Mismatch))?;
            let agreed = negotiate(
                edge,
                mine,
                theirs,
                &corners[&edge.start],
                &corners[&edge.end],
                &self.elevation,
            )
            .map_err(seam)?;
            agreements.push(agreed);
        }

        plan.generator
            .clone()
            .build_traced(Region::clone(region), &agreements, &plan.trace)
            .map_err(|e| WorldError::Terrain(id, e))
    }
}

impl HeightField for WorldTerrain {
    /// See [`WorldTerrain::height`].
    fn height(&self, p: DVec2) -> f32 {
        WorldTerrain::height(self, p)
    }

    /// See [`Ground::sample`] for [`WorldTerrain`].
    fn sample(&self, p: DVec2) -> GroundSample {
        Ground::sample(self, p)
    }
}

impl Ground for WorldTerrain {
    /// See [`WorldTerrain::height`].
    fn height(&self, p: DVec2) -> f32 {
        WorldTerrain::height(self, p)
    }

    /// The slope of the region holding `p`, from its terrain's own
    /// gradient (see [`Terrain::sample`]). NaN where that region's terrain
    /// can't be built.
    fn gradient(&self, p: DVec2) -> Vec2 {
        Ground::sample(self, p).gradient
    }

    /// The height and slope of the region holding `p` (see
    /// [`Terrain::sample`]). Both NaN where that region's terrain can't be
    /// built.
    fn sample(&self, p: DVec2) -> GroundSample {
        self.terrain_at(p).map_or(
            GroundSample {
                height: f32::NAN,
                gradient: Vec2::NAN,
            },
            |t| t.sample(p),
        )
    }
}

/// A slot that is built once, by whichever thread asks first.
type Slot<T> = Arc<OnceLock<Result<Arc<T>, WorldError>>>;

/// Values kept per region, at most `capacity` of them, dropping the one
/// used longest ago.
struct Cache<T> {
    capacity: usize,
    state: Mutex<CacheState<T>>,
}

struct CacheState<T> {
    /// Counts every use, to find the one used longest ago.
    clock: u64,
    /// Each region's slot and when it was last used.
    slots: HashMap<RegionKey, (u64, Slot<T>)>,
}

impl<T> Cache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(CacheState {
                clock: 0,
                slots: HashMap::new(),
            }),
        }
    }

    /// The value for `key`, built with `build` if it isn't kept. Threads
    /// asking for the same key while it builds wait for that build.
    fn get(
        &self,
        key: RegionKey,
        build: impl FnOnce() -> Result<T, WorldError>,
    ) -> Result<Arc<T>, WorldError> {
        // Find or make the slot, without holding the lock while building.
        let slot = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.clock += 1;
            let now = state.clock;
            if let Some((used, slot)) = state.slots.get_mut(&key) {
                *used = now;
                slot.clone()
            } else {
                let slot = Slot::default();
                state.slots.insert(key, (now, slot.clone()));
                if state.slots.len() > self.capacity {
                    let oldest = state
                        .slots
                        .iter()
                        .min_by_key(|(_, (used, _))| *used)
                        .map(|(k, _)| *k)
                        .expect("the cache isn't empty");
                    state.slots.remove(&oldest);
                }
                slot
            }
        };
        slot.get_or_init(|| build().map(Arc::new)).clone()
    }
}
