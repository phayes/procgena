// Exclusive prefix sum over a region of a `u32` arena, replacing each entry
// with the sum of all entries before it. Sums wrap on overflow.
//
// To scan `count` entries at `data_offset`: run `scan_blocks`, which writes
// one total per block of `SCAN_BLOCK` entries to the region at
// `sums_offset`. If there is more than one block, scan those totals the same
// way (recursively), then run `add_block_offsets` with the scanned totals.
//
// Entries past the number a caller really uses may hold anything: the
// prefix at position `i` depends only on entries before `i`, so every prefix
// up to and including the used count is still right.

/// Entries handled by one workgroup.
const SCAN_BLOCK: u32 = 256u;

/// One level of a scan: how many entries, and where they and their block
/// totals are, as word offsets into the arena.
struct ScanParams {
    count: u32,
    data_offset: u32,
    sums_offset: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> scan: ScanParams;
@group(0) @binding(1) var<storage, read_write> arena: array<u32>;

var<workgroup> partial: array<u32, SCAN_BLOCK>;

/// Returns the block this workgroup handles. Dispatch rows of workgroups
/// along x, adding rows along y when one row cannot cover every block.
fn block_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x;
}

/// Replaces each entry with the sum of the entries before it in its block,
/// and writes each block's total to the sums region. Dispatch at least one
/// workgroup per block.
@compute @workgroup_size(SCAN_BLOCK)
fn scan_blocks(
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let block = block_index(workgroup_id, num_workgroups);
    let i = block * SCAN_BLOCK + local;
    var value = 0u;
    if i < scan.count {
        value = arena[scan.data_offset + i];
    }
    partial[local] = value;
    workgroupBarrier();

    // Inclusive scan within the workgroup (Hillis-Steele).
    for (var offset = 1u; offset < SCAN_BLOCK; offset *= 2u) {
        var add = 0u;
        if local >= offset {
            add = partial[local - offset];
        }
        workgroupBarrier();
        partial[local] += add;
        workgroupBarrier();
    }

    if i < scan.count {
        arena[scan.data_offset + i] = partial[local] - value;
    }
    // Workgroups past the last block, from rounding up a row dispatch, write nothing.
    if local == SCAN_BLOCK - 1u && block * SCAN_BLOCK < scan.count {
        arena[scan.sums_offset + block] = partial[local];
    }
}

/// Adds each block's scanned total to its entries, completing the scan.
/// Dispatch at least one workgroup per block.
@compute @workgroup_size(SCAN_BLOCK)
fn add_block_offsets(
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let block = block_index(workgroup_id, num_workgroups);
    let i = block * SCAN_BLOCK + local;
    if i < scan.count {
        arena[scan.data_offset + i] += arena[scan.sums_offset + block];
    }
}
