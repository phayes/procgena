// Exclusive prefix sum over pairs of counts, replacing each entry with the sum
// of all entries before it.
//
// To scan `count` entries: run `scan_blocks` over `data` with `sums` receiving
// one total per block of `SCAN_BLOCK` entries. If there is more than one block,
// scan `sums` the same way (recursively), then run `add_block_offsets` over
// `data` with the scanned `sums`.

/// Entries handled by one workgroup.
const SCAN_BLOCK: u32 = 256u;

/// Size of the array being scanned at this level.
struct ScanParams {
    count: u32,
}

@group(0) @binding(0) var<uniform> scan: ScanParams;
@group(0) @binding(1) var<storage, read_write> data: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read_write> sums: array<vec2<u32>>;

var<workgroup> partial: array<vec2<u32>, SCAN_BLOCK>;

/// Returns the block this workgroup handles. Dispatch rows of workgroups along
/// x, adding rows along y when one row cannot cover every block.
fn block_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x;
}

/// Replaces each entry with the sum of the entries before it in its block, and
/// writes each block's total to `sums`. Dispatch at least one workgroup per block.
@compute @workgroup_size(SCAN_BLOCK)
fn scan_blocks(
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let block = block_index(workgroup_id, num_workgroups);
    let i = block * SCAN_BLOCK + local;
    var value = vec2<u32>(0u);
    if i < scan.count {
        value = data[i];
    }
    partial[local] = value;
    workgroupBarrier();

    // Inclusive scan within the workgroup (Hillis-Steele).
    for (var offset = 1u; offset < SCAN_BLOCK; offset *= 2u) {
        var add = vec2<u32>(0u);
        if local >= offset {
            add = partial[local - offset];
        }
        workgroupBarrier();
        partial[local] += add;
        workgroupBarrier();
    }

    if i < scan.count {
        data[i] = partial[local] - value;
    }
    // Workgroups past the last block, from rounding up a row dispatch, write nothing.
    if local == SCAN_BLOCK - 1u && block * SCAN_BLOCK < scan.count {
        sums[block] = partial[local];
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
        data[i] += sums[block];
    }
}
