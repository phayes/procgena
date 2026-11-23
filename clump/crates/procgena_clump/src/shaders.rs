//! Shader source for generating clumps on the GPU.
//!
//! WESL is WGSL, the WebGPU shading language, extended with imports between
//! modules. These shaders provide the GPU equivalent of CPU clump
//! generation. GPU rounding can change vertex counts, ordering and values, so
//! a GPU clump describes the same surface as a CPU one, not the same buffers.
//!
//! The modules import each other as `package::<name>`, so register each one
//! under the path listed in [`SHADER_MODULES`]. Library modules such as
//! [`HASH_WESL`] declare no bindings and can be imported into your own shaders.

/// Repeatable pseudo-random integers and values in `[0, 1)`, matching the
/// CPU generator's random choices. Import as `package::hash`.
pub const HASH_WESL: &str = include_str!("../shaders/hash.wesl");

/// Vector helpers that round like the CPU generator's. Import as `package::math`.
pub const MATH_WESL: &str = include_str!("../shaders/math.wesl");

/// Triangle quality and intersection tests. Import as `package::geometry`.
pub const GEOMETRY_WESL: &str = include_str!("../shaders/geometry.wesl");

/// Coverage settings and the height of a clump at a surface point.
/// Import as `package::field`.
pub const FIELD_WESL: &str = include_str!("../shaders/field.wesl");

/// Checks for merging two points while simplifying the working surface.
/// Import as `package::collapse`.
pub const COLLAPSE_WESL: &str = include_str!("../shaders/collapse.wesl");

/// Exclusive prefix sums over regions of a `u32` arena. Declares bindings
/// and entry points. Import as `package::scan`.
pub const SCAN_WESL: &str = include_str!("../shaders/scan.wesl");

/// Stable radix sort of element indices by multi-word keys. Declares
/// bindings and entry points. Import as `package::sort`.
pub const SORT_WESL: &str = include_str!("../shaders/sort.wesl");

/// The generator's shared bindings over a batch of requests: the per-request
/// settings, batch-wide region offsets, scratch arena and counters.
/// Import as `package::batch`.
pub const BATCH_WESL: &str = include_str!("../shaders/batch.wesl");

/// Generator passes that read each request's input mesh. Declares bindings
/// and entry points. Import as `package::ingest`.
pub const INGEST_WESL: &str = include_str!("../shaders/ingest.wesl");

/// Generator passes that weld input vertices and measure whether each input
/// is closed. Declares entry points. Import as `package::weld`.
pub const WELD_WESL: &str = include_str!("../shaders/weld.wesl");

/// Builds a BVH over each request's input triangles, and the ray and
/// closest-point queries that use it. Declares entry points.
/// Import as `package::bvh`.
pub const BVH_WESL: &str = include_str!("../shaders/bvh.wesl");

/// Measurements of points on a request's input: room, inside spans and
/// exposure. Declares entry points. Import as `package::measure`.
pub const MEASURE_WESL: &str = include_str!("../shaders/measure.wesl");

/// Scattered clumps: placing them and building their meshes. Declares entry
/// points. Import as `package::scatter`.
pub const SCATTER_WESL: &str = include_str!("../shaders/scatter.wesl");

/// Writes each request's finished mesh, draw arguments and status. Declares
/// bindings and entry points. Import as `package::output`.
pub const OUTPUT_WESL: &str = include_str!("../shaders/output.wesl");

/// The working surface a layer clump grows on. Declares entry points.
/// Import as `package::patch`.
pub const PATCH_WESL: &str = include_str!("../shaders/patch.wesl");

/// Simplifies a layer's working surface toward its target edge length.
/// Declares bindings and entry points. Import as `package::simplify`.
pub const SIMPLIFY_WESL: &str = include_str!("../shaders/simplify.wesl");

/// Finishes a layer's working surface: how far the input rises and dips
/// around each point, each point's measurements, and the triangles a clump
/// may cover. Declares entry points. Import as `package::envelope`.
pub const ENVELOPE_WESL: &str = include_str!("../shaders/envelope.wesl");

/// Builds a layer's sheet from its working surface and refines it toward
/// the target edge length. Declares bindings and entry points. Import as
/// `package::refine`.
pub const REFINE_WESL: &str = include_str!("../shaders/refine.wesl");

