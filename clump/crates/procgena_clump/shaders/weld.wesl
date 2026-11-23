// Welds the batch's input vertices and measures whether each input is
// closed, matching `procgena_mesh::Surface::welded` and
// `Surface::is_closed_welded`.
//
// Vertices at the same position (bit for bit, with -0.0 equal to 0.0) share
// a welded id. Ids count up from 0 within each request, in order of first
// appearance. Pass order, with V vertex slots and E = 3 × triangle slots:
//
// 1. `weld_keys` (V), scan the weld flags (V + 1), `weld_list` (V).
// 2. Sort the list by key x, y, z, request; the count is the flags' total.
// 3. `weld_starts` (V + 1), scan the starts (V + 1), `weld_heads` (V),
//    scan the ranks (V + 1), `weld_ids` (V), `weld_counts` (requests).
// 4. `edge_keys` (E), scan the edge flags (E + 1), `edge_list` (E).
// 5. Sort the edge list by low id, high id, request.
// 6. `edge_starts` (E + 1), scan the starts (E + 1), `edge_heads` (E),
//    `edge_runs` (E).

import package::batch::{
    arena, batch, counters, requests, region, item_index, load3, store3, counter,
    request_of_vertex, request_of_triangle,
    COUNTER_STRIDE, COUNTER_TRIANGLES, COUNTER_VERTICES, COUNTER_WELDED, COUNTER_OPEN_EDGES,
    REGION_POSITIONS, REGION_NORMALS, REGION_CORNERS,
    REGION_WELD_FLAGS, REGION_WELD_LIST_A, REGION_WELD_KEY_X, REGION_WELD_KEY_Y,
    REGION_WELD_KEY_Z, REGION_WELD_KEY_REQUEST, REGION_WELD_STARTS, REGION_WELD_HEADS,
    REGION_WELD_RANK, REGION_WELD_ID, REGION_WELDED_NORMALS,
    REGION_EDGE_FLAGS, REGION_EDGE_LIST_A, REGION_EDGE_KEY_LO, REGION_EDGE_KEY_HI,
    REGION_EDGE_KEY_REQUEST, REGION_EDGE_STARTS, REGION_EDGE_HEADS, REGION_WELD_SORTED,
    REGION_EDGE_SORTED, WORKGROUP_SIZE,
};
import package::math::normalize_or_zero;

/// Returns the sorted weld list.
fn weld_sorted() -> u32 {
    return region(REGION_WELD_SORTED);
}

/// Returns the sorted edge list.
fn edge_sorted() -> u32 {
    return region(REGION_EDGE_SORTED);
}

/// Returns how many vertices, and how many edges, are being sorted: the
/// totals of the flag scans.
fn weld_count() -> u32 {
    return arena[region(REGION_WELD_FLAGS) + batch.total_vertices];
}
fn edge_count() -> u32 {
    return arena[region(REGION_EDGE_FLAGS) + 3u * batch.total_triangles];
}

/// Flags each used vertex slot and writes its sort keys: its position's
/// bits, with -0.0 made 0.0, and its request. Also clears its rank flag.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_keys(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        // The extra slots hold 0, so the scans' totals land after the last.
        if slot == batch.total_vertices {
            arena[region(REGION_WELD_FLAGS) + slot] = 0u;
            arena[region(REGION_WELD_RANK) + slot] = 0u;
        }
        return;
    }
    let r = request_of_vertex(slot);
    let local = slot - requests[r].vertex_base;
    let used = local < min(counter(r, COUNTER_VERTICES), requests[r].vertex_capacity);
    arena[region(REGION_WELD_FLAGS) + slot] = select(0u, 1u, used);
    arena[region(REGION_WELD_RANK) + slot] = 0u;
    let at = region(REGION_POSITIONS) + 3u * slot;
    arena[region(REGION_WELD_KEY_X) + slot] = position_key(arena[at]);
    arena[region(REGION_WELD_KEY_Y) + slot] = position_key(arena[at + 1u]);
    arena[region(REGION_WELD_KEY_Z) + slot] = position_key(arena[at + 2u]);
    arena[region(REGION_WELD_KEY_REQUEST) + slot] = r;
}

