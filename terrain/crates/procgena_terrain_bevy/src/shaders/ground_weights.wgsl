// CPU GroundStack's ordered alpha-over and largest-remainder quantisation.
// Inputs retain analytic CPU slopes and feature tags. No drawing result
// feeds terrain queries or placement.
@group(0) @binding(0) var<storage, read> data: array<vec4<f32>>;
@group(0) @binding(1) var low: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var high: texture_storage_2d<rgba8unorm, write>;

fn edge(a: f32, b: f32, x: f32) -> f32 {
    if a == b { return select(0.0, 1.0, x >= a); }
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
// Each site is three words: [height, slope, relief height, height above
// water], [known bits, road tag bits, landform bit, flag bits] and [water
// distance, road distance, curvature, unused]. Known bits: 1 relief height,
// 2 height above water, 4 road tags, 8 water distance, 16 road distance,
// 32 curvature, 64 labels. A condition on an unknown measure counts as 1.
const SITE_WORDS: u32 = 3u;
fn known(bits: u32, bit: u32) -> bool {
    return (bits & bit) != 0u;
}
fn weights(at: vec2<u32>) -> array<f32, 8> {
    let width = u32(data[0].x);
    let index = 2u + SITE_WORDS * (at.y * width + at.x);
    let site = data[index];
    let labels = data[index + 1u];
    let measures = data[index + 2u];
    let bits = u32(labels.x);
    var cursor = 2u + SITE_WORDS * width * u32(data[0].y);
    var mix: array<f32, 8>;
    for (var layer = 0u; layer < u32(data[0].z); layer++) {
        let info = data[cursor];
        cursor++;
        var w = info.y;
        for (var c = 0u; c < u32(info.z); c++) {
            let rule = data[cursor];
            let band = data[cursor + 1u];
            cursor += 2u;
            let kind = u32(rule.x);
            var x = site.y;
            if kind == 1u { x = site.x / data[0].w; }
            if kind == 2u {
                if !known(bits, 1u) { continue; }
                x = site.z;
            }
            if kind == 3u {
                if !known(bits, 2u) { continue; }
                x = site.w;
            }
            if kind == 4u {
                if known(bits, 4u) { w *= select(0.0, 1.0, (u32(labels.y) & u32(rule.w)) != 0u); }
                continue;
            }
            if kind == 5u {
                if !known(bits, 8u) { continue; }
                x = measures.x;
            }
            if kind == 6u {
                if !known(bits, 16u) { continue; }
                x = measures.y;
            }
            if kind == 7u {
                if !known(bits, 32u) { continue; }
                x = measures.z;
            }
            if kind == 8u {
                if known(bits, 64u) { w *= select(0.0, 1.0, (u32(labels.z) & u32(rule.w)) != 0u); }
                continue;
            }
            if kind == 9u {
                // Any of rule.w (or none asked for), and none of band.x.
                if known(bits, 64u) {
                    let flags = u32(labels.w);
                    let any = u32(rule.w);
                    let passes = (any == 0u || (flags & any) != 0u) && (flags & u32(band.x)) == 0u;
                    w *= select(0.0, 1.0, passes);
                }
                continue;
            }
            var factor = 1.0;
            if rule.y != 0.0 { factor *= edge(band.x, band.y, x); }
            if rule.z != 0.0 { factor *= 1.0 - edge(band.z, band.w, x); }
            w *= factor;
        }
        for (var slot = 0u; slot < 8u; slot++) { mix[slot] *= 1.0 - w; }
        mix[u32(info.x)] += w;
    }
    var total = 0.0;
    for (var slot = 0u; slot < 8u; slot++) { total += mix[slot]; }
    var out: array<f32, 8>;
    var remainder: array<f32, 8>;
    var sum = 0u;
    let count = u32(data[1].w);
    for (var slot = 0u; slot < count; slot++) {
        let scaled = mix[slot] / total * 255.0;
        out[slot] = floor(scaled);
        remainder[slot] = scaled - out[slot];
        sum += u32(out[slot]);
    }
    // Stable ties to lower material id, as in the CPU reference.
    for (var left = sum; left < 255u; left++) {
        var best = 0u;
        for (var slot = 1u; slot < count; slot++) {
            if remainder[slot] > remainder[best] { best = slot; }
        }
        out[best] += 1.0;
        remainder[best] = -1.0;
    }
    return out;
}
@compute @workgroup_size(8, 8, 1)
fn paint(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(data[0].xy);
    if any(id.xy >= size) { return; }
    var result: array<f32, 8>;
    if data[1].z == 1.0 {
        result = weights(id.xy);
    } else {
        // Stand-ins filter the parent's quantised weights, exactly as the
        // CPU image subdivision does; they do not invent new ground sites.
        let p = (data[1].xy + data[1].z * vec2<f32>(id.xy) / vec2<f32>(size - 1u)) * vec2<f32>(size - 1u);
        let a = vec2<u32>(floor(p));
        let b = min(a + 1u, size - 1u);
        let f = fract(p);
        let aa = weights(a);
        let ba = weights(vec2<u32>(b.x, a.y));
        let ab = weights(vec2<u32>(a.x, b.y));
        let bb = weights(b);
        for (var i = 0u; i < 8u; i++) {
            let top = aa[i] * (1.0 - f.x) + ba[i] * f.x;
            let bottom = ab[i] * (1.0 - f.x) + bb[i] * f.x;
            result[i] = floor(top * (1.0 - f.y) + bottom * f.y + 0.5);
        }
    }
    textureStore(low, id.xy, vec4<f32>(result[0], result[1], result[2], result[3]) / 255.0);
    textureStore(high, id.xy, vec4<f32>(result[4], result[5], result[6], result[7]) / 255.0);
}
