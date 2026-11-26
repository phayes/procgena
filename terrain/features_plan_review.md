# Review of the terrain features proposal

Reviewed on 2026-11-24 against [ARCHITECTURE.md](../ARCHITECTURE.md),
[the terrain plan](plan.md), [the WorldTerrain plan](world_plan.md),
[ROADMAP.md](../ROADMAP.md) and decisions D1 to D3 in
[docs/decisions](../docs/decisions). The proposal itself is
[features_plan.md](features_plan.md); its status is "proposed", and its
section 12 lists the decisions it asks for.

Section labels here are "Conflict 1", "Gap 1" and so on. Those are labels
in this review only, not the roadmap's milestones (C1, G1 and so on).

In short:

- The proposal fits the architecture well. The user asked for it to be
  integrated, so its common feature type and lifecycle are adopted, and
  most of the rest with them (see "Agreements").
- Four points change what the terrain plan or roadmap already say, and
  need the user's call before the phases they touch start (Conflicts 1
  to 4).
- Nothing in flight is blocked: W3b and G1 are unaffected. W4's region
  cache is touched lightly (see "Milestone order").
- The worlds the user signed off on 2026-11-23 ("Terrains look good") stay
  identical through steps 1 and 2. They change only when a preset opts in
  to a new feature.

## How the proposal maps onto the plans

