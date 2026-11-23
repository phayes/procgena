# procgena_fruit: plan

Fruit and flowers become their own asset, in a new crate, `procgena_fruit`,
with a Bevy crate beside it. This plan covers what moves out of the tree,
what the new crate owns, how the tree uses it, what it costs to draw, and
the order of the work. It is a plan only. The coordinator changes the
docs (see "Proposed doc edits"). The code waits for F2, for D3 with its
Addresses (section 7b of [D3](../docs/decisions/D3.md)), and for the
user's answers, which are recorded under "Decisions".

Names below are the ones after F1: `procgena_specimen`, `Specimen`,
`GeneratedSpecimen`, `specimen()`.

## Bottom line

- **New crates.** `fruit/crates/procgena_fruit` (engine-free) and
  `fruit/crates/procgena_fruit_bevy`. One crate covers fruit and flowers,
  as the user named it. It has **no Scatter**: the tree is its only placer
  today.
- **A fruit specimen is one whole cluster**, built in its own cluster
  frame: an apple on its stalk, a dogwood bloom, or a willow catkin spray.
  The tree keeps *where* clusters grow (which branches, how many, which
  way they face). The fruit crate owns *what a cluster is*: its kind,
  items, stalks, shape, colour and maturity, plus its detail levels and
  its impostor.
- **A new idea in the hierarchy: the attached asset.** This is an asset
  whose copies are placed by a host asset, not by a Scatter. It is not a
  Modifier. The section "Asset, Modifier or something new" gives the
  reasons. An attached copy has no id of its own. Its address is the
  host's address plus a part path pinned to each specimen, such as
  `/ fruit 17 / item 2`, as D3's Addresses define it.
- **The World limits fruit like any other pool.** Each fruit options value
  is a pool with `N` slots. A slot is a fruit specimen seeded from its
  `SpecimenId`. Each cluster on a tree picks a slot.
- **Slots are chosen per tree specimen, not per tree copy.** Every copy
  of a tree specimen then carries the same fruit. As a result:
  - fruit can be baked into the tree's own impostor;
  - the grove draws fruit from the tree's existing visible lists, with no
    extra culling;
  - the tree's impostor bake key must include the fruit pool's hash and
    `N`.
