//! An endless world of terrains: the region graph, the world elevation E,
//! and a terrain for each region, built when it is first asked for.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
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
use crate::seam::{CornerAgreement, EdgeOffer, SeamError, negotiate, negotiate_corner};
use crate::terrain::{
    HeightField, LabelField, RiverTrace, Terrain, TerrainError, TerrainGenerator,
};

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
/// The first planning stage: relief, river trace and offers. The feature
/// tiers are later stages, cached as terrains of each tier.
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
    /// must be from 0 to 1), if `options.cache` is 0, or if
    /// `options.scale` isn't finite and positive.
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

    /// Resolves ownership in small parallel batches, then holds each
    /// terrain once while sampling all points. Failed regions remain
    /// missing for the whole batch, just as in the point-at-a-time API.
    pub(crate) fn sample_batch<T: Send>(
        &self,
        points: &[DVec2],
        tier: FeatureTier,
        sample: impl Fn(Option<&Terrain>, DVec2) -> T + Sync,
    ) -> Vec<T> {
        let batches: Vec<_> = points.chunks(256).collect();
        let owners: Vec<RegionKey> = crate::batch::map(&batches, |points| {
            let mut regions = HashMap::new();
            let mut sites = HashMap::new();
            points
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
                .collect::<Vec<_>>()
        })
        .into_iter()
        .flatten()
        .collect();
        let keys: Vec<_> = owners
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let terrains: HashMap<_, _> =
            crate::batch::map(&keys, |&key| (key, self.terrain_tier(key.id(), tier).ok()))
                .into_iter()
                .collect();
        let located: Vec<_> = points.iter().zip(&owners).collect();
        crate::batch::map(&located, |&(p, key)| sample(terrains[key].as_deref(), *p))
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

    /// Agrees every corner and edge of region `key` with the regions
    /// round it, and builds its terrain at the roughest tier.
    fn build_roughest(&self, key: RegionKey) -> Result<Terrain, WorldError> {
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

/// One kept value, built once by whichever thread claims it first, while
/// the others wait for that build (single flight).
///
/// No lock is held while the value builds, and a build never runs on a
/// Rayon worker. A worker waiting on Rayon work inside a build picks up
/// other queued jobs on top of it, and if one of those waits for a value
/// whose build needs the build buried below, nothing moves again. So a
/// worker that claims a build hands it to a thread of its own, which keeps
/// the value as soon as it is built; that thread waits for its own Rayon
/// work without picking up other jobs. A worker waiting for a value helps
/// with queued Rayon work meanwhile, so the builds always have workers.
///
/// Rayon work inside a build must not read the world's caches: it may
/// run on a worker that is waiting for a value lower down its stack.
struct Slot<T> {
    /// The value, once built.
    value: OnceLock<Result<Arc<T>, WorldError>>,
    /// Whether a thread is building the value. Cleared again if the build
    /// panics, so another thread can try.
    claimed: AtomicBool,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: OnceLock::new(),
            claimed: AtomicBool::new(false),
        }
    }
}

impl<T: Send + Sync> Slot<T> {
    /// The value, built with `build` if no thread has claimed it yet, else
    /// waited for.
    fn get(
        &self,
        build: impl FnOnce() -> Result<T, WorldError> + Send,
    ) -> Result<Arc<T>, WorldError> {
        let mut build = Some(build);
        loop {
            if let Some(value) = self.value.get() {
                return value.clone();
            }
            // Claim the build, or wait for the thread that has.
            if self
                .claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // The building thread keeps the value itself, so it is
                // ready as soon as it is built, whatever the claiming
                // thread is doing by then.
                let build = build.take().expect("only the claiming thread builds");
                off_worker(|| {
                    let unclaim = Unclaim(&self.claimed);
                    let value = build().map(Arc::new);
                    let _ = self.value.set(value);
                    std::mem::forget(unclaim);
                });
                return self.value.get().expect("kept by the build").clone();
            }
            wait_until(|| self.value.get().is_some() || !self.claimed.load(Ordering::Acquire));
        }
    }
}

/// Runs `build`, on a thread of its own if this is a Rayon worker (see
/// [`Slot`]), helping with Rayon work while it runs. A panic in `build`
/// carries on in this thread.
fn off_worker(build: impl FnOnce() + Send) {
    #[cfg(feature = "rayon")]
    if rayon::current_thread_index().is_some() {
        std::thread::scope(|scope| {
            let builder = scope.spawn(build);
            wait_until(|| builder.is_finished());
            builder
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        });
        return;
    }
    build();
}

