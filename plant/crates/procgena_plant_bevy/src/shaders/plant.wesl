//! procgena_plant_bevy::shaders::plant — draws branched plants at near, mid and far detail
//!
//! Builds stems, branches and leaves for the main view, prepass and shadows.
//! Near detail shows every branch; mid detail keeps fewer branches; far detail
//! keeps a few of the largest parts as single triangles.
//!
//! Uses the bindings in `blade.wesl`. Vertex order matches
//! `shader_model::ShapeClass::indices`: main stem, branches, then leaves.

import package::shaders::common::{
    Field, Level, Instance, TAU, WIDTH_RANGE, blade_curve, instance_random, periodic, profile_width,
    stem_droop,
};
import package::shaders::blade::{BladeRef, BladeEye, plant_view, fields, frame, noise_texture, noise_sampler, wind};
import package::shaders::wind::{Wind, wind_gust};

/// Main-stem rows at near (0) or mid (1) detail. Twin of `shader_model::PLANT_ROWS`.
fn plant_rows(lod: u32) -> u32 {
    return select(4u, 8u, lod == 0u);
}

/// Leaf rows used for near-detail shadows.
///
/// Each leaf becomes one triangle to reduce the cost of drawing shadows.
/// Twin of `shader_model::SHADOW_LEAF_ROWS`.
const SHADOW_LEAF_ROWS: u32 = 1u;

/// Number of rows used to draw each first-level branch at near (0) or mid (1) detail.
///
/// Branches are leaves unless `twigs` is true, when they carry another level of
/// leaves. Near-detail shadows simplify leaves to `SHADOW_LEAF_ROWS`, matching
/// `shader_model::ShapeClass::shadow_indices`. Twin of `shader_model::CHILD_ROWS`.
fn child_rows(lod: u32, twigs: bool) -> u32 {
    var leaf = select(1u, 4u, lod == 0u);
@if(PLANT_SHADOW_PASS)
    leaf = select(leaf, SHADOW_LEAF_ROWS, lod == 0u);
    let twig = select(1u, 2u, lod == 0u);
    return select(leaf, twig, twigs);
}

/// Number of rows used to draw leaves growing from twigs at near (0) or mid (1)
/// detail.
///
/// Near-detail shadows use `SHADOW_LEAF_ROWS`. Twin of
/// `shader_model::GRANDCHILD_ROWS`.
fn grandchild_rows(lod: u32) -> u32 {
    var rows = select(1u, 3u, lod == 0u);
@if(PLANT_SHADOW_PASS)
    rows = select(rows, SHADOW_LEAF_ROWS, lod == 0u);
    return rows;
}

/// Maximum branches retained on each parent at mid detail.
///
/// Twin of `shader_model::MID_MAX_CHILDREN`.
const MID_MAX_CHILDREN: u32 = 8u;

/// Maximum width multiplier used to fill gaps between mid-detail branches.
const MID_MAX_COVER: f32 = 2.4;

/// Number of branches retained on each parent at mid detail.
///
/// Keeps roughly half, up to `MID_MAX_CHILDREN`, preserving complete pairs
/// when the original count is even. Twin of `shader_model::mid_children`.
fn mid_children(children: u32) -> u32 {
    let half = 2u * (children / 4u) + min(children % 4u, 2u);
    return min(half, MID_MAX_CHILDREN);
}

/// Selects an original branch for position `k` in the reduced mid-detail set.
///
/// Selections are spread along the parent. With `opposite`, branches growing
/// in pairs remain paired. Twin of `shader_model::mid_child`.
fn mid_child(k: u32, children: u32, opposite: bool) -> u32 {
    let kept = mid_children(children);
    if opposite {
        let points = max(children / 2u, 1u);
        let kept_points = max(kept / 2u, 1u);
        return 2u * ((2u * (k / 2u) + 1u) * points / (2u * kept_points)) + k % 2u;
    }
    return (2u * k + 1u) * children / (2u * kept);
}

/// Single-triangle parts a stem keeps at far detail. Twin of
/// `shader_model::FAR_PARTS`.
const FAR_PARTS: u32 = 6u;

