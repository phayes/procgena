# Terrain features: architecture proposal

Status: proposed for review, 2026-11-23. This document supplements [the architecture](../../ARCHITECTURE.md), [the terrain plan](../../terrain/plan.md) and [the world terrain plan](../../terrain/world_plan.md). It proposes changes to those plans, not claims that the changes have already been implemented.

Reviewed 2026-11-24 in [terrain-features-review.md](terrain-features-review.md). That review holds what has been adopted into the other plans, how the migration steps map onto the terrain phases and roadmap milestones, and the points that wait for the user (meshing, rock outcrops, lakes, macro cells, and later border features and performance budgets).

## Bottom line

- A **TerrainFeature** is a planned, identifiable, spatially bounded contribution to the terrain. It can change the heightfield, add or remove volume, generate a mesh, provide annotations, or combine these outputs.
- Road and River are the first specialized feature families. Retain their existing planning and carving algorithms; generalize their lifecycle, storage, indexing and queries.
- Add Cliff (including bluffs and escarpments), Cave (including caverns and crevices), and a composable general-purpose Landform family. Lake is a proposed additional specialized family, because containment, spill levels and outlets differ from river routing; initially Pond may remain a hydrology output.
- A feature's family, spatial form, evaluation stage and geometry representation are independent. Do not make heightfield and volumetric features mutually exclusive classes.
- Keep TerrainFeature inside the terrain crate, not as a sixth level of the world architecture. Terrain constructs the ground; Biome binds its labels to Ecotopes; Scatters populate it.
- **Speed is a first-class requirement:** deterministic random access, bounded planning work, no mesh dependency for point queries, sparse local volumetric evaluation, independently cached stages and direct coarse realization. Far vistas must not wait for full road routing or detailed meshes.
- Preserve negotiated region seams and canonical world identity at every LOD.

## 1. Motivation and scope

The current noise-based relief is suitable for broad elevation, but landscapes with distinct grassy shelves, exposed angular rock faces, terraces, coherent river valleys and isolated outcrops need structures at multiple scales. Noise remains useful for shaping and perturbing these structures; it need not be responsible for generating all of them.

A feature is not just an object placed on a surface. It can construct its surroundings and provide the information that later terrain and cover generators consume. A detached decorative stone belongs in StoneScatter; a rock formation that changes the ground, creates a cliff, or controls the top surface belongs in Terrain.

This proposal does not require geological simulation, a global drainage pass, an arbitrary scene graph interpreter in the per-sample path, or volumetric meshing of the entire world.

## 2. Existing implementation

The terrain crate already implements a specialized feature engine:

| Existing code | Responsibility |
|---|---|
| [relief.rs](../../terrain/crates/procgena_terrain/src/relief.rs) | Natural relief, shared world noise and convergence |
| [plan.rs](../../terrain/crates/procgena_terrain/src/plan.rs) | River sources, port wishes, river/pond/road planning, valley profiles |
| [carve.rs](../../terrain/crates/procgena_terrain/src/carve.rs) | FeatureLine, Pond, Features, Carved, RoadTags and spatially indexed carving |
| [road/](../../terrain/crates/procgena_terrain/src/road/mod.rs) | Routing, grade fit, cut/fill, crossing costs |
| [river.rs](../../terrain/crates/procgena_terrain/src/river.rs) | Downhill tracing and joins |
| [terrain.rs](../../terrain/crates/procgena_terrain/src/terrain.rs) | TerrainGenerator, Terrain, seam conformance, point and tile queries |
| [seam.rs](../../terrain/crates/procgena_terrain/src/seam.rs), [ports.rs](../../terrain/crates/procgena_terrain/src/ports.rs) | Symmetric border agreements and agreed crossings |
| [grid.rs](../../terrain/crates/procgena_terrain/src/grid.rs) | Bounded planning grids |

Today TerrainGenerator::offers traces own rivers and proposes road and river crossings. TerrainGenerator::build constructs the conformed base and eagerly calls plan::features. Features holds rivers, valley shapes, ponds, roads, bank shapes and points of interest; a 64 m uniform spatial index holds segment references. Features::apply evaluates river/pond lowering, then road cut/fill, water and road tags. Road proximity also suppresses gullies.

These are the foundations for TerrainFeature. Do not replace working routing, carving or seam negotiation merely to introduce the abstraction.

