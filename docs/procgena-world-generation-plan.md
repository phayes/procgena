# Procgena: large-world biome generation

**Implementation plan | 25 November 2026 | Proposed extension to milestone W0**

## 1. Purpose and architectural decision

Generate an effectively unbounded world of regions, each containing exactly one Biome instance. A Biome selects a Terrain preset (or inline terrain settings) and eventually determines Ecotopes and Scatters. The world must support Minecraft-style random access: requesting a region at any location must return the same result regardless of exploration order, thread scheduling, cache contents or previous queries.

Use **field-conditioned probabilistic biome selection with a fixed number of deterministic synchronous neighbourhood updates**. Geographical fields establish large-scale structure; weighted adjacency preferences encourage locally coherent geography. A rare juxtaposition is natural variation, not a failure to solve a constraint. Do not implement conventional Wave Function Collapse, backtracking, global convergence, or a mutable world-generation frontier.

Keep biome selection separate from terrain construction. The World chooses the Biome; WorldTerrain constructs continuous land. Biome preferences are soft, while physical border agreements remain hard requirements.

### Acceptance requirements

- Identical world seed, generation version, configuration, preset catalogue and RegionId produce identical Biome selection.
- A query uses a bounded neighbourhood independent of distance from the origin and the number of regions already visited.
- Every region has exactly one Biome instance; adjacent regions can share the same preset without sharing instance identity.
- Selection requires no terrain mesh, river plan, Ecotope or Scatter generation.
- Neighbour compatibility is probabilistic; finite positive weights do not prohibit a combination.
- Authored pins remain fixed throughout selection and influence their neighbours.
- World and TerrainSource always agree on a region's final Biome and terrain settings.
- The existing terrain seam, corner, shared noise and feature-crossing guarantees remain intact.
- Cache eviction, parallel execution and repeated requests cannot change results.

## 2. Repository baseline and scope

The current implementation was reviewed through the repository's `master` branch on 25 November 2026. Relevant sources are linked in Appendix A. No repository files were changed, compiled or tested during this planning exercise.

| Existing component | Current responsibility / proposed change |
|---|---|
| `procgena_world::World` | Exposes `biome`, `biome_at`, `fields`, `region_at` and `terrain`. Keep public query API. |
| `world.rs::Choice` | Holds resolved Biomes and TerrainOptions, rules, pins and selector fields. Currently chooses first matching rule, otherwise fallback. Extend with a shared deterministic selector. |
| `options.rs::WorldOptions` | Contains seed (`u32`), scale, region graph, elevation, convergence, selector fields, BiomeRules and pins. Add versioned optional selection settings. |
| `procgena_terrain::WorldTerrain` | Owns region graph, shared elevation, terrain source and lazy terrain construction. Keep independent of biome concepts. |
| `procgena_biome` | Biome references Terrain preset or inline settings. Preserve independent region-specific Biome seed and world-seeded terrain noise. |
| `procgena_core` | Stable hashes, `region_seed`, named seed roles and stable identifiers. Reuse rather than introduce unrelated random functions. |

Current field selection uses E (world elevation), ruggedness and moisture sampled at the region site. `Choice::choose` is called from both `World::biome` and the terrain-source closure. Both paths must continue to use the same final choice. The current defaults use bare-land Biomes `plains`, `rolling_hills` and `mountains`; more ecological or marine Biomes in this document are illustrative, not assumed implemented.

### Scope boundaries

This work extends W0 (World of bare land), not the terrain mesher, Ecotope implementation, GPU placement or streaming system. It must provide the selection foundation for later W1 and streaming. Global river routing, guaranteed connected continents, settlements, climate simulation and complete marine terrain are separate work. Do not make them prerequisites.

## 3. Geometry and scale

Use the existing region graph: a staggered lattice of sites, jittered Voronoi polygons, short-edge collapse and warped shared borders. The logical arrangement is hexagonal, but actual regions need not have exactly six edges. Define neighbours as regions sharing an actual edge, not merely a corner and not six hard-coded axial offsets. If strictly regular hexagons are later required, that is a separate geometry decision.

Retain RegionId and canonical site positions. A region is a geographical unit, not a render chunk or individual mesh tile. Several neighbouring regions with the same preset form a larger biome area. Use the graph's existing finite local construction and stable identifiers; verify actual adjacency is reciprocal and bounded for all supported jitter settings.