/// Twigs whose leaves survive at far detail when leaves on twigs dominate the stem.
const FAR_TWIGS: u32 = 2u;

/// Leaves kept on each surviving twig at far detail.
const FAR_LEAVES_PER_TWIG: u32 = FAR_PARTS / FAR_TWIGS;

/// Maximum width multiplier for far parts standing in for the parts left out.
const FAR_MAX_COVER: f32 = 6.0;

/// Widens far parts to fill gaps left by culled parts. `ratio` is the parts the
/// level has divided by those kept.
///
/// The factor is above 1 because culling also removes the thin stalks and twigs,
/// which still cover pixels at a distance once widened to the minimum pixel width.
/// At 1.25 the 50–90 m band of `FOREST_FERNS` and `FOREST_FLOOR` covers as much
/// ground as mid detail carried out to 90 m (within 0.01 of 1).
fn far_cover(ratio: f32) -> f32 {
    return min(1.25 * ratio, FAR_MAX_COVER);
}

/// Means "no child": the parent keeps fewer far parts than were asked for.
const NO_CHILD: u32 = 0xffffffffu;

/// The level whose parts a stem keeps at far detail: 0 for the stalk, 1 for the
/// branches on it, 2 for leaves on those branches. Picks the level with the largest
/// total area. Twin of `shader_model::far_level`.
fn far_level(length: f32, width: f32, field: Field) -> u32 {
    let first = field.levels[0].size.x * length;
    let second = field.levels[1].size.x * first;
    let count = u32(field.levels[0].attach.x);
    let stalk = length * width;
    let branches = f32(count) * first * first * field.levels[0].size.y;
    let leaves = f32(count * u32(field.levels[1].attach.x)) * second * second * field.levels[1].size.y;
    var level = 0u;
    var best = stalk;
    if branches > best {
        level = 1u;
        best = branches;
    }
    if leaves > best {
        level = 2u;
    }
    return level;
}

/// Children a parent keeps at far detail when it may keep up to `kept`: never more
/// than mid detail keeps, in whole pairs when `opposite`. Twin of
/// `shader_model::far_kept`.
fn far_kept(children: u32, opposite: bool, kept: u32) -> u32 {
    let step = select(1u, 2u, opposite);
    let mid_points = mid_children(children) / step;
    return min(kept / step, mid_points) * step;
}

/// The original child shown as far part `k`, or `NO_CHILD`. Keeps the mid-detail
/// children nearest `widest`, where children are longest. Twin of
/// `shader_model::far_child`.
fn far_child(k: u32, children: u32, opposite: bool, widest: f32, kept: u32) -> u32 {
    let step = select(1u, 2u, opposite);
    let mid_points = mid_children(children) / step;
    let points = far_kept(children, opposite, kept) / step;
    let point = k / step;
    if point >= points {
        return NO_CHILD;
    }
    let centre = widest * f32(mid_points) - 0.5 - f32(points - 1u) * 0.5;
    let start = min(u32(max(floor(centre + 0.5), 0.0)), mid_points - points);
    return mid_child(step * (start + point) + k % step, children, opposite);
}

/// Widens retained branches to help fill gaps left by omitted branches.
///
/// `ratio` is the original count divided by the retained count. Width increases
/// with this ratio, up to `MID_MAX_COVER`.
fn mid_cover(ratio: f32) -> f32 {
    return min(0.85 * ratio, MID_MAX_COVER);
}

/// Child length relative to the longest child at `widest`. `u` runs from base (0) to
/// tip (1) of the parent's leafy part. Twin of `mirror::length_envelope`.
fn length_envelope(u: f32, widest: f32) -> f32 {
    let w = clamp(widest, 0.01, 0.99);
    let rise = u / w;
    let fall = (1.0 - u) / (1.0 - w);
    return pow(clamp(select(fall, rise, u < w), 0.0, 1.0), 0.6);
}

