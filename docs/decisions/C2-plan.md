# C2 plan: the `Scatter` trait, masks out, World-anchored ids

Worker plan for roadmap milestone C2 (wave 5). Written 2026-11-24 before any
code. Follows ARCHITECTURE.md ("Masks and placement", rules 4 and 5), D2,
D3 and C1. Plant is out (R3).

## Bottom line

- **New in `procgena_core` (engine-free):**
  - `address`: `RegionId(u64)`, `CellId(u64)` (packed `x`/`z` like
    `RegionKey::id`), `AddressPrefix { region, ecotope, scatter }` and
    `CopyAddress { region, ecotope, scatter, cell, index: u16 }` (D3 §7b).
    Part paths (`PartStep`, `Address`) are not in C2.
  - `scatter::lattice`: the integer placement contract, ported from the D2
    spike's tested CPU twin (`docs/decisions/d2/spike/src/int.rs`,
    `cpu.rs`): plant's 32-bit PCG `hash`/`hash2`, World fixed point
    (1/256 m, `i64` on the CPU), a `Lattice` of whole fixed-point cells
    (any size, not only powers of two), `u24` ranks, the density test
    `u24 × 255 < p24 × local8` and the rank order
    `u_b × local_a < u_a × local_b` with ties broken by cell then index,
    `K_d` (`d` odd, default 5), the window margins, and a `ChunkGrid`
    (64 m by default) that owns a spot by its cell's corner.
  - `scatter::footprint`: `Disc` (fixed-point centre and radius) and
    `Footprints`, a bucketed set of discs. **Hard exclusion is
    disc-against-disc**: a candidate whose own footprint discs overlap any
    disc above it (`|a − b|² < (r_a + r_b)²` in integers) is dropped. A
    log gives a chain of discs along its length; trees, stones, stumps and
    mushrooms give one disc.
  - `scatter::mask`: `MaskLayer`, an 8-bit texture on World-anchored texels
    (0.5 m for footprints), read with the spike's fixed-point bilinear
    filter. It implements `Mask`, so one Scatter's mask out is the next
    one's mask in.
  - The **`Scatter` trait**, object-safe so an Ecotope can hold user
    Scatters as `Box<dyn Scatter>`:
    `lattice()`, `reach() -> Reach { s, j, f, rounds }`, `draw_range()`,
    `mask_source() -> MaskSource { Copies, Density }`, `slots(world_seed)`
    (each slot's `PoolId`/`SpecimenId`, since core cannot name
    `procgena_specimen::Specimen`), and `place(&ScatterInput) ->
    Vec<ScatterCopy>`. Provided methods: `resolve(&input, &CopyAddress)`,
    `mask(&input)` (places the area grown by `F` and stamps discs, drawing
    nothing) and `footprints(&input)`. `ScatterCopy` is the common copy:
    `address`, `slot`, `transform` (relative to the area's centre) and its
    footprint discs.
  - `ChunkCache`: the CPU cache of placed copies per chunk for one Scatter,
    with batched `resolve_all` (sort addresses, group by chunk, place each
    chunk once) and footprints over a region (window B: a chunk of a lower
    Scatter reads the finished copies of the Scatters above from the
    cache).
  - `ScatterInput` gains `prefix: AddressPrefix` (zero when built on its
    own: `RegionId(0)`, zero salts) and `exclude: Option<&Footprints>`,
    with builders; `ScatterInput::new` keeps its signature.
- **One meaning of "area" for every Scatter:** placing over an area returns
  the copies **whose lattice cell's corner lies in the area** (half-open in
  fixed point). A chunk is just an area on the chunk grid, so the chunks of
  an area tile it exactly and each copy has one owner. Copies may stand up
  to `J` outside their area. Every per-candidate value is a pure function of
  (seed, World cell, index) and World-space inputs, never of the area or
  the chunk, and `K_d` runs over the area grown by `S + 2J`.
- **Stage order for every Scatter:** density (rank against mask × own
  density) → exact disc exclusion → slope and footing (`place()`) → `K_d`
  spacing → the Scatter's own patches/glades → mask out. Exclusion comes
  before spacing, so a candidate inside a trunk never suppresses a
  neighbour.