/// Clears a slot's claim if its build panics.
struct Unclaim<'a>(&'a AtomicBool);

impl Drop for Unclaim<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Waits until `done` holds. A Rayon worker helps with queued Rayon work
/// meanwhile (see [`Slot`]); any other thread sleeps between looks, a
/// little longer each time.
fn wait_until(done: impl Fn() -> bool) {
    let mut nap = Duration::from_micros(20);
    while !done() {
        #[cfg(feature = "rayon")]
        if matches!(rayon::yield_now(), Some(rayon::Yield::Executed)) {
            continue;
        }
        std::thread::sleep(nap);
        nap = (nap * 2).min(Duration::from_millis(1));
    }
}

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
    slots: HashMap<RegionKey, (u64, Arc<Slot<T>>)>,
}

impl<T: Send + Sync> Cache<T> {
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
        slot.value.get()?.as_ref().ok().cloned()
    }

    /// The value for `key`, built once with `build` if it isn't kept.
    /// Threads asking for the same key while it builds wait for that build.
    /// A build may read other keys first and then run Rayon work, but that
    /// Rayon work must not read the caches (see [`Slot`]).
    fn get(
        &self,
        key: RegionKey,
        build: impl FnOnce() -> Result<T, WorldError> + Send,
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
                let slot = Arc::new(Slot::default());
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
        slot.get(build)
    }
}

#[cfg(all(test, feature = "rayon"))]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::time::Duration;

    use glam::IVec2;
    use rayon::prelude::*;

    use super::*;

    /// Key `i` of the test caches.
    fn key(i: i32) -> RegionKey {
        RegionKey(IVec2::new(i, 0))
    }

    /// Builds value `i` the way region stages build: it first reads the
    /// values it depends on (`i + 1` to `i + 3`, below `n`) from the same
    /// cache, then runs Rayon work of its own on them, slowly enough that
    /// other threads ask for the same keys meanwhile. Counts each build in
    /// `builds`.
    fn value(cache: &Cache<u64>, builds: &AtomicU64, i: i32, n: i32) -> Arc<u64> {
        cache
            .get(key(i), || {
                builds.fetch_add(1, Ordering::Relaxed);
                let below: Vec<u64> = (i + 1..(i + 4).min(n))
                    .map(|j| *value(cache, builds, j, n))
                    .collect();
                let sum: u64 = (0..8u64)
                    .into_par_iter()
                    .map(|k| {
                        std::thread::sleep(Duration::from_micros(300));
                        below.get(k as usize).copied().unwrap_or(0)
                    })
                    .sum();
                Ok(sum + 1)
            })
            .expect("builds")
    }

    #[test]
    fn builds_that_use_rayon_never_wait_on_each_other_forever() {
        // Many Rayon jobs ask for overlapping keys at once, each build
        // running Rayon work of its own: a thread waiting inside a build
        // can pick up another job that needs a key still being built.
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            for round in 0..20 {
                let n = 40;
                let (cache, builds) = (Cache::new(1000), AtomicU64::new(0));
                let mut order: Vec<i32> = (0..n).collect();
                order.rotate_left(round * 7 % n as usize);
                let sums: Vec<u64> = order
                    .par_iter()
                    .map(|&i| *value(&cache, &builds, i, n))
                    .collect();
                // Each key is built once, and every value agrees with one
                // built on a single thread.
                assert_eq!(builds.load(Ordering::Relaxed), n as u64);
                let (single, count) = (Cache::new(1000), AtomicU64::new(0));
                for (&i, &sum) in order.iter().zip(&sums) {
                    assert_eq!(sum, *value(&single, &count, i, n));
                }
            }
            done.send(()).unwrap();
        });
        assert!(
            finished.recv_timeout(Duration::from_secs(120)).is_ok(),
            "the cache deadlocked"
        );
    }

    #[test]
    fn a_cold_batch_read_builds_every_region_stage_once() {
        // Batch reads build regions on Rayon workers; every stage of every
        // region still starts exactly once.
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

    #[test]
    fn other_threads_share_one_build_per_key() {
        use std::sync::atomic::AtomicUsize;

        // Eight plain threads ask for the same few keys at once; each key
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