/// World-space segment curve and the dimensions and orientation of its visible strip.
struct Segment {
    p0: vec3<f32>,
    p1: vec3<f32>,
    p2: vec3<f32>,
    /// Length scale before bending, in metres.
    length: f32,
    /// Width scale before applying the outline profile, in metres.
    width: f32,
    /// Direction across the strip at its base.
    across: vec3<f32>,
    /// Total base-to-tip rotation of the strip, in radians.
    twist: f32,
    profile: f32,
    fold: f32,
}

fn bezier_point(s: Segment, t: f32) -> vec3<f32> {
    let u = 1.0 - t;
    return s.p0 * (u * u) + s.p1 * (2.0 * u * t) + s.p2 * (t * t);
}

/// Twin of `mirror::bezier_tangent`.
fn bezier_tangent(s: Segment, t: f32) -> vec3<f32> {
    let d = (s.p1 - s.p0) * (2.0 * (1.0 - t)) + (s.p2 - s.p1) * (2.0 * t);
    return normalize(d + vec3<f32>(0.0, 1e-5, 0.0));
}

/// Rotates `v` around the unit-length `axis` by `angle` radians. Twin of
/// `mirror::turn_about`.
fn turn_about(v: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    // A zero rotation leaves the vertex unchanged, so no trigonometry is needed.
    if angle == 0.0 {
        return v;
    }
    let c = cos(angle);
    let s = sin(angle);
    return v * c + cross(axis, v) * s + axis * dot(axis, v) * (1.0 - c);
}

/// Position and orientation along a segment at parameter `t`. `frame_at` is the twin
/// of `mirror::frame_at`.
struct SegmentFrame {
    point: vec3<f32>,
    tangent: vec3<f32>,
    /// Twisted width direction, perpendicular to the curve tangent.
    across: vec3<f32>,
    /// Surface-facing direction, computed as across × tangent.
    normal: vec3<f32>,
}

fn frame_at(s: Segment, t: f32) -> SegmentFrame {
    var f: SegmentFrame;
    f.point = bezier_point(s, t);
    f.tangent = bezier_tangent(s, t);
    let twisted = turn_about(s.across, f.tangent, s.twist * t);
    let flat = twisted - f.tangent * dot(twisted, f.tangent);
    f.across = select(vec3<f32>(1.0, 0.0, 0.0), normalize(flat), dot(flat, flat) > 1e-10);
    f.normal = normalize(cross(f.across, f.tangent));
    return f;
}

/// Position of a vertex within its strip: `t` runs from base (0) to tip (1), and side
/// is -0.5 or 0.5, meeting at 0 at the tip. Matches `blade.wesl`. With
/// `strip_vertex`, the twin of `mirror::strip_vertices`.
struct StripAt {
    t: f32,
    side: f32,
}

fn strip_at(local: u32, rows: u32) -> StripAt {
    let is_tip = local >= 2u * rows;
    let row = select(local / 2u, rows, is_tip);
    return StripAt(f32(row) / f32(rows), select(f32(local % 2u) - 0.5, 0.0, is_tip));
}

/// World-space vertex and shading coordinates for a branched stem.
struct PlantVertex {
    world_position: vec3<f32>,
    normal: vec3<f32>,
    /// Base-to-tip colour coordinate within this segment, from 0 to 1.
    t: f32,
    /// Normalised height above the plant root, clamped to 0–1 for ground tint and
    /// shading.
    h: f32,
    /// Colour-source level: 0 for the main stem, 1 or 2 for branches.
    level: u32,
}

/// Builds a strip vertex with the supplied local width and segment orientation.
fn strip_vertex(s: Segment, at: StripAt, width: f32) -> PlantVertex {
    let f = frame_at(s, at.t);
    // Folding narrows the strip and tilts edge normals inward. Unfolded strips tilt
    // normals outward to suggest a rounded surface.
    let cup = 0.577 * (1.0 - 2.0 * s.fold);
    var out: PlantVertex;
    out.world_position = f.point + f.across * at.side * width * (1.0 - 0.4 * s.fold);
    out.normal = normalize(f.normal + f.across * at.side * 2.0 * cup);
    out.t = at.t;
    return out;
}

