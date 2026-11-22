// Draws pooled rocks into shadow maps: depth only. Group 0 is the light's view
// (declared in `pool_bridge` under `POOL_SHADOW_PASS`), group 1 is empty and
// group 2 holds the pool.

import package::shaders::pool_bridge::view_clip_from_world;
import package::shaders::pool_draw::pool_vertex;

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) visible: vec2<u32>,
) -> @builtin(position) vec4<f32> {
    let v = pool_vertex(position, normal, visible);
    return view_clip_from_world() * vec4<f32>(v.world_position, 1.0);
}
