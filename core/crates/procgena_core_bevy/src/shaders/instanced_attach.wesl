// Chooses which attached copies (rows of an `InstancedAttached`) one view
// draws, and at which detail level, for the shared instanced renderer.
//
// Runs after `cull` in `instanced_cull.wesl`, with the same counts, and
// before its `write_args`. `cull_hosts` looks at each host copy once and
// writes its state; `cull_rows` then looks at each row of each host copy.
// Rows are listed per host copy from thread `hosts[h].first_thread`. An
// attached slot's list at `level` starts at entry
// `level * attached_count + slots[slot].start` of `visible`, four words of
// four per entry (see `emit`).

import procgena_core_bevy::shaders::dither::{dither_level, is_drawn, switch_distance};

/// Mirrors `CullUniform` in `src/instanced/render.rs` (see `CullView` in
/// `instanced_cull.wesl`).
struct CullView {
    planes: array<vec4<f32>, 6>,
    lod_origin: vec4<f32>,
    band_scale: f32,
    copy_count: u32,
    list_count: u32,
    part_count: u32,
    levels: u32,
    shadow: u32,
    skip_near: u32,
    host_count: u32,
    row_threads: u32,
    attached_count: u32,
}

/// Mirrors `Slot` in `instanced_cull.wesl`. For a slot rows draw, `start`
/// and `count` are in the attached copies' entries.
struct Slot {
    start: u32,
    count: u32,
    level_count: u32,
    shadow_mask: u32,
    first_range: u32,
}

/// One host copy: a copy whose slot has rows. Mirrors `GpuHost`.
struct Host {
    /// The copy's world transform, by row: `xyz` from its 3×3 part, `w`
    /// from its translation.
    world: array<vec4<f32>, 3>,
    /// World-space sphere around every row's bounds.
    sphere: vec4<f32>,
    /// World-space origin in `xyz`; the copy's size over its slot's
    /// reference size in `w`, for the copy's own level.
    origin: vec4<f32>,
    /// `x`: how far row origins lie from `origin`, in world units. `y`: the
    /// copy's own scale, which multiplies each row's size factor. `z` and
    /// `w`: the smallest and largest row size factors.
    rows: vec4<f32>,
    /// The copy's slot.
    slot: u32,
    /// The copy's mesh record.
    record: u32,
    first_row: u32,
    row_count: u32,
    first_thread: u32,
    /// A slot whose levels every row's slot shares, or `NO_SLOT`.
    whole_slot: u32,
    /// The farthest a row is drawn, over the rows, in reference metres
    /// times the row's size factor.
    reach: f32,
    /// The largest factor the copy's world transform stretches by.
    world_scale: f32,
}

/// One row of a host slot's table. Mirrors `GpuRow`.
struct Row {
    /// Position in the host's coordinates in `xyz`, scale in `w`.
    place: vec4<f32>,
    /// Rotation in the host's coordinates, as a quaternion.
    turn: vec4<f32>,
    /// The row's wind numbers in `xyz`; its size over its slot's reference
    /// size in `w`.
    wind: vec4<f32>,
    /// The row's bounding sphere in the host's coordinates.
    sphere: vec4<f32>,
    slot: u32,
    /// Bit `l` set when the row is drawn while its host is at level `l`.
    host_levels: u32,
}

@group(0) @binding(0) var<uniform> view: CullView;
@group(0) @binding(1) var<storage, read> slots: array<Slot>;
@group(0) @binding(2) var<storage, read> ranges: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> hosts: array<Host>;
@group(0) @binding(4) var<storage, read> rows: array<Row>;
/// Per host copy: what `cull_hosts` decided (see `host_state_of`).
@group(0) @binding(5) var<storage, read_write> host_state: array<u32>;
@group(0) @binding(6) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(7) var<storage, read_write> visible: array<vec4<u32>>;

const NO_SLOT: u32 = 0xffffffffu;
/// Host modes, in the low 16 bits of a host state. Modes from `WHOLE` up
/// draw every row at level `mode - WHOLE`. The host's own level is in the
/// high 16 bits.
const SKIP: u32 = 0u;
const EACH_ROW: u32 = 1u;
const WHOLE: u32 = 2u;

/// Whether a sphere is at least partly inside the view's frustum, as
/// `in_frustum` in `instanced_cull.wesl` works it out.
fn in_frustum(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        if i == 4u && view.skip_near == 1u {
            continue;
        }
        let plane = view.planes[i];
        if dot(plane.xyz, center) + plane.w < -radius {
            return false;
        }
    }
    return true;
}

/// A point in a host's coordinates, in world space.
fn world_point(host: Host, point: vec3<f32>) -> vec3<f32> {
    let p = vec4<f32>(point, 1.0);
    return vec3<f32>(dot(host.world[0], p), dot(host.world[1], p), dot(host.world[2], p));
}