| World class | Region spacing | Horizontal scale | Vertical scale |
|---|---:|---:|---:|
| Small | 2 km | 0.5 | 0.5 |
| Medium-small (default) | 2 km | 1 | 1 |
| Medium | 4 km | 2 | 2 |
| Large | 8 km | 6 | 4 |
| Real life (stretch goal) | 16 km | 20 | 8 |

These are existing draft values in `presets.rs`, not performance targets or a new scale decision. All field wavelengths continue to follow horizontal scale; elevation preferences follow vertical scale. Region spacing is in real metres. Objects, LOD and render chunks must not be scaled by this selector.

## 4. Mathematical model

### 4.1 Inputs and notation

Let G=(V,E) be the actual undirected region adjacency graph, i a region, N(i) its neighbours, d(i) its degree, b a candidate Biome key, and B_t(i) its assignment after pass t. The candidate catalogue is a validated, stable ordered set. The World seed is a u32, consistent with current code.

Compute F(i) = (elevation, ruggedness, moisture) at the canonical region site using existing fields. Keep elevation independent of selected Biomes. Additional fields may be introduced later through a versioned schema.

### 4.2 Intrinsic suitability

For each candidate b, define a positive prevalence w_b and optional field preferences. A Gaussian-shaped preference provides a simple initial implementation:

    U_i(b) = ln(w_b) - 0.5 * sum_k (((x_ik - centre_bk) / spread_bk)^2)

Omitted fields contribute zero. All spreads must be finite and strictly positive. Evaluate elevation in reference-scale metres (divide sampled elevation by vertical scale), or equivalently scale both centre and spread. Ruggedness and moisture remain in their native dimensionless units. Prevalence is a relative prior, not a guaranteed global frequency. Support one-sided preferences later if needed; do not implement arbitrary scripting in the first version.

The original field ranges are not automatically equivalent to Gaussian preferences. Preserve the original selector as a legacy mode until weighted presets are authored and reviewed. If physical constraints require a genuinely unavailable candidate, represent eligibility separately from preferences; e.g. a terrain type that cannot be constructed in a supported world configuration. Validate that at least one eligible candidate exists. Do not turn ordinary geographical improbability into hard rejection.

### 4.3 Compatibility

Define C(a,b)>0 as the relative affinity of neighbouring Biome keys. Prefer a symmetric matrix for the first implementation. Store unordered pairs canonically, with an explicit default for unspecified pairs. Resolve keys to dense indices during World construction and precompute log affinities.

Example, illustrative only:

| | Plains | Rolling hills | Mountains |
|---|---:|---:|---:|
| Plains | 1.0 | 0.9 | 0.25 |
| Rolling hills | 0.9 | 1.0 | 0.9 |
| Mountains | 0.25 | 0.9 | 1.0 |

Weights are affinities, not literal probabilities. Multiplying all affinities by the same constant does not alter a softmax distribution. Self-affinity promotes same-type clusters; high cross-affinity promotes natural transitions. Zero, negative, nonfinite or contradictory duplicate pair entries are invalid in v1. Hard exclusions are explicitly out of scope for this matrix.

### 4.4 Initial assignment and updates

For every unpinned region, draw B_0(i) from softmax(U_i / T). At each subsequent pass t=1..K, compute candidate scores using only the previous pass:

    S_i,t(b) = U_i(b) + lambda / d(i) * sum_j ln(C(b, B_(t-1)(j)))
    P_i,t(b) = exp((S_i,t(b)-max(S_i,t))/T) / sum_c exp((S_i,t(c)-max(S_i,t))/T)

For degree zero, the neighbour term is zero. T>0 is temperature; lambda>=0 is neighbourhood strength. The degree normalization prevents regions with extra Voronoi neighbours from automatically receiving stronger adjacency influence. This is a synchronous probabilistic cellular update inspired by generalized Potts/Markov random field models; it is not claimed to sample the equilibrium distribution of a Potts model or monotonically optimize a global energy.

Draw a categorical sample with a deterministic, region-and-pass-specific random value. A pinned region returns its specified key at every pass, including pass zero, even if its suitability is low. Pin validity means the preset exists and its terrain settings resolve, not that it wins field scoring.