/// Builds the instance's main segment for the current or previous view state. Without
/// wind, the twin of `mirror::resting_stem`.
fn own_segment(instance: Instance, field: Field, eye: BladeEye) -> Segment {
    let height = instance.height * field.shape.y;
    let droop = stem_droop(field.form.y, instance_random(instance, 0x7c2u), instance.height);
    let yaw = instance.facing * TAU;
    let face = vec2<f32>(sin(yaw), cos(yaw));
    let bend = select(instance.bend, instance.previous_bend, eye.previous);
    let curve = blade_curve(height, face * field.motion.x, bend, droop.x, droop.y);
    var s: Segment;
    s.p0 = instance.root + curve[0];
    s.p1 = instance.root + curve[1];
    s.p2 = instance.root + curve[2];
    s.length = height;
    s.width = instance.width * WIDTH_RANGE * field.shape.z;
    s.across = vec3<f32>(face.y, 0.0, -face.x);
    s.twist = field.form.x * TAU * (instance_random(instance, 0x7c1u) * 2.0 - 1.0);
    s.profile = field.look.z;
    s.fold = field.form.z;
    return s;
}

/// Attaches child `index` using this level's shape settings. `key` varies its wind
/// sway, while the parent's fold tilts its initial direction. Without sway, the twin of
/// `mirror::resting_child`.
fn child_segment(parent: Segment, level: Level, index: u32, count: u32, key: f32, eye: BladeEye) -> Segment {
    // Opposite children share attachment points; alternate children have one each.
    // Index parity selects the side in both arrangements.
    let opposite = level.attach.y < 0.5;
    let points = select(count, max(count / 2u, 1u), opposite);
    let at_point = select(index, index / 2u, opposite);
    let side = select(1.0, -1.0, (index % 2u) == 1u);
    let u = (f32(at_point) + 0.5) / f32(points);
    let start = level.attach.z;
    let f = frame_at(parent, start + (1.0 - start) * u);
    let length = level.size.x * parent.length * length_envelope(u, level.attach.w);

    // Spread rotates attachments around the parent; angle sweeps them towards its tip.
    // The parent fold also tilts their initial direction.
    let spread = level.root_color.w;
    let roll = f32(at_point) * spread * 3.14159265;
    let aside = turn_about(f.across * side, f.tangent, roll);
    let face = turn_about(f.normal, f.tangent, roll);
    let angle = level.size.z * 1.5707963;
    var out_dir = normalize(aside * cos(angle) + f.tangent * sin(angle));
    out_dir = normalize(out_dir - face * parent.fold * 0.6);

    // Add branch sway to inherited movement using the gust at the parent's base.
    // Trampling is not added again here. Stiffer branches sway less and faster.
    var current = wind;
    current.gust_offset = select(wind.gust_offset, wind.previous_gust_offset, eye.previous);
    let gust = wind_gust(current, noise_texture, noise_sampler, parent.p0.xz);
    let time = select(frame.time, frame.previous_time, eye.previous);
    let stiffness = level.size.w;
    let rate = periodic(mix(2.6, 5.2, stiffness) * max(wind.sway_speed, 0.0));
    let swing = sin(time * rate + key * TAU + f32(index) * 1.7);
    let sway = wind.strength * (0.3 + gust) * (1.0 - stiffness) * 0.35 * wind.sway * swing;

    // Drooping the second half of the curve suggests the branch's weight.
    let droop = level.form.y;
    let end_dir = normalize(out_dir + vec3<f32>(0.0, -1.2 * droop, 0.0) + f.normal * sway);

    var s: Segment;
    s.p0 = f.point;
    s.p1 = f.point + out_dir * (0.5 * length);
    s.p2 = s.p1 + end_dir * (0.5 * length);
    s.length = length;
    s.width = level.size.y * length;
    // Flat arrangements align leaf width along the parent. With spread enabled, a
    // horizontal width direction turns the leaf face towards the sky.
    let along = f.tangent - out_dir * dot(f.tangent, out_dir);
    s.across = select(face, normalize(along), dot(along, along) > 1e-6);
    let level_across = cross(out_dir, vec3<f32>(0.0, 1.0, 0.0));
    if spread > 0.0 && dot(level_across, level_across) > 1e-6 {
        s.across = normalize(level_across);
    }
    s.twist = level.form.z * TAU * (fract(key * 7.31 + f32(index) * 0.618) * 2.0 - 1.0);
    s.profile = level.form.x;
    s.fold = level.form.w;
    return s;
}