/// The level a camera draws every row at when the rows' distances, in
/// reference metres, all lie from `near` to `far`, in `slot`'s levels:
/// `WHOLE + level` if they all lie inside one level and outside its fade
/// bands, or `EACH_ROW` if rows must be tested one by one.
fn whole_level(slot: Slot, near: f32, far: f32) -> u32 {
    var mode = EACH_ROW;
    for (var level = 0u; level < slot.level_count; level++) {
        let range = ranges[slot.first_range + level];
        if near >= range.y && far < range.z {
            mode = WHOLE + level;
        } else if !(far < range.x || near >= range.w) {
            return EACH_ROW;
        }
    }
    return mode;
}

/// A host copy's state for this view: its own level, and whether its rows
/// are skipped, drawn whole at one level, or tested one by one.
fn host_state_of(host: Host) -> u32 {
    if !in_frustum(host.sphere.xyz, host.sphere.w) {
        return SKIP;
    }
    let distance = length(host.origin.xyz - view.lod_origin.xyz);

    // The host's own level, switching in the middle of each fade band.
    let slot = slots[host.slot];
    let host_distance = distance / (host.origin.w * view.band_scale);
    var host_level = 0u;
    for (var l = 1u; l < slot.level_count; l++) {
        if host_distance >= switch_distance(ranges[slot.first_range + l]) {
            host_level = l;
        }
    }

    // The range of the rows' distances, in reference metres.
    let row_scale = host.rows.y * view.band_scale;
    let near = max(distance - host.rows.x, 0.0);
    let far = distance + host.rows.x;
    if near >= host.reach * row_scale {
        return SKIP;
    }
    var mode = EACH_ROW;
    if view.shadow == 0u && host.whole_slot != NO_SLOT {
        mode = whole_level(
            slots[host.whole_slot],
            near / (host.rows.w * row_scale),
            far / (host.rows.z * row_scale),
        );
    }
    return mode | (host_level << 16u);
}

/// Writes each host copy's state.
@compute @workgroup_size(64)
fn cull_hosts(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= view.host_count {
        return;
    }
    host_state[id.x] = host_state_of(hosts[id.x]);
}

/// Adds a host's row to its slot's list at `level`.
fn emit(host: Host, row: Row, level: u32, dither: i32) {
    let slot = slots[row.slot];
    let at = atomicAdd(&counts[row.slot * view.levels + level], 1u);
    let entry = (level * view.attached_count + slot.start + at) * 4u;
    visible[entry] = vec4<u32>(host.record, bitcast<u32>(dither), 0u, 0u);
    visible[entry + 1u] = bitcast<vec4<u32>>(row.place);
    visible[entry + 2u] = bitcast<vec4<u32>>(row.turn);
    visible[entry + 3u] = bitcast<vec4<u32>>(vec4<f32>(row.wind.xyz, 0.0));
}

/// The index of the host whose rows include thread `thread`.
fn host_of(thread: u32) -> u32 {
    var low = 0u;
    var high = view.host_count;
    while high - low > 1u {
        let middle = (low + high) / 2u;
        if hosts[middle].first_thread <= thread {
            low = middle;
        } else {
            high = middle;
        }
    }
    return low;
}

/// Tests one row of one host copy, as the host's state says, and adds it to
/// the lists of the levels it is drawn at. In a fade band a camera draws
/// both levels, dithered; a shadow view draws the level whose switch
/// distance was passed last, if that level casts shadows.
@compute @workgroup_size(64)
fn cull_rows(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let thread = id.x + id.y * groups.x * 64u;
    if thread >= view.row_threads {
        return;
    }
    let index = host_of(thread);
    let state = host_state[index];
    let mode = state & 0xffffu;
    if mode == SKIP {
        return;
    }
    let host = hosts[index];
    let row = rows[host.first_row + thread - host.first_thread];
    if ((row.host_levels >> (state >> 16u)) & 1u) == 0u {
        return;
    }
    let slot = slots[row.slot];
    if slot.level_count == 0u {
        return;
    }

    // Every row at one level, without a test of its own.
    if mode >= WHOLE {
        let level = mode - WHOLE;
        if level < slot.level_count {
            emit(host, row, level, 0);
        }
        return;
    }

    // The row on its own.
    if !in_frustum(world_point(host, row.sphere.xyz), row.sphere.w * host.world_scale) {
        return;
    }
    let distance = length(world_point(host, row.place.xyz) - view.lod_origin.xyz)
        / (row.wind.w * host.rows.y * view.band_scale);
    if view.shadow == 1u {
        let last = ranges[slot.first_range + slot.level_count - 1u];
        if distance >= 0.5 * last.z + 0.5 * last.w {
            return;
        }
        var level = 0u;
        for (var l = 1u; l < slot.level_count; l++) {
            if distance >= switch_distance(ranges[slot.first_range + l]) {
                level = l;
            }
        }
        if (slot.shadow_mask & (1u << level)) != 0u {
            emit(host, row, level, 0);
        }
        return;
    }
    for (var level = 0u; level < slot.level_count; level++) {
        let dither = dither_level(ranges[slot.first_range + level], distance);
        if is_drawn(dither) {
            emit(host, row, level, dither);
        }
    }
}
