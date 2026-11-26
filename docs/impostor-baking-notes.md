# Impostor Baking System: Best Practices and Design Notes

## Overview

An impostor is a billboard rendered from the real 3D model, swapped in for the geometry at distance. Every impostor is a billboard, but not every billboard is an impostor. This doc covers the octahedral approach (Ryan Brucks, 2018) plus refinements and our own design ideas for a Rust baker.

## View Layout

- Capture the model from directions on an octahedral lattice, flatten the octahedron to a square, and store one snapshot per grid cell (for example 8x8 or 16x16).
- Octahedral mapping spreads directions fairly evenly, avoiding pole bunching.
- Use hemi-octahedral layouts for assets rarely seen from below (trees, rocks); the saved cells buy sharper snapshots.
- At runtime, map the camera direction through the same formula, take the three nearest snapshots and blend them by weight.
- Use per-snapshot depth to reproject before blending. This cuts ghosting and improves parallax.

* Choose views per pixel, not per card: intersect the view ray with the impostor's bounding sphere and map that hit direction to the lattice. The card-centre direction is subtly wrong across the card.
* Clip pixels outside the bounding sphere; a rotating card can otherwise show atlas content beyond it.

## What to Bake

Bake surface properties only, never lighting. The bake is a frozen G-buffer.

- **Albedo:** unlit base colour, in sRGB.
- **Normals:** object space, so they stay valid under any placement; packed from -1..1 to 0..1. Derived from interpolated vertex normals perturbed by the material's normal map via the tangent frame. Store linear; prefer two-channel compression (BC5) and rebuild Z.
- **Depth:** normalised to the bounding sphere for precision, often in the normal texture's alpha. Used for pixel depth offset, shadow receiving and reprojection.
- **Optional:** roughness, metalness, ambient occlusion (geometry-dependent, safe to bake), and a thickness or translucency mask for foliage.

All lighting, fog and atmospherics are applied at runtime by the same shaders as real geometry, so a plain, minimal bake environment is correct. Never bake from a lit beauty render.

### Roughness at distance (specular anti-aliasing)

Mipmapping averages many varied normals into one smooth normal, so distant impostors look too shiny and shimmer. Compensate by raising roughness where normals vary:

Do this in the baker at mip generation, never by hand:

1. Average the unnormalised normals of each texel's footprint. The shorter the averaged vector, the more the underlying normals vary.
2. Turn that length into a variance estimate (Toksvig's formula, or a von Mises-Fisher fit for a more accurate version).
3. Add that variance to the material's own roughness (for GGX, add to alpha squared) and store the widened roughness in that mip.
4. Renormalise the stored normal.

Matching shininess also reduces pop at the mesh-to-impostor switch.

## Bake Pipeline Best Practices

- Use orthographic cameras sized to the bounding sphere, not the box, so nothing clips at any angle.
- Keep the pivot consistent with the source mesh.
- Dilate edge pixels outward and pad between cells to prevent fringes under mipmapping and blending.
- Trade off frame count against per-frame resolution based on real viewing distances.
- Wind and animation are not captured; accept static distant impostors or add a cheap matching shader wobble.
- Make baking part of the asset build so impostors regenerate when meshes or materials change.
- Match the impostor material to the full mesh, and cross-fade or dither at the mesh-to-impostor switch to avoid pop.
- Casting good shadows is hard; consider a simple shadow proxy.

* Add a simplified mesh LOD (for example from meshoptimizer) between the full mesh and the impostor, and switch to the impostor only further out. Draw impostors instanced, with BVH frustum culling; a three.js demo ran 200k trees this way.

## Storage Format

Ship impostors as GPU block-compressed textures with prebuilt mips in a KTX2 container, so loading is a straight upload with no decode.

| Channel | Desktop format | Notes |
| --- | --- | --- |
| Albedo + alpha | BC7 (BC1/BC3 if memory-bound) | sRGB |
| Normals | BC5 | Two channels, rebuild Z in shader; linear |
| Depth, roughness, AO | BC4 each, or packed into BC7 | Linear |

- Mobile: ASTC for all channels.
- Container: KTX2 (mips, array layers, format info, optional Zstandard supercompression). DDS is the older DirectX alternative.
- Basis Universal in KTX2 transcodes to the target GPU format at load; useful for desktop plus mobile, at a small quality and load-time cost.
- Pipeline order: render to uncompressed targets, generate mips with the roughness correction, BC-encode, write KTX2. Never compress before mipping.
- Keep snapshot cells a multiple of 4 pixels (BC works on 4x4 blocks), with padding between cells.
- Rust: the ktx2 crate reads the container; wgpu supports BC and ASTC when the device feature is enabled. BC7 encoding is the slow step; use a fast CPU library or a compute-shader encoder.

## Tight-Fitting Cards

A plain quad (2 triangles) is mostly transparent, and alpha-tested pixels still cost shading. Replace it with a polygon that hugs the silhouette.

1. Take the maximum alpha across all snapshots to get one combined footprint.
2. Threshold it and compute the convex hull.
3. Simplify to a small number of corners (often 6 to 8) while minimising area and keeping everything enclosed.
4. Pad slightly for blending, reprojection shifts and lower mips.

An N-corner convex polygon fans into N-2 triangles (8 corners give 6 triangles). The shape is fixed per asset at bake time and lives in the card's local space. Typical savings are around 30 to 50% of pixels for a negligible vertex cost.

## Per-View Outlines

A fancier variant stores one outline per snapshot; the vertex shader selects and combines the outlines for the views currently blended. Triangle count stays fixed while the shape changes with view angle.

**Benefits:** a tighter fit for assets that look very different by angle (tall, thin or lopsided), with the biggest gains in hemi-octahedral layouts.

**Trade-offs, assuming a correct implementation:**

- More data: one polygon per snapshot rather than per asset.
- Heavier vertex shader; this hurts when vertex-bound with many tiny distant impostors.
- Savings sit between the shared outline and the single-view ideal, because the shape must cover all blended views.
- Little gain for roughly symmetric assets such as round bushes or rocks.

## Our Design Ideas: Adaptive Outline Selection

### Distance tiers

Use per-view outlines for near impostors and the shared outline for distant ones. Savings scale with screen area while vertex cost is fixed. The swap is invisible because both outlines only remove pixels that would be discarded anyway, so no cross-fade is needed. Costs: two shader variants and separate draw batches. Profile to confirm the near tier covers enough screen to justify it.

### Bake-time, per-asset decision

After baking, measure each asset's potential savings and only enable per-view outlines where they pay off:

1. Compute the shared outline's area.
2. Compute the average area of the per-view outlines, using the combined outline of the views blended at each angle.
3. Store the fractional saving as an asset property.

### Threshold in screen pixels

The vertex overhead is fixed, so the decision should rest on absolute pixels saved, but in screen space, not atlas space (every asset fills its atlas cell regardless of size). Estimate:

> screen pixels saved = fractional saving x expected on-screen area

where the expected on-screen area comes from the asset's world size and its nearest impostor distance. Compare that against a fixed threshold, tuned by profiling. Optionally weight by instance count. The bake decides whether an asset gets the near tier; distance decides when it is used.

## Context: Nanite

Unreal 5's Nanite virtualises geometry: meshes are split into hierarchical triangle clusters, selected per frame to approach one triangle per pixel, partly rasterised in compute and streamed from disk. It reduces the need for impostors, especially for rocks and buildings; foliage has held out longest but is improving.

## References and Licensing

- [Ryan Brucks, Octahedral Impostors](https://shaderbits.com/blog/octahedral-impostors/): the standard reference.
- [Original ImpostorBaker repo](https://github.com/ictusbrucks/ImpostorBaker): UE4.19 only and has no license, so treat it as all rights reserved. Study it, but do not copy or port its code or shaders. The technique itself is free to implement. (Not legal advice.)
- [meshoptimizer-rs octahedral impostor PR](https://github.com/Tavrin/meshoptimizer-rs/pull/102): a CPU baker on a fork, apparently albedo and coverage only; check merge status.
- Amplify Impostors (Unity): example of depth-aware blending and extra baked channels.