/// Leaf width multiplier, adjusted to preserve coverage when drawn as one triangle.
///
/// `t` runs from base (0) to tip (1). A one-row leaf uses a wider base to
/// approximate the area of the full outline, including oval leaves whose bases
/// would otherwise have zero width. This applies at mid detail and in near-detail
/// shadows.
fn leaf_width(t: f32, profile: f32, rows: u32) -> f32 {
    if rows == 1u {
        return 2.0 * mix(0.5833333, 0.7853982, profile);
    }
    return profile_width(t, profile);
}

/// Width multiplier needed to meet the view's minimum pixel width at this root.
fn widen_for(instance: Instance, eye: BladeEye, width: f32) -> f32 {
    // Shadow views use a zero pixel scale and keep the plant's original width.
    if eye.pixel_scale == 0.0 {
        return 1.0;
    }
    let metres_per_pixel = (eye.clip_from_world * vec4<f32>(instance.root, 1.0)).w * eye.pixel_scale;
    return max(1.0, plant_view.min_width_px * metres_per_pixel / max(width, 1e-6));
}

/// Builds vertex `local` of a stem at far detail: one of `FAR_PARTS` single triangles.
///
/// The triangles are the largest parts of the stem's dominant level (`far_level`),
/// widened for the parts left out as mid detail widens its leaves. A stalk that
/// dominates is one triangle. Parts a stem does not have collapse to a point at the
/// root, so they draw nothing.
fn far_vertex(instance: Instance, field: Field, own: Segment, local: u32, eye: BladeEye) -> PlantVertex {
    let part = local / 3u;
    let strip = strip_at(local % 3u, 1u);
    let level = far_level(own.length, own.width, field);
    let first = u32(field.levels[0].attach.x);
    let second = u32(field.levels[1].attach.x);
    let opposite_firsts = field.levels[0].attach.y < 0.5;
    let opposite_seconds = field.levels[1].attach.y < 0.5;

    var out: PlantVertex;
    out.world_position = instance.root;
    out.normal = vec3<f32>(0.0, 1.0, 0.0);
    out.level = level;
    if level == 0u {
        // The stalk is the only part.
        if part == 0u {
            let width = own.width * leaf_width(strip.t, own.profile, 1u);
            out = strip_vertex(own, strip, width * widen_for(instance, eye, own.width));
            out.level = 0u;
        }
    } else if level == 1u {
        // The largest branches on the stalk.
        let index = far_child(part, first, opposite_firsts, field.levels[0].attach.w, FAR_PARTS);
        if index != NO_CHILD {
            let kept = far_kept(first, opposite_firsts, FAR_PARTS);
            let cover = far_cover(f32(first) / f32(kept));
            let child = child_segment(own, field.levels[0], index, first, instance.seed, eye);
            let width = child.width * cover * leaf_width(strip.t, child.profile, 1u);
            out = strip_vertex(child, strip, width * widen_for(instance, eye, child.width));
            out.level = 1u;
        }
    } else {
        // The largest leaves on the largest twigs.
        let twig = far_child(part / FAR_LEAVES_PER_TWIG, first, opposite_firsts, field.levels[0].attach.w, FAR_TWIGS);
        let leaf = far_child(part % FAR_LEAVES_PER_TWIG, second, opposite_seconds, field.levels[1].attach.w, FAR_LEAVES_PER_TWIG);
        if twig != NO_CHILD && leaf != NO_CHILD {
            let kept = far_kept(first, opposite_firsts, FAR_TWIGS) * far_kept(second, opposite_seconds, FAR_LEAVES_PER_TWIG);
            let cover = far_cover(f32(first * second) / f32(kept));
            let parent = child_segment(own, field.levels[0], twig, first, instance.seed, eye);
            let child = child_segment(parent, field.levels[1], leaf, second, fract(instance.seed + 0.37), eye);
            let width = child.width * cover * leaf_width(strip.t, child.profile, 1u);
            out = strip_vertex(child, strip, width * widen_for(instance, eye, child.width));
            out.level = 2u;
        }
    }
    return out;
}