Use f64 internally for score accumulation and stable softmax. Subtract the maximum before exponentiation. Extremely small positive mathematical probabilities can underflow to zero in finite precision; document this, bound unreasonable configuration ranges if needed, and do not promise that every nonzero affinity yields a representable probability. No convergence stopping rule: K is fixed in the versioned configuration.

### 4.5 Determinism details

Use `procgena_core::region_seed(world_seed, "world.biome.selection", region_id)` and derive a pass-specific seed via an existing stable core hash helper. Freeze the role string and pass derivation; add it to the repository's seed-key checks. Do not consume RNG state shared across regions or across passes. Map the hash to a specified uniform interval such as [0,1), then choose using cumulative weights in canonical candidate-key order. Specify the boundary convention and last-positive-weight fallback for rounding.

Sort neighbours by RegionId before accumulation; avoid depending on HashMap iteration order, input catalogue ordering or parallel reduction order. Retain existing Biome instance and Terrain local seed roles. A selector change may change the chosen preset, but it must not change the seed derivation for the region's instance. Cross-platform bitwise guarantees require explicit numerical implementation and golden tests; until then promise order independence on the supported numerical implementation, not identical transcendental rounding on every platform.

## 5. Bounded random access

The final choice B_K(i) depends only on B_(K-1) at adjacent regions, which depend on the preceding pass, and so on. Hence all initial values needed are within graph distance K of i. The dependency radius is independent of world size and exploration history.

For a regular hexagonal lattice, a radius-r disk contains D(r)=1+3r(r+1) regions. For K=3, the computation is:

| Stage | Radius retained | Regions evaluated |
|---|---:|---:|
| Initial field samples and B0 | 3 | 37 |
| Pass 1 | 2 | 19 |
| Pass 2 | 1 | 7 |
| Pass 3 | 0 | 1 |
| Total | | 64 |

For K passes, regular-lattice work is sum_(r=0..K) D(r) = (K+1)^3 region evaluations including initialization, before multiplication by candidates and neighbour degree. Relaxation-only evaluations are K^3. The actual jittered graph may differ; use BFS graph-distance rather than this formula in production. Degree and graph construction bounds must be checked for supported options. Do not recursively recompute the same `(RegionId, pass)` pair.

### 5.1 Reference algorithm (bottom-up)

1. Collect the actual edge-adjacency graph within distance K of target i using BFS. Store the minimum distance for each RegionId, stable adjacency, and canonical site.
2. Evaluate fields and intrinsic scores once per collected region; initialize B0 for all collected regions. Pins take precedence.
3. For t in 1..K, evaluate only regions whose distance from i is <= K-t. Read exclusively from the previous pass buffer. Each required neighbour is in the previous buffer by the triangle inequality for graph distance.
4. Swap previous and current buffers after the entire pass completes. Return B_K(i).

No artificial edge conditions or fallback neighbour values are necessary. Outer layers are discarded before their unknown outside neighbours could influence the answer. A multi-target query uses the union of their K-neighbourhoods and retains regions within distance K-t of any target. It must return the same values as individual queries.

### 5.2 Rust-oriented reference pseudocode

The following names are proposed interfaces, not a claim that these methods currently exist. Adapt to the actual RegionGraph APIs.

```rust
fn select(&self, target: RegionId) -> BiomeIndex {
    let k = self.options.iterations;
    let nodes = self.graph_ball(target, k);
    let intrinsic = self.intrinsic_scores(&nodes);
    let mut prev = self.initialize(&nodes, &intrinsic);

    for pass in 1..=k {
        let mut next = BTreeMap::new();
        for node in nodes.within_distance(k - pass) {
            let id = node.id;
            let chosen = if let Some(pin) = self.pin(id) {
                pin
            } else {
                let scores = self.score(
                    &intrinsic[&id],
                    node.sorted_neighbours(),
                    &prev,
                );
                self.sample(id, pass, &scores)
            };
            next.insert(id, chosen);
        }
        prev = next;
    }
    prev[&target]
}
```

