// Stable least-significant-digit radix sort of element indices, 4 bits per
// pass, over regions of a `u32` arena.
//
// The sort reorders a permutation: a list of element indices. Each pass
// reads the list at `src_offset`, takes 4 bits (at `shift`) of each
// element's key word from the key array at `key_offset`, and writes the list
// reordered by those bits to `dst_offset`, keeping equal digits in order.
// Sorting by several key words, or by more bits, is done by running passes
// from the least significant digit up, swapping `src` and `dst` each time.
//
// One pass: `sort_count` writes per-block digit counts, digit-major, to the
// region at `hist_offset`; scan that region (`16 × blocks` entries) with
// `scan.wesl`; then `sort_scatter` writes the reordered list.

/// Elements handled by one workgroup.
const SORT_BLOCK: u32 = 256u;

/// Different values of one 4-bit digit.
const SORT_DIGITS: u32 = 16u;

/// One pass of the sort. Offsets are word offsets into the arena.
struct SortParams {
    /// Word holding how many elements to sort, or `0xffffffff` to sort all of
    /// `capacity`. Never more than `capacity` are sorted.
    count_offset: u32,
    capacity: u32,
    /// The list read and the list written.
    src_offset: u32,
    dst_offset: u32,
    /// Key words, indexed by element.
    key_offset: u32,
    /// Lowest bit of this pass's digit.
    shift: u32,
    /// Per-block digit counts: `SORT_DIGITS × blocks` words, digit-major.
    hist_offset: u32,
    /// Blocks covering `capacity`.
    blocks: u32,
}

@group(0) @binding(0) var<uniform> sort: SortParams;
@group(0) @binding(1) var<storage, read_write> arena: array<u32>;

var<workgroup> digit_counts: array<atomic<u32>, SORT_DIGITS>;
var<workgroup> block_digits: array<u32, SORT_BLOCK>;

/// Returns how many elements this pass sorts.
fn sort_count_of() -> u32 {
    if sort.count_offset == 0xffffffffu {
        return sort.capacity;
    }
    return min(arena[sort.count_offset], sort.capacity);
}

/// Returns this pass's digit of the element at list position `i`.
fn digit_at(i: u32) -> u32 {
    let element = arena[sort.src_offset + i];
    return (arena[sort.key_offset + element] >> sort.shift) & (SORT_DIGITS - 1u);
}

/// Returns the block this workgroup handles. Dispatch rows of workgroups
/// along x, adding rows along y when one row cannot cover every block.
fn sort_block_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x;
}

/// Counts each digit in each block. Dispatch at least `blocks` workgroups.
@compute @workgroup_size(SORT_BLOCK)
fn sort_count(
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let block = sort_block_index(workgroup_id, num_workgroups);
    if local < SORT_DIGITS {
        atomicStore(&digit_counts[local], 0u);
    }
    workgroupBarrier();
    let i = block * SORT_BLOCK + local;
    if block < sort.blocks && i < sort_count_of() {
        atomicAdd(&digit_counts[digit_at(i)], 1u);
    }
    workgroupBarrier();
    if block < sort.blocks && local < SORT_DIGITS {
        arena[sort.hist_offset + local * sort.blocks + block] = atomicLoad(&digit_counts[local]);
    }
}

/// Writes each element to its sorted position, using the scanned digit
/// counts. Dispatch at least `blocks` workgroups.
@compute @workgroup_size(SORT_BLOCK)
fn sort_scatter(
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let block = sort_block_index(workgroup_id, num_workgroups);
    let i = block * SORT_BLOCK + local;
    let sorted = block < sort.blocks && i < sort_count_of();

    // Share each element's digit; unsorted slots get a digit no element has.
    var digit = SORT_DIGITS;
    if sorted {
        digit = digit_at(i);
    }
    block_digits[local] = digit;
    workgroupBarrier();
    if !sorted {
        return;
    }

    // Rank among the earlier elements of this block with the same digit.
    var rank = 0u;
    for (var j = 0u; j < local; j++) {
        rank += select(0u, 1u, block_digits[j] == digit);
    }
    let start = arena[sort.hist_offset + digit * sort.blocks + block];
    arena[sort.dst_offset + start + rank] = arena[sort.src_offset + i];
}
