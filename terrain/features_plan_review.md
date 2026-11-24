# Review of the terrain features proposal

Reviewed on 2026-11-24 against [ARCHITECTURE.md](../ARCHITECTURE.md),
[the terrain plan](plan.md), [the WorldTerrain plan](world_plan.md),
[ROADMAP.md](../ROADMAP.md) and decisions D1 to D3 in
[docs/decisions](../docs/decisions). The proposal itself is
[features_plan.md](features_plan.md); its status is "proposed", and its
section 12 lists the decisions it asks for.

In short: the proposal fits the architecture well and most of it is
adopted as written (see "Agreements"). Four points change things the
terrain plan or roadmap already say, and need the user's call before the
phases they touch start (see "Conflicts"). None of them blocks work in
flight: W3b, W4 and G1 are unaffected.

## How the proposal maps onto the plans

The proposal's migration steps (its section 11) against today's phases in
[plan.md](plan.md#phases) and milestones in [ROADMAP.md](../ROADMAP.md).
This is the one place the mapping is kept; the other documents link here.

| Proposal step | What it is | Terrain phase today | Milestone | Waits on |
|---|---|---|---|---|
| 1. Behaviour-preserving extraction | One feature type for rivers, roads and ponds, with their index | New, before Phase 4 | G3 (first part) | Nothing |
| 2. Planning and evaluation split | Stable feature ids, separately cached planning stages, one batched sample path, cold and warm benchmarks, coarse relief without inland road routing | New, before Phase 4 | G3 (first part) | Nothing; §C4 decides how far it goes |
| 3. First structural features | Plateau and mesa landforms and a cliff profile, heightfield only, with labels and protected corridors | Phase 3 ("landforms") | G3 | Step 2 |
| 4. Heightfield realization and LOD | Regular chunk meshes, coarse vistas, canonical versus provisional | Phase 7, if §C1 goes that way | G6 | Phase 5 (labels split meshes) |
| 5. Parametric geometry | Outcrop and cliff-face meshes, bridges | Phases 4 (bridges) and 6 | G3 (bridges), G6 | §C1, §C2 |
| 6. Localized volume | Caves, crevices, overhangs | Phase 6 | G6 | §C1 |
| 7. Further hydrology | Lakes as their own family, macro-cell networks, geology field | Phase 4 | G3 | §C3, §C4 |

## Conflicts (the user decides)

### C1. Default meshing: one volume everywhere, or heightfield with local volume

- **The terrain plan says:** the whole terrain is a signed distance
  field, meshed everywhere with Transvoxel-style marching cubes
  ([plan.md](plan.md), "Bottom line", "Chunks and meshing"; Phases 6 and
  7). That came from the user's early answer "volumetric from day one",
  but the plan's own open question 1 ("Volumetric reading") was never
  confirmed.
- **The proposal says:** a regular heightfield grid for most ground,
  direct parametric meshes for rock, and volume meshing only in bounded
  patches around caves and overhangs (its sections 8 and 12).
- **What changes:** Phase 7's mesher, the Transvoxel licence question
  (plan.md open question 5, which matters much less if marching cubes is
  only for patches), and G6's scope. Both readings keep a pure top-surface
  `height(x, z)` and annotations that don't need a mesh.
- **Options:**
  - (a) Heightfield by default, volume only where a feature asks for it
    (the proposal).
  - (b) Keep one volume mesher everywhere (the plan as written).
  - (c) Heightfield now, and decide on volume when the first cave is
    built.
- **Recommendation: (a).** It is cheaper for far views, which story 1
  under "Loading by chunk" needs, and keeps caves possible. The risk is
  the seam between a volume patch and the heightfield, which the proposal
  already says must be tested before patches are used.

### C2. Rock outcrops: a rock field inside the terrain, or rock meshes

- **The terrain plan says:** open question 6 was answered on 2026-11-22:
  rock outcrops are Voronoi-cell solids in the terrain's volume.
- **The proposal says:** outcrops and cliff faces are direct low-poly
  meshes (blocks, swept profiles, layered strips) laid into the ground,
  with the ground blended around them (its sections 4 and 8).
- **Options:**
  - (a) Parametric meshes (the proposal). This needs the gap in §G1
    closed: the terrain must still answer height and occupancy where a
    rock mesh stands.
  - (b) Voronoi solids in the volume, as answered. This needs C1 (b).
  - (c) Parametric meshes, built by reusing `procgena_stone`'s rock
    shapes rather than a new rock mesher (see §S1).
- **Recommendation: (c),** if C1 goes to (a). One rock look across the
  stones and the outcrops, and less new code.

### C3. When lakes arrive

- **Today:** Phase 4 builds lakes at hollows, and G3 is done when "lakes
  fill hollows and overflow as rivers". The user decided that deep river
  cuts through hollow rims wait for those lakes (terrain review item 8).
- **The proposal:** a separate Lake family comes last (migration step 7),
  and ponds stay a hydrology output until then.
- **Options:**
  - (a) Keep lakes in G3 as decided. Build them as hydrology output first,
    and decide whether they become their own family when step 7 comes.
  - (b) Move lakes to step 7, so deep cuts wait longer.
- **Recommendation: (a).** It keeps the decision already taken and
  doesn't stop the proposal's later choice.

### C4. Macro cells inside a region

- **Today:** the plan's purity claim ("Every point is a pure function",
  item 4) and Phase 4 rest on 1 km macro cells: rivers and roads are
  planned per cell, with ports between cells, so planning is bounded.