/// Builds one branched-stem vertex for the requested view state and detail level. At
/// rest and nearest detail, the twin of `mirror::resting_vertices`.
fn plant_vertex(at: BladeRef, eye: BladeEye) -> PlantVertex {
    let instance = at.instance;
    let field = fields[instance.field];
    let lod = at.lod;
    let own = own_segment(instance, field, eye);
    let first = u32(field.levels[0].attach.x);
    let second = u32(field.levels[1].attach.x);
    let firsts = select(select(0u, mid_children(first), lod == 1u), first, lod == 0u);
    let seconds = select(select(0u, mid_children(second), lod == 1u), second, lod == 0u);
    let opposite_firsts = field.levels[0].attach.y < 0.5;
    let opposite_seconds = field.levels[1].attach.y < 0.5;
    let key = instance.seed;

    var local = at.local;
    var out: PlantVertex;
    let own_rows = plant_rows(lod);
    let own_count = 2u * own_rows + 1u;
    if lod == 2u {
        out = far_vertex(instance, field, own, local, eye);
    } else if local < own_count {
        let strip = strip_at(local, own_rows);
        let width = own.width * profile_width(strip.t, own.profile) * widen_for(instance, eye, own.width);
        out = strip_vertex(own, strip, width);
        out.level = 0u;
    } else {
        local -= own_count;
        let rows = child_rows(lod, second > 0u);
        let per_child = 2u * rows + 1u;
        // Widen the retained branches to help preserve the plant's fullness.
        // Leaves on twigs account for missing twigs as well as missing leaves.
        let first_ratio = f32(first) / f32(max(firsts, 1u));
        let leaf_ratio = first_ratio * f32(second) / f32(max(seconds, 1u));
        let cover = select(1.0, mid_cover(first_ratio), lod == 1u);
        let leaf_cover = select(1.0, mid_cover(leaf_ratio), lod == 1u);
        if local < firsts * per_child {
            let drawn = local / per_child;
            let index = select(drawn, mid_child(drawn, first, opposite_firsts), lod == 1u);
            let child = child_segment(own, field.levels[0], index, first, key, eye);
            let strip = strip_at(local % per_child, rows);
            // Keep twigs tapered; only leaves without further branches need the
            // width adjustment for a single triangle.
            var shape = profile_width(strip.t, child.profile);
            if second == 0u {
                shape = leaf_width(strip.t, child.profile, rows);
            }
            let width = child.width * cover * shape;
            out = strip_vertex(child, strip, width * widen_for(instance, eye, child.width));
            out.level = 1u;
        } else {
            local -= firsts * per_child;
            let leaf_rows = grandchild_rows(lod);
            let per_leaf = 2u * leaf_rows + 1u;
            let leaf = local / per_leaf;
            let drawn = leaf / max(seconds, 1u);
            let parent_index = select(drawn, mid_child(drawn, first, opposite_firsts), lod == 1u);
            let parent = child_segment(own, field.levels[0], parent_index, first, key, eye);
            let kept_leaf = leaf % max(seconds, 1u);
            let leaf_index = select(kept_leaf, mid_child(kept_leaf, second, opposite_seconds), lod == 1u);
            let child = child_segment(parent, field.levels[1], leaf_index, second, fract(key + 0.37), eye);
            let strip = strip_at(local % per_leaf, leaf_rows);
            let width = child.width * leaf_cover * leaf_width(strip.t, child.profile, leaf_rows);
            out = strip_vertex(child, strip, width * widen_for(instance, eye, child.width));
            out.level = 2u;
        }
    }
    out.h = clamp((out.world_position.y - instance.root.y) / max(own.length, 1e-3), 0.0, 1.0);
    return out;
}