Start with this reference implementation and prove correctness before adding a persistent memoization layer or parallel evaluation. An isolated unpinned node with zero neighbours samples its intrinsic scores on every pass. When lambda=0, final assignments remain independent weighted samples, but need not equal B0 because each pass has its own seed. K=0 means initial selection only.

## 6. Code architecture and integration

Suggested structure within `world/crates/procgena_world/src/`:

```text
world.rs                 existing World, shared Choice and TerrainSource
options.rs               existing WorldOptions + versioned selection config
presets.rs               existing WorldPreset catalogue
selection/mod.rs         validated selector and shared entry point
selection/suitability.rs field scores and candidate resolution
selection/compatibility.rs dense affinity matrix and validation
selection/relaxation.rs  graph-ball evaluation and multi-target query
selection/sampling.rs    stable seeded categorical draws
selection/cache.rs       optional bounded derived-data cache
```

Keep the selector engine-free. Do not add Bevy or GPU dependencies. Reuse existing core hashing and terrain graph. `World::with_biomes` should resolve presets, validate the selector, resolve pins and construct a graph/elevation context before exposing the World. Use a shared immutable selector/context (e.g. Arc) for both public World queries and TerrainSource. The existing terrain closure receives RegionId, site and `&Elevation`; the selector will additionally require access to geometry-only neighbour/site lookup. Inspect RegionGraph ownership/API; share the same graph or construct an identical immutable graph using exactly the same seed/options. Never ask WorldTerrain to build terrain during selection and never create a reference cycle from the selector to WorldTerrain.

Proposed internal interface:

```rust
struct BiomeSelector { /* validated immutable settings */ }
impl BiomeSelector {
    fn select(&self, id: RegionId, e: &Elevation) -> BiomeIndex;
    fn select_many(&self, ids: &[RegionId], e: &Elevation)
        -> Vec<BiomeIndex>;
}
```

`select_many` preserves input order, including duplicates, and is optional for the first correctness milestone. Ensure the elevation argument cannot vary inconsistently with the selector's fixed World; if necessary bind one immutable elevation context instead of accepting arbitrary elevation. Return `RegionBiome` as today: region, preset, existing biome_seed, pinned flag and FieldValues. Selection errors belong at World construction; valid queries should not silently fall back due to an invalid configuration.

### Terrain integration sequence

Resolve geometry and neighbours; obtain final Biome assignments; resolve their TerrainOptions; apply existing shared E and relief convergence; negotiate seams, corners and crossings; build only the requested terrain and subsequent coverage. The selector must not change terrain options directly. Shared terrain noise remains world-seeded; Ecotope and Scatter seeds remain region-derived. Existing pins choose Biome only; height bumps continue to be the independent mechanism for authored elevation.

## 7. Configuration and migration

Add optional `biome_selection` to WorldOptions with serde defaults so existing `.world.yml` files and built-in presets remain valid. When absent, retain the current first-match `BiomeRules`/fallback semantics. When present, weighted mode uses its explicit candidate list; legacy rules remain stored for compatibility but are not consulted for selection. Document this precedence and reject conflicting/ambiguous entries within weighted configuration. A later explicit migration may make weighted selection default after tuning; do not silently change existing seeded worlds.

Proposed configuration example (extension to a complete existing World preset, not a standalone currently valid file):

```yaml
biome_selection:
  version: 1
  iterations: 3
  temperature: 0.8
  neighbour_strength: 1.5
  suitability:
    plains:
      prevalence: 1.0
      ruggedness: { centre: -0.5, spread: 0.6 }
    rolling_hills:
      prevalence: 1.0
      ruggedness: { centre: 0.3, spread: 0.6 }
    mountains:
      prevalence: 0.7
      elevation: { centre: 150.0, spread: 80.0 }
      ruggedness: { centre: 0.8, spread: 0.5 }
  compatibility:
    default: 0.5
    pairs:
      - { a: plains, b: plains, weight: 1.0 }
      - { a: plains, b: rolling_hills, weight: 0.9 }
      - { a: plains, b: mountains, weight: 0.25 }
      - { a: rolling_hills, b: rolling_hills, weight: 1.0 }
      - { a: rolling_hills, b: mountains, weight: 0.9 }
      - { a: mountains, b: mountains, weight: 1.0 }
```