The existing terrain plan anticipates a heightfield-backed implicit field, volumetric modifiers, provenance labels, macro-cell feature caches and marching cubes/Transvoxel. This proposal changes the default meshing strategy and planning granularity, while retaining the useful field interface.

## 3. Position in the architecture

```text
World
  WorldTerrain: E, region graph, terrain source, seam protocol
    Terrain: natural relief + planned TerrainFeatures
      River / Road / Cliff / Cave / Landform / (Lake)
      height, gradient, water, volume, annotation, spatial queries
  Biome: one Terrain and rules mapping labels to Ecotopes
    Ecotope -> Scatter -> Asset
```

The engine-free terrain crate owns feature options, presets, planning, evaluation, annotation and geometry output. The eventual Bevy crate owns streaming, scheduling, draw LOD and rendering. WorldTerrain remains unaware of Biomes and Ecotopes. A feature's geometry and effects must be queryable without drawing it.

TerrainFeature and Scatter have different contracts:

| TerrainFeature | Scatter |
|---|---|
| Contributes to the ground | Consumes Ground |
| May change relief, water, volume and traversability | Places copies on the resulting ground |
| Unique planned spatial structure | Specimen/copy placement |
| Emits terrain provenance, flags and measures | Emits coverage/masks and copies |

The existing pure, keyed, Send + Sync hook rules apply to feature extensions. Biome rules read feature effects through Terrain annotations, not through a dependency on particular feature implementations.

## 4. Feature families

Each family owns its options, presets, planning and composition rules. Use a family when there is a substantially distinct planning algorithm or geometric model, not for every visually distinguishable feature.

| Family | Variants | Main rules |
|---|---|---|
| River | Streams, rivers, rapids, waterfalls, valley profiles | Downhill flow, drainage, joins, level and width, agreed ports |
| Road | Trails, roads, switchbacks | Grade, turn radius, routing, cut/fill, water crossings, agreed ports |
| Cliff | Cliffs, bluffs, escarpments, exposed strata | Crest/toe paths, height and face angle, bedding, fracture orientation, apron |
| Cave | Caves, caverns, crevices, fissures, tunnels | Subtractive solids, overburden, entrances, bounded connectivity |
| Landform | Mesas, plateaus, terraces, isolated outcrops, depressions, clearings, arches | Composable shapes, profiles, blends and optional volumes/meshes |
| Lake (proposed) | Ponds, lakes, wetland basins | Containment, flat water level, shoreline, spill and outlet |

### River and Road

Keep them distinct despite their common polyline geometry. Reuse path sampling, segment indexing, longitudinal interpolation and cross-sectional evaluation where useful, without conflating their rules. River valleys are part of the river's terrain effect, not unrelated decorations. Road can generate cuttings, embankments and eventually retaining walls or bridge requirements.

