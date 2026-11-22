// Estimates ground placement and suggested burial depth on the GPU, matching
// the CPU's StonePlacement analysis. Burial is suggested, not applied.
//
// Run once per shape and placement settings. Set `request_index` to the output
// status entry and `resolution` to the analysis grid size. Run these passes in order:
// `sample_field`; `count_dc`, the prefix sum and `emit_dc_crossings`; then
// `place_com_slices`, `place_com`, `place_heights`, `place_descend`,
// `REFINE_ROUNDS` times `place_refine_heights` then `place_refine_pick`,
// `place_finish`, `place_profile_slices` and `place_embed`. Mesh requests then
// use that status entry as their `placement_index` to apply the result.

import package::generate::{
    inside, item_index, sample_count, sample_position, samples_per_axis, vertex_position,
    WORKGROUP_SIZE, prefix, request, status,
};
import package::dc::crossings;

/// Number of candidate resting directions distributed around the rock.
const DIRECTION_COUNT: u32 = 512u;
/// Maximum number of moves towards a lower resting position.
const MAX_DESCENT_STEPS: u32 = 64u;
/// Minimum dot product for directions close enough to count as neighbours.
const NEIGHBOUR_COS: f32 = 0.95;
/// Number of nearby tilts tested per search, including the current direction.
const REFINE_CANDIDATES: u32 = 48u;
/// Initial range of nearby tilts, approximately in radians.
const REFINE_SPREAD: f32 = 0.157;
/// Angular spacing, in turns, used to distribute candidate directions.
const GOLDEN_FRACTION: f32 = 0.38196602;
/// Radians in one complete turn.
const TAU: f32 = 6.2831855;
/// Maximum horizontal slices supported; the analysis resolution must be at most 72.
const MAX_PROFILE_BINS: u32 = 128u;
/// Weights for averaging nearby slice areas before choosing a burial depth.
const PROFILE_KERNEL: array<u32, 7> = array<u32, 7>(1u, 6u, 15u, 20u, 15u, 6u, 1u);
/// Percentage of the widest slice's area that a lower slice needs to count as
/// nearly as wide, and so be chosen as the ground line instead.
const NEARLY_WIDEST_PERCENT: u32 = 90u;
/// Sum of `PROFILE_KERNEL`.
const PROFILE_KERNEL_TOTAL: u32 = 64u;

/// Temporary 32-bit storage shared by placement passes; offsets are given below.
@group(0) @binding(9) var<storage, read_write> place_scratch: array<u32>;

// Storage order: coordinate sums and counts for each z slice, analysis state,
// candidate resting heights, nearby directions with heights, then horizontal
// slice counts for each z slice. All offsets are in 32-bit words.

/// Offset of the z-slice totals: three coordinate sums and one interior sample count.
fn sums_base() -> u32 {
    return 0u;
}

/// Offset of shared analysis values indexed by the `STATE_*` constants.
fn state_base() -> u32 {
    return 4u * samples_per_axis();
}

/// State offsets for the balance point, interior and surface counts, downward
/// direction, balance-point height, tilt range, and maximum and minimum burial depth.
const STATE_COM: u32 = 0u;
const STATE_COUNT: u32 = 3u;
const STATE_POINTS: u32 = 4u;
const STATE_DOWN: u32 = 5u;
const STATE_HEIGHT: u32 = 8u;
const STATE_SPREAD: u32 = 9u;
const STATE_LIMIT: u32 = 10u;
const STATE_MINIMUM: u32 = 11u;
/// Number of 32-bit words reserved for shared analysis values.
const STATE_WORDS: u32 = 16u;

/// Offset of the balance-point heights for candidate resting directions.
fn heights_base() -> u32 {
    return state_base() + STATE_WORDS;
}

/// Offset of nearby candidates, each stored as three direction words and one height.
fn candidates_base() -> u32 {
    return heights_base() + DIRECTION_COUNT;
}

/// Offset of horizontal slice counts, with `profile_bins()` words per z slice.
fn profiles_base() -> u32 {
    return candidates_base() + 4u * REFINE_CANDIDATES;
}

fn state_u32(k: u32) -> u32 {
    return place_scratch[state_base() + k];
}

fn state_f32(k: u32) -> f32 {
    return bitcast<f32>(place_scratch[state_base() + k]);
}

fn state_vec3(k: u32) -> vec3<f32> {
    return vec3<f32>(state_f32(k), state_f32(k + 1u), state_f32(k + 2u));
}