/// Returns the weld key of one coordinate's bits: the bits themselves, with
/// -0.0 made 0.0. Works on bits, so fast float math can't change it.
fn position_key(bits: u32) -> u32 {
    return select(bits, 0u, bits == 0x80000000u);
}

/// Lists the used vertex slots in order, after the flags are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_list(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let slot = item_index(global_id, num_workgroups);
    if slot >= batch.total_vertices {
        return;
    }
    let flags = region(REGION_WELD_FLAGS);
    if arena[flags + slot + 1u] != arena[flags + slot] {
        arena[region(REGION_WELD_LIST_A) + arena[flags + slot]] = slot;
    }
}

/// Returns whether vertex slots `a` and `b` have the same weld key.
fn same_weld_key(a: u32, b: u32) -> bool {
    return arena[region(REGION_WELD_KEY_X) + a] == arena[region(REGION_WELD_KEY_X) + b]
        && arena[region(REGION_WELD_KEY_Y) + a] == arena[region(REGION_WELD_KEY_Y) + b]
        && arena[region(REGION_WELD_KEY_Z) + a] == arena[region(REGION_WELD_KEY_Z) + b]
        && arena[region(REGION_WELD_KEY_REQUEST) + a] == arena[region(REGION_WELD_KEY_REQUEST) + b];
}

/// Flags each sorted position where a run of equal keys starts, and marks
/// each run's first vertex (its lowest slot) as a representative. Dispatch
/// V + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_starts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    if i > batch.total_vertices {
        return;
    }
    let list = weld_sorted();
    var start = 0u;
    if i < weld_count() {
        start = select(0u, 1u, i == 0u || !same_weld_key(arena[list + i], arena[list + i - 1u]));
        if start == 1u {
            arena[region(REGION_WELD_RANK) + arena[list + i]] = 1u;
        }
    }
    arena[region(REGION_WELD_STARTS) + i] = start;
}

/// Records each run's start position, after the starts are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_heads(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    let starts = region(REGION_WELD_STARTS);
    if i < weld_count() && arena[starts + i + 1u] != arena[starts + i] {
        arena[region(REGION_WELD_HEADS) + arena[starts + i]] = i;
    }
}

/// Gives each vertex its request's welded id, after the ranks are scanned,
/// and sums each welded position's normals in vertex order.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_ids(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    let count = weld_count();
    if i >= count {
        return;
    }
    let list = weld_sorted();
    let starts = region(REGION_WELD_STARTS);
    let ranks = region(REGION_WELD_RANK);

    // This vertex's run, and the run's representative.
    let run = arena[starts + i + 1u] - 1u;
    let head = arena[region(REGION_WELD_HEADS) + run];
    let representative = arena[list + head];
    let slot = arena[list + i];
    let r = request_of_vertex(slot);
    let welded = arena[ranks + representative];
    arena[region(REGION_WELD_ID) + slot] = welded - arena[ranks + requests[r].vertex_base];

    // The run's first position sums the run's normals, in vertex order.
    if head == i {
        var sum = vec3<f32>(0.0);
        for (var j = i; j < count && arena[starts + j + 1u] - 1u == run; j++) {
            sum += load3(region(REGION_NORMALS) + 3u * arena[list + j]);
        }
        store3(region(REGION_WELDED_NORMALS) + 3u * welded, normalize_or_zero(sum));
    }
}

/// Records each request's number of welded positions. One item per request.
@compute @workgroup_size(WORKGROUP_SIZE)
fn weld_counts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let r = item_index(global_id, num_workgroups);
    if r >= batch.request_count {
        return;
    }
    let ranks = region(REGION_WELD_RANK);
    let start = requests[r].vertex_base;
    let end = start + requests[r].vertex_capacity;
    atomicStore(&counters[r * COUNTER_STRIDE + COUNTER_WELDED], arena[ranks + end] - arena[ranks + start]);
}