/// Pushes a layer's refined sheet out into an open clump. Declares bindings
/// and entry points. Import as `package::shell`.
pub const SHELL_WESL: &str = include_str!("../shaders/shell.wesl");

/// Closes a layer's sheet into a solid with an underside and walls.
/// Declares entry points. Import as `package::close`.
pub const CLOSE_WESL: &str = include_str!("../shaders/close.wesl");

/// Every shader module as `(module path, source)`, for registering with a WESL resolver.
pub const SHADER_MODULES: [(&str, &str); 20] = [
    ("package::hash", HASH_WESL),
    ("package::math", MATH_WESL),
    ("package::geometry", GEOMETRY_WESL),
    ("package::field", FIELD_WESL),
    ("package::collapse", COLLAPSE_WESL),
    ("package::scan", SCAN_WESL),
    ("package::sort", SORT_WESL),
    ("package::batch", BATCH_WESL),
    ("package::ingest", INGEST_WESL),
    ("package::weld", WELD_WESL),
    ("package::bvh", BVH_WESL),
    ("package::measure", MEASURE_WESL),
    ("package::scatter", SCATTER_WESL),
    ("package::output", OUTPUT_WESL),
    ("package::patch", PATCH_WESL),
    ("package::simplify", SIMPLIFY_WESL),
    ("package::envelope", ENVELOPE_WESL),
    ("package::refine", REFINE_WESL),
    ("package::shell", SHELL_WESL),
    ("package::close", CLOSE_WESL),
];

/// Modules with entry points, compiled to WGSL by `build.rs`. Keep in step
/// with `ENTRY_MODULES` there.
#[cfg(test)]
pub(crate) const ENTRY_MODULES: [&str; 14] = [
    "scan", "sort", "ingest", "weld", "bvh", "measure", "scatter", "output", "patch", "simplify",
    "envelope", "refine", "shell", "close",
];

/// WGSL compiled at build time from [`ENTRY_MODULES`], as `(module name, source)`.
#[cfg(feature = "gpu")]
pub(crate) const COMPILED_WGSL: [(&str, &str); 14] = [
    ("scan", include_str!(concat!(env!("OUT_DIR"), "/scan.wgsl"))),
    ("sort", include_str!(concat!(env!("OUT_DIR"), "/sort.wgsl"))),
    (
        "ingest",
        include_str!(concat!(env!("OUT_DIR"), "/ingest.wgsl")),
    ),
    ("weld", include_str!(concat!(env!("OUT_DIR"), "/weld.wgsl"))),
    ("bvh", include_str!(concat!(env!("OUT_DIR"), "/bvh.wgsl"))),
    (
        "measure",
        include_str!(concat!(env!("OUT_DIR"), "/measure.wgsl")),
    ),
    (
        "scatter",
        include_str!(concat!(env!("OUT_DIR"), "/scatter.wgsl")),
    ),
    (
        "output",
        include_str!(concat!(env!("OUT_DIR"), "/output.wgsl")),
    ),
    (
        "patch",
        include_str!(concat!(env!("OUT_DIR"), "/patch.wgsl")),
    ),
    (
        "simplify",
        include_str!(concat!(env!("OUT_DIR"), "/simplify.wgsl")),
    ),
    (
        "envelope",
        include_str!(concat!(env!("OUT_DIR"), "/envelope.wgsl")),
    ),
    (
        "refine",
        include_str!(concat!(env!("OUT_DIR"), "/refine.wgsl")),
    ),
    (
        "shell",
        include_str!(concat!(env!("OUT_DIR"), "/shell.wgsl")),
    ),
    (
        "close",
        include_str!(concat!(env!("OUT_DIR"), "/close.wgsl")),
    ),
];

/// Entries per workgroup of the prefix sum (`SCAN_BLOCK` in `scan.wesl`).
#[cfg(any(test, feature = "gpu"))]
pub(crate) const SCAN_BLOCK: u32 = 256;

/// Elements per workgroup, and values of one digit, of the radix sort
/// (`SORT_BLOCK` and `SORT_DIGITS` in `sort.wesl`).
#[cfg(any(test, feature = "gpu"))]
pub(crate) const SORT_BLOCK: u32 = 256;
#[cfg(any(test, feature = "gpu"))]
pub(crate) const SORT_DIGITS: u32 = 16;

#[cfg(test)]
mod tests;