- **The proposal:** keep planning per region for now, and add macro cells
  later, "if needed", with a fixed search budget and a documented
  fallback when routing runs over it (its section 9).
- **What changes:** G3's scope, and how long a region plan takes, which
  "Region plans ahead of view" (S1) and D4's region size depend on.
- **Options:**
  - (a) Region-level planning first, macro cells only if the benchmarks
    from step 2 say so (the proposal).
  - (b) Macro cells in Phase 4, as planned.
- **Recommendation: (a), with a gate.** Step 2's cold benchmarks decide.
  D4 should know the answer before it fixes region sizes, since bigger
  regions make per-region planning slower.

### C5. Compatibility wording (no user decision needed)

The proposal asks to "keep existing public River/Road accessors during
migration" and "avoid changing serialization or seeds without a
deliberate compatibility decision" (its sections 6 and 11). The workspace
rule is that no change keeps compatibility unless the user asks, and F3 is
already renaming the terrain's `seed` and turning its presets into
structs. So the rule stands. What survives from the proposal is a
behaviour test: step 1 must leave every feature fixture and test result
unchanged. It is not a promise about the API.

## Gaps

- **G1. Height where a rock mesh stands.** A parametric outcrop that
  isn't part of `height(x, z)` breaks two rules: placement samples the true
  ground ("Placement does not depend on distance"), and nothing stands
  inside a boulder (story 3). Every mesh a feature adds must also give the
  terrain an analytic height or occupancy over its footprint, used by
  `height`, `annotate` and Scatter exclusion.
- **G2. Slope for every operator.** G2 (`Terrain` as `Ground`) needs an
  analytic gradient through relief, border bending and carving. Each new
  height operator (raise, flatten, terrace, cliff profile) needs one too,
  or a documented central-difference fallback. The proposal doesn't say.
- **G3. How a feature's footprint reaches the Scatters.** The proposal has
  features "reserve" footprints and emit "coverage/exclusion information".
  Today masks flow only between Scatters inside an Ecotope (D2,
  "Masks and placement"), and Terrain reaches cover only through labels
  that the Biome's Ecotope rules read. Proposed path: a footprint becomes
  a terrain flag (for example `OCCUPIED` or `ROCK`) plus its analytic
  occupancy. Ecotope rules read the flag for density, and C2's exact
  exclusion tests read the occupancy, as they read a boulder's disc. This
  needs settling with C2, the Scatter-trait milestone.
