//! Shader source for generating and placing rocks on the GPU.
//!
//! WESL is WGSL, the WebGPU shading language, extended with imports between
//! modules. These shaders provide the GPU equivalent of CPU rock generation
//! and placement. GPU rounding can change vertex counts, ordering, and values;
//! nearly equally stable resting sides can also settle differently.
//!
//! The modules import each other as `package::<name>`, so register each one
//! under the path listed in [`SHADER_MODULES`]. The library modules
//! ([`HASH_WESL`], [`NOISE_WESL`], [`FIELD_WESL`], [`TABLES_WESL`]) declare no
//! bindings and can be imported into your own shaders. The generator modules
//! ([`GENERATE_WESL`], [`MC_WESL`], [`DC_WESL`], [`SCAN_WESL`], [`PLACE_WESL`])
//! declare bindings and compute entry points for the generator's own pipelines.

/// Repeatable pseudo-random integers and values in `[0, 1)`. Import as `package::hash`.
pub const HASH_WESL: &str = include_str!("../shaders/hash.wesl");

/// Smooth random patterns and triplanar roughness. Import as `package::noise`.
pub const NOISE_WESL: &str = include_str!("../shaders/noise.wesl");

/// Rock shape building blocks: carving, roughness, and cube clipping, in the
/// same stone-space and carve-space units as [`crate::StoneField`].
/// Import as `package::field`.
pub const FIELD_WESL: &str = include_str!("../shaders/field.wesl");

/// Cell corner and edge tables shared by both meshers. Import as `package::tables`.
pub const TABLES_WESL: &str = include_str!("../shaders/tables.wesl");

/// Generator bindings, grid helpers, and the field sampling pass.
/// Import as `package::generate`.
pub const GENERATE_WESL: &str = include_str!("../shaders/generate.wesl");

/// Marching cubes generator passes. Import as `package::mc`.
pub const MC_WESL: &str = include_str!("../shaders/mc.wesl");

/// Dual contouring generator passes. Import as `package::dc`.
pub const DC_WESL: &str = include_str!("../shaders/dc.wesl");

/// Exclusive prefix sum passes used between counting and writing mesh data.
/// Import as `package::scan`.
pub const SCAN_WESL: &str = include_str!("../shaders/scan.wesl");

/// GPU ground placement and suggested burial depth, equivalent to
/// [`crate::StonePlacement`]. Import as `package::place`; follow the pass order
/// documented in the source before generating placed meshes.
pub const PLACE_WESL: &str = include_str!("../shaders/place.wesl");

/// Every shader module as `(module path, source)`, for registering with a WESL resolver.
pub const SHADER_MODULES: [(&str, &str); 9] = [
    ("package::hash", HASH_WESL),
    ("package::noise", NOISE_WESL),
    ("package::field", FIELD_WESL),
    ("package::tables", TABLES_WESL),
    ("package::generate", GENERATE_WESL),
    ("package::mc", MC_WESL),
    ("package::dc", DC_WESL),
    ("package::scan", SCAN_WESL),
    ("package::place", PLACE_WESL),
];

/// WGSL compiled at build time from the modules with entry points, as
/// `(module name, source)`.
#[cfg(feature = "gpu")]
pub(crate) const COMPILED_WGSL: [(&str, &str); 5] = [
    (
        "generate",
        include_str!(concat!(env!("OUT_DIR"), "/generate.wgsl")),
    ),
    ("mc", include_str!(concat!(env!("OUT_DIR"), "/mc.wgsl"))),
    ("dc", include_str!(concat!(env!("OUT_DIR"), "/dc.wgsl"))),
    ("scan", include_str!(concat!(env!("OUT_DIR"), "/scan.wgsl"))),
    (
        "place",
        include_str!(concat!(env!("OUT_DIR"), "/place.wgsl")),
    ),
];

#[cfg(any(test, feature = "gpu"))]
pub(crate) use layout::*;

#[cfg(any(test, feature = "gpu"))]
mod layout {
    use crate::field::StoneField;
    use crate::mesh::tables::TRI_TABLE;
    use crate::options::{MAX_CARVE_STEPS, PlacementOptions};
    use crate::placement::{DIRECTION_COUNT, REFINE_CANDIDATES, profile_bins};

    /// Byte size of the `Request` uniform in `generate.wesl`.
    pub(crate) const REQUEST_SIZE: usize = 96 + 16 * MAX_CARVE_STEPS as usize;