/// Flags each used triangle edge slot and writes its sort keys: its welded
/// ends, lowest first, and its request. Dispatch E + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_keys(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    let edges = 3u * batch.total_triangles;
    if e >= edges {
        if e == edges {
            arena[region(REGION_EDGE_FLAGS) + e] = 0u;
        }
        return;
    }
    let t = e / 3u;
    let k = e % 3u;
    let r = request_of_triangle(t);
    let used = t - requests[r].triangle_base < counter(r, COUNTER_TRIANGLES);
    arena[region(REGION_EDGE_FLAGS) + e] = select(0u, 1u, used);
    if !used {
        return;
    }
    let corners = region(REGION_CORNERS) + 3u * t;
    let base = requests[r].vertex_base;
    let a = arena[region(REGION_WELD_ID) + base + arena[corners + k]];
    let b = arena[region(REGION_WELD_ID) + base + arena[corners + (k + 1u) % 3u]];
    arena[region(REGION_EDGE_KEY_LO) + e] = min(a, b);
    arena[region(REGION_EDGE_KEY_HI) + e] = max(a, b);
    arena[region(REGION_EDGE_KEY_REQUEST) + e] = r;
}

/// Lists the used edge slots in order, after the flags are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_list(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let e = item_index(global_id, num_workgroups);
    if e >= 3u * batch.total_triangles {
        return;
    }
    let flags = region(REGION_EDGE_FLAGS);
    if arena[flags + e + 1u] != arena[flags + e] {
        arena[region(REGION_EDGE_LIST_A) + arena[flags + e]] = e;
    }
}

/// Returns whether edge slots `a` and `b` join the same welded positions in
/// the same request.
fn same_edge(a: u32, b: u32) -> bool {
    return arena[region(REGION_EDGE_KEY_LO) + a] == arena[region(REGION_EDGE_KEY_LO) + b]
        && arena[region(REGION_EDGE_KEY_HI) + a] == arena[region(REGION_EDGE_KEY_HI) + b]
        && arena[region(REGION_EDGE_KEY_REQUEST) + a] == arena[region(REGION_EDGE_KEY_REQUEST) + b];
}

/// Flags each sorted position where a run of equal edges starts. Dispatch
/// E + 1 items.
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_starts(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    if i > 3u * batch.total_triangles {
        return;
    }
    let list = edge_sorted();
    var start = 0u;
    if i < edge_count() {
        start = select(0u, 1u, i == 0u || !same_edge(arena[list + i], arena[list + i - 1u]));
    }
    arena[region(REGION_EDGE_STARTS) + i] = start;
}

/// Records each run's start position, after the starts are scanned.
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_heads(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    let starts = region(REGION_EDGE_STARTS);
    if i < edge_count() && arena[starts + i + 1u] != arena[starts + i] {
        arena[region(REGION_EDGE_HEADS) + arena[starts + i]] = i;
    }
}

/// Counts, per request, the welded edges not used by exactly two triangle
/// sides. A request with none is closed.
@compute @workgroup_size(WORKGROUP_SIZE)
fn edge_runs(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let i = item_index(global_id, num_workgroups);
    let count = edge_count();
    let starts = region(REGION_EDGE_STARTS);
    if i >= count || arena[starts + i + 1u] == arena[starts + i] {
        return;
    }
    // This run ends where the next starts, or at the end of the list.
    let run = arena[starts + i];
    var end = count;
    if run + 1u < arena[starts + count] {
        end = arena[region(REGION_EDGE_HEADS) + run + 1u];
    }
    if end - i != 2u {
        let r = arena[region(REGION_EDGE_KEY_REQUEST) + arena[edge_sorted() + i]];
        atomicAdd(&counters[r * COUNTER_STRIDE + COUNTER_OPEN_EDGES], 1u);
    }
}