The proposal's migration steps (its section 11), against today's phases
in [plan.md](plan.md#phases) and milestones in
[ROADMAP.md](../ROADMAP.md). This table lives here only; the other
documents link to it.

| Proposal step | What it is | Terrain phase today | Milestone | Waits on |
|---|---|---|---|---|
| 1. Feature extraction | One feature type for rivers, roads and ponds, with their index; results may move, each moved test reported | New | G3, first | Nothing |
| 2. Planning and evaluation split | Stable feature ids, planning stages cached on their own, one batched sample path (height, gradient, water, labels), cold and warm benchmarks, coarse relief without inland road routing | New | G3, second; meets G2 and G4 (see below) | Step 1 |
| 3. First structural features | Plateau and mesa landforms and a cliff profile, heightfield only, with labels and protected corridors | Phase 3 ("landforms") | G3 | Step 2 |
| 4. Heightfield realization and LOD | Regular chunk meshes, coarse vistas, canonical versus provisional | Phase 7, if Conflict 1 goes that way | G6 | Phase 5 (labels split meshes) |
| 5. Parametric geometry | Outcrop and cliff-face meshes; bridge decks | Phase 6; bridge decks after Phase 4 | G6 | Conflicts 1 and 2 |
| 6. Localized volume | Caves, crevices, overhangs | Phase 6 | G6 | Conflict 1 |
| 7. Further hydrology | Lakes as their own family, macro-cell networks, a geology field | Phase 4 | G3 | Conflicts 3 and 4 |

### Milestone order

- **Steps 1 and 2 come first in G3**, before Phase 3's landforms and
  before Phase 4. G1 (finishing Phase 2.8) comes before them, as now.
- **Step 2 meets G2 and G4.** Its one sample path is where G2's analytic
  gradient through carving and G4's `annotate` would live. Two orders
  work:
  - (a) Step 2 first, and G2 and G4 write into its sample path. Nothing
    is written twice, but G2 waits.
  - (b) G2 first, on today's code (it is small), and step 2 then moves
    its gradient into the new path.

  Recommendation: (b) for G2, since C1's Scatters want a terrain slope
  soon; G4 after step 2.
- **W4's region cache is where the staged plans live.** W4 can cache a
  whole region plan as today and split it per stage when step 2 lands.
  It isn't blocked.
- **D4 waits on step 2 for one input.** If Conflict 4 goes to (a), region
  size sets how long a region's plan takes, so D4 should see step 2's
  cold benchmarks before it fixes region sizes.
- **G6 may be reshaped** by Conflicts 1 and 2. It isn't reordered until
  they are answered.

## The proposal's own decisions (its section 12)

| Its question | Where it is handled |
|---|---|
| Adopt TerrainFeature, with specialised River, Road, Cliff and Cave and a composable Landform? | Adopted, because the user asked for the proposal to be integrated. The families beyond River and Road come with steps 3 to 6. |
| Lake as its own family, or standing water in hydrology at first? | Conflict 3 |
| Heightfield meshing by default, volume opt-in? | Conflict 1 |
| How structural features enter natural-height offers versus shared fields; new local features start inside the region only | The "inside only" default is adopted. How a large structural feature (an escarpment across a border) enters the offers is Conflict 5, for when the first one is built. |
| Feature ids, cache versions, composition order, and where provisional ends, before more families | Adopted as a gate inside step 2. Ids: Gap 4. Provisional: Gap 7. |
| Streaming and far-view budgets on real hardware, as gates | Conflict 6: the user picks the hardware and signs off the budgets. Step 2 measures. |

## Decisions (user, 2026-11-23)

- **Conflict 1, meshing:** a heightfield mesh by default, with volume
  meshing only in bounded patches for caves and overhangs.
- **Conflict 2, rock outcrops:** meshes built by the terrain itself, laid
  into the ground. The terrain does not depend on `procgena_stone`.
- **Conflict 3, lakes:** stay in G3 (Phases 3 and 4), as decided before.
- **Conflict 4, macro cells:** planning stays per region. 1 km macro cells
  are added only if step 2's benchmarks show they are needed.
- Conflicts 5 and 6 are still open; they wait for later milestones.
- **Feature tiers (2026-11-24; superseded 2026-11-26 by a highest chunk
  level per feature type, [terrain_lod.md](terrain_lod.md)):** each
  feature type declares which load
  tiers it is drawn at (rough, …, fine), and builds each as it sees fit:
  one algorithm at every tier if cheap, fine only, or a different method
  per tier. Only fine is canonical. See features_plan.md §9, "Feature
  tiers".

## Conflicts (the user decides)

### Conflict 1. Default meshing: one volume everywhere, or heightfield with local volume

- **The terrain plan says:** the whole terrain is a signed distance
  field, meshed everywhere with Transvoxel-style marching cubes
  ([plan.md](plan.md), "Bottom line", "Chunks and meshing"; Phases 6 and
  7). That came from the user's early answer "volumetric from day one",
  but the plan's own open question 1 ("Volumetric reading") was never
  confirmed.
- **The proposal says:** a regular heightfield grid for most ground,
  direct meshes for rock, and volume meshing only in bounded patches
  around caves and overhangs (its sections 8 and 12).
- **What changes:** Phase 7's mesher, and G6's scope. The Transvoxel
  licence question (plan.md open question 5) matters much less if
  marching cubes is only for patches. Both readings keep a pure
  top-surface `height(x, z)` and labels that don't need a mesh.
- **Options:**
  - (a) Heightfield by default, volume only where a feature asks for it
    (the proposal).
  - (b) One volume mesher everywhere (the plan as written).
  - (c) Heightfield now, and decide on volume when the first cave is
    built.
- **Recommendation: (a).** It is cheaper for far views, which story 1
  under "Loading by chunk" needs, and it keeps caves possible. The risk
  is the join between a volume patch and the heightfield, which the
  proposal already says must be tested before patches are used.

### Conflict 2. Rock outcrops: a rock field inside the terrain, or rock meshes

- **The terrain plan says:** open question 6 was answered on 2026-11-22.
  Rock outcrops are Voronoi-cell solids in the terrain's volume, and
  `procgena_stone` stays out of the terrain.
- **The proposal says:** outcrops and cliff faces are direct low-poly
  meshes (blocks, swept profiles, layered strips) laid into the ground,
  with the ground blended around them (its sections 4 and 8).
- **Options:**
  - (a) Meshes of the terrain's own (the proposal). Needs Gap 1 closed.
  - (b) Voronoi solids in the volume, as answered. Needs Conflict 1 (b).
  - (c) Meshes built from `procgena_stone`'s rock shapes. One rock look
    for loose stones and outcrops, and no second rock generator. The cost
    is that the terrain then depends on the stone crate, which the
    2026-11-22 answer kept out, and on whatever that crate pulls in.
- **Recommendation:** (a) or (c), if Conflict 1 goes to (a). The choice
  between them is whether one rock look is worth the dependency.

### Conflict 3. When lakes arrive

- **Today:** Phase 4 builds lakes at hollows, and G3 is done when "lakes
  fill hollows and overflow as rivers". The user decided that deep river
  cuts through hollow rims wait for those lakes (terrain review item 8).
- **The proposal:** a separate Lake family comes last (step 7), and ponds
  stay a hydrology output until then.
- **Options:**
  - (a) Keep lakes in G3, as decided. Build them as hydrology output
    first, and decide whether they become their own family at step 7.
  - (b) Move lakes to step 7, so deep cuts wait longer.
- **Recommendation: (a).** It keeps the decision already taken and
  doesn't rule out the proposal's later choice.

### Conflict 4. Macro cells inside a region

- **Today:** the plan's purity claim ("Every point is a pure function",
  item 4) and Phase 4 rest on 1 km macro cells. Rivers and roads are
  planned per cell, with ports between cells, so planning is bounded.