Use serde types consistent with the existing `serde-saphyr` preset pattern; reject unknown fields. Proposed types: `BiomeSelectionOptions`, `BiomeSuitability`, `FieldPreference`, `BiomeCompatibility`, `CompatibilityPair`. At construction validate known keys, uniqueness, nonempty candidates, finite positive prevalence/spread/temperature/affinity, finite nonnegative lambda, bounded K, and valid version. Pins may name any resolved Biome, even if not in the weighted candidate list: include pinned-only keys in the affinity lookup but never sample them as free candidates. Validate compatibility pair endpoints against the resolved catalogue; require explicit treatment of unspecified affinities through the default. Preserve legacy pin errors and duplicate detection.

Initial experimental settings: K=3, T=0.8, lambda=1.5. Propose maximum K=8 for v1 to cap cost; confirm with benchmarks. The default maximum is not an artistic requirement. All values and suitability centres above require visual and statistical tuning.

## 8. Performance, caches and streaming

Keep selection cheap enough for map previews independent of terrain building. With B candidates, degree d and K fixed, each relaxation evaluation costs O(B*d); regular-lattice total cost is O(B*d*K^3), plus field/geometry lookup. Initial field evaluations are O(K^2) and score arrays O(B*K^2). Two rolling assignment buffers avoid retaining all passes in the reference implementation. Actual graph bounds depend on its geometry.

Cache only derived data. Begin without persistent selection cache; profile first. Add bounded cache entries for geometry/site/neighbours if not already cached, field values/intrinsic scores, intermediate `(RegionId, pass)` assignments and final assignments. A per-World cache may rely on immutable context identity; shared or persisted caches must include generation version and content identity of seed, graph, scale, fields, suitability, affinities, catalogue and pins. Cache only completed deterministic values. Never hold a cache lock during recursive dependency evaluation. Duplicate computation is acceptable initially; use bounded eviction and ensure locks cannot deadlock across overlapping requests. Clear caches on configuration replacement. A pin affects at most K graph steps of final selection (geometry unchanged); selective invalidation is optional, not necessary in v1.

For regional map previews, batch queries over the viewport plus K-step dependency halo and discard halo-only results. Cache final selections independently of Terrain. Existing lazy Terrain construction, terrain-stage caches and streaming remain separate. Benchmark cold single-region selection, warm selection, overlapping viewport queries, memory usage, and concurrent requests. Report median and p95 on a documented machine, for K=0,1,3,5,8 and representative catalogue sizes. Establish latency and memory budgets from measurements rather than inventing targets. Confirm no terrain plan is created by selection-only workloads.

## 9. Verification and acceptance tests

### Deterministic unit and property tests

- Golden seed vectors for multiple positive/negative lattice coordinates and passes, using existing RegionId encoding. Golden categorical sample vectors and boundary conventions.
- Stable ordering: permute catalogue input and neighbour enumeration; equivalent configurations yield identical choices. Verify stable key and affinity resolution.
- Numeric tests: equal-score distribution, dominant scores, low temperature, extreme finite values, finite results, rejection of invalid config and no-eligible-candidate handling if eligibility is implemented.
- K=0, lambda=0, degree zero, one candidate, pinned-only candidate, duplicate pins, unknown keys and unsupported version.
- Reference simulation over a large graph with sufficient halo: compare every interior region against independent `select`; repeat for varied K, seeds, scale, jitter, negative coordinates and far-from-origin positions.
- Actual graph neighbour reciprocity, corner-only non-adjacency and graph-radius correctness for short-edge collapse and warped borders.
- Different query sequences, threads, cache capacities/eviction and batch partitions produce identical selections. Pins are immutable and their influence cannot propagate beyond K steps with other inputs fixed.
- Legacy mode retains existing golden outputs and old `.world.yml` deserialization. Weighted schema round-trips through the existing preset loader.

### Integration tests

- `World::biome`, `biome_at` and the TerrainSource choose identical presets/options at the same region.
- Selection produces no Terrain build; validate using a build counter or test seam.
- Adjacent different and same Biome types preserve existing seam and corner checks with E, relief convergence and border negotiation.
- Terrain and Biome instance seed derivations are unchanged; same-type adjacent regions remain distinct instances (seed collisions remain theoretically possible).
- Run relevant crate tests and the workspace's established CI commands; document any pre-existing failures rather than treating them as passing.