Road planning may read river plans. At a bridge, the road stops filling/cutting the river corridor; the bridge supplies a deck and optional structure, with a separate surface/query contract from the ground beneath. This addresses the existing known issue in the feature probes where road carving removes river water, including at confluences (see [Phase 2.7](../../terrain/plan.md#road-rework-phase-27)).

### Cliff

A cliff/bluff is a longitudinal path or boundary with a varying cross-section. Its crest and toe may differ in plan position. Parameters can include height, face inclination, top and base blending widths, overhang, rock layers, fracture spacing, talus apron and roughness. An escarpment may be a major structural feature before hydrology; a bluff exposed by a river can be planned afterwards.

Prefer a smooth, low-amplitude grass surface and directional rock detail on steep faces. Generate cliffs from sustained slopes, high local relief, ridge/plateau edges or riverbanks; alternatively construct an escarpment in an otherwise smooth slope. A parametric face (swept profile, layered strips, fractured blocks) can create much of the desired appearance without a volumetric solver.

### Cave

Use bounded subtractive implicit volumes: capsule chains, warped tunnels, ellipsoidal chambers, fracture planes, and limited smooth unions. A cave may generate an entrance on a cliff or bluff and may affect top-surface queries at openings. A crevice may be open to the sky rather than a closed tunnel.

Only chunks overlapping a conservative 3D influence bound should evaluate the cave. Limit depth, reach, primitive count and blend radius. Never rely on a worldwide noise-defined cavity that requires evaluating every column to discover whether it exists.

### Landform

This is the general-purpose family, implemented initially as a small set of composable bounded operators rather than a separate custom generator for each shape.

- Height operators: raise, lower, replace/blend to a target, flatten, terrace, warp.
- Spatial supports: circles/ellipses, compact polygons, paths, bounded noise masks, implicit solids.
- Geometry operators: direct parametric solids or surfaces, optional implicit union/subtraction/intersection.
- Semantic outputs: provenance, flags, measures, surface material hints and coverage/exclusion information.

Examples: mesa = flat-topped lift with a steep edge; outcrop = angular solid plus terrain foundation blend; basin = bounded depression; terrace = remapped distance/elevation profile; clearing = flatten plus semantic annotation.

Compile preset compositions into a small, validated, bounded set of operators at planning time. No recursive graph interpretation in the inner sampling loop. A feature may compose other primitives without becoming a new family.

## 5. Spatial form, placement and orientation

Spatial form is independent of family and output:

| Form | Examples |
|---|---|
| Point/footprint | Outcrop, peak, sinkhole, clearing |
| Path/network | River, road, ridge, escarpment, cave tunnel |
| Area | Lake, plateau, basin, terrace system |
| Volume | Cavern, arch, overhanging rock |

A feature descriptor contains a stable address/seed, family-specific planned data, conservative horizontal influence bounds, geometry bounds, and a maximum error/reach description where applicable. Point features can use a transform; paths use a local tangent/normal frame and profiles along distance; area and volume features have their own coordinate maps. Avoid imposing a single position/quaternion representation on an entire river.

### Placement

For local features, generate a fixed number of deterministic candidates from world-space cells (or region-local addresses anchored to the world), then score them with family-specific suitability. Use bounded exclusion/spacing rules and deterministic tie-breaks. Candidate acceptance must not depend on which chunk is requested first.

Useful inputs are natural or earlier-stage height, gradient, slope, curvature, local relief at the feature's scale, distance to boundaries and existing features, water/drainage hints, and geological parameters. Use a fixed sample budget per candidate, sharing coarse grids when practical.

Examples:

- Cliff: sustained steep terrain or a planned crest, adequate height difference and space for a toe.
- Outcrop: exposed ridge or convex hillside, suitable rock province, clear of roads and river corridors unless intentionally associated.
- Cave entrance: exposed face with adequate overburden and clearance.
- Talus: downhill from a parent cliff or fractured outcrop.

Point feature cells may have several fixed spatial scales (e.g. tens of metres for boulders, hundreds for cliffs, kilometres for major landforms). These are feature-planning cells, not rendering chunks. Rivers and roads may retain region/macro-cell network planning rather than forced point sampling.

### Orientation and geology

Use the existing x-z ground plane, y up and f64 world positions. Given gradient g = grad h, downhill is -g and the horizontal contour tangent is perpendicular to g. A cliff normally faces downhill and extends along the contour. Compute derivatives at the scale of the feature, not from sub-metre roughness. In near-flat terrain, use a seeded or geological direction rather than an unstable normalized gradient.

An optional shared geological field can provide rock type, bedding strike/dip, fracture directions and layer thickness. This is a low-cost, spatially coherent field, independent of render LOD. Nearby rock formations should share compatible orientations instead of independently random rotations. Defer a full geological model; a few regional parameters and smoothly varying fields are enough initially.

### Integration zones

Each feature has a core, a terrain integration zone and an optional wider influence zone. The core produces the main shape. The integration zone blends terrain into the feature. The influence zone changes labels, material hints or secondary placement. Zones need not be circular or symmetric; talus is preferentially downhill and river influence follows its corridor.

A bounded target-height blend is one useful operator: h_new = lerp(h_old, h_target, weight), with weight smoothly zero at its support boundary. Other operators use min/max, clamps, or additive deltas as appropriate. Do not force all effects through additive blending.

A rock outcrop may raise its foundation, expose rock, reserve its occupied footprint and emit a downhill scree hint. Terrain supplies these hints and labels; the cover stack can realize small debris through StoneScatter. Secondary terrain features may be planned in a bounded later stage using parent-derived ids, never via unbounded recursion.

## 6. Planning, evaluation and realization

Planning determines what exists; evaluation determines the canonical terrain; realization determines what geometry is needed to display it. These follow the architecture's existing separation of plan, place and draw.

A conceptual Rust interface (not a final API):

```rust
struct TerrainFeature {
    id: FeatureId,
    seed: u64,
    bounds: FeatureBounds,
    data: FeatureData,
}

enum FeatureData {
    River(RiverPlan),
    Road(RoadPlan),
    Pond(PondPlan),
    Cliff(CliffPlan),
    Cave(CavePlan),
    Landform(LandformPlan),
    // Lake(LakePlan), once lake planning exists.
}
```

Prefer concrete typed data and an enum for built-ins. Trait objects may be appropriate for keyed generator hooks, but not a mandatory allocation or virtual call per sample. Keep existing public River/Road accessors during migration. Avoid changing serialization or seeds without a deliberate compatibility decision.

The common lifecycle is:

1. Plan from immutable earlier-stage inputs, options and stable seed.
2. Validate bounds and index the descriptor or its segments.
3. Evaluate requested fields (height, gradient, water, provenance, volume) without constructing a mesh.
4. Realize geometry directly at a requested detail/error budget.
5. Expose feature queries and optional points of interest.

A feature does not have to implement every output. A river emits a carve profile and water surface; a road emits grading and road provenance; a cave emits a bounded subtraction; an outcrop may emit height, a solid and independent geometry.

Stable addresses should follow the existing seed/address conventions: owner region or shared edge or world cell, family/key salt, and deterministic local index. Shared port pieces need one canonical identity even if both regions hold evaluation copies. Mesh ownership/clipping must prevent double drawing. Cache keys include the inputs, options, seam agreements, feature/hook versions and relevant earlier-plan hashes. Eviction and regeneration must not change results.

## 7. Generation dependencies and borders

Family is not an evaluation stage. A major escarpment can precede hydrology while a river bluff follows it; a mesa can precede roads while a clearing follows them.

Proposed dependency graph:

```text
E + natural relief / structural landforms
  -> convergence, analytic erosion, natural-height offers
  -> own river traces + road wishes (self-only)
  -> symmetric seam / port agreements
  -> conformed base
  -> hydrology plans and profiles
  -> road/infrastructure plans (read hydrology)
  -> local cliffs, outcrops, caves and clearings (respect earlier constraints)
  -> final terrain evaluation and annotation
  -> Ecotope and Scatter placement
```

This is a dependency outline, not a demand to change the existing ordering of relief internals in the initial refactor. Any structural feature affecting the natural field must appear consistently in the natural-height offers, river source tracing, and conformance inputs. If a structural feature needs neighbor-dependent information, use a shared pure field or explicit negotiation, not a recursive neighbor build.

Planning and evaluation dependencies differ. For example, river height carving precedes road grading, but river gullies can read the already-planned road footprint. Preserve the existing deepest-river-lowering and road-clamp composition. Define explicit precedence and overlap rules; use stable iteration order for noncommutative operators. Later features must respect earlier protected corridors unless their purpose is a negotiated interaction.

### Border behavior

Three policies:

- **Interior:** bounded support and smooth fade before the protected seam margin; initial default for local cliffs, outcrops and caves.
- **Negotiated:** shared path/shape constraints with agreed crossing geometry; existing Road and River ports are the model.
- **Shared field:** both sides derive compatible features from the same world-space inputs; for larger geological structures, still account for region-specific relief/conformance so the final border matches.

Preserve height and slope continuity, symmetric offers and corner agreements, and the rule that outbound wishes never depend on inbound flow. Do not require a neighboring full feature plan. A new feature must either stay away from a seam, participate in the natural field before negotiation, or explicitly conform to a shared agreement. Chunk boundaries are not region boundaries: they must sample the same canonical features and lattice, with no arbitrary fade at chunk edges.

## 8. Geometry: heightfield first, sparse 3D where needed

Heightfield effects and fully 3D geometry belong to the same feature abstraction. Preserve the fast analytic heightfield as the default representation. A heightfield can be represented implicitly by F(x,y,z) = y - h(x,z); this is an implicit field, not generally an exact signed distance. Bounded volume operators may combine it with other implicit fields using min/max or smooth blends.

Use three realization paths:

| Path | Use |
|---|---|
| Regular heightfield grid | Most ground, terraces, valleys, ordinary slopes and far vistas |
| Direct parametric mesh | Angular rock faces, slabs, outcrops, bridge decks, retaining walls |
| Local volumetric meshing | Caves, connected overhangs and genuinely complex surface topology |

Avoid general-purpose mesh Booleans and marching cubes for ordinary terrain. Prefer direct low-poly convex blocks, deformed prisms, swept cliff profiles and fractured strata. Fine LODs add deterministic subdivisions and perturbations to the same base shape.

A direct feature mesh may intersect the ground intentionally, but must have an explicit integration/ownership contract to avoid hidden surfaces, gaps and z-fighting. If a mesh replaces the terrain surface or opens a cavity, mask or replace the affected heightfield region and stitch to the surrounding terrain. A localized volumetric patch needs a tested transition to the regular heightfield lattice and to different LODs. Until this is proven, restrict complex patches or use the existing volumetric plan for them.

Keep analytic top-surface and annotation queries independent of rendered meshes. Height(x,z) is the highest terrain surface, including features where applicable, in keeping with the existing terrain plan. It is not sufficient for navigation under overhangs, on a bridge or in a cave; add explicit surface/ray/volume queries for those cases rather than changing Ground semantics silently.

## 9. Real-time generation and far vistas

### Current cost and coupling

The repository's recorded measurements (2026-11-23, [road rework](../../terrain/plan.md#road-rework-phase-27)) report roughly 80–170 ms for road-grid heights, 25 ms for plains routing and 0.6–1.1 s for mountain routing after A* improvements. These are historical measurements, not new benchmarks. The current TerrainGenerator::build eagerly plans features, and the architecture proposes preplanning a region ring and disk caching.

Preplanning and caching are useful, but not sufficient for cold distant views, teleportation and rapid flight. A mountain silhouette should not wait for a fine road search.

### Independently cacheable plans

Separate natural/structural relief, seam inputs and agreements, hydrology, infrastructure, and local features into independently reusable immutable plans. Preserve the requirement that exact seam agreements need the port offers; skipping inland road routing does not imply skipping those offers. Cache own-river traces used by both offers and final planning rather than rerunning them. Use per-key single-flight builds, bounded caches and versioned disk persistence. Expensive planning runs on workers; cancel or deprioritize superseded draw requests.

Region-level network planning can remain initially. Later subdivide large regions into the macro cells envisioned in the terrain plan with negotiated ports, bounded network windows, and fixed candidate/search budgets. Do not introduce a global hydrology dependency. If a route search exceeds its planning budget, use a deterministic documented fallback rather than a timing-dependent result.

### Feature indexing and batch evaluation

Generalize the existing 64 m segment index. Index conservative effect bounds (including blends), not only visible geometry. Long paths are indexed by segments; very large area features can use a coarser level instead of being replicated into many fine buckets. Give each feature type a bounded maximum reach and density; bound the number of relevant planning cells and primitive evaluations.

For a tile, resolve candidate lists once or in spatial buckets, and evaluate only affected samples. A feature-centric stamp path may be useful for compact operators; preserve overlap reductions and deterministic order. The scalar point evaluator remains the correctness reference.

Source-level opportunities to profile before optimizing:

- Features::apply collects road candidates in a Vec per relevant sample; reuse scratch space or use an allocation-free path.
- Terrain::height, water_level and road_tags currently repeat base and carving work; add a unified sample path with requested outputs.
- Reuse segment projections, precomputed coefficients and coarse planning samples.
- Fill height/gradient/labels in one batch when needed; avoid calculating expensive annotations for height-only requests.
- Reuse tile buffers and index topology, batch GPU uploads, avoid per-sample allocation.
- Benchmark SIMD and GPU compute for parallel sampling; retain CPU reference and CPU network routing.

A feature with no intersecting support should incur no feature-specific inner-loop evaluation. Do not evaluate full 3D fields for every height sample.

### Canonical terrain versus display LOD

Feature identity and placement are independent of view, load tier, chunk size and generation order. The canonical terrain and feature effects determine Scatter placement, masks, gameplay, edits and authoritative queries. Render approximations must never become the source of truth.

For distant display, select detail using projected size and conservative geometric error, not distance alone. A major cliff or mountain still matters many kilometres away. Coarse meshes and filtered detail should preserve the same broad silhouette, absolute position, water levels and major landforms. Suppress only contributions whose error is acceptable at that tier, or mark the result provisional until the canonical plan arrives. Unplanned contributions without a defensible error bound cannot be assumed negligible.

| Feature | Near | Far |
|---|---|---|
| River | Bed, banks, gullies, water mesh | Valley and major water ribbon |
| Road | Cut/fill, graded surface, bridge | Coarse ribbon or omitted if below error budget |
| Cliff | Fractured face, shelves, strata | Coarse crest/toe profile |
| Cave | Detailed local volume | Entrance proxy or omitted where invisible |
| Landform | Detailed terrain integration and mesh | Base profile or low-poly proxy |

Generate meshes directly at the requested LOD from a shared descriptor; do not require generation of a high-resolution mesh first. Distant rock proxies may use a low-poly solid or heightfield projection, but never add the same elevation twice. Near/far transitions need stitching or controlled morphing. Keep the common world sampling lattice; filter high-frequency noise consistently rather than introducing unrelated terrain at a distance.

Use the architecture's coarse-first streaming, request-driven loading, zoom-aware detail and cancellation. A distant visual tile may be provisional, but any dependent Scatter placement must use true ground and canonical exclusion masks (the existing “no trees in boulders” user story). (Since 2026-11-26 "true ground" is the canonical TIN of the Scatter's placement level, and far ground is canonical too; see [terrain_lod.md](../../terrain/terrain_lod.md).) If that information is unavailable, defer the affected placement rather than inventing different placements.

### Feature tiers (decided 2026-11-24, user)

> **Superseded 2026-11-26 (the user):** every chunk level is a canonical
> TIN level, and each feature type has a highest level where its lines
> are kept and its carving sampled. Tiers give way to those cut-off
> levels; see [terrain_lod.md](../../terrain/terrain_lod.md).

A region's features are planned in **tiers**, in order: the relief and
border offers first, then each tier of features from the roughest to
**fine**. Fine is always the last tier, and only the terrain with every
fine feature is canonical. Anything drawn from an earlier tier is
provisional (drawing only); placement, masks and queries read fine.

- **Each feature type declares the tiers it is drawn at**, honestly, by
  its cost. How it builds each one is its own business:
  - **The same algorithm at every tier**, when that is cheap enough
    (rivers today: the trace is cheap, so a rough river is the fine one).
  - **Fine only**, when it is cheap but not needed far away, or has no
    useful rough form: it is planned only in the last tier, and earlier
    tiers draw the ground without it.
  - **A different method per tier**, when the fine form is costly: roads
    could route on a coarse grid, or run straight from each agreed border
    crossing to the hub, at a rough tier, and do today's 8 m search at
    fine.
- **Dependencies set the order.** A feature can't be drawn at a tier
  earlier than anything it reads. Roads read rivers, and gullies read the
  nearest road, so a rough road needs rivers by that tier, and a gully
  that is fine only can't feed a rough road.
- **Earlier tiers stay close to fine.** A feature's earlier forms keep its
  agreed border crossings, so neighbours still meet; stay inside its
  fine form's bounds; and declare the largest height difference from the
  fine form (for a fine-only feature, its whole effect). Those bounds set
  the skirts between chunks of different tiers, instead of today's fixed
  skirt depth.

Today's road-free stage is the two-tier case: rivers and ponds at every
tier, roads fine only. Two tiers are enough to start; more let far views
drop detail step by step.

**Built (2026-11-24):** `FeatureTier` (`Rough`, `Fine`), each
`FeatureKind`'s `tiers` and `reads` with the order check as a test,
`Terrain::with_tier` and `tier_difference`, `WorldTerrain::terrain_tier`
caching each tier, and skirts sized from the measured difference. A
fine-only feature's bound is measured once it is planned (roads can cut
as deep as they need, so nothing bounds them up front); chunks beside a
finer chunk are redrawn with deeper skirts when it lands. Not built: a
different method per tier, which waits for its first user (rough roads).