    /// Workgroup width of the per-item passes (`WORKGROUP_SIZE` in `generate.wesl`).
    pub(crate) const WORKGROUP_SIZE: u32 = 64;

    /// Entries per workgroup of the prefix sum (`SCAN_BLOCK` in `scan.wesl`).
    pub(crate) const SCAN_BLOCK: u32 = 256;

    /// Byte sizes of one `Vertex`, `DrawArgs`, and `Status` in `generate.wesl`.
    pub(crate) const VERTEX_SIZE: u64 = 24;
    pub(crate) const DRAW_ARGS_SIZE: u64 = 20;
    pub(crate) const STATUS_SIZE: u64 = 80;

    /// Number of 32-bit words reserved for shared placement analysis values.
    pub(crate) const PLACE_STATE_WORDS: u32 = 16;

    /// Maximum number of horizontal slices supported by the placement shader.
    pub(crate) const MAX_PROFILE_BINS: u32 = 128;

    /// Returns the required `place_scratch` length in 32-bit words for an
    /// analysis grid with `resolution` cells per axis.
    pub(crate) fn place_scratch_words(resolution: u32) -> u64 {
        let slices = resolution as u64 + 1;
        4 * slices
            + PLACE_STATE_WORDS as u64
            + DIRECTION_COUNT as u64
            + 4 * REFINE_CANDIDATES as u64
            + slices * profile_bins(resolution) as u64
    }

    /// `Status.overflow` bits in `generate.wesl`.
    pub(crate) const OVERFLOW_VERTICES: u32 = 1;
    pub(crate) const OVERFLOW_INDICES: u32 = 2;
    pub(crate) const OVERFLOW_CROSSINGS: u32 = 4;

    /// Where one request's results go in the shared output buffers.
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) struct RequestSlots {
        /// Entry in the draw-argument and status buffers.
        pub(crate) request_index: u32,
        pub(crate) vertex_base: u32,
        pub(crate) vertex_capacity: u32,
        pub(crate) index_base: u32,
        pub(crate) index_capacity: u32,
        /// Surface-point slots available to dual contouring or placement analysis.
        pub(crate) crossing_capacity: u32,
        /// Status entry containing the completed placement to apply to this mesh.
        pub(crate) placement_index: u32,
    }

    /// Encodes a shader request for `field`, a grid resolution, and output slots.
    /// Supply validated settings and valid buffer slots. Only analysis passes
    /// use `placement`; mesh passes read placement from `slots.placement_index`.
    pub(crate) fn pack_request(
        field: &StoneField,
        resolution: u32,
        slots: &RequestSlots,
        placement: &PlacementOptions,
    ) -> Vec<u8> {
        let carve = field.carve();
        let roughness = field.roughness();
        let scale = field.carve_scale();
        let mut words: Vec<u32> = vec![
            scale.x.to_bits(),
            scale.y.to_bits(),
            scale.z.to_bits(),
            carve.base_radius().to_bits(),
            carve.smoothness().to_bits(),
            field.displacement().to_bits(),
            roughness.spread.to_bits(),
            roughness.seed,
            roughness.octaves,
            carve.spheres().len() as u32,
            resolution,
            slots.request_index,
            slots.vertex_base,
            slots.vertex_capacity,
            slots.index_base,
            slots.index_capacity,
            slots.crossing_capacity,
            slots.placement_index,
            placement.settle as u32,
            placement.max_embed_fraction.to_bits(),
            placement.min_embed_fraction.to_bits(),
        ];
        // Pad the header to 24 words so the sphere array starts on a 16-byte boundary.
        words.resize(24, 0);
        for sphere in carve.spheres() {
            let c = sphere.center;
            words.extend([c.x, c.y, c.z, sphere.radius].map(f32::to_bits));
        }
        words.resize(REQUEST_SIZE / 4, 0);
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// Packs the marching cubes triangle table for the `tri_table` uniform in `mc.wesl`.
    pub(crate) fn packed_tri_table() -> [[u32; 4]; 256] {
        TRI_TABLE.map(|row| {
            let edges: Vec<u32> = row
                .iter()
                .take_while(|&&e| e >= 0)
                .map(|&e| e as u32)
                .collect();
            let mut entry = [0, 0, edges.len() as u32, 0];
            for (k, &edge) in edges.iter().enumerate() {
                entry[k / 8] |= edge << (4 * (k % 8));
            }
            entry
        })
    }
}

#[cfg(test)]
mod tests;