- **G4. Feature ids and seeds.** The proposal's sketch has `seed: u64`.
  Core seeds are 32-bit (`derive_seed`, `child_seed`, `region_seed` in D3).
  A feature id should follow D3:
  - a frozen family key under a code `kind` key, via `child_seed`;
  - a region-wide choice from `region_seed(W, "terrain.region.local",
    region)`;
  - a World-cell feature from the World seed and its cell.

  Ids anchored to a region inherit D4's risk: if D4 reshapes regions,
  those ids move. Features placed on World cells don't.
- **G5. The interior margin on warped borders.** "Interior" features fade
  out before the protected seam margin. Since W3 the border is a wiggly
  polyline with an amplitude of up to about 134 m. The margin must be
  measured from the warped border, through the border segment index, and
  that lookup is the cost W3b is cutting. Step 2's benchmarks should
  include warped worlds.
- **G6. A bridge deck is not ground.** The proposal gives bridges a
  separate surface query. `Ground::height` stays the ground under the
  bridge (the riverbank and the water), so Scatters never plant on a deck.
  The deck goes in the "surface, ray and volume" queries the proposal
  adds. This should be written into the `Ground` docs when bridges land.
- **G7. Far-view display versus placement.** The proposal allows a far
  tile to be "provisional" until the canonical plan arrives. That fits,
  as long as only drawing uses provisional tiles. Its own rule says
  dependent placement waits (section 9). ARCHITECTURE's open question 9
  (far copies on coarse terrain) is the related open item and stays open.

## Suggestions

- **S1. Reuse the stone generator for outcrop rock.** `procgena_stone`
  already makes rock shapes, with a GPU path and LOD. An outcrop could
  take a stone specimen, scaled and embedded, plus the terrain's footprint
  blend and occupancy (§G1), instead of a second rock mesher.
- **S2. Feature seed keys in the registry.** New families get dotted seed
  keys (`terrain.feature.cliff` and so on) in the terrain crate's key
  registry, so they are unique and frozen like every other key.
- **S3. Measure before building.** The proposal's step 2 benchmarks
  (cold and warm, p50/p95/p99, warped borders included) should come
  before step 3, and their figures go into plan.md beside the 2026-11-23
  road measurements.
- **S4. Rename at once.** Name the feature family `LandformFeature` from
  the start, so it never clashes with the label enum `Landform`.

## Agreements

Adopted as written. These are folded into plan.md, world_plan.md and
ARCHITECTURE.md:

- **Features live inside the Terrain**, not as a new level. Terrain makes
  the ground, the Biome binds labels to Ecotopes, and Scatters place on
  the result. A loose stone is a Scatter copy; a formation that shapes the
  ground is a terrain feature.
- **One lifecycle** for every feature (plan, check bounds, evaluate,
  realise, query), with specialised planners kept (rivers, roads). Family,
  spatial form, stage and geometry are independent.
- **Bounded influence:** every feature declares conservative bounds and a
  maximum reach. A feature outside its bounds contributes nothing, and is
  never evaluated there.
- **Three border policies:** interior, negotiated (today's ports) and
  shared field. A new feature must use one of them, and never needs a
  neighbour's full plan. Chunk edges are not region edges: no fade at
  chunk edges.
- **Placement of local features** from fixed candidates per World cell,
  scored with a fixed sample budget, independent of request order.
- **The dependency order** of section 7, kept as an outline. The existing
  order inside the relief code doesn't change in the first refactor.
- **Canonical terrain versus display:** placement, masks, gameplay and
  queries use the canonical terrain; drawing may use coarser meshes, chosen
  by geometric error, never adding the same height twice.
- **Separately cached planning stages,** so a far view needs only relief
  and seam offers, not inland road routing. Self-only offers are kept.
- **Explicit surface queries** for overhangs, bridges and caves;
  `height(x, z)` stays the highest surface, with no silent change to
  `Ground`.

## Not in this review's scope

`plan.md` still names `procgena_mesh` in places (the crate is
`procgena_specimen` since F1). That is left for a docs pass.
