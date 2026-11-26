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
use crate::feature::FeatureTier;
use crate::graph::{EdgePolicy, RegionGraph, RegionGraphError, RegionGraphOptions, RegionKey};
use crate::label::Labels;
use crate::options::{OptionsError, RoadGrades, TerrainOptions};
use crate::region::{CornerId, EdgeId, Region, RegionId};
use crate::scale::WorldScale;
use crate::seam::{EdgeOffer, SeamError, negotiate};
use crate::terrain::{
    HeightField, LabelField, RiverTrace, Terrain, TerrainError, TerrainGenerator,
};
use crate::tin::TinTolerance;

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
    /// How much the world stretches its landscape against reference scale
    /// 1. Every region's terrain gets it (see [`Terrain::with_scale`]).
    pub scale: WorldScale,
    /// How closely the world's canonical ground meshes (see
    /// [`tin`](crate::tin)) follow its ground. Every chunk of a world is
    /// built within this one tolerance, so neighbours meet exactly; ask
    /// for it with [`ChunkSource::tin_tolerance`](crate::ChunkSource::tin_tolerance).
    /// [`TinTolerance::GENERAL`] by default; a level such as
    /// [`TinTolerance::LOW_POLY`] gives faceted, low-poly ground.
    pub tin: TinTolerance,
}

impl Default for WorldTerrainOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            elevation: ElevationOptions::default(),
            regions: RegionGraphOptions::default(),
            convergence: Convergence::default(),
            cache: DEFAULT_CACHE,
            scale: WorldScale::default(),
            tin: TinTolerance::GENERAL,
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
    /// A part of the scale isn't finite and positive.
    Scale(WorldScale),
    /// The canonical ground tolerance has a value out of range (see
    /// [`TinTolerance::is_valid`]).
    Tin,
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
            Self::Scale(s) => write!(
                f,
                "the scale must be finite and positive, not {} × {}",
                s.horizontal, s.vertical
            ),
            Self::Tin => write!(f, "the canonical ground tolerance is out of range"),
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
    /// Terrains at each tier, in [`FeatureTier::ALL`] order.
    tiers: [Cache<Terrain>; FeatureTier::ALL.len()],
    /// Told when each region's stages start and finish.
    progress: Option<ProgressHook>,
}

/// One stage of building a region, in the order a region goes through
/// them. Their costs differ a lot: on the machines measured so far, the
/// fine tier (the roads) takes several times as long as the rest together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegionStage {
    /// The region's own side of its borders: its relief, its rivers'
    /// courses and its border offers. Every region next to a region being
    /// built needs this stage too.
    Plan,
    /// The features of one tier (see [`FeatureTier`]): for the roughest,
    /// the ground fitted to its borders with that tier's features; for each
    /// later one, the features it adds.
    Tier(FeatureTier),
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
/// The first planning stage: the converged relief. The river trace and
/// each edge's offer are worked out the first time they are asked for, so
/// a neighbour that needs one edge's offer pays for that edge only. The
/// feature tiers are later stages, cached as terrains of each tier.
struct Plan {
    region: Arc<Region>,
    /// The generator, converged towards its neighbours.
    generator: TerrainGenerator,
    /// The region's own rivers, traced once for the offers and the build.
    trace: OnceLock<RiverTrace>,
    /// For each shared edge, its index in the region and its offer without
    /// the crossing samples.
    offers: HashMap<EdgeId, (usize, OnceLock<EdgeOffer>)>,
    /// For each shared edge, its offer with the crossing samples.
    crossing_offers: HashMap<EdgeId, OnceLock<EdgeOffer>>,
}

impl Plan {
    /// The region's own rivers.
    fn trace(&self) -> &RiverTrace {
        self.trace
            .get_or_init(|| self.generator.trace_rivers(&self.region))
    }

