# procgena_mushroom — plan

Procedural mushrooms for Rust and Bevy: single mushrooms, clusters, and
mushrooms scattered over another generator's surface, at several levels of
detail, down to a very cheap open "flat" form for mass instancing.

## Status (2026-11-22)

| Phase | State |
|---|---|
| 0 — Workspace members, `.gitignore`, CLAUDE.md/README rows | ✅ (tags live in `procgena_mushroom::tags`, not `procgena_mesh`) |
| 1–7 — Core: options, presets, profiles, Solid/Shell/Flat meshing, ring, warts, colours, object, cluster, scatter | ✅ |
| 8 — Bevy crate: plugin, material + WESL, ribbon billboard, LOD, cluster/scatter components, viewer | ✅ |
| 9 — OBJ/glTF export + example | ✅ |
| 10 — Studio tab | ✅ Mushrooms tab with preset, seed, every setting, cap texture, save/load and GLB/OBJ export (see below) |
| 11 — `textures.yml` + texture sets | ✅ one cap set, `mushroom_top_001` (done in the texture session) |
| 12 — Docs polish | ✅ comment rewrite (by Claude; Codex was out of credits) and `mushroom/README.md` |
| Extra tests | ✅ silhouette overlap per level, cluster cap spacing, GPU shader-pipeline test (`--include-ignored`) |

Deviations from the design below:
- Mushroom tags are in this crate, to avoid editing `procgena_mesh` while another agent works on it.
- Segment counts are fixed per named detail; the target-edge-length helper was not built.
- The billboard code is copied from procgena_tree (open question 1 still stands).
- Gill lines and patterns are drawn in the fragment shader using a zone id in UV1.

### Studio tab

- `procgena/src/mushrooms/`: preset picker, seed, Random Seed, and a settings
  panel (Cap, Underside, Stem, Ring, Warts, Colours, Pattern, Cap Texture).
- `procgena/src/export/mushroom.rs`: Save/Load JSON, GLB (optionally every
  detail level) and OBJ. Exports carry vertex colours, not the cap texture.
- Scene: `SceneMode::Mushroom`, `ViewerCommand::SetMushroom`,
  `ViewerEvent::MushroomBuilt`. Textures load from `mushroom/assets/textures`
  through the `procgena_mushroom://` asset source. The camera view is sized
  from the mushroom's height. The scene uses `MushroomPlugin`'s default detail
  levels, so `options.detail` is ignored there too.
- The tab bar now wraps onto a second row when tabs don't fit the panel.
- Tests: control checks for every preset, status line, export, a studio test
  (preset, reframe, seed), and GPU render tests (texture drawn when idle,
  rejected settings keep the old mushroom).

The `mushroom_top_001` files are published in `gs://ze_textures/mushroom/`,
so the studio downloads them on first use on a machine without a local copy.

The tab has a Level of Detail picker; the overlay gives the size of the
level on screen.

Not done: exports leave out the cap texture; no cluster or scatter in the
studio.

## Bottom line

- **Two new crates**, following the stone/tree split:
  - `mushroom/crates/procgena_mushroom` — engine-free core. Options, presets,
    profile-based meshing, LOD, clusters, scatter records, `procgena_mesh::Object`
    output, OBJ/glTF export. Depends on `glam`, `procgena_mesh`, `serde`,
    `serde_json` (+ optional `gltf-json`, already in the lock). **No new
    dependencies.**
  - `mushroom/crates/procgena_mushroom_bevy` — plugin, async build, cache,
    `VisibilityRange` LOD switching, vertex-colour + WESL material, ribbon-stem
    billboard shader, cluster and scatter-on-host components.
- **One shape model: revolved profiles around a bent stem curve.** The cap is a
  2D profile (radius, height) chosen from infinigen's nine cap families and
  revolved around the stem's head frame; the stem is a radius profile swept
  along a curved centre line. Every LOD is a re-sampling of the same profiles,
  so silhouettes stay consistent and resolution is a free parameter.
