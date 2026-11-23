// Cross-fading between levels of detail by dithering, for every render pass.
//
// Import as `procgena_core_bevy::shaders::dither` once `LodPlugin` is added.
// Nothing here reads Bevy's bindings or shader defs, so the main, prepass,
// shadow and compute shaders of any renderer can use it.
//
// A level's range is packed in a `vec4<f32>` as `LodRange::to_vec4` packs it:
// (fade-in start, fade-in end, fade-out start, fade-out end), in metres.
// Dither levels run from -16 to 16 as in Bevy's visibility ranges: 0 is fully
// drawn, -16 to 0 fading in, 0 to 16 fading out, and -16 or 16 not drawn.

/// How far `distance` is through the band `start..end`, from 0 to 16.
/// A band of zero width counts as passed.
fn band_level(start: f32, end: f32, distance: f32) -> i32 {
    if end <= start {
        return 16;
    }
    return clamp(i32(round((distance - start) / (end - start) * 16.0)), 0, 16);
}

/// The dither level of a copy `distance` metres from the viewer, at the
/// level of detail whose packed range is `range`.
fn dither_level(range: vec4<f32>, distance: f32) -> i32 {
    if distance < range.x {
        return -16;
    }
    if distance >= range.w {
        return 16;
    }
    if distance < range.y {
        return -16 + band_level(range.x, range.y, distance);
    }
    if distance < range.z {
        return 0;
    }
    return band_level(range.z, range.w, distance);
}

/// Whether any part of a copy with dither level `dither` is drawn.
fn is_drawn(dither: i32) -> bool {
    return dither > -16 && dither < 16;
}

/// The distance at which the level with packed range `range` takes over from
/// the one before: the middle of its fade-in band. Passes that draw a single
/// level, such as shadow maps, switch here.
fn switch_distance(range: vec4<f32>) -> f32 {
    return 0.5 * (range.x + range.y);
}

/// Bevy's 4×4 ordered-dither thresholds, one byte each.
const DITHER_THRESHOLD_MAP: vec4<u32> = vec4<u32>(
    0x0a020800u,
    0x060e040cu,
    0x09010b03u,
    0x050d070fu,
);

/// Discards the fragments a copy with dither level `dither` hides at the
/// pixel `frag_coord`, exactly as Bevy's visibility-range dithering does, so
/// two levels fading over the same band cover each pixel once whichever of
/// them Bevy draws. Call it from fragment shaders only.
fn dither_discard(frag_coord: vec4<f32>, dither: i32) {
    if dither == 0 {
        return;
    }
    if dither <= -16 || dither >= 16 {
        discard;
    }
    let coords = vec2<u32>(floor(frag_coord.xy)) % 4u;
    let threshold = i32((DITHER_THRESHOLD_MAP[coords.y] >> (coords.x * 8u)) & 0xffu);
    if (dither >= 0 && dither + threshold >= 16) || (dither < 0 && 1 + dither + threshold <= 0) {
        discard;
    }
}