fn set_state_u32(k: u32, value: u32) {
    place_scratch[state_base() + k] = value;
}

fn set_state_f32(k: u32, value: f32) {
    place_scratch[state_base() + k] = bitcast<u32>(value);
}

fn set_state_vec3(k: u32, value: vec3<f32>) {
    set_state_f32(k, value.x);
    set_state_f32(k + 1u, value.y);
    set_state_f32(k + 2u, value.z);
}

/// Returns whether analysis found no interior or no surface. Call after `place_com`.
fn is_empty() -> bool {
    return state_u32(STATE_COUNT) == 0u || state_u32(STATE_POINTS) == 0u;
}

/// Returns support height: how far the centre of mass (the rock's balance
/// point) sits above ground with unit `down` pointing down. Call after
/// `place_com`, for a nonempty rock.
fn support_height(down: vec3<f32>) -> f32 {
    let com = state_vec3(STATE_COM);
    var h = -3.4028235e38;
    let points = state_u32(STATE_POINTS);
    for (var k = 0u; k < points; k++) {
        h = max(h, dot(vertex_position(crossings[k]) - com, down));
    }
    return h;
}

/// Returns unit candidate `i`, where `i < DIRECTION_COUNT`. The last is nearest -y.
fn sphere_direction(i: u32) -> vec3<f32> {
    let y = 1.0 - f32(2u * i + 1u) / f32(DIRECTION_COUNT);
    let r = sqrt(max(1.0 - y * y, 0.0));
    let angle = fract(f32(i) * GOLDEN_FRACTION) * TAU;
    return vec3<f32>(r * cos(angle), y, r * sin(angle));
}

/// Returns a tilt of unit `down` within roughly `spread` radians. Pass
/// `j < REFINE_CANDIDATES`; zero keeps the current direction.
fn refine_direction(down: vec3<f32>, spread: f32, j: u32) -> vec3<f32> {
    if j == 0u {
        return down;
    }
    let helper = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(down.y) < 0.9);
    let u = normalize(cross(helper, down));
    let v = cross(down, u);
    let radius = spread * sqrt(f32(j) / f32(REFINE_CANDIDATES - 1u));
    let angle = fract(f32(j) * GOLDEN_FRACTION) * TAU;
    return normalize(down + radius * (cos(angle) * u + sin(angle) * v));
}

/// Returns the shortest turn from unit `down` to -y; requires `down.y <= 0`.
fn smallest_rotation_to_down(down: vec3<f32>) -> mat3x3<f32> {
    let c = -down.y;
    let v = cross(down, vec3<f32>(0.0, -1.0, 0.0));
    let k = mat3x3<f32>(
        vec3<f32>(0.0, v.z, -v.y),
        vec3<f32>(-v.z, 0.0, v.x),
        vec3<f32>(v.y, -v.x, 0.0),
    );
    let identity = mat3x3<f32>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
    );
    return identity + k + (k * k) * (1.0 / (1.0 + c));
}

/// Turns unit `down` towards the ground (-y), including a half-turn about x
/// when the supplied direction points upwards.
fn rotation_to_down(down: vec3<f32>) -> mat3x3<f32> {
    if down.y > 0.0 {
        let half_turn = mat3x3<f32>(
            vec3<f32>(1.0, 0.0, 0.0),
            vec3<f32>(0.0, -1.0, 0.0),
            vec3<f32>(0.0, 0.0, -1.0),
        );
        return smallest_rotation_to_down(half_turn * down) * half_turn;
    }
    return smallest_rotation_to_down(down);
}

/// Returns the number of horizontal, one-cell-tall slices covering a rotated rock.
fn profile_bins() -> u32 {
    return (request.resolution * 7u + 3u) / 4u + 1u;
}

/// Totals interior coordinates and counts per z slice to estimate the balance
/// point. Dispatch `samples_per_axis()` items after sampling the field.
@compute @workgroup_size(WORKGROUP_SIZE)
fn place_com_slices(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let z = item_index(gid, num_workgroups);
    let n = samples_per_axis();
    if z >= n {
        return;
    }
    var sum = vec3<u32>(0u);
    var count = 0u;
    for (var y = 0u; y < n; y++) {
        for (var x = 0u; x < n; x++) {
            if inside(vec3<u32>(x, y, z)) {
                sum += vec3<u32>(x, y, z);
                count += 1u;
            }
        }
    }
    let at = sums_base() + 4u * z;
    place_scratch[at] = sum.x;
    place_scratch[at + 1u] = sum.y;
    place_scratch[at + 2u] = sum.z;
    place_scratch[at + 3u] = count;
}

