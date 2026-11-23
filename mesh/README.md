# procgena_mesh

The shared description of what procgena's generators produce, so one
generator can build on another's result. Every generator returns an
`Object`: detail levels, each made of tagged triangle-mesh parts, such as a
tree's `bark` + `trunk` or a rock's `rock`. A part also says whether it is
watertight and how it splits into separate pieces. Another generator picks
the parts it wants by tag: snow, for example, grows on rock and bark but not
on leaves.

The crate also holds `Surface` and `SurfaceMesh`, the plain triangle meshes
the generators read and write. With the `bevy` feature, a `GeneratedObject`
component puts an object on a Bevy entity; the procgena Bevy plugins add it
to the rocks, trees and clumps they generate.

See the crate documentation for the types, and
[PLAN-clump-bevy.md](../PLAN-clump-bevy.md) for the design.