## 10. Queries, annotations and interaction

Extend the planned Landform/Flags/Measures approach rather than introduce a separate classification system. To avoid confusion between the existing annotation enum Landform and the general-purpose feature family, use a disambiguated Rust name such as LandformFeature or LandformPlan.

Provenance identifies the feature that made a surface (road, riverbed, cliff face, cave floor); morphology describes slope, curvature, relief and similar measurements. A unified terrain sample can share height, gradient, water and provenance work; requested expensive measures may be evaluated only when needed. Distance-to-road/water queries should reuse appropriate indexes, with explicit search radius or a documented bounded approximation.

Expose features intersecting a rectangle or volume, stable feature lookup, and points of interest without creating render meshes. Add surface-specific queries for bridges, overhangs and caves as needed. Annotation resolution must define priority when features overlap, while allowing multiple flags or contributors.

## 11. Migration and performance gates

1. **Behavior-preserving extraction.** Move/generalize the current Features representation, index and typed plans. Preserve seeds, public accessors, river reductions, road ordering, port behavior and existing fixtures. Keep River, Road and Pond as first concrete feature types.
2. **Planning and evaluation split.** Add stable ids, bounds, reusable own-river plans, independently cached planning stages and unified/batch sampling. Add cold and warm benchmarks before new complexity. Make coarse relief/seam display possible without inland road routing.
3. **First structural features.** Add bounded Landform presets for plateau/mesa and a Cliff profile generator, initially heightfield-only. Establish before/after-hydrology placement rules. Add semantic labels and protected corridors.
4. **Heightfield realization and LOD.** Implement regular chunk meshes, coarse vistas, canonical/provisional distinction and transition tests. Establish the performance envelope on target hardware.
5. **Parametric geometry.** Add rock outcrops, cliff faces and exposed strata with local terrain integration and direct LOD. Implement road/river bridge interaction when infrastructure realization is ready (can proceed independently of rock meshes); preserve water at crossings.
6. **Localized volume.** Add Cave/cavern/crevice and connected overhangs, bounded implicit fields and heightfield-volume transitions. Benchmark before making volumetric work part of normal chunk generation.
7. **Further hydrology and generalization.** Lake containment and spill planning, macro-cell networks if needed, more presets and optional geological field; introduce specialized Lake only as warranted by the implementation.