- **The proposal:** keep planning per region for now, and add macro cells
  later "if needed", with a fixed search budget and a documented fallback
  when routing runs over it (its section 9).
- **What changes:** G3's scope, and how long a region plan takes. "Region
  plans ahead of view" (S1) and D4's region size both depend on that.
- **Options:**
  - (a) Region-level planning first, and macro cells only if step 2's
    benchmarks call for them (the proposal).
  - (b) Macro cells in Phase 4, as planned.
- **Recommendation: (a), with that gate.** D4 should know the answer
  before it fixes region sizes.

### Conflict 5. Large structural features at borders (later)

An escarpment or a geological field that spans regions must appear the
same in both sides' natural-height offers, or the seams break. The
proposal leaves open whether it enters through the natural field (before
offers) or through a shared field both sides sample. Nothing needs this
until the first cross-border structural feature, so it is a question to
settle then, not now.

### Conflict 6. Performance budgets

The proposal asks for streaming and far-view budgets measured on
representative hardware, used as gates before expensive features. Step 2
measures. The user decides which machines count and what the budgets are.
Until then, the D2 timing run owed on a low-end integrated GPU and on a
discrete GPU is the nearest precedent.

### Compatibility wording (no user decision needed)

The proposal asks to "keep existing public River/Road accessors during
migration" and to "avoid changing serialization or seeds without a
deliberate compatibility decision" (its sections 6 and 11). The workspace
rule is that no change keeps compatibility unless the user asks, and F3 is
already renaming the terrain's `seed` and turning its presets into
structs. So the rule stands. Step 1 doesn't have to keep results
unchanged either (user, 2026-11-24: the repo is early): results may move
where the new structure makes that simpler, as long as each moved test is
reported and its intent kept.

## Gaps

- **Gap 1. Labels where a rock mesh stands.** Nothing may stand inside a
  boulder (story 3). The answer is annotation: every mesh a feature adds
  labels and segments the ground over its footprint, so placement knows
  the spot is taken. Changing `height(x, z)` there is optional, chosen
  per feature for how it looks and how things stand next to it. How the
  label reaches the Scatters exactly enough is Gap 3.
- **Gap 2. Slope for every operator.** G2 (`Terrain` as `Ground`) needs
  an analytic gradient through relief, border bending and carving. Each
  new height operator (raise, flatten, terrace, cliff profile) needs one
  too, or a documented fallback to central differences. The proposal
  doesn't say.
- **Gap 3. How a feature's footprint reaches the Scatters.** The proposal
  has features "reserve" footprints and give "coverage/exclusion
  information".
  - **Today:** masks flow only between Scatters inside an Ecotope, and
    Terrain reaches cover only through labels that the Biome's Ecotope
    rules read.
  - **D2's constraints:** hard exclusion is an exact disc test with
    integer maths, the same bit for bit on the CPU and the GPU. Masks
    drive density only.
  - **Candidate:** a feature gives its footprint as fixed-point discs,
    which C2's exclusion test reads like a boulder's disc, plus a terrain
    flag that Ecotope rules read for density.

  This is open. It belongs to C2 (the Scatter-trait milestone), which
  builds the disc tests.
