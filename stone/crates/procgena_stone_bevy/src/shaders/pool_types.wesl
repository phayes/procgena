// Data shared by the pooled-rock culling and drawing shaders.

/// One pooled rock. Mirrors `GpuInstance` in `src/pool/render/pools.rs`.
struct PoolInstance {
    /// Rows of the world-from-local affine transform: `world = rows · (local, 1)`.
    /// Local coordinates are the rock's mesh units (the generated unit cube
    /// times `PoolMaterial::scale`).
    rows: array<vec4<f32>, 3>,
    /// Which of the pool's shapes this rock uses.
    variant: u32,
}

/// The world-space centre of a rock's mesh.
fn instance_center(instance: PoolInstance) -> vec3<f32> {
    return vec3<f32>(instance.rows[0].w, instance.rows[1].w, instance.rows[2].w);
}

/// The linear part of a rock's transform, as columns.
fn instance_linear(instance: PoolInstance) -> mat3x3<f32> {
    return transpose(mat3x3<f32>(instance.rows[0].xyz, instance.rows[1].xyz, instance.rows[2].xyz));
}