Suggested module layout (incremental, not a compulsory file move):

```text
src/
  feature/
    mod.rs          # descriptor, ids, outputs
    index.rs
    evaluation.rs
    placement.rs
    river.rs
    road.rs
    cliff.rs
    cave.rs
    landform.rs
    lake.rs         # later
  mesh/
    heightfield.rs
    parametric.rs
    volumetric.rs
    transitions.rs
  terrain.rs
  relief.rs
  seam.rs
  ...
```

### Tests and measurements

Keep the existing seam, convergence, elevation and feature suites. Add:

- Same canonical chunk/feature addresses in shuffled order, concurrent requests, eviction/rebuild and different render LODs.
- Height and slope at region edges/corners; identical port effects from both sides; chunk-edge equality at common world lattice samples; no duplicated shared meshes.
- Feature support tests: outside the declared bounds, no height/volume/semantic contribution; bounded overlap and child generation.
- River monotonicity and joins, road grade, preserved water under bridge, protected corridors, correctly integrated cliff/outcrop footprints.
- Top-surface versus cave/bridge queries, no false ground on openings, no cracks at heightfield/volume or LOD transitions.
- Coarse/fine silhouette and geometry-error measurements; Scatter placement and masks independent of render LOD.
- Benchmarks: cold region/offer/feature-stage planning; cache hits and memory; scalar point and batch tile samples; typical and worst-case feature density; near-chunk generation and meshing; cold far-vista workload; teleport/fast flight and concurrent requests.

Record p50/p95/p99 latency, throughput, allocations, peak memory and frame-time impact on identified hardware. Set budgets based on those measurements before adding expensive stages. Do not claim that a particular chunk latency is achieved before measuring it. In particular, keep a heightfield-only baseline to compare against parametric and volumetric additions.

## 12. Decisions for review

- Adopt TerrainFeature as the common abstraction inside Terrain, with specialized River/Road/Cliff/Cave and composable Landform?
- Separate Lake eventually, or keep all standing water in the hydrology family initially?
- Make regular heightfield meshing the default and sparse 3D an opt-in realization, revising the terrain plan's universal implicit mesher?
- How should structural features contribute to natural-height offers versus shared fields? Start new local features with interior-only support.
- Define feature ids, cache versioning, deterministic composition rules and the canonical/provisional boundary before expanding the feature catalogue.
- Establish target streaming and far-vista budgets on representative hardware, then use those as gates for the feature implementation.

The aim is not to make all features behave alike. It is to make their lifecycle, spatial influence, queries and realization predictable while retaining their specialized algorithms. River and Road are already the prototype for that architecture.
