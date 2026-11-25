# GPU terrain implementation

2026-11-25. Implementation of the GPU stages in `edit_cache_plan.md`,
starting with ground painting because every streamed chunk has weights.

## Landed

1. Retained `GroundGrid`: sample once, repaint independently. CPU painting
   retains its original palette reduction and quantisation. Ground-layer
   tests check equality with a fresh read and zero reads during repaint.
2. GPU ordered layer evaluation and weight quantisation, writing the two
   RGBA8 weight textures directly. The terrain streamer selects this path
   on compute-capable devices for supported `GroundStack` painters.
   It covers the standard terrain stack: height, slope, water and road
   conditions, opacity, ordering and vertical scale. Unknown measures
   preserve the CPU semantics. Stable material-id palettes avoid a GPU
   reduction/readback. Parent subdivision filters quantised parent weights.
3. Material-only edits reuse retained grids, keep chunk geometry, and update
   material weights. In-flight reads are repainted if their paint revision
   is stale. Source/tier changes still rebuild terrain. Replace the source
   Arc to signal a source-content change.

Custom painters, noise layers, stacks with more than eight materials, and
non-finite ramp endpoints use CPU painting. Devices without compute keep
the CPU path. Terrain queries and planning never use GPU readback.

## Validation

- `cargo test -p procgena_terrain --test ground_layers`
- `cargo test -p procgena_terrain_bevy --lib`
- `cargo test -p procgena_terrain_bevy --lib gpu_weights_match_cpu_and_parent_subdivision -- --ignored`
- `cargo clippy -p procgena_terrain_bevy --all-targets --no-deps -- -D warnings`

The Metal readback test compares full chunks and child subdivision to the
CPU reference within one byte per channel; unsplit weights sum to 255.
Streaming tests check zero source reads and unchanged chunk entities after
a settled ground edit, and current paint after an edit during loading.

## Remaining

This is the painting portion of GPU2, not the entire GPU terrain plan.
Initial height, slope and feature-site sampling still runs on the CPU.
No cold-build speedup has been claimed or benchmarked. Retaining sites adds
memory per loaded chunk (including world positions and optional measures);
GPU jobs retain the same Arc rather than a second grid.

- Move GPU2's expensive site/measure evaluation to GPU buffers, with parity
  around roads, water and seams. Do not silently substitute grid-difference
  slopes for analytic gradients without measuring the visual error.
- Port layer noise with split coordinates and CPU/GPU parity; keep fallback
  for arbitrary custom painters and palettes requiring reduction.
- GPU1: build the relief-only CPU preview twin, port noise and E, and wire
  provisional drawing into studio edits. It must not become game terrain.
- GPU3 (later in the accepted plan): border/convergence/carving buffers,
  chunk-relative f32 height evaluation, and reusable GPU-displaced grids.
  Prove millimetre-scale height parity and shared-edge agreement before
  removing CPU sampling from the drawn view.
