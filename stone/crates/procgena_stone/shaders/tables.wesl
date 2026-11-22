// Cell corner and edge tables for marching cubes and dual contouring, matching
// the Rust crate's `mesh::tables`. The 256-case triangle table is not here: the
// generator supplies it in a uniform buffer.

/// Offsets of the eight cell corners from the minimum corner. Corner `i` sets
/// bit `i` of a cell's case index when it is outside the rock.
const MC_CORNERS: array<vec3<u32>, 8> = array<vec3<u32>, 8>(
    vec3<u32>(0u, 0u, 1u),
    vec3<u32>(1u, 0u, 1u),
    vec3<u32>(1u, 0u, 0u),
    vec3<u32>(0u, 0u, 0u),
    vec3<u32>(0u, 1u, 1u),
    vec3<u32>(1u, 1u, 1u),
    vec3<u32>(1u, 1u, 0u),
    vec3<u32>(0u, 1u, 0u),
);

/// Offset of each of the twelve cell edges' lower endpoint from the cell's
/// minimum corner, in the order the triangle table refers to them.
const MC_EDGE_OFFSETS: array<vec3<u32>, 12> = array<vec3<u32>, 12>(
    vec3<u32>(0u, 0u, 1u),
    vec3<u32>(1u, 0u, 0u),
    vec3<u32>(0u, 0u, 0u),
    vec3<u32>(0u, 0u, 0u),
    vec3<u32>(0u, 1u, 1u),
    vec3<u32>(1u, 1u, 0u),
    vec3<u32>(0u, 1u, 0u),
    vec3<u32>(0u, 1u, 0u),
    vec3<u32>(0u, 0u, 1u),
    vec3<u32>(1u, 0u, 1u),
    vec3<u32>(1u, 0u, 0u),
    vec3<u32>(0u, 0u, 0u),
);

/// Axis of each cell edge, matching `MC_EDGE_OFFSETS`: 0 for x, 1 for y, 2 for z.
const MC_EDGE_AXES: array<u32, 12> = array<u32, 12>(0u, 2u, 0u, 2u, 0u, 2u, 0u, 2u, 1u, 1u, 1u, 1u);
