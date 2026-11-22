# TODO

## Phase 9: polish (last planned phase)

- [ ] Presets: boulder (Unity defaults), pebble, slab, shard, built from
      `stretch` and the carve settings. `crates/procgena_stone/src/presets.rs` was
      started in another session; check its state before continuing.
- [ ] README: a top-level overview of the CPU, GPU and pool paths.
- [ ] Crate docs for `procgena_stone` and `procgena_stone_bevy`.
- [ ] Feature comparison with the Unity original: what matches, and what changed on purpose.
- [ ] Codex comment pass for Phase 4 (still pending in `port_plan.md`).

## Next feature: landscape scatter

- [ ] GPU-placed rocks without entities, generated per chunk around the camera
      (like grass), built on the stone pool and its per-view culling.

## Stone pool follow-ups

- [ ] Skip empty draws: use `multi_draw_indexed_indirect_count` where supported,
      instead of one indirect draw per (variant, level) per view and pass.
- [ ] Respect `RenderLayers` on cameras and pooled rocks.
- [ ] Re-upload only changed rocks instead of all of a pool's rocks.
- [ ] Correct motion vectors for moving rocks (store previous transforms).
- [ ] `.stone_pool.json` asset loader.
- [ ] Honour `ExtractedView::invert_culling`, and support negative-scale transforms.
- [ ] Point and spot shadow views choose detail levels from the first camera;
      revisit for split-screen.
- [ ] Run the multi-batch split on a GPU (it is unit-tested only).
- [ ] Viewer: wait for generation to finish before a pool screenshot, instead of a fixed delay.

## Memory

- [ ] Reserve less mesh room per pool variant (about 2.6 MB with the default
      levels) and rely on the overflow retry to grow meshes that need more.

## Examples

- [ ] Physics example: a Rapier collider for a pooled rock, built from
      `StonePool::variant_options`.

## Textures

- [x] Upload the regenerated textures, `orh` KTX2 files and `orm` JPEGs, with both
      manifests to GCS, then remove the old `*-roughness-*` files locally and in the bucket.
- [ ] Pooled rocks ignore screen-space ambient occlusion (SSAO) in forward rendering
      (deferred applies it). The CPU material gets it from Bevy's standard input.
- [ ] Optional material settings: height-blend sharpness (fixed at 0.2 today)
      and ambient-occlusion strength.