- **Gap 4. Feature ids and seeds.** The proposal's sketch has `seed: u64`.
  Core seeds are 32-bit (D3).
  - A built-in family takes a code seed key with `seed_for` (for example
    `terrain.feature.cliff`), registered in the terrain crate's key
    registry.
  - `child_seed` is for children named in data, such as a landform preset
    a user adds.
  - The parent seed is the World seed for features placed on World cells,
    and `region_seed(W, "terrain.region.local", region)` for choices made
    once per region.

  Proposed, not decided: place local features on World cells wherever
  they can be, because ids tied to a region move if D4 reshapes regions.
- **Gap 5. The inside margin on warped borders.** Features kept inside
  the region fade out before the border margin. Since W3 the border is a
  wiggly line with an amplitude of up to about 134 m. So the margin must
  be measured from the warped border, through the border segment index,
  and that lookup is the cost W3b is cutting. Step 2's benchmarks should
  include warped worlds.
- **Gap 6. A bridge deck is not ground.**
  - The fix for water today ("the road stops cutting or filling over
    water", with a bridge label) stays in Phase 4, as plan.md's Phase 2.7
    bridge item and terrain review items 5 and 6 say. Only the deck mesh
    moves to step 5.
  - `Ground::height` stays the ground under the bridge, so Scatters never
    plant on a deck. The deck goes in the proposal's new surface, ray and
    volume queries.
- **Gap 7. Far-view display versus placement.** The proposal lets a far
  tile be "provisional" until the full plan arrives. That fits as long as
  only drawing uses it, which the proposal's own section 9 says.
  ARCHITECTURE's open question 9 (far copies on coarse terrain) is the
  related item, and stays open.

## Suggestions

- **Suggestion 1. Seed keys in the registry.** New families get dotted
  seed keys (`terrain.feature.cliff` and so on) in the terrain crate's key
  registry, so they are unique and frozen like every other key.
- **Suggestion 2. Measure before building.** Step 2's benchmarks (cold
  and warm, p50/p95/p99, warped borders included) come before step 3, and
  their figures go into plan.md beside the 2026-11-23 road measurements.
- **Suggestion 3. Name it once.** Call the general family
  `LandformFeature` from the start, so it never clashes with the label
  enum `Landform`.

## Agreements

Adopted as written. These are folded into plan.md, world_plan.md and
ARCHITECTURE.md:

- **Features live inside the Terrain**, not as a new level. Terrain makes
  the ground, the Biome binds labels to Ecotopes, and Scatters place on
  the result. A loose stone is a Scatter copy; a formation that shapes the
  ground is a terrain feature.
- **One lifecycle** for every feature (plan, check bounds, evaluate,
  realise, query), keeping the specialised planners (rivers, roads).
  Family, spatial form, stage and geometry are independent.
- **Bounded influence:** every feature declares conservative bounds and a
  maximum reach, contributes nothing outside them, and is never evaluated
  there.
- **Three border policies:** inside the region, negotiated (today's
  ports) and shared field. A new feature uses one of them, and never needs
  a neighbour's full plan. Chunk edges are not region edges: no fade at
  chunk edges.
- **Local features placed** from fixed candidates per World cell, scored
  with a fixed sample budget, independent of request order.
- **The dependency order** of section 7, kept as an outline. The existing
  order inside the relief code doesn't change in the first refactor.
- **Canonical terrain versus display:** placement, masks, gameplay and
  queries use the canonical terrain. Drawing may use coarser meshes,
  chosen by geometric error, and never adds the same height twice.
- **Planning stages cached on their own,** so a far view needs only relief
  and seam offers, not inland road routing. Offers still depend only on
  their own region.
- **Explicit surface queries** for overhangs, bridges and caves.
  `height(x, z)` stays the highest ground surface, with no silent change to
  `Ground`.

## Not in this review's scope

`plan.md` still names `procgena_mesh` in places (the crate is
`procgena_specimen` since F1). That is left for a docs pass.