/// Estimates the balance point, assuming uniform density, and records surface
/// point count. Dispatch one item after slice totals and surface crossings are ready.
@compute @workgroup_size(1)
fn place_com() {
    var sum = vec3<u32>(0u);
    var count = 0u;
    for (var z = 0u; z < samples_per_axis(); z++) {
        let at = sums_base() + 4u * z;
        sum += vec3<u32>(place_scratch[at], place_scratch[at + 1u], place_scratch[at + 2u]);
        count += place_scratch[at + 3u];
    }
    set_state_u32(STATE_COUNT, count);
    set_state_u32(STATE_POINTS, min(prefix[sample_count()].x, request.crossing_capacity));
    if count == 0u {
        return;
    }
    let mean = vec3<f32>(f32(sum.x), f32(sum.y), f32(sum.z)) / f32(count);
    set_state_vec3(STATE_COM, mean / f32(request.resolution) - vec3<f32>(0.5));
}

/// Measures the balance-point height for each candidate resting side. Dispatch
/// `DIRECTION_COUNT` items after `place_com`; skips empty rocks or disabled settling.
@compute @workgroup_size(WORKGROUP_SIZE)
fn place_heights(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(gid, num_workgroups);
    if i >= DIRECTION_COUNT || is_empty() || request.settle == 0u {
        return;
    }
    place_scratch[heights_base() + i] = bitcast<u32>(support_height(sphere_direction(i)));
}

/// Chooses progressively lower resting orientations, starting near upright.
/// Keeps the generated orientation when settling is off. Dispatch one item
/// after `place_heights`.
@compute @workgroup_size(1)
fn place_descend() {
    if is_empty() {
        return;
    }
    set_state_f32(STATE_SPREAD, REFINE_SPREAD);
    if request.settle == 0u {
        let down = vec3<f32>(0.0, -1.0, 0.0);
        set_state_vec3(STATE_DOWN, down);
        set_state_f32(STATE_HEIGHT, support_height(down));
        return;
    }
    var current = DIRECTION_COUNT - 1u;
    for (var step = 0u; step < MAX_DESCENT_STEPS; step++) {
        let here = sphere_direction(current);
        var best = current;
        for (var i = 0u; i < DIRECTION_COUNT; i++) {
            let height = bitcast<f32>(place_scratch[heights_base() + i]);
            let best_height = bitcast<f32>(place_scratch[heights_base() + best]);
            if dot(sphere_direction(i), here) >= NEIGHBOUR_COS && height < best_height {
                best = i;
            }
        }
        if best == current {
            break;
        }
        current = best;
    }
    set_state_vec3(STATE_DOWN, sphere_direction(current));
    set_state_f32(STATE_HEIGHT, bitcast<f32>(place_scratch[heights_base() + current]));
}

/// Measures heights for smaller tilts around the chosen resting side.
/// Dispatch `REFINE_CANDIDATES` items before each `place_refine_pick`.
@compute @workgroup_size(WORKGROUP_SIZE)
fn place_refine_heights(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let j = item_index(gid, num_workgroups);
    if j >= REFINE_CANDIDATES || is_empty() || request.settle == 0u {
        return;
    }
    let d = refine_direction(state_vec3(STATE_DOWN), state_f32(STATE_SPREAD), j);
    let at = candidates_base() + 4u * j;
    place_scratch[at] = bitcast<u32>(d.x);
    place_scratch[at + 1u] = bitcast<u32>(d.y);
    place_scratch[at + 2u] = bitcast<u32>(d.z);
    place_scratch[at + 3u] = bitcast<u32>(support_height(d));
}

/// Selects the nearby tilt with the lowest balance point and narrows the next
/// search. Dispatch one item after `place_refine_heights`.
@compute @workgroup_size(1)
fn place_refine_pick() {
    if is_empty() || request.settle == 0u {
        return;
    }
    var down = state_vec3(STATE_DOWN);
    var height = state_f32(STATE_HEIGHT);
    for (var j = 1u; j < REFINE_CANDIDATES; j++) {
        let at = candidates_base() + 4u * j;
        let h = bitcast<f32>(place_scratch[at + 3u]);
        if h < height {
            down = vec3<f32>(
                bitcast<f32>(place_scratch[at]),
                bitcast<f32>(place_scratch[at + 1u]),
                bitcast<f32>(place_scratch[at + 2u]),
            );
            height = h;
        }
    }
    set_state_vec3(STATE_DOWN, down);
    set_state_f32(STATE_HEIGHT, height);
    set_state_f32(STATE_SPREAD, 0.5 * state_f32(STATE_SPREAD));
}