- **Two detail axes**, as in stone and tree:
  1. *Quality* — a `MushroomDetail` (segment counts, body shape, which extras are
     meshed), with named constants (`FULL`, `HIGH`, `MEDIUM`, `LOW`, `FLAT`).
  2. *Distance* — `LodLevel { distance, hysteresis, detail }`, scaled by mushroom
     size, plus a **cull distance** after which nothing draws (mushrooms are
     centimetre-scale).
- **Three body shapes** per level (`BodyShape`):
  - `Solid` — watertight stem and cap (closed `Part`s with shells).
  - `Shell` — open cap dome (top surface only) and open stem tube, two-sided.
  - `Flat` — open low-poly cap shell + **tree-style ribbon stem**: one strip
    that the vertex shader turns about the stem axis to face the camera, plus a
    perpendicular fallback strip in a `cull` buffer for renderers that don't
    rotate. Available as a standalone single-level asset, not only as the last LOD.
- **v1 anatomy:** cap (9 profile families, lobed/asymmetric rim, surface noise),
  stem (bulb, waist, neck, collar, lean, bend), **ring/annulus as a flat
  non-volumetric sheet**, **warts/scales** as small meshed bumps that degrade to
  colour spots at distance. **Gills are shading only** (underside zone + radial
  stripe pattern); **morel/pitted caps are out of scope** for v1.
- **Materials — both:** per-zone vertex colours (cap top, underside, stem, ring,
  warts) driven by a palette, plus a small WESL pattern extension (spots,
  speckle, cracks, gill stripes). UVs and tangents on every surface so optional
  CC0 texture sets (`mushroom/textures.yml`) can be multiplied on top;
  `texture_root: None` means colours only (stone convention).
- **Asset units:** `Mushroom` (one), `MushroomCluster` (1–N non-overlapping,
  infinigen's ring and line layouts), `MushroomScatter` (instance records over a
  host `Object`, following the clump-on-host conventions in
  `PLAN-clump-bevy.md`). Clusters and scatter emit **placement records over a
  small variant library**, so a renderer instances shared meshes instead of
  baking one big mesh.
- **Coordinator-only edits** (see Phase 0): root `Cargo.toml`/`Cargo.lock`, new
  tags in `procgena_mesh/src/tag.rs`, `.gitignore`/`_typos.toml` for
  `mushroom/prior_art`, `CLAUDE.md` package rows, studio wiring.

---

## Prior art

Shallow clones in `mushroom/prior_art/` (untracked, ~350 MB).