    /// The offer for shared edge `edge`, with the crossing samples only
    /// with `crossing` (see [`EdgeOffer::inner`]); `None` if the region has
    /// no such shared edge.
    fn offer(&self, edge: EdgeId, crossing: bool) -> Option<&EdgeOffer> {
        let (index, offer) = self.offers.get(&edge)?;
        let offer = offer
            .get_or_init(|| (self.generator).edge_offer(&self.region, *index, self.trace(), false));
        if !crossing {
            return Some(offer);
        }
        Some(self.crossing_offers[&edge].get_or_init(|| {
            let samples = self.generator.crossing_samples(&self.region, *index);
            offer.clone().with_crossing(samples)
        }))
    }
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
    /// must be from 0 to 1), if `options.cache` is 0, if `options.scale`
    /// isn't finite and positive, or if `options.tin` isn't valid.
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
        if !options.scale.is_valid() {
            return Err(WorldError::Scale(options.scale));
        }
        if !options.tin.is_valid() {
            return Err(WorldError::Tin);
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
            tiers: std::array::from_fn(|_| Cache::new(cache)),
            progress: None,
            options,
            source,
            elevation,
            graph,
        })
    }

    /// This world on a lattice graph ([`RegionGraph::lattice`]): every
    /// region a hexagon with six neighbours and drifting corners, with
    /// only the edges `edges` names warped. Call it straight after
    /// [`new`](Self::new), before asking for any region.
    #[must_use]
    pub fn with_lattice_regions(mut self, edges: Arc<dyn EdgePolicy>) -> Self {
        self.graph = RegionGraph::lattice(self.options.regions, self.options.seed)
            .expect("checked by `new`")
            .with_edge_policy(edges);
        self
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
        self.terrain_tier(region, FeatureTier::Fine)
    }

    /// The terrain of the region holding `p` (see [`region_at`](Self::region_at)).
    ///
    /// # Errors
    ///
    /// As for [`terrain`](Self::terrain).
    pub fn terrain_at(&self, p: DVec2) -> Result<Arc<Terrain>, WorldError> {
        self.terrain(self.region_at(p))
    }

    /// The terrain of `region` with its features planned to `tier` (see
    /// [`FeatureTier`]), built now if it isn't kept already. At
    /// [`Fine`](FeatureTier::Fine) it is [`terrain`](Self::terrain).
    ///
    /// An earlier tier comes sooner, since the costly features are left
    /// out or drawn in outline: today the roads, left out at
    /// [`Rough`](FeatureTier::Rough) and on a coarse route at
    /// [`Mid`](FeatureTier::Mid). It is the same ground away from the
    /// features it leaves out or outlines. Use it
    /// only to draw far ground while the fine terrain is built; placement
    /// and queries need the fine terrain. Each tier is kept on its own, so
    /// asking for an earlier tier gives that tier even when a later one is
    /// built.
    ///
    /// # Errors
    ///
    /// As for [`terrain`](Self::terrain).
    pub fn terrain_tier(
        &self,
        region: RegionId,
        tier: FeatureTier,
    ) -> Result<Arc<Terrain>, WorldError> {
        let key = RegionKey::from_id(region);
        let index = tier.position();
        self.tiers[index].get(key, || {
            self.reported(key, RegionStage::Tier(tier), || {
                match index.checked_sub(1) {
                    // The roughest tier: fit the ground to its borders.
                    None => self.build_roughest(key),
                    // A later tier: add its features to the tier before.
                    Some(before) => {
                        let earlier = self.terrain_tier(region, FeatureTier::ALL[before])?;
                        Ok(Terrain::clone(&earlier).with_tier(tier))
                    }
                }
            })
        })
    }

    /// The terrain of `region` at `tier` if it is kept already, without
    /// building anything.
    pub(crate) fn kept_terrain(&self, region: RegionId, tier: FeatureTier) -> Option<Arc<Terrain>> {
        self.tiers[tier.position()].peek(RegionKey::from_id(region))
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

    /// The labels at `p` (x and z, metres) from the region holding it (see
    /// [`Terrain::annotate`]). `None` if that region's terrain can't be
    /// built.
    pub fn annotate(&self, p: DVec2) -> Option<Labels> {
        Some(self.terrain_at(p).ok()?.annotate(p))
    }

    /// The region holding `p`: the first of the nearby regions, nearest
    /// site first, whose outline holds it, or failing that (a point right
    /// on a border) the one whose border is nearest.
    fn owner(&self, p: DVec2) -> RegionKey {
        self.owner_using(p, &self.nearby_sites(self.graph.cell_near(p)), |key| {
            self.region_of(key)
        })
    }

    /// Sites considered by ownership, independent of the point within a
    /// graph cell. A sampling batch can reuse them for nearby points.
    fn nearby_sites(&self, near: RegionKey) -> [(DVec2, RegionKey); 25] {
        std::array::from_fn(|i| {
            let key = RegionKey(near.0 + IVec2::new((i % 5) as i32 - 2, (i / 5) as i32 - 2));
            (self.graph.site(key), key)
        })
    }

    /// The same ownership decision for individual and batched reads.
    /// A batch supplies retained boundaries instead of locking the world
    /// cache for every point. In particular, border ties stay unchanged.
    fn owner_using(
        &self,
        p: DVec2,
        sites: &[(DVec2, RegionKey); 25],
        mut region: impl FnMut(RegionKey) -> Arc<Region>,
    ) -> RegionKey {
        let mut candidates = sites.map(|(site, key)| (site.distance_squared(p), key));
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut nearest_border: Option<(f64, RegionKey)> = None;
        for (_, key) in candidates {
            let boundary = &region(key).boundary;
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

    /// Finds every point's region, then holds each region's terrain once
    /// while sampling all points. Failed regions remain missing for the
    /// whole batch, just as in the point-at-a-time API.
    pub(crate) fn sample_batch<T>(
        &self,
        points: &[DVec2],
        tier: FeatureTier,
        sample: impl Fn(Option<&Terrain>, DVec2) -> T,
    ) -> Vec<T> {
        // Each point's owner, reusing the regions and nearby sites already
        // looked up for the batch.
        let mut regions = HashMap::new();
        let mut sites = HashMap::new();
        let owners: Vec<RegionKey> = points
            .iter()
            .map(|&p| {
                let near = self.graph.cell_near(p);
                let nearby = sites.entry(near).or_insert_with(|| self.nearby_sites(near));
                self.owner_using(p, nearby, |key| {
                    regions
                        .entry(key)
                        .or_insert_with(|| self.region_of(key))
                        .clone()
                })
            })
            .collect();

        // Each owner's terrain once, in region order, then every sample.
        let terrains: HashMap<RegionKey, Option<Arc<Terrain>>> = owners
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| (key, self.terrain_tier(key.id(), tier).ok()))
            .collect();
        points
            .iter()
            .zip(&owners)
            .map(|(&p, key)| sample(terrains[key].as_deref(), p))
            .collect()
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
        // Targets only for the edges that need a border (see
        // `edge_is_active`).
        let mut targets = Vec::new();
        for edge in &region.edges {
            let Some(neighbor) = edge.neighbor else {
                continue;
            };
            if self.edge_is_active(&region, neighbor, &settings)? {
                targets.push(EdgeTarget {
                    edge: edge.id,
                    middle: target(&[id, neighbor]),
                    start: corners[&edge.start].clone(),
                    end: corners[&edge.end].clone(),
                });
            }
        }

        // The converged generator; its trace and offers come when asked for.
        let neighbours: Vec<(RegionId, TerrainOptions)> = settings
            .iter()
            .filter(|(r, _)| **r != id)
            .map(|(r, s)| (*r, s.generator.options().clone()))
            .collect();
        let generator = settings[&id]
            .generator
            .clone()
            .with_edge_targets(&region, &targets, &neighbours)
            .map_err(|e| WorldError::Terrain(id, e))?;
        let shared = || {
            region
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| e.neighbor.is_some())
        };
        let offers = shared()
            .map(|(i, e)| (e.id, (i, OnceLock::new())))
            .collect();
        let crossing_offers = shared().map(|(_, e)| (e.id, OnceLock::new())).collect();
        Ok(Plan {
            region,
            generator,
            trace: OnceLock::new(),
            offers,
            crossing_offers,
        })
    }

    /// Whether the edge between `region` and `neighbor` needs a border:
    /// their reliefs differ, or they are the same but either of them
    /// touches a region with a different relief, whose border could reach
    /// this edge. Both regions on the edge decide alike, from the settings
    /// alone; `settings` must hold `region`'s and every region touching it.
    fn edge_is_active(
        &self,
        region: &Region,
        neighbor: RegionId,
        settings: &BTreeMap<RegionId, Arc<Settings>>,
    ) -> Result<bool, WorldError> {
        let own = &settings[&region.id].generator;
        if !own.same_relief(&settings[&neighbor].generator) {
            return Ok(true);
        }
        Ok(self.touches_other_relief(region.id)? || self.touches_other_relief(neighbor)?)
    }

    /// Whether any region on `region`'s edges or corners has a relief other
    /// than its own.
    fn touches_other_relief(&self, region: RegionId) -> Result<bool, WorldError> {
        let outline = self.region_of(RegionKey::from_id(region));
        let own = self.settings(RegionKey::from_id(region))?;
        let touching = outline
            .corners
            .iter()
            .flat_map(|c| c.incident.iter().copied())
            .chain(outline.edges.iter().filter_map(|e| e.neighbor));
        for other in touching {
            let settings = self.settings(RegionKey::from_id(other))?;
            if !own.generator.same_relief(&settings.generator) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Agrees the river and road crossings of every edge of region `key`
    /// with the regions round it, and builds its terrain at the roughest
    /// tier.
    fn build_roughest(&self, key: RegionKey) -> Result<Terrain, WorldError> {
        let plan = self.plan(key)?;
        let region = &plan.region;
        let id = region.id;
        let seam = |e| WorldError::Seam(id, e);

        // Every shared edge's crossings, from both sides' offers, with their
        // crossing samples only where either side wishes for a crossing. The
        // edge's height profile is already in the plan.
        let mut agreements = Vec::new();
        for edge in &region.edges {
            let Some(neighbor) = edge.neighbor else {
                continue;
            };
            let other = self.plan(RegionKey::from_id(neighbor))?;
            let border = plan.generator.border(edge.id);
            let mismatch = || WorldError::Seam(id, SeamError::Mismatch);
            let wished = |o: &EdgeOffer| !o.rivers.is_empty() || !o.roads.is_empty();
            let mut mine = plan.offer(edge.id, false).ok_or_else(mismatch)?;
            let mut theirs = other.offer(edge.id, false).ok_or_else(mismatch)?;
            if wished(mine) || wished(theirs) {
                mine = plan.offer(edge.id, true).ok_or_else(mismatch)?;
                theirs = other.offer(edge.id, true).ok_or_else(mismatch)?;
            }
            agreements.push(negotiate(edge, mine, theirs, border, &self.elevation).map_err(seam)?);
        }

        plan.generator
            .clone()
            .build_traced(Region::clone(region), &agreements, plan.trace())
            .map(|terrain| terrain.with_scale(self.options.scale))
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

impl LabelField for WorldTerrain {
    /// See [`WorldTerrain::annotate`].
    fn annotate(&self, p: DVec2) -> Option<Labels> {
        WorldTerrain::annotate(self, p)
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

/// One kept value, built once by whichever thread asks first, while any
/// other thread asking waits for that build.
///
/// A build runs on the thread that asked first and may read other values
/// from the caches, but only values that come before it (regions and
/// settings, then plans, then each tier in order), so no wait ever closes
/// a cycle. If a build panics, the next thread to ask builds the value.
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

    /// The value for `key` if it is kept and built, without building it.
    fn peek(&self, key: RegionKey) -> Option<Arc<T>> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (_, slot) = state.slots.get(&key)?;
        slot.get()?.as_ref().ok().cloned()
    }

    /// The value for `key`, built once with `build` if it isn't kept.
    /// Threads asking for the same key while it builds wait for that build
    /// (see [`Slot`]).
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use glam::IVec2;

    use super::*;

    /// Key `i` of the test caches.
    fn key(i: i32) -> RegionKey {
        RegionKey(IVec2::new(i, 0))
    }

    #[test]
    fn a_cold_batch_read_builds_every_region_stage_once() {
        // A batch read builds each region stage it needs once.
        let started: Arc<Mutex<HashMap<(RegionId, RegionStage), usize>>> = Arc::default();
        let hook: ProgressHook = {
            let started = Arc::clone(&started);
            Arc::new(move |event: &RegionProgress| {
                if let RegionProgress::Started { region, stage } = *event {
                    *started.lock().unwrap().entry((region, stage)).or_default() += 1;
                }
            })
        };
        let world = WorldTerrain::new(
            crate::fixtures::world_medium(),
            Arc::new(|id: RegionId, _: DVec2, _: &Elevation| {
                if id.0.is_multiple_of(2) {
                    crate::fixtures::plains_medium().terrain.clone()
                } else {
                    crate::fixtures::rolling_hills_medium().terrain.clone()
                }
            }),
        )
        .unwrap()
        .with_progress(hook);
        let size = world.options().regions.size;
        let points: Vec<DVec2> = (0..40)
            .flat_map(|i| (0..40).map(move |j| DVec2::new(f64::from(i), f64::from(j))))
            .map(|p| (p / 39.0 - 0.5) * 1.5 * size)
            .collect();
        let heights = world.sample_batch(&points, FeatureTier::Rough, |terrain, p| {
            terrain.map(|t| t.height(p))
        });
        assert!(heights.iter().all(|h| h.is_some_and(f32::is_finite)));
        let started = started.lock().unwrap();
        assert!(started.len() > 4, "{started:?}");
        for (key, &count) in started.iter() {
            assert_eq!(count, 1, "{key:?} built {count} times");
        }
    }

    /// A medium world of plains (even region ids, stiffness 1) and
    /// mountains (odd ids, stiffness 3).
    fn plains_and_mountains() -> WorldTerrain {
        WorldTerrain::new(
            crate::fixtures::world_medium(),
            Arc::new(|id: RegionId, _: DVec2, _: &Elevation| {
                let (mut options, stiffness) = if id.0.is_multiple_of(2) {
                    (crate::fixtures::plains_medium().terrain.clone(), 1.0)
                } else {
                    (crate::fixtures::mountains_medium().terrain.clone(), 3.0)
                };
                options.edges.stiffness = stiffness;
                options
            }),
        )
        .unwrap()
    }

    #[test]
    fn neighbouring_plans_agree_on_their_border_in_either_order() {
        // A plains region and a mountains region sharing an edge.
        let probe = plains_and_mountains();
        let a = probe.region_at(DVec2::ZERO);
        let (edge, b) = probe
            .region(a)
            .edges
            .iter()
            .find_map(|e| {
                let n = e.neighbor?;
                (n.0 % 2 != a.0 % 2).then(|| (e.clone(), n))
            })
            .expect("a neighbour of the other kind");
        let border = |world: &WorldTerrain, id: RegionId| {
            let plan = world.plan(RegionKey::from_id(id)).unwrap();
            plan.generator
                .border(edge.id)
                .cloned()
                .expect("the edge is active")
        };

        // Each plan alone in a fresh world, and both in either order.
        let a_alone = border(&plains_and_mountains(), a);
        let b_alone = border(&plains_and_mountains(), b);
        let world = plains_and_mountains();
        let (b_first, a_second) = (border(&world, b), border(&world, a));
        for other in [&b_alone, &b_first, &a_second] {
            assert_eq!(&a_alone, other);
            assert_eq!(format!("{a_alone:?}"), format!("{other:?}"), "bit for bit");
        }
        assert!(!a_alone.is_relative(), "different reliefs share a profile");

        // On the edge both regions' ground is the profile, and it leans to
        // the stiffer mountains: nearer their own ground than the plains'.
        let plans = [a, b].map(|id| world.plan(RegionKey::from_id(id)).unwrap());
        let stiff = usize::from(a.0.is_multiple_of(2));
        let segments = crate::region::segments(&edge.polyline);
        let length = edge.length();
        let (mut to_stiff, mut to_soft) = (0.0, 0.0);
        for k in 1..50 {
            let (p, _) = crate::region::point_at(&segments, length * f64::from(k) / 50.0);
            let [ha, hb] = [0, 1].map(|i| plans[i].generator.ground_height(p));
            assert_eq!(ha, hb, "both regions meet the profile at {p}");
            let own = [0, 1].map(|i| plans[i].generator.natural_height(p));
            to_stiff += f64::from((ha - own[stiff]).abs());
            to_soft += f64::from((ha - own[1 - stiff]).abs());
        }
        assert!(to_stiff < to_soft, "{to_stiff} against {to_soft}");
    }

    #[test]
    fn other_threads_share_one_build_per_key() {
        // Eight threads ask for the same few keys at once; each key
        // is built once and all get that value.
        let cache = Cache::new(100);
        let builds = AtomicUsize::new(0);
        let got: Vec<Vec<u64>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|t| {
                    let (cache, builds) = (&cache, &builds);
                    scope.spawn(move || {
                        (0..4)
                            .map(|i| {
                                let i = (i + t) % 4;
                                *cache
                                    .get(key(i), || {
                                        builds.fetch_add(1, Ordering::Relaxed);
                                        std::thread::sleep(Duration::from_millis(20));
                                        Ok(u64::from(i.unsigned_abs()) * 10)
                                    })
                                    .unwrap()
                            })
                            .collect()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(builds.load(Ordering::Relaxed), 4);
        for (t, values) in got.iter().enumerate() {
            for (i, &v) in values.iter().enumerate() {
                assert_eq!(v, ((i + t) % 4) as u64 * 10);
            }
        }
    }

    #[test]
    fn a_build_that_panics_lets_the_next_thread_build() {
        let cache: Cache<u64> = Cache::new(10);
        let first = std::thread::scope(|scope| {
            scope
                .spawn(|| cache.get(key(0), || panic!("the build fails")))
                .join()
        });
        assert!(first.is_err());
        assert_eq!(*cache.get(key(0), || Ok(7)).unwrap(), 7);
    }
}