/// Stores the ground rotation and offset, then calculates the burial cap.
/// Empty rocks get no rotation, offset, or burial. Dispatch one item after
/// choosing the resting direction.
@compute @workgroup_size(1)
fn place_finish() {
    let slot = request.request_index;
    if is_empty() {
        status[slot].rotation = mat3x3<f32>(
            vec3<f32>(1.0, 0.0, 0.0),
            vec3<f32>(0.0, 1.0, 0.0),
            vec3<f32>(0.0, 0.0, 1.0),
        );
        status[slot].translation = vec3<f32>(0.0);
        status[slot].embed_depth = 0.0;
        return;
    }
    let down = state_vec3(STATE_DOWN);
    let height = state_f32(STATE_HEIGHT);
    let rotation = rotation_to_down(down);
    status[slot].rotation = rotation;
    status[slot].translation = vec3<f32>(0.0, height, 0.0) - rotation * state_vec3(STATE_COM);
    let top = height + support_height(-down);
    set_state_f32(STATE_LIMIT, request.max_embed_fraction * top);
    set_state_f32(STATE_MINIMUM, request.min_embed_fraction * top);
}

/// Groups interior samples by height above ground to estimate horizontal area.
/// Dispatch `samples_per_axis()` items after `place_finish`.
@compute @workgroup_size(WORKGROUP_SIZE)
fn place_profile_slices(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let z = item_index(gid, num_workgroups);
    let n = samples_per_axis();
    if z >= n || is_empty() {
        return;
    }
    let placement = status[request.request_index];
    let bins = profile_bins();
    let r = f32(request.resolution);
    var counts: array<u32, MAX_PROFILE_BINS>;
    for (var y = 0u; y < n; y++) {
        for (var x = 0u; x < n; x++) {
            let s = vec3<u32>(x, y, z);
            if inside(s) {
                let height = (placement.rotation * sample_position(s)).y + placement.translation.y;
                let bin = clamp(i32(floor(height * r)), 0, i32(bins) - 1);
                counts[bin] += 1u;
            }
        }
    }
    let at = profiles_base() + bins * z;
    for (var b = 0u; b < bins; b++) {
        place_scratch[at + b] = counts[b];
    }
}

/// Stores the suggested burial depth at the widest allowed horizontal slice,
/// averaging nearby areas. Ties favour the lower slice; if no slice centre is
/// allowed, uses the cap itself. Dispatch one item after `place_profile_slices`.
@compute @workgroup_size(1)
fn place_embed() {
    if is_empty() {
        return;
    }
    let bins = profile_bins();
    let r = f32(request.resolution);
    var counts: array<u32, MAX_PROFILE_BINS>;
    for (var z = 0u; z < samples_per_axis(); z++) {
        let at = profiles_base() + bins * z;
        for (var b = 0u; b < bins; b++) {
            counts[b] += place_scratch[at + b];
        }
    }

    // Average neighbouring slice areas for every slice centred within the limit.
    // Slices past either end of the profile are left out of the average.
    let limit = state_f32(STATE_LIMIT);
    var areas: array<u32, MAX_PROFILE_BINS>;
    var allowed = 0u;
    var widest = 0u;
    for (var bin = 0u; bin < bins; bin++) {
        if (f32(bin) + 0.5) / r > limit {
            break;
        }
        var area = 0u;
        var weight = 0u;
        for (var k = 0u; k < 7u; k++) {
            let at = i32(bin) + i32(k) - 3;
            if at >= 0 && at < i32(bins) {
                area += PROFILE_KERNEL[k] * counts[at];
                weight += PROFILE_KERNEL[k];
            }
        }
        area = area * PROFILE_KERNEL_TOTAL / weight;
        areas[bin] = area;
        widest = max(widest, area);
        allowed = bin + 1u;
    }

    // Choose the lowest slice that is nearly as wide as the widest one, then
    // sink at least the minimum depth.
    var embed = limit;
    for (var bin = 0u; bin < allowed; bin++) {
        if areas[bin] * 100u >= widest * NEARLY_WIDEST_PERCENT {
            embed = (f32(bin) + 0.5) / r;
            break;
        }
    }
    status[request.request_index].embed_depth = max(embed, state_f32(STATE_MINIMUM));
}