### Statistical and visual evaluation

Generate reproducible preview maps across a fixed seed set and scale classes. Record Biome proportions, unordered edge-type frequencies, mean log-affinity, connected-component size distribution per Biome, singleton frequency, field suitability distribution and selection cost. Compare K=0 against K>0 and vary lambda/T independently. Use the same fields and seeds for comparisons; do not assert that every pass or every seed improves compatibility. Review rare adjacencies visually, and tune them rather than automatically eliminating them. Check for checkerboard/oscillating patterns, excess uniformity, overly fragmented regions and field mismatch. Keep representative golden map images or stable region-key fixtures for regression, separate from artistic tuning.

## 10. Delivery milestones

| Milestone | Work | Exit gate |
|---|---|---|
| W0a | Versioned schema, suitability, deterministic categorical sampling, legacy migration policy | Validation and legacy regression tests pass; K=0 works. |
| W0b | Affinity matrix, candidate resolution and single-pass scoring | Matrix symmetry/default tests and pin tests pass. |
| W0c | Actual graph-ball traversal and bounded synchronous reference implementation | Random-access output equals reference simulation with sufficient halo. |
| W0d | Shared selector integrated with World and TerrainSource | No circular dependency; public selection and terrain agree; seam tests pass. |
| W0e | Profiling, batching and bounded cache if justified | Parallel/order/cache invariants pass; measured latency and memory reported. |
| W0f | Existing large-world example and later Studio controls | Preview maps and metrics available without building terrain. |
| W0g | Tune weighted presets, migration review | Developer and project owner review visual/statistical outcomes before changing defaults. |

Deliver code, configuration documentation, unit/integration tests, benchmark results, representative map previews and a short architectural decision record. Update `ARCHITECTURE.md`, `ROADMAP.md` and `terrain/world_plan.md` to reflect the weighted mode while preserving field-first ownership. Do not mark the original W0 implementation as absent; these milestones are extensions to it.

## 11. Open decisions and risks

- Geometry: preserve current jittered polygons (recommended here) or require exactly six logical neighbours independent of physical edges. The plan assumes actual shared-edge adjacency; confirm with project owner.
- Default migration: legacy mode remains default until weighted presets are tuned; confirm whether new worlds should opt in automatically after that point.
- Suitability representation: Gaussian scores are the smallest useful first implementation; one-sided ranges or authored curves may be necessary for ocean/coast and extreme elevations.
- Numerical contract: define supported platform reproducibility and whether a deterministic approximation to exp/log is warranted.
- Parameter limits: K=8 is a proposed safety cap, pending measurements. Avoid treating prototype settings as final artistic choices.
- Global coherence: local updates cannot guarantee long-distance topology or drainage. Such properties require separate field design or bounded hierarchical systems.
- Synchronous updates may oscillate or produce weakly compatible combinations; fixed K avoids nontermination. If visually problematic, consider a deterministic retain/refresh schedule in a new algorithm version, not an order-dependent in-place update.

The intended outcome is geographically coherent, naturally varied worlds, not maximum adjacency score. This distinction should govern implementation, testing and tuning.

## Appendix A. Repository references

- Architecture: https://github.com/phayes/procgena/blob/master/ARCHITECTURE.md
- Roadmap (W0, W1, D3–D5): https://github.com/phayes/procgena/blob/master/ROADMAP.md
- WorldTerrain plan: https://github.com/phayes/procgena/blob/master/terrain/world_plan.md
- World implementation: https://github.com/phayes/procgena/blob/master/world/crates/procgena_world/src/world.rs
- World settings: https://github.com/phayes/procgena/blob/master/world/crates/procgena_world/src/options.rs
- World presets: https://github.com/phayes/procgena/blob/master/world/crates/procgena_world/src/presets.rs
- Biome: https://github.com/phayes/procgena/blob/master/biome/crates/procgena_biome/src/biome.rs
- Seed derivation: https://github.com/phayes/procgena/blob/master/core/crates/procgena_core/src/seed.rs

These links point to a moving branch; pin a commit when implementation begins. The proposed schema, algorithms and Rust signatures are design specifications, not verified existing APIs or compiled code.