- **What differs between slots must be added for single-item clusters.**
  Today Apple and Dogwood clusters are one item each. Their slots would
  differ only by size and spin, and a copy's transform already carries
  both. So `N` would buy nothing. This plan adds seeded per-specimen
  variation, which the user approved (see "What differs between a pool's
  N slots").
- **Cost.** A whole Willow (783 clusters, the coordinator's figure) costs:
  - a 783-entry cluster table, about 50 KB, once per tree specimen;
  - for each tree copy in fruit range, one instanced draw per fruit slot
    and level, expanded on the GPU.

  There are no per-cluster mesh records and no per-cluster culling.
- **Detail is chosen in two steps.** A coarse check on the CPU, once per
  tree per view, gives the range of fruit levels the tree's clusters can
  be in. A fine check on the GPU, once per cluster, runs only for trees
  whose range spans more than one level.
- **Impostor requirements.** `procgena_impostor` gains three things:
  - per-bake view count and frame size;
  - view coverage declared by each preset, as the lowest viewer
    elevation baked: the full sphere (−90°) by default, and −45° for
    trees so they look right from below on a slope. Coverage is part of
    the bake key, and the atlas resolution goes only where it is
    declared;
  - an instanced path whose instance is the tree's record plus a cluster
    index, with the cluster's rigid wind offset added in the vertex
    shader.
- **Order.**
  - The extraction and the pools are one step, A1+C1, done once in the
    cluster-local frame after F2 and D3. A separate bit-exact move was
    considered and turned down (see "Move map").
  - Fruit's own detail levels use R1's LOD type.
  - Fruit impostors follow I1.
  - I1b is replaced by this plan.

## What exists today

All of it lives in `procgena_tree`:

| Piece | Where | Fate |
|---|---|---|
| Cluster skeleton: `SkeletonCluster`, `ClusterStalk`, `ClusterItem`, `grow_clusters`, `ClusterBuilder`, `Path` | `src/cluster.rs` | Placement (`grow_clusters`, `density_count`) stays in tree; `ClusterBuilder`, `Path` and the three types move to fruit, rebuilt in the cluster frame |
| Meshers: `mesh_fruit`, `mesh_flowers`, `mesh_clusters`, `ItemSelection`, `Stalks`, `ClusterWind`, `fruit.rs`, `flower.rs` | `src/cluster_mesh/` (about 1,540 lines) | Move to fruit, except `ItemSelection`'s cluster stride (see below) |
| Options: `FruitOptions`, `FlowerOptions`, `ClusterPlacement`, `Facing`, `ClusterShape`, `ClusterKind`, `FruitShape`, `FlowerShape` | `src/options.rs` | Split, see "Options" |
| LOD fields: `LodDetail::{cluster_stride, item_stride, item_scale, item_detail, item_stalks, flat_item_size, min_item_size, billboard_fruit}`, `ItemDetail` | `src/mesh.rs` | `cluster_stride` and a cluster scale stay; the rest become fruit's own levels |
| Outputs: `TreeMesh::{fruit, flowers, fruit_spans, flower_spans}`, `BillboardCull` fruit, `ClusterSpan` | `src/mesh.rs` | Replaced by a per-tree cluster table |
| Hash: `Hash for FruitOptions`, `FlowerOptions`, `ClusterPlacement` | `src/tree_id.rs` | The tree hashes its placement plus the fruit's options hash |
| Materials: `FruitMaterial`, `FlowerMaterial`, `fruit_material`, `flower_material`, `TreePartKind::{Fruit, Flowers}` | tree core and Bevy | Move to fruit Bevy |
| Export: `MeshPart::{Fruit, Flowers}` | `src/export/` | See "Today's tree work" |
| Tests: `tests/clusters.rs`, `tests/cluster_mesh.rs`, the fruit checks in `tests/silhouette.rs` | | Split: shape tests move, placement and outline stay |
| Studio: fruit and flower panels | `procgena/src/tree/params.rs`, `export/tree.rs` | Tree panel picks a fruit preset; fruit gets its own tab later |

Three tree presets grow clusters (checked in `presets/*.json`):

| Preset | Part | Placement | Cluster | Facing |
|---|---|---|---|---|
| Apple | fruit | 2 per branch, tips 0.5 | single, 1 item, 5 cm | hang |
| Flowering Dogwood | flowers | 5 per branch, tips 1.0 | single, 1 item, 10.5 cm | outward |
| Spring Pussy Willow | fruit (catkins) | density 16 per metre | panicle, 3 items, 1.5 cm | up |

The rest disable both (counts, density and tips all zero), but still carry
fruit and flower option blocks.

**One cluster shape per preset?** The TODO asked this to be checked
first. Yes: each preset has one `ClusterShape` and one item shape per
part. Its clusters differ only by the tree's random draws, and by the host
inputs listed next.

## Asset, Modifier or something new

ARCHITECTURE §1.1 defines a Modifier as something that "grow[s] on top of
an asset, or on any tagged mesh", owns "a `TagFilter` saying which
surfaces they grow on", and "changes how an asset looks in place; it is
not a new kind of asset". `MushroomGrowth` and `PlantGrowth` are
Modifiers that grow mushrooms and plants on stumps, logs and rocks.

| Test | Modifier (Clump, `MushroomGrowth`) | Fruit |
|---|---|---|
| What places the copies | Sampling a finished host mesh through a `TagFilter` | The host's own skeleton: branch, section, side, tip |
| Is it a kind of asset | No (Clump); it places an existing asset (`MushroomGrowth`) | Yes: its own options, presets, LODs, impostor, export |
| In the host's content hash | No: it is applied on top | Yes: `tree_id` already hashes fruit, and placement is a tree option |
| Moves with the host | Clump follows its host's levels; rule 9 says clumps on a tree sway with it | Sways with its branch, and is thinned by the tree's levels |
| Owned by | The asset says which modifiers grow on it (for now) | The tree's options opt in |

The verdict is a new idea, not a Modifier. It is close to what
`MushroomGrowth` places, but fruit is placed differently. `Mushroom` is
already the precedent for an asset that is placed by more than one thing:
by `MushroomScatter` on the ground, and later by `MushroomGrowth` on
hosts. Fruit is an asset of the same kind, placed by a third kind of
placer: its host's structure.

The name is **attached asset**, which the user accepted: an asset whose
copies a host asset places, as part of the host's own specimen. It has
the full asset contract (options, presets, `default_seed`, LOD,
impostor, specimen, export). Its copies have no ground, no mask, no
Scatter and no copy address of their own. `MushroomGrowth` and
`PlantGrowth` stay Modifiers. The difference is the placer: a tag
sampler places a Modifier's copies, and the host's skeleton places an
attached asset's copies.

**Does it still need its own §1.2 once Addresses exist?** Yes. D3 7b
reaches the same answer. For identity alone it adds nothing: the
cluster-to-fruit hop is a **link** (a placement that picks a pool slot)
with the same shape as copy-to-tree-specimen, and an attached copy is
just an address passing through it. The idea still governs four things
that Addresses don't:
- who places the copies (the host's skeleton);
- whose hash covers the attached options (the host's);
- when the slot is chosen (per host specimen, so the host impostor can
  show it);
- what the host's impostor bake key must include (the attached pool and
  its `N`).

So §1.2 keeps the name and the section. Its identity sentence is phrased
as the hop: "an attached copy is addressed by its host's address plus a
part path through the link, each step pinned to the `SpecimenId` it
indexes".

## Options

The split follows "what" versus "where":

```text
procgena_fruit                          procgena_tree
--------------                          -------------
FruitOptions {                           TreeOptions {
  kind: ClusterKind,                       ...
  items, items_variance, spread,           fruit: Option<AttachedFruit>,
  axis_length, droop, taper,               flowers: Option<AttachedFruit>,
  side_stalks, side_length,              }
  stalk_length, stalk_radius,            AttachedFruit {
  stalk_color,                             fruit: FruitOptions,   // inline
  size, size_variance,                     placement: ClusterPlacement,
  item: ItemShape,                       }
  maturity, variation: Variation,        ClusterPlacement {
}                                          min_level, tips, count,
ItemShape = Fruit(FruitShape)              density, start,
          | Flower(FlowerShape)            facing: Facing, facing_jitter,
ClusterKind, FruitShape, FlowerShape       scale, scale_variance,   // per copy
FruitPreset (catalogue)                  }
```

- **One options type for fruit and flowers.** Today they are two types
  that share `ClusterShape` and differ only in item shape and maturity
  meaning. A fruit or flower is a cluster whose items are one or the
  other. One type means one pool kind and one hash.
- **`size` and `size_variance` move into fruit.** An item's size is part
  of what the fruit is. The tree keeps a per-copy `scale` (default 1) and
  `scale_variance`, for trees that want smaller fruit than the preset.
- **`Facing` and `facing_jitter` stay in tree.** They describe how the
  cluster is turned on its host. But the fruit specimen is built knowing
  its facing class, because of droop (see the next section). So `Facing`
  is re-exported from fruit, and the fruit options name the facing
  they're shaped for (`FruitOptions::hangs: HangClass { Down, Up, Out }`).
  The tree validates that its `facing` matches.
- **Inline, not by key.** The tree holds `FruitOptions` by value, usually
  copied from a `FruitPreset`. The tree's options hash then covers it,
  tree files stay self-contained, and F3's base-plus-difference files can
  name a fruit preset as the base.
- **Presets follow B0.** `FruitPreset { key, name, description, group,
  options, default_seed }`, with `const`s such as `FruitPreset::APPLE`, a
  `GROUPS` list, `all()` and `named(key)`. There is no file loading until
  F3. Starting presets: `APPLE`, `DOGWOOD_BLOOM`, `PUSSY_WILLOW_CATKIN`,
  matching today's three trees. The other trees' disabled blocks are
  dropped (`None`).

## The cluster frame: what is baked, what is per copy, what is lost

A specimen must be reusable on any branch, so every input it takes from
its host has to be fixed in the cluster frame, carried per copy, or
dropped. Today, `ClusterBuilder::build` and the meshers take these inputs:

| Input today | Where | Proposed | What is lost |
|---|---|---|---|
| `out`: direction away from the branch surface (along the branch at a tip) | `build`; the start heading of every stalk, and the axis's turn over `AXIS_TURN` | Baked: for `Hang` and `Up`, the frame's `out` is local +Z, at 90° to facing; for `Outward`, `out` is the facing itself (+Y) | A tip cluster's stalk starts at the same angle as a side cluster's, not along the branch. Visible only on long-stalked tips, which no preset has |
| Facing (`Hang`, `Up`, `Outward`) | `build` | Baked as `HangClass`; local +Y is the facing | None |
| `facing_jitter` tilt | `tilt` | Per copy: part of the copy's rotation | Droop is bent toward the frame's down, not the true down, so it is off by at most the jitter angle (20° on Willow) |
| Random spin about facing | `build` | Per copy: twist. For `Hang` and `Up` it turns local +Z toward the real `out`; for `Outward` it turns local −Z toward the true down | None |
| Droop toward world −Y | `Path::curve` | Baked: down in the frame is −Y for `Up`, +Y for `Hang`, and −Z for `Outward` (facing assumed level) | Outward clusters on steep branches droop as if their branch were level |
| Item count (`items_variance`), umbel azimuth jitter, raceme, panicle and catkin phase, item sizes (`size_variance`), item spins | `ClusterBuilder` | Baked: slot seed | None. This is what makes slots differ |
| `outward(cluster.origin)`: blush direction, flat-disc and billboard facing | `Style::new`, `mesh_disc`, `mesh_billboard` | Baked: blush faces local +Z (away from the branch); flat forms face the camera at draw time instead | Blush faces away from the branch, not away from the trunk. On side clusters these are close |
| Branch wind at the attachment: `[trunk_bend, branch_weight, branch_phase]` | `ClusterWind::new` via `wind_at` | Per copy, 12 bytes | None |
| Flutter along the stalks (`ClusterWind` reach and distance) | per vertex | Baked: per-vertex flutter in the specimen | None |

## What differs between a pool's N slots

From the table above, a slot today varies:

- **Multi-item clusters (Willow's panicle):**
  - the item count, which is fixed at 3 today because `items_variance` is
    unset;
  - the phase of the side stems;
  - each item's size, with `size_variance` 0.3;
  - each item's spin.

  That is real variety: no two catkin sprays match.
- **Single-item clusters (Apple, Dogwood):** one item's size and spin. A
  copy's scale and twist carry both, so every slot looks the same and
  `N` above 1 limits nothing.

Proposal: add `Variation`, seeded per specimen, to the fruit options:

- `maturity_spread`: each item's maturity varies around `maturity`. On
  fruit this moves size, colour and blush. On flowers it gives a bud, an
  open flower and a fading flower in one pool.
- `hue_spread` and `blush_spread`: small colour shifts per item.
- `shape_jitter`: asymmetry of the fruit body (lobe phase and depth, a
  slight lean of `elongation`), and of petals (the angle and length of
  each petal).
- `stem_curve_jitter`: the direction of the stem's bend.

Defaults: off (zero) for exact repeats. Presets set them, so Apple's
slots differ in ripeness and shape and Dogwood's in petal and bloom stage.
The suggested default `N` per preset: Apple 6, Dogwood 6, Willow catkin 8.

A copy's own tint stays out, per rule 4 ("each copy carries its own
transform, scale, tint"). Tint on a fruit copy could replace part of the
colour spread, but it would also tint the stalks. That is a question for
the user.

## Where a slot is chosen: per tree specimen

Decided by the user: per tree specimen.

**The choice.** Cluster `i` of tree specimen `T` takes slot
`slot(fruit pool, key = hash(T.SpecimenId, i))`. `slot` is D3's rendezvous
hashing over the fruit pool's `N` (or a subset). `i` is the cluster's
index in the skeleton's fruit (or flower) list. That is the stable part
index D3 7b names `fruit i` (or `flower i`). It is not the cluster's row
in the cluster table, which is sorted by slot.

**Why not per tree copy.**

- Per-copy fruit means the tree impostor can't show fruit, because every
  copy would differ from the bake. On Pussy Willow the catkins are the
  canopy: the outline test's control `the_outline_check_catches_dropped_catkins`
  exists because dropping them leaves only twigs. A fruitless impostor
  would fail the same way.
- It also needs a cluster table per tree copy instead of per tree
  specimen. For a grove of 1,000 Willows that is 783,000 entries, about
  50 MB, instead of the specimen budget's handful.
- What it buys, fruit varying between copies of one tree specimen, the
  tree budget already gives across tree slots.

**What follows.**

1. **The tree's options hash** covers its `AttachedFruit`, so the fruit's
   options too. This is the same as `tree_id` today.
2. **The tree's `SpecimenId` does not cover the fruit pool's `N`.** `N`
   is World data, not options. Two Worlds with the same seed and
   different fruit `N` share tree `SpecimenId`s but grow different fruit
   on them. Therefore:
   - the tree's mesh cache key stays `SpecimenId` plus level, because the
     tree's own mesh has no fruit in it;
   - the tree's cluster table (cluster to slot) is keyed by `SpecimenId`,
     the fruit pool hash and the fruit `N` (or the subset);
   - **the tree's impostor bake key** is `SpecimenId`, the fruit pool
     hash, fruit `N` or subset, and the bake settings. A World that
     changes fruit `N` rebakes its fruiting trees' impostors, and nothing
     else.
3. **A fruit specimen's `SpecimenId`** is the World seed, the fruit
   options hash and the slot, the same as any asset's. A fruit built on
   its own uses its preset's `default_seed`.
4. **Addresses** (D3 7b). A fruit copy is the tree copy's address plus
   the part path `fruit i`, and then `item k` inside the fruit specimen:

   ```text
   (orchard, apples, cell (4031, -77) #0) / fruit 17 / item 2
   ```

   - **Kinds and pins.** The part kinds are `tree.part.fruit` and
     `tree.part.flower` on the tree's skeleton, then `fruit.part.stalk`
     and `fruit.part.item` on the fruit specimen. `fruit 17` is pinned
     to the tree's `SpecimenId`, and `item 2` to the fruit's.
   - **When a pin no longer matches.** If the fruit pool's `N` changes,
     rendezvous can move cluster 17 to another slot. The fruit pin then
     no longer matches, and `item 2` is set aside, while `fruit 17`
     still resolves.
   - **The cluster table keeps each row's skeleton index `i`**, so
     picking a drawn cluster gives its address.
   - **Indices don't change with detail.** Each index counts the
     skeleton or the specimen, not the mesh, so it stays the same at
     every detail level. The tree's cluster stride thins draws, never
     indices. Fruit's item stride does the same inside a specimen.
   - **Edits stay per copy.** Harvesting `fruit 17` on one tree hides
     that row for that tree copy at draw time (open question 10). The
     fruit specimen and both impostors are untouched.
   - **Stable lists.** Fruit's builder must keep `stalks`
     and `items` as flat, stable lists in the cluster frame. Today's
     `SkeletonCluster` already does.

## Fruit's own LOD chain

Fruit levels follow R1's shared LOD type: distances for a reference size,
scaled by the cluster's world size (Willow's 1.5 cm catkins and Apple's
5 cm apples get different distances from one table), with hysteresis and
dither.

| Level | Form | Reference distance (10 cm cluster) | From today's code |
|---|---|---|---|
| 0 | Full mesh: stalks, item stems, calyx | 0 to 3 m | `ItemDetail::Full` |
| 1 | Simple mesh: half the segments, 3-sided stalks | 3 to 8 m | `ItemDetail::Simple` |
| 2 | Minimal: every second item, enlarged by 1.25, no item stems; small items drawn flat (disc, triangle) | 8 to 20 m | `ItemDetail::Minimal`, `item_stride`, `flat_item_size` |
| 3 | Impostor: one camera-facing quad from the specimen's bake | 20 m to the tree impostor | replaces `billboard_fruit` and the level-3 flat items |
| cull | Ends where the tree's impostor fades in (default 120 m), because the tree impostor carries fruit | | |

How the two chains relate:

- **The tree decides which clusters are drawn.** `cluster_stride` and
  the enlargement that makes up for it stay tree options per tree level.
  At tree level 3 today, every second cluster is kept and scaled by 1.4.
  This thins copies, so it belongs to the placer, the same way a Scatter
  draws fewer copies far away.
- **The fruit decides how each drawn cluster looks**, from that
  cluster's own distance, in two steps. The user decided this.
  1. **Coarse, on the CPU, per tree per view**, in today's `cull_groves`
     (and the single-tree path). The tree's distance, plus or minus its
     crown radius, gives the range of fruit levels its clusters can be
     in, after size scaling by the smallest and largest cluster scale.
     - If the range is one level, the tree goes straight into that
       level's list. The clusters are then drawn without any check of
       their own. Most trees are in this case.
     - If the range is empty (all beyond the fruit cull), the tree draws
       no fruit.
  2. **Fine, on the GPU, per cluster**, only for trees whose range spans
     more than one level.
     - A compute pass expands each such tree × its kept clusters.
     - It measures each cluster's own distance, and frustum-culls it
       against a sphere from the cluster table.
     - It appends the cluster to its level's instance list, with its own
       dither value, and writes indirect draw arguments.
     - Stone's GPU-driven pool (per-view and shadow culling) is the
       model. This is the shape R2's shared renderer takes, so the pass
       moves there when R2 lands.

  The cost is one dispatch per view, over trees in a transition only.
  There is still no per-cluster work on the CPU. Shadow views use the
  camera's distances, as the grove does today.
- **The last fruit level must end at the tree's impostor.** The tree
  tells the fruit its cull distance. A tree with no impostor lets fruit
  run to the fruit's own cull.
- **Shadows.** Fruit levels 0 and 1 cast shadows. Levels 2 and 3 don't.
  Rule 2 says the last levels drop shadows "where it should", and these
  are small.

## Impostor cost: requirements

The user chose full impostors per cluster. Today's `procgena_impostor`
falls short of that in four ways.

### 1. View count and size per bake

`ImpostorSettings::frames` (8) and `frame_size` (128 px) are global
today. They become fields of `ImpostorRequest`, and part of the bake key.
A fruit cluster is a few pixels wide where its impostor draws, so fruit
bakes use 6×6 views of 32 px: two 192 px RGBA8 atlases (colour and
normals), about 290 KB per specimen before compression and mipmaps.
Apple, Dogwood and Willow at `N` = 6, 6 and 8 total 20 bakes, about
5.8 MB. Trees keep 8×8 at 128 px.

### 2. View coverage, declared per preset

The layout covers the upper hemisphere of the object's own frame, and
folds views from below onto the horizon (`view_cell`, `d.y = max(d.y,
0)`). The shader takes the view in the object's local frame with its
full rotation (`local_from_world` in `impostor_corner`), so the
hemisphere turns with the copy. With local +Y as facing:

- **Hang** (+Y points down in the world): viewers below are at the pole,
  which is covered. Viewers above, such as from a hill or the air, fall
  below the horizon and are folded.
- **Up** (Willow, +Y up): viewers below the crown are folded. That is
  the usual view of a tall shrub's catkins.
- **Outward** (+Y roughly level): half of all viewers are folded.

The user decided that each preset declares the views its impostor
covers, and that the default is all of them. Two values aren't enough,
because tree impostors must look right from below on a mountain slope,
"maybe not right from the bottom". So coverage is an elevation range.

**The type.**

```rust
/// The viewer elevations an impostor is baked for, in the object's own
/// frame: from straight above (+90°) down to `lowest` degrees.
pub struct ViewCoverage { pub lowest_elevation_deg: f32 }  // -90..=90

impl ViewCoverage {
    pub const SPHERE: Self = Self { lowest_elevation_deg: -90.0 };      // the default
    pub const UPPER_HEMISPHERE: Self = Self { lowest_elevation_deg: 0.0 }; // today's layout
}
```

- **It is a field of every asset's impostor settings.** Presets set it,
  and the default is `SPHERE`. It travels on `ImpostorRequest` and is in
  the bake key, by its float bits, beside the view count and frame size.
- **It has one bound, not two.** The coordinator suggested
  `{ min_elevation_deg, max_elevation_deg }`. One lower bound, with the
  top fixed at +90°, is equivalent for every case we have. Views from
  above are always needed: aerial and hill views, and directional sun
  shadows. The shadow pass draws the impostor from the light's
  direction, which is above the horizon whenever the sun casts shadows.
  A band that leaves out the top would never be declared.
  - With one bound, the covered views always form a single cap around
    +Y, which keeps the layout one warped octahedron (below) instead of
    a ring.
  - If a band is ever needed, `highest` can be added later without
    changing the layout's form.
- **Views below the lowest elevation fold onto the lowest ring**, just
  as views below the horizon fold onto the horizon today. A viewer
  beyond the declared range sees the nearest declared view, which is
  wrong only by the excess angle.

**How the octahedral layout maps a partial range.** The atlas
resolution goes only where coverage is declared:

- **`lowest = -90` (`SPHERE`):** the standard full octahedral layout:
  the whole octahedron unfolded into the square. `view_basis` gains a
  case for the lower pole as well as the upper.
- **`lowest > -90`:** a warped hemi-octahedron.
  - Each direction's polar angle from +Y, `θ` in `[0, θmax]` with
    `θmax = 90° − lowest`, is scaled to `θ' = θ · 90° / θmax`.
  - The result goes through today's hemisphere fold.
  - So the declared cap fills the whole diamond, and none of the atlas
    is spent below `lowest`.
  - At `lowest = 0`, the warp is the identity, so `UPPER_HEMISPHERE` is
    exactly today's layout and today's bakes need no change of layout.
- **Code that changes:** `view_cell` and `view_direction` (and their
  WGSL twins in `layout.wesl`) apply the warp and its inverse. `view_cell`
  clamps `θ` to `θmax` first. The existing round-trip test runs for
  several values of `lowest`.
- **Cost of a wider cap.** The same view count covers more sky. A cap
  down to −45° is 1.71 times the solid angle of the hemisphere. At 8×8
  views, the spacing between views is then 1.31 times today's. At 10×10
  (100 views), the density is about 92 % of today's 8×8 hemisphere, for
  an atlas
  of 1,280 px instead of 1,024 px at 128 px per view (1.56 times the
  memory).

**Trees: −45°, and why.** The question is how steeply a viewer can look
up at a tree that is far enough away to be an impostor (120 m by
default).

- **The view elevation is the mean slope of the line of sight.** It is
  set by the average gradient of the ground between the viewer and the
  tree, plus a little for the tree's centre being above its base. It is
  not set by the cliff angle. When you stand on the same hillside as
  the tree, that average gradient is the hillside's slope.
- **Trees grow on gentle ground.** Grove trees grow only where the
  ground is at most `max_slope`, which defaults to 35°. Shrubs and
  saplings may set it higher.
- **Worked numbers.** Take a tree centre 10 m above its base, an eye at
  1.7 m, and 120 m horizontally, up a uniform slope. The elevation is
  37.6° below level for a 35° slope, and 42.2° for 40°.
- **So the trees' default is −45°.** It covers every grove tree at its
  default slope limit, plus species set up to about 40°, with a margin.
- **What it gives up: the bottom.** A tree at the top of a terrain cliff
  (65° and steeper, the terrain's `CLIFF_SLOPE`), seen from the cliff's
  foot, looks up more steeply than −45°. It folds to the −45° ring: a
  view from 45° below instead of, say, 60°. Today the same view folds
  45° or more, to the horizon. Those viewpoints are rare at impostor
  distance, because a mean of 65° over 120 m needs a cliff 250 m high.
- **Tree impostors move to 10×10 views at 128 px**, to keep today's view
  density. Whether that is worth 1.56 times the memory is a setting the
  tree preset declares. The plan suggests 10×10, and 8×8 stays possible.

**Defaults per asset.**

- **Fruit:** `SPHERE`, the default.
- **Trees:** −45°.
- **Stone, deadwood and mushroom** (when they gain impostors under I1):
  each picks its own value. A boulder on a slope has the same geometry as
  a tree, so −45° is the natural starting point. Low things seen mostly
  from above, such as logs and mushroom tufts, may declare
  `UPPER_HEMISPHERE` to get denser views.

The bake is framed on the bounds centre (`bounding_sphere` in
`bake.rs`), not the origin, so a cluster hanging from its attachment
point frames correctly without changes.

### 3. Instancing: tree × cluster

The grove keeps one Bevy `MeshUniform` record per tree. It culls on the
CPU per tree per view (`cull_groves`), and draws with an 8-byte instance
`(record index, dither)`. The impostor's instanced pipeline uses the same
8 bytes (`INSTANCE_SIZE`).

One record per cluster is not viable. A grove of 1,000 Willows has
783,000 clusters. Each `MeshUniform` is well over 100 bytes, so the
records alone pass 100 MB, and CPU culling would test every cluster for
the camera and each shadow view.

The proposal is a composed instance:

- **Per tree specimen** (once): a cluster table in a storage buffer.
  Sorted by fruit slot, each entry holds:
  - the cluster's tree-local transform: 32 bytes as position, rotation
    quaternion and scale;
  - the branch wind at its attachment: 12 bytes;
  - a bounding radius, for the fine step's culling: 4 bytes;
  - the skeleton index `i`, for addresses and picking: 4 bytes;
  - padding to 64 bytes.

  Each entry is 64 bytes, so 783 clusters take 50 KB. Each tree level
  also keeps its stride and scale for the kept clusters.
- **Per tree copy:** nothing new. The tree's record and its entry in the
  visible list are reused.
- **Per view:** the two-step check from "Fruit's own LOD chain". The
  coarse step is one range test per tree. Trees inside one level are
  listed whole. Trees spanning levels go to the GPU pass, which writes
  per-cluster instances `(tree entry, cluster row, dither)` and indirect
  arguments.
- **Per draw:** one draw per tree slot, fruit level and fruit slot. For
  whole-listed trees the instance count is visible trees times that
  slot's kept clusters, and the vertex shader splits `instance_index`
  into `(tree entry, cluster)`. For GPU-sorted trees it reads the
  appended instance. Either way it reads the tree's record and the
  cluster's entry, and composes `world_from_tree × tree_from_cluster`.
- **Impostor crate requirement:** an instanced variant whose instance
  names a record plus a sub-transform from a storage buffer, or a hook
  that lets a material supply `world_from_local` itself. The fruit
  material then reuses the impostor shader's library functions
  (`impostor_corner`, `view_cell`, atlas sampling).

What a grove costs:

- **Memory:** about 50 KB per Willow tree slot, plus the fruit bakes
  above. Nothing per copy.
- **CPU per frame:** unchanged in kind. Per tree per view, two or three
  more band tests.
- **GPU per frame:** trees inside the fruit range times their kept
  clusters. With 100 Willows inside 120 m, about 60 % kept after the
  tree's thinning, and most drawn at fruit level 3, that is roughly
  47,000 quads, or 94,000 triangles. The coordinator recorded 45,414
  triangles for one Willow at tree level 2 today.
- **Draw calls:** tree slots × fruit slots × fruit levels × passes, for
  example 4 × 8 × 4 × 2 = 256 for one Willow grove. Fruit slots of one
  pool can share one texture array, so a level's slots share one material
  and one draw. That gives 4 × 4 × 2 = 32.

### 4. Wind that follows the branch

Today every fruit vertex carries `(trunk_bend, branch_weight,
branch_phase, flutter)`, taken from its attachment and its distance along
the stalks.

- **Per cluster instance:** `[trunk_bend, branch_weight, branch_phase]`,
  12 bytes, in the cluster table. The vertex shader calls `wind_offset` with
  `flutter` 0 at the cluster origin, using the tree's anchor. It adds the
  result to the whole cluster, so the cluster moves rigidly with its
  branch.
- **Per vertex in the specimen** (mesh levels only): flutter alone, from
  distance along the stalks. It is added on top of the rigid offset.
- **Impostor:** rigid offset only. The impostor crate gains a hook for a
  vertex offset in its forward, prepass and shadow shaders. Today it
  says "impostors don't move with their object's animation".

The wind module is the tree's today. Rule 9's one wind moves it to
`procgena_core_bevy`, and fruit depends on that, not on the tree.

## Today's tree work

- **`cluster_mesh::ItemSelection`** (keep and enlarge) splits in two:
  - `cluster_stride` and the cluster scale stay in the tree, as copy
    thinning per tree level, applied to the cluster table;
  - `item_stride` and the item scale move to fruit, as level 2's item
    thinning inside one specimen.

  Its rule ("every way of drawing them keeps the same items") still
  holds inside fruit: levels 2 and 3 bake the same kept items.
- **Level-3 flat items and `billboard_fruit`.** The flat items (discs for
  fruit and triangles for flowers smaller than 15 cm) and the
  billboard-hexagon fruit with their `BillboardCull` fallbacks are
  removed. Fruit's level 2 keeps flat items for small items. Its level 3
  is the impostor. `LodDetail` loses `billboard_fruit`, `min_item_size`,
  `flat_item_size`, `item_detail`, `item_stalks`, `item_stride` and
  `item_scale`, and keeps `cluster_stride` plus a `cluster_scale`.
- **The outline test** (`tests/silhouette.rs`, the per-preset floor
  `PRESET_MINIMUM` = 0.7, the average floors, and the two broken-case
  controls) stays in `procgena_tree`, because it tests the whole tree.
  Its `measure` gains a step that places each kept cluster's fruit mesh
  at the fruit level for that tree level's distance. Each copy is
  transformed by its table entry, using `procgena_fruit`'s CPU path as a
  dev-dependency. Level 3 places the impostor's level-2 mesh, because the
  CPU test has no bake; that matches what the bake shows. Thresholds stay
  unchanged.
  - `the_outline_check_catches_dropped_catkins` changes how it drops the
    catkins: it sets the tree's level-3 cluster stride to "none" instead
    of `min_item_size`. It must still name Spring Pussy Willow.
  - `the_outline_check_catches_missing_leaves` is unchanged.
- **Silhouette for fruit alone.** A new test in `procgena_fruit` checks
  silhouette IoU for each level against level 0, for every preset (rule
  13's test helper).
- **glTF and OBJ export.**
  - `procgena_fruit` exports one specimen (rule 11).
  - Tree export writes the tree and its fruit. glTF writes one mesh per
    fruit slot and one node per cluster referencing it (plain glTF
    instancing, no extension). OBJ has no instancing, so it writes
    `fruit` and `flowers` objects merged as today.
  - `MeshPart::{Fruit, Flowers}` stay as export part names.
  - Export uses each fruit slot's level 0 (or a chosen level), because
    files have no impostor.
- **Studio.**
  - The tree panels replace their fruit and flower shape controls with a
    preset picker, placement controls and "edit fruit". The last opens
    the fruit tab on that inline options value.
  - The fruit tab, which shows one cluster and its levels, comes after
    the core.

## Order of work

Each step is a worker task. "Needs" lists what must be merged first.

| Step | What | Needs | Touches |
|---|---|---|---|
| P0 | This plan | | `fruit/plan.md` |
| AC1 | The crate and the pools in one step, in the cluster-local frame: everything in "Move map", plus fruit `SpecimenId`s, per-tree-specimen slots, `Variation`, `FruitPreset`, part paths and `part()`, the outline test composing fruit, export, and the studio's tree panels | F1, F2, D3 with Addresses; `tree/` free of other writers | `fruit/`, `tree/`, `procgena/src/tree/`, root `Cargo.toml` (member, added last), `Cargo.lock` |
| B1 | Fruit's own LOD levels on R1's LOD type, sized by the cluster | AC1, R1 | `fruit/` |
| B2 | Fruit Bevy crate: materials, cluster-table instancing in the grove and single-tree spawn, the two-step detail check, rigid branch wind | AC1, B1 (R2 later moves the GPU pass onto the shared renderer) | `fruit/`, `tree/crates/procgena_tree_bevy` |
| D1 | Impostor crate: per-bake views and size, `ViewCoverage` (lowest elevation, `SPHERE` by default, warped octahedral layout), a composed instance or material hook, a vertex offset hook | I1 (keys by `SpecimenId`) | `impostor/`, and the tree's impostor settings (declares −45°, 10×10 views) |
| D2 | Fruit impostor level; the tree impostor bakes fruit; the tree impostor key gains the fruit pool and `N` | B2, D1 | `fruit/`, `tree/` |

The coordinator decided that A1 (a pure move) is not done on its own.
A bit-exact move before F2 would have meant moving about 1,500 lines,
rewriting the moved `Hash` impls again in F2 straight afterwards, and
reworking the frame and the cluster link once Addresses land. Doing it
once, as AC1, is cheaper.

I1 is not reordered. D1 is fruit's part of the impostor work, and runs
beside or after I1.

## Move map (for AC1)

Everything the cluster code takes from the tree today, and where it
lands. This is in the cluster-local frame, so AC1 changes fruit numbers
on purpose.

**Types**

| Today, in `procgena_tree` | In AC1 |
|---|---|
| `ClusterStalk`, `ClusterItem` | Fruit, as the specimen's structure (`FruitSkeleton { stalks, items }`) in the cluster frame. These are flat, stable lists: the `stalk k` and `item k` part indices |
| `SkeletonCluster` | Stays in tree, slimmed to a placement: `parent: Attachment`, `origin`, `orientation` (facing, jitter tilt and twist), `scale`, and the link (fruit slot). It keeps no stalks or items |
| `ClusterBuilder`, `Path`, `tilted`, `turn_toward` | Fruit: the generator, seeded from the fruit `SpecimenId` through F2's seed function. The facing tilt and twist move out to the tree's placement (see "The cluster frame") |
| `grow_clusters`, `density_count` | Stay in tree, drawing from the same `FRUIT_SALT` and `FLOWER_SALT` streams. They make fewer draws than today, because shape draws move to fruit. So cluster positions change, but branches and leaves, which use other streams, do not |
| `ClusterShape`, `ClusterKind`, `FruitShape`, `FlowerShape` | Fruit, merged into the one `FruitOptions` (see "Options") |
| `FruitOptions`, `FlowerOptions` (tree) | Replaced by `AttachedFruit { fruit: procgena_fruit::FruitOptions, placement: ClusterPlacement }` on `TreeOptions::{fruit, flowers}` |
| `ClusterPlacement`, `Facing` | Stay in tree. `size` and `size_variance` move to fruit, and `scale` and `scale_variance` are added |
| `ItemDetail` | Fruit |

**Meshing**

| Today | In AC1 |
|---|---|
| `MeshBuffers`, which fruit writes through `mesh_fruit` and `mesh_flowers` | Stays in tree. Fruit gets its own `ClusterBuffers` (positions, normals, colors, wind, indices) in the cluster frame |
| `TreeMesh::{fruit, flowers}` and `ClusterSpan` (`fruit_spans`, `flower_spans`) | Leave `TreeMesh`. The tree gains `Skeleton::clusters()`, the cluster table rows, and a CPU helper that composes the fruit specimens' buffers at given fruit levels into one `MeshBuffers`, for export, tests and colliders. The Bevy crate draws instanced (B2) |
| `LodDetail::{item_detail, item_stride, item_scale, item_stalks, flat_item_size, min_item_size}` | Fruit's own level table, `ClusterDetail`, with three mesh levels until R1 (B1) |
| `LodDetail::cluster_stride` | Stays in tree, with a new `cluster_scale` |
| `LodDetail::billboard_fruit`, `mesh_billboard`, the crossed `BillboardCull` fruit hexagons, `BillboardVertex` used for fruit | Removed. No default level uses them, and only the dropped-catkins control did. Fruit's flat level 2 and its impostor (D2) replace them |
| `ItemSelection` | Splits: tree keeps the cluster stride and scale; fruit keeps the item stride and scale |
| `outward(origin)`, `blush_direction(origin)` | Baked in the frame. Blush faces local +Z. Flat forms face the camera at draw time |
| `srgb_hex_to_linear` (tree `material.rs`) | F2's or core's colour helper if one exists. Otherwise a private copy in fruit until rule 13's shared plumbing |

**Wind**

| Today | In AC1 |
|---|---|
| `BranchWind`, `wind_at` | Stay in tree. Each cluster table row carries `[trunk_bend, branch_weight, branch_phase]` |
| `ClusterWind` (reach, per-vertex flutter) | Fruit, writing `(0, 0, 0, flutter)` per vertex. The rigid branch part is added per copy |

**Hashing and addresses**

| Today | In AC1 |
|---|---|
| `impl Hash for ClusterShape, FruitShape, FlowerShape` in `tree_id.rs` | Fruit's options get F2's content hash, which gives the fruit pool id. The tree's hash covers `AttachedFruit` (placement, plus the fruit options hash) |
| Cluster identity: a position in the list | Part paths (D3 7b): `tree.part.fruit` and `tree.part.flower` through `Skeleton::part`, which return the cluster's frame and its link (fruit pool, slot key `hash(T.SpecimenId, i)`). Then `fruit.part.stalk` and `fruit.part.item` on the fruit specimen |

**Tests**

| Today | In AC1 |
|---|---|
| `tests/cluster_mesh.rs` and the unit tests in `cluster_mesh/mod.rs` | Move to fruit and are adapted to the frame, with their intent kept |
| `tests/clusters.rs` (placement) | Stays in tree. Its shape assertions move to fruit |
| `tests/silhouette.rs` | Stays in tree, and composes fruit through the CPU helper (fruit as a dev-dependency) |

**AC1's gate**

- **Branch, bark and leaf buffers are bit-identical** to before for every
  preset at every default level. They come from other RNG streams, so
  any change there is a bug.
- **The outline test's thresholds are unchanged, and both controls still
  fire:**
  - the dropped-catkins control now drops the catkins through the tree's
    level-3 cluster stride;
  - the missing-leaves control is unchanged.
- **Fruit scores will move.** If any preset falls below its floor, the
  worker stops and reports, and does not change a threshold. Apple is
  the closest to its floor (0.699, the coordinator's figure, against
  0.7).
- **Every fruit preset's slots differ** once `Variation` is set, checked
  by a test that compares slot meshes.
- **Part paths resolve to the same frame at every detail level.**

## Decisions

The user answered on 2026-11-23:

1. **Slot per tree specimen**, as recommended.
2. **The name "attached asset"**, as recommended. It keeps its own §1.2,
   phrased as the hop from host to attached specimen (see "Asset,
   Modifier or something new").
3. **Seeded `Variation`** makes single-item slots differ, as
   recommended.
4. **Impostor views are declared per preset, defaulting to all angles.**
   This changes the recommendation, which was opt-in for fruit only.
   Anything that relies on fewer views declares it, and the coverage is
   in the bake key. A later requirement: "Tree imposters need to look
   good looking up a mountain slope... Maybe not right from the bottom."
   So coverage is the lowest viewer elevation baked, and trees declare
   −45° (see "View coverage, declared per preset").
5. **Detail level in two steps**: a coarse check on the parent, then a
   fine check per cluster. This changes the recommendation, which was the
   tree's distance only. See "Fruit's own LOD chain".
6. **One crate named `procgena_fruit`**, as recommended.
7. **No per-copy tint** on fruit, as recommended.
8. **Seasons later**, as recommended. They stay out of this plan.

## Proposed doc edits (for the coordinator to apply)

### ARCHITECTURE.md

**§ Bottom line, first bullet.** After the Modifiers sentence, add:

> **Attached assets** such as fruit and flowers are assets that a host
> asset places on itself, with no Scatter of their own.

**§ The hierarchy (mermaid).** Add `Fruit["Fruit<br/>fruit and flower
clusters"]` inside `Assets`, and an edge `Tree -.->|"places on its
branches"| Fruit`. Add no Scatter for it.

**§ 1. Asset, Status.** Old:

> Built: `procgena_tree`, `procgena_plant`, `procgena_deadwood`,
> `procgena_stone`, and `procgena_mushroom`.

New:

> Built: `procgena_tree`, `procgena_plant`, `procgena_deadwood`,
> `procgena_stone`, and `procgena_mushroom`. Planned: `procgena_fruit`,
> an attached asset (see 1.2 and [fruit/plan.md](fruit/plan.md)).

**New § 1.2 Attached assets**, after 1.1:

> **What they are.** Assets whose copies a host asset places as part of
> its own structure: fruit and flower clusters on a tree's branches. They
> follow the whole asset contract (options, presets, `default_seed`, LOD,
> impostor, `Specimen`, export) but have no Scatter, no ground and no
> mask.
>
> **How they differ from Modifiers.** A Modifier samples a finished host
> mesh through a `TagFilter`, and stays outside the host's content hash.
> An attached asset is placed by the host's own generator (branch,
> section, tip), is part of the host's options and hash, and sways and
> thins with the host's levels.
>
> **Specimens and copies.** Each attached asset's options are a pool
> with its own budget `N`. The host chooses a slot for each attached copy
> per host *specimen*, so every copy of a host specimen carries the same
> attached specimens, and the host's impostor can include them. A host
> impostor's bake key therefore includes the attached pool's hash and `N`.
>
> **Addresses.** The hop from a host's part to an attached specimen is a
> *link*, like the hop from a copy to its specimen. An attached copy is
> addressed by its host's address plus a part path through the link,
> each step pinned to the `SpecimenId` it indexes, for example
> `… / fruit 17 / item 2` (see "Queries: reading the result" and
> [D3](docs/decisions/D3.md) section 7b).
>
> **Status.** Planned: `procgena_fruit` ([fruit/plan.md](fruit/plan.md)).

**§ Asset alignment, rule 2, the "small repeated parts" bullet.** Old:

> **Small repeated parts can use baked cards before the far form.** Tree
> fruit and flowers do this at levels 1 and 2 (decided 2026-11-23): each
> preset's cluster is baked once into a small atlas of a few views,
> keyed by the options hash plus the bake settings (one bake per pool,
> outside the specimen budget). Each kept cluster then draws as a
> two-triangle camera-facing card through the leaf billboard path, so it
> keeps wind and instancing, with its view chosen from the cluster's
> own axis, so hanging, upright and outward facings survive. Level 0 stays
> real meshes. This is a part's form inside the mesh levels, not a
> second impostor level.

New:

> **Attached assets own their levels.** Fruit and flowers are their own
> asset (decided by the user, superseding the cluster-card plan of
> 2026-11-23): each cluster specimen has its own mesh levels, sized by
> the cluster, then its own impostor, keyed by its `SpecimenId` like any
> specimen and inside the specimen budget. The host decides which
> attached copies each of its levels draws; the attached asset decides
> how each looks. The host picks it in two steps: a coarse range per
> host on the CPU, then, only for hosts that span more than one attached
> level, a fine check per attached copy on the GPU.

**§ Asset alignment, rule 2, "The levels run in order".** Append:

> An attached asset's chain ends where its host's impostor begins, and
> the host's impostor includes it. Every impostor covers the viewer
> elevations its preset declares, from straight above down to a lowest
> elevation: −90° (the full sphere) by default. Trees declare −45°, so
> they look right from below on a mountain slope.

**§ Where shared code lives, the `procgena_impostor` paragraph.** Append:

> Each bake sets its own view count and frame size, and its view
> coverage, which its preset declares as the lowest viewer elevation
> baked (`ViewCoverage`, −90°, the full sphere, by default). The
> octahedral layout is warped so that the whole atlas covers only the
> declared views. Views below the lowest elevation show the lowest baked
> ring. Coverage is part of the bake key. An instanced draw may compose
> a host's record with an attached copy's transform.

**§ Architectural TODOs, Asset alignment.** Replace the "Cluster
impostor cards for tree fruit and flowers" item with:

> - [ ] **Fruit as an attached asset** ([fruit/plan.md](fruit/plan.md)):
>   the `procgena_fruit` crate (cluster builder and meshers moved from
>   the tree, in the cluster frame), the tree's `AttachedFruit`, pools and
>   per-tree-specimen slots, fruit LOD levels and impostors, cluster-table
>   instancing in the grove with a two-step detail check. Needs F1, F2
>   and D3 for its first step, then R1 and I1.

and add to the Asset alignment list:

> - [ ] **Impostor bake options**: per-bake view count and frame size,
>   view coverage declared per preset as a lowest viewer elevation
>   (−90° by default, −45° for trees, in the bake key), with the
>   octahedral layout warped to the declared range, a composed
>   host-plus-attached instance, and a vertex offset hook for wind.

**§ Asset alignment, the "Impostors per specimen, prepared per Ecotope"
TODO (I1).** Append:

> The impostor's default view coverage is the full sphere (lowest
> elevation −90°). The tree declares −45°: a viewer looking up a
> hillside at an impostor-distance tree sees it from at most about 38°
> below, at the default 35° slope limit. Stone, deadwood and mushroom,
> when they gain impostors, each declare their own lowest elevation:
> −45° for things on slopes, and 0° (today's upper hemisphere) for low
> things seen mostly from above.

**§ Specimen budget.** After "No cap across pools", add:

> - **Attached pools are pools.** An attached asset's options (fruit on
>   a tree) are a pool with their own `N`, counted like any other. The
>   host picks attached slots per host specimen, so attached specimens
>   never multiply with host copies.

**§ Open questions, 10.** Append:

> An attached copy (one fruit) is addressed by its host's address plus
> a part path, so the same choice covers it. A part edit such as a
> harvest hides that copy for that host copy only, and leaves the shared
> specimens and impostors alone.

### ROADMAP.md

**I1b.** Replace the whole milestone with:

> #### I1b: Fruit as an attached asset
>
> Decided by the user 2026-11-23, superseding the cluster-card plan.
> Plan: [fruit/plan.md](fruit/plan.md).
>
> - **Covers:** TODO "Fruit as an attached asset": the `procgena_fruit`
>   crate, the tree's `AttachedFruit`, per-tree-specimen slots, fruit LOD
>   levels and impostors, cluster-table instancing, and TODO "Impostor
>   bake options".
> - **Needs:** F1, F2 and D3 with Addresses for the crate and pools
>   (step AC1); R1 for fruit levels (B1); I1 for impostors (D1, D2).
> - **Done when:**
>   - Apple, Flowering Dogwood and Spring Pussy Willow draw their fruit
>     from fruit pools with a World `N`, and the slots visibly differ.
>   - The outline test passes with fruit composed in.
>   - A grove of Willows draws its catkins through the cluster table,
>     with no record per cluster, and with a per-cluster detail check
>     only on trees that span levels.
>   - Catkins draw as full-sphere impostors seen from below.
>   - The tree impostor includes the fruit and is rebaked when the fruit
>     `N` changes.
>   - A part path `/ fruit i / item k` resolves on every level.

**I1.** Append to "Covers":

> View coverage becomes a lowest viewer elevation, in the bake key, with
> the octahedral layout warped so the atlas covers only that range. The
> default is −90°, the full sphere. Tree presets declare −45° at 10×10
> views, so trees look right from below on a mountain slope, at today's
> view density. 0° reproduces today's layout exactly. Stone, deadwood and
> mushroom declare their own values. Done when a tree seen from 40°
> below, up a slope, shows a baked view rather than a horizon view.

**Dependency graph.** Add `I1b["I1b Fruit as an attached asset"]` with
edges `F1 --> I1b`, `F2 --> I1b`, `D3 --> I1b`, `R1 --> I1b` and
`I1 --> I1b`.

**Wave table.** Wave 5 gains "I1b AC1 (one tree writer), then B1". Wave
6 gains "I1b B2, D1, D2 (after I1)".

### CLAUDE.md, package table

Add after `deadwood/`:

> | `fruit/` | `procgena_fruit` | Fruit and flower clusters, an attached asset placed by `procgena_tree`. Features `export-obj`, `export-gltf`. One file per area in `tests/`. |
> | `fruit/` | `procgena_fruit_bevy` | Materials, cluster instancing, impostor level. Changing it rebuilds `procgena_tree_bevy`. |

and note under `tree/`: "Depends on `procgena_fruit`; a fruit change
rebuilds the tree."
