# procgena_clump

Grow procedural clumps of snow, moss and other materials on top of a surface
mesh in Rust. `procgena_clump` takes a triangle mesh, such as a rock or a tree's
bark, or a tagged object from another procgena generator, and builds a
separate clump mesh that sits on it. The input is never changed: the clump's
edges thin out and sink just below the surface, so no seam shows. Presets
cover snow caps, moss carpets and cushions, and burls.

The workspace also includes `procgena_clump_bevy`, a Bevy plugin: spawn a
`ClumpParams` as a child of a rock from `procgena_stone_bevy`, a tree from
`procgena_tree_bevy`, or your own tagged mesh, and the clump grows in the
background and is drawn with projected snow and moss textures. See its crate
documentation, and try it with:

```bash
cargo run -p procgena_clump_bevy --example viewer
```

Clump textures are not in the repository. Download them with:

```bash
uv run --project tools textures download clump
```

This project is under construction. See [plan.md](plan.md) for the clump
algorithm's plan and what is still open, and
[procgena_specimen](../specimen/README.md) for the tagged specimens
generators share.