| Repo | Commit | Licence | Use |
|---|---|---|---|
| [princeton-vl/infinigen](https://github.com/princeton-vl/infinigen) | `53033dd` (2026-11-10) | BSD-3 | **May port with attribution**: cap profile anchor tables (`assets/objects/mushroom/cap.py`), stem/ring anchors (`stem.py`), cluster layouts (`generate.py`), hue/colour rules (`growth.py`, cap shaders). The `perf/` path imports `infinigen_gpl` (GPL): **may be read for general algorithmic ideas, but no code is copied or transliterated from it**. |
| [siliconjungle/inkwell-webgpu-mushrooms](https://github.com/siliconjungle/inkwell-webgpu-mushrooms) | `a0949dc` (2026-08-11) | MIT | **May port with attribution**: archetype parameter tables and closed-form cap/stem curves (`src/lib/webgpu-mushroom-engine.ts`), near/mid/far tier vertex budgets, far-tier silhouette shapes. |
| [Gabrielazmbr/Mushroom-Generator](https://github.com/Gabrielazmbr/Mushroom-Generator) | `0a46834` (2026-04-14) | none | **Ideas only.** Species dataclass, Hermite/Catmull-Rom profile curves, parallel-transport stem frames, wart placement on cap normals, gill ribbons. |
| [raefu22/mushroomGenerator](https://github.com/raefu22/mushroomGenerator) | `eec8902` (2024-05-18) | none | **Ideas only.** Maya revolve of a bezier outline; normal-distributed heights and bend within a cluster. |

### What each contributes

**infinigen** (Blender, most complete anatomy)
- Cap = bezier profile `(x_anchors, z_anchors)` spun around Z, with nine
  families and weights: campanulate 2, conical 2, convex 2, depressed 1, flat 2,
  infundibuliform 1, ovate 2, umbilicate 1, umbonate 1. Random `x_scale`,
  `z_scale` in 0.7–1.4. `vector_locations` mark sharp (non-smooth) anchors.
- `inner_radius = log_uniform(0.2, 0.35) * radius` — stem top radius from cap.
- Gills: adnexed / decurrent / none as a spun loop (we take only the *shape*
  idea for the underside profile, not gill geometry).
- Stem: profile `x = (0, r, log_uniform(1,2)·r, r·uniform(1,1.2), 0)`,
  `length = log_uniform(0.4, 0.8)`, wave "band" texture on the lower part,
  bend deform up to 90°.
- Ring ("web"): solid (spun profile below the cap) or hollow (lacy voronoi
  cut-outs). We do the flat sheet; lace = alpha or ragged edge.
- Deforms: displacement noise 0.008, twist ±45°, per-quadrant asymmetric scale
  `r1..r4 ∈ ±0.25`, cluster-wide bend ±22.5°.
- Clusters: 1–5 mushrooms, scale 0.3–1.2, *cluster* layout (polygon angles,
  outward lean) or *directional* (a line), pushed apart along a direction
  until key points are ≥ 8 mm apart.
- Colour: base hue 40% any, else 0.02–0.15 (browns/oranges); cap shaders
  `cap` (z-ramp lighter toward rim), `noise`, `voronoi` (cracks), `speckle`.
- LOD: `triangle_budget(profile, face_size)` — triangle count from the
  profile's projected area; a good model for picking segment counts from a
  target edge length.

**inkwell** (WebGPU, everything rebuilt per vertex from an integer id)
- Eight archetypes: storybook, bell, bolete, chanterelle, morel, enoki,
  puffball, carved fantasy — each a handful of numbers (cap width/height/
  thickness/ellipse, stem base/neck radius) plus a closed-form cap-top curve.
- Stem radius = rooted bulb + waist→neck blend + collar near the top.
- Stem centre line: height, lean, curve power, lean angle; wind adds a
  quadratic offset along the stem.
- Cap rim lobes: `3–7` lobes, amplitude per archetype, growing with `r^2.1`.
- Tiers: near 792 verts stem/body + 144 gill, mid 288 + 72, far = one
  6-vertex camera quad with a per-archetype analytic silhouette in the
  fragment shader. Gills in the body shader are a `pow(|sin(angle·28)|, 10)`
  stripe — our shading-only gills.

**Mushroom-Generator** — species as one flat parameter struct (stem, cap,
gills, scales); scales are small noisy spheres aligned to cap vertex normals.

**mushroomGenerator** — a cluster is N mushrooms with normally distributed
height and bend around a mean.

---

## Workspace patterns to reuse

- **Options:** stone style — snake_case serde, `from_json` deep-merges over
  `Default`, `to_json`, `validate() -> Result<(), OptionsError>` that rejects
  (never clamps), `#[non_exhaustive] OptionsError`, `MAX_*` consts.
- **Presets:** `MushroomPreset` enum with `ALL`, `name()`, `options()`,
  written in Rust like `StonePreset`.
- **RNG/noise:** copy clump's counter-based `pcg3d` `Rng` (`hash.rs`) and
  stone's noise; no `rand`.
- **LOD:** tree's `LodLevel { distance, hysteresis, detail }`; stone's
  "distances written for the default size and scaled by size".
- **Billboards:** tree's `BillboardVertex { pivot, axis, offset }`,
  `push_ribbon_ring` (one rotating strip + perpendicular fallback in `cull`),
  and `tree_billboard.wesl` in the Bevy crate. Copy the mechanism (the
  established route, as clump copies triplanar), see Open questions.
- **Object model:** `procgena_mesh::{Object, Level, Part, Tag, TagSet,
  TagFilter, SurfaceMesh, Surface}`; `GeneratedObject` behind `bevy`.
- **Bevy plugin shape:** `MushroomPlugin`, `MushroomParams::{Options, Asset}`,
  `.mushroom.json` loader, `MushroomPart { lod, kind }`, `MushroomGenerating`,
  `MushroomFinished` `EntityEvent`, cache keyed by options JSON,
  `AsyncComputeTaskPool`, `VisibilityRange` per level with
  `distance ± distance·hysteresis` margins, public `mushroom_mesh` /
  `mushroom_material` helpers, `examples/viewer.rs`, headless
  `MinimalPlugins` tests.
- **Host-surface consumers:** `PLAN-clump-bevy.md` — child of host, wait for
  host `GeneratedObject`, world directions → host-local, `NonUniformScale` and
  `NoSurface` errors, rebuild when host object/transform changes, no
  cross-host cache in v1.
- **Exports:** `to_obj` / `to_glb` / `to_gltf` returning bytes, one node per
  LOD, options in `asset.extras`.
- **Studio:** the 10 touch points used for stone (tab, editor params, export
  panel, `SceneMode`, `ViewerCommand`, subject spawn, plugin, assets,
  environment, Cargo).

---

## Core design (`procgena_mushroom`)

### Module list

`options`, `presets`, `rng`, `noise`, `profile` (curves), `stem`, `cap`,
`ring`, `warts`, `colour`, `mesh` (buffers, LOD), `billboard`, `object`,
`cluster`, `scatter`, `export/{obj,gltf}`.

### Options

```rust
pub struct MushroomOptions {
    pub seed: u32,
    pub size: f32,              // cap diameter in metres (default 0.08)
    pub cap: CapOptions,        // family, width/height ratio, rim lobes, asymmetry, twist, noise
    pub underside: UndersideOptions, // gill attachment (free / adnexed / decurrent) → profile only
    pub stem: StemOptions,      // height ratio, base bulb, waist, neck, collar, lean, bend
    pub ring: Option<RingOptions>,   // height on stem, skirt length, flare, ragged edge, lace
    pub warts: Option<WartOptions>,  // count, size, jitter, coverage (top fraction), flatness
    pub material: MaterialOptions,   // palette (sRGB hex per zone), pattern, roughness, texture sets
    pub detail: MushroomDetail,      // quality for single-level use
}
```

All lengths in metres; shape ratios are relative to `size` so presets scale.

### Shape

- **Cap profile.** Each `CapFamily` (infinigen's nine) is an anchor table in
  unit cap radius, with sharp/smooth flags. Sampled as a Catmull-Rom/Hermite
  curve with sharp anchors honoured. The profile runs rim → apex for the top
  and rim → neck for the underside (from `UndersideOptions`). Infundibuliform /
  depressed families give funnel caps; puffball is a closed sphere-like cap
  family with zero stem.
- **Revolution.** Angle `θ` around the head frame; radius modulated by rim
  lobes (`r^2.1` weighting), per-quadrant asymmetry, ellipse ratio, twist, and
  a low-frequency noise displacement. One function `cap_point(t, θ)` is used
  by every LOD, so they agree.
- **Stem.** Centre line from height, lean, curve power and lean angle (inkwell)
  plus a bend; frames by parallel transport. Radius profile = base bulb +
  waist→neck + collar. Neck radius ties to `inner_radius` of the cap.
- **Ring.** An open revolved sheet hanging from the stem at a height: flare
  outward, droop, ragged lower edge by angular noise. Two-sided. Optional lace
  via an alpha mask in the material (geometry stays a sheet). Tag `RING`,
  `closed: false`.
- **Warts.** Poisson-disc points over the cap top `(t, θ)` with coverage and
  density; each a tiny low-poly dome aligned to the cap normal and sunk into
  the surface. Each level picks `WartDetail::{Mesh, Spots, None}` — `Spots`
  bakes them into vertex colour only.
- **Gills (shading only).** Underside vertices carry a zone id and an angular
  coordinate; the material draws radial stripes. No gill geometry in v1.

### Detail

```rust
pub struct MushroomDetail {
    pub shape: BodyShape,         // Solid | Shell | Flat
    pub radial_segments: u32,     // around cap and stem
    pub cap_rings: u32,           // along cap profile
    pub stem_rings: u32,          // along stem
    pub ring: bool,               // mesh the annulus sheet
    pub warts: WartDetail,        // Mesh | Spots | None
}
pub struct LodLevel { pub distance: f32, pub hysteresis: f32, pub detail: MushroomDetail }
pub struct MushroomLod { pub levels: Vec<LodLevel>, pub cull_distance: f32 }
```

Default ladder for the default 8 cm cap (scaled by `size / 0.08`):

| Level | From | Shape | Radial × cap × stem | Ring | Warts |
|---|---|---|---|---|---|
| 0 | 0 m | Solid | 48 × 16 × 12 | yes | Mesh |
| 1 | 1.5 m | Solid | 24 × 8 × 6 | yes | Mesh (fewer) |
| 2 | 4 m | Shell | 12 × 4 × 2 | yes | Spots |
| 3 | 10 m | Flat | 6 × 2 × ribbon | no | Spots |
| cull | 25 m | — | — | — | — |

Numbers are starting points to tune in the viewer. A helper picks segment
counts from a target edge length (infinigen's `triangle_budget` idea) so very
large and very small caps get sensible counts.

- **Solid:** cap top + underside meet at the rim and close against the stem
  neck; stem tube with a closed base. Watertight per part, verified with
  `Surface::is_closed`, shells recorded.
- **Shell:** cap top only (open at the rim, a short downturned lip so the
  edge reads from below), stem tube without ends. Two-sided; normals are the
  outward surface normals (rounded, like plant strips) so back faces light
  plausibly.
- **Flat:** cap = `Shell` at minimum resolution; stem = a tree-style ribbon:
  each stem section emits one strip with `BillboardVertex { pivot: centre,
  axis: stem tangent, offset }`, and the perpendicular strip goes into `cull`.
  `Mushroom::mesh_with_detail(&MushroomDetail::FLAT)` builds it on its own.

### Mesh output

```rust
pub struct MushroomMesh {
    pub surfaces: MeshBuffers,          // positions, normals, uvs, tangents, colours (linear RGBA), zone, indices
    pub billboards: Option<Billboards>, // ribbon stem: pivot/axis/offset per vertex
    pub cull: MeshBuffers,              // fallback ribbon strips
    pub parts: Vec<PartSpan>,           // index ranges per Cap/Underside/Stem/Ring/Warts
}
```

UVs: cap top polar (u = θ / 2π wrapped, v = profile arc length), underside the
same, stem cylindrical (u around, v along arc length), ring u around / v down.
Tangents for normal maps; ribbons get none (as in tree).

### Object

`Mushroom::object(&MushroomLod) -> Object` — per level, parts tagged
`MUSHROOM` + `CAP` / `STEM` / `RING` / `WART`. Solid parts `closed: true` with
shells. Ribbon stems have no surface normals and are skipped (same rule as
tree ribbons); the cap shell is included `closed: false`.

### Colour

- Palette per zone in sRGB hex (presets) converted to linear, plus
  infinigen's random-hue rule for "random species".
- Vertex colours: cap top radial ramp (centre ↔ rim), underside colour,
  stem gradient (base darker), ring, warts; `Spots` warts painted in.
- Pattern parameters (`Pattern::{None, Speckle, Cracks, Spots}` with scale and
  strength) go to the Bevy material uniform, evaluated in object space so they
  stay put across LODs.

### Cluster

```rust
pub struct ClusterOptions { seed, count: RangeInclusive<u32>, layout: Layout /* Tuft | Line */,
                            scale_range, lean, spacing, variants: u32, bend }
pub struct MushroomCluster { variants: Vec<Mushroom>, members: Vec<Member> }
pub struct Member { variant: u32, transform: Affine3A /* translation, rotation, uniform scale */ }
```

- Members derive from one base `MushroomOptions` (same species), varied by
  seed, scale 0.3–1.2, lean; drawn from `variants` distinct meshes (default 3)
  so instancing works.
- Non-overlap: each member gets a conservative proxy (stem capsule + cap
  oblate ellipsoid); members slide outward along their layout direction until
  proxies clear (infinigen's search, without the KD-tree/remesh).
- `MushroomCluster::object(lod)` merges members per level for hosts that need
  one `Object` (e.g. clump on a cluster).

### Scatter

```rust
pub struct ScatterOptions { seed, grows_on: TagFilter, up: Vec3, max_slope, density /* per m² */,
                            min_spacing, cluster_chance, cluster: ClusterOptions, mushroom: MushroomOptions }
pub struct MushroomScatter { variants: Vec<Mushroom>, clusters: Vec<MushroomCluster>,
                             instances: Vec<Instance> }
MushroomScatter::on(&Object, &ScatterOptions) -> Result<MushroomScatter, ScatterError>
```

- Takes `object.merged(0, grows_on)`; area-weighted triangle sampling with a
  facing mask (`normal · up ≥ cos(max_slope)`), Poisson-style `min_spacing`
  rejection; each site is a single mushroom or a cluster.
- Orientation blends surface normal and up (infinigen `normal_ratio` 0.4–0.6)
  with a random yaw; base sunk slightly into the surface.
- Output is instance records + variant library; `object()` bakes on request.
- Errors: `NoSurface` (filter matched nothing), invalid options.

### Exports

`export-obj` and `export-gltf` features; one node per LOD
(`mushroom_LOD{n}`), vertex colours exported, options in
`asset.extras.mushroomOptions`. Ribbon stems export the fallback strips.

---

## Bevy design (`procgena_mushroom_bevy`)

- `MushroomPlugin { texture_root: Option<String>, texture_compression, lod: MushroomLod }`.
- `MushroomParams::{Options(Arc<MushroomOptions>), Asset(Handle<…>)}` with
  `new` / `preset`; `.mushroom.json` loader validating options.
- Build on `AsyncComputeTaskPool`; cache keyed by options JSON; children
  `MushroomPart { lod, kind }` with `VisibilityRange` margins and the last
  level ending at `cull_distance`; `GeneratedObject` inserted on finish.
- `MushroomMaterial = ExtendedMaterial<StandardMaterial, MushroomExtension>`:
  vertex colour × optional texture, object-space pattern, gill stripes on the
  underside zone, two-sided for `Shell`/`Flat`/ring.
- Ribbon stem: `mushroom_billboard.wesl` (mechanism copied from
  `tree_billboard.wesl`, including prepass/shadows).
- `MushroomClusterParams` — spawns one child per member sharing the variant's
  `Handle<Mesh>` per level (automatic batching).
- `MushroomScatterParams` — child of a host, waits for the host's
  `GeneratedObject`, converts `up` to host-local, rejects non-uniform scale,
  rebuilds when host object/transform changes; spawns shared-mesh instances.
- `examples/viewer.rs` (presets, LOD distance overlay, scatter on a stone).

---

## Presets (v1)

| Preset | Cap family | Extras | Notes |
|---|---|---|---|
| Fly agaric | convex | ring, warts | red cap, white warts, bulb base |
| Porcini / bolete | convex (thick) | — | fat stem, sponge-like underside colour |
| Chanterelle | infundibuliform | — | decurrent underside, wavy rim |
| Parasol | umbonate (large) | ring, scales | tall slim stem, flat scales |
| Liberty cap | conical | — | thin bent stem |
| Shaggy ink cap | ovate | scales | elongated cap |
| Button | convex (small) | — | white, short stem |
| Enoki | convex (tiny) | — | long thin stems, cluster default |
| Puffball | puffball (no stem) | warts (tiny) | sphere-like body |
| Storybook | campanulate | spots (colour) | stylised, inkwell-like |

---

## Tests

- `tests/options.rs`: defaults validate; JSON round-trip; `from_json`
  partial merge; each invalid range rejected.
- `tests/mesh.rs`: determinism (same seed ⇒ identical buffers); no NaN; unit
  normals; Solid parts closed (`Surface::is_closed`); Shell/Flat parts open;
  triangle counts strictly decrease along the default ladder; ribbon billboard
  data present only for Flat.
- `tests/silhouette.rs`: LOD outlines vs LOD0 by rasterised IoU (tree method).
- `tests/object.rs`: tags and closedness per level.
- `tests/cluster.rs`: proxies don't overlap; member count in range; variants
  reused.
- `tests/scatter.rs`: on `SurfaceMesh::ground` all instances lie on the
  surface and within slope; `grows_on` respected; `NoSurface` error.
- `tests/export_obj.rs`, `tests/export_gltf.rs` behind features.
- Bevy: headless spawn tests, LOD ranges, two-sidedness per shape, scatter on
  a host, `tests/convert.rs`; shader compile test for the billboard WESL.

---

## Phases

| Phase | Work | Depends on |
|---|---|---|
| 0 | **Coordinator:** add both crates to root `Cargo.toml`; tags `MUSHROOM`, `CAP`, `STEM`, `RING`, `WART` in `procgena_mesh/src/tag.rs` (another agent is editing `procgena_mesh` now); add `mushroom/prior_art/` to `.gitignore` and the `_typos.toml` excludes; `CLAUDE.md` package rows; README table row | — |
| 1 | Scaffolding, `options`, `presets` (data only), `rng`, `noise`, `profile` curves + cap family tables | 0 |
| 2 | Stem + cap meshing for `Solid` and `Shell`, UVs/tangents, LOD ladder, `mesh_with_detail`, `lod_meshes` | 1 |
| 3 | `Flat`: open cap shell + ribbon stem with billboard data and `cull` fallback | 2 |
| 4 | Ring sheet; warts (`Mesh` / `Spots`) | 2 (parallel with 3) |
| 5 | Colour zones, palettes, patterns; finish presets | 2 |
| 6 | `object()`; cluster | 2–4 |
| 7 | Scatter on `Object` | 6 |
| 8 | Bevy crate: plugin, material, billboard shader, LOD, cluster/scatter components, viewer | 3–7 (single-mushroom part can start after 5) |
| 9 | Exports (OBJ, glTF) + example | 2–6 (parallel with 8) |
| 10 | Studio "Mushroom" tab (10 touch points) | 8 |
| 11 | `mushroom/textures.yml` + tools entry; CC0 sets for cap/stem | 5 (sources need a decision) |
| 12 | Docs, READMEs, polish | all |

## Non-goals for v1

- Gill blades as geometry (shading only).
- Morel / pitted (honeycomb) caps.
- Shelf / bracket fungi and oyster fans (non-axisymmetric heads).
- A GPU-procedural field like inkwell or `procgena_plant` (no stored
  meshes) — scatter produces records for instanced meshes instead.
- Wind sway (mushrooms are stiff; a small stem sway could reuse tree's wind
  WESL later).
- Growth/ageing animation.

## Open questions

1. **Shared billboard code.** Copy tree's `BillboardVertex` + ribbon builder +
   billboard WESL into the mushroom crates (established pattern), or move them
   into `procgena_mesh` (+ a shared Bevy shader) so tree and mushroom share
   one implementation? The latter is a cross-crate refactor touching tree.
2. **Scatter mask.** Copy clump's facing/exposure mask, or start with facing
   only? (Plan assumes facing + slope only in v1.)
3. **Texture sources** for cap/stem sets (CC0 only) — none chosen yet.
4. **Lace ring.** Alpha-masked sheet (needs alpha-test material on the ring)
   vs ragged geometric edge only.
5. **Default size and ladder distances** — tune in the viewer after Phase 3.