- **Each asset** keeps its `new(&options, &ScatterInput)` (now the World's
  answer clipped to the area, through the same chunk-independent code), so
  the `*_bevy` crates, the studio and C3a's adapters keep compiling. The
  `<Asset>Scatter` type implements the trait:
  - **Tree**: `TreeScatter` (options; variants are cheap) implements it.
    The 0.5 m lattice moves to World cells and PCG hashes, the greedy
    rank-order spacing becomes `K_5`, `TreeCopy::id: u64` becomes
    `address: CopyAddress`. Species (`ScatterTree`) get a `key: ChildKey`;
    per-species streams (clustering, patches) are salted by the key, not
    the list index. Keys are written in `.tree_scatter.yml` (explicit in
    the 8 built-in files; a missing key defaults to the named tree preset's
    key) and validated unique with distinct salts.
  - **Stone**: **one lattice for all classes** (D3: the address has one
    cell and an index, and the mix entry is decided from the address, not
    part of it). The cell is the smallest class spacing, shrunk if needed so
    the summed chance per cell stays ≤ 1. The class is picked from the cell
    by its density share, like tree species. Cluster members keep their own
    `index` in the cell. Spacing keeps today's rule (radius sum, and the
    class spacing within a class between cells); classes earlier in the
    list still win, because the `K_d` order is (class, rank, cell, index).
    `StoneInstance::{cell, index}` become `address`. Classes get
    `ChildKey`s too. `StoneScatter::prepare(options, world_seed)` builds
    variants without placing.
  - **Deadwood**: the corner-anchored `1/sqrt(density)` grid and the
    scan-order spacing go. The cell becomes `min_spacing` (shrunk so the
    chance per cell stays ≤ 1), density is the rank threshold, spacing is
    `K_5`, a slope limit as today. `Instance` gains `address`; logs give a
    chain of discs. `EndKind::Taper` becomes `Taper {}` so unknown keys are
    refused (its written YAML stays `kind: taper`).
  - **Mushroom** (ground scatter only): its lattice moves to World cells;
    its one-round neighbour rule becomes `K_5` like the rest; the
    whole-area truncation to `MAX_SITES` by rank goes (the count cap stays
    a check on an area build). `MushroomCopy::cell` becomes `address`.
    `MushroomScatter::on` (host growth) is untouched; the trait places
    nothing for a Scatter built with `on`... see "Open points".
- **Bevy**: `tree_bevy` keeps the area build (no streaming loader in C2);
  ids become `CopyAddress` through `PlacedTree`, `TreeScatterInstances`,
  `PromotedTree` and `TreeScatterHit`; the private "grove" names in
  `tree_bevy` and `procgena_tree` become "scatter". Other `*_bevy` crates
  and the studio get call-site fixes only.
- **Not in C2:** a GPU path, a Bevy chunk-streaming loader, the quantised
  slope byte for GPU parity (the CPU decides slope from `Ground` as today),
  part paths, `DrawHandle` (it needs loaded GPU chunks), a per-Scatter `d`
  option in files (the trait reports `rounds`; every Scatter uses 5), and
  the canopy-shade influence mask (`MaskSource::Density` exists for plant
  and later shade).

## Steps (one commit each)

1. **Core types**: `address`, `scatter::lattice` (hash, fixed point,
   lattice, rank order, `K_d`, chunk grid, margins), `scatter::footprint`,
   `scatter::mask`, `ChildKey` serde, the trait, `ScatterCopy`,
   `ChunkCache`, `ScatterInput` additions. Unit tests: PCG reference
   values, `K_d` never breaks spacing and is a subset of greedy for odd `d`,
   a chunk built with the margin equals the whole area (any order, two
   chunk sizes), **negative control** (a margin one `R` short differs),
   disc tests exact at the boundary, bilinear reads, cache `resolve_all`.
2. **Tree** (core crate, then `tree_bevy`, then the studio call sites).
3. **Stone** (core crate, then the viewer call site).
4. **Deadwood** (core crate, `Taper {}`, then `deadwood_bevy` call sites).
5. **Mushroom** (core crate, then `mushroom_bevy` call sites).
6. **Cross-asset tests and docs**: per asset, chunks built in row, reverse
   and shuffled order give the same copies and addresses as one area build;
   a copy standing past its chunk's edge appears exactly once; resolve
   finds every copy and `None` for empty cells; stones excluded from tree
   discs never overlap a trunk; mask over a chunk without drawing.
   ARCHITECTURE.md TODOs ticked, ROADMAP C2 status.

## Tests that change intent (reported as moved, not deleted)

- "Same layout at any centre" tests (tree
  `trees_are_placed_relative_to_the_area_centre`, stone
  `even_stones_on_flat_ground_look_the_same_anywhere`, mushroom
  `a_far_centre_gives_the_same_layout`) become "the same World spot gives
  the same copy and id from any area that holds it".
- Deadwood `scatter_is_repeatable_spaced_and_in_bounds`: copies are owned
  by cell corner, so a copy may stand up to one cell outside the area.
- Every expected count or position moves (new hash, World lattice, `K_5`).

## Open points (sent to the coordinator; work continues meanwhile)

1. `RegionId` lives in `procgena_terrain` today. C2 defines it in core;
   terrain should switch to core's (terrain/W0 lane follow-up).
2. Studio: only call-site fixes (`ScatterTree` gains `key`); its
   `grove`/`field` module renames stay for a later wave.
3. Mushroom `on()` (host growth, moving to `MushroomGrowth`): the trait is
   implemented by the ground-scatter path only.
4. C3a's adapters call `X::new(&options, &ScatterInput)`, which keeps its
   signature. Results move (new hash, lattice and spacing), so C3a tests
   that pin counts may need new numbers when C2 merges after it.
