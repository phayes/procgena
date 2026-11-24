//! Checks that a [`Scatter`] keeps the shared placement contract, for the
//! tests of every Scatter, built-in or your own. Each check panics with a
//! message naming the first copy that breaks the contract.

use std::collections::HashMap;

use glam::{DVec2, DVec3};

use super::{ChunkCache, ChunkGrid, Footprints, Scatter, ScatterCopy, ScatterInput};
use crate::address::CopyAddress;

/// How far apart, in metres, two placements of one copy may stand and still
/// count as the same: float rounding of positions relative to different
/// area centres.
const SAME_PLACE: f64 = 1e-3;

/// Where a copy's transform puts its origin in the World, for a copy
/// placed over an area around `centre`.
fn world(copy: &ScatterCopy, centre: DVec2) -> DVec3 {
    let t = copy.transform.translation;
    DVec3::new(
        centre.x + f64::from(t.x),
        f64::from(t.y),
        centre.y + f64::from(t.z),
    )
}

/// Checks that placing `input`'s area chunk by chunk on `grid`, in row
/// order, in reverse and in a shuffled order, gives the same copies as
/// placing the whole area at once: the same addresses, slots, footprints and
/// places, with each copy owned by exactly one chunk. Returns how many
/// copies the area holds.
///
/// # Panics
///
/// Panics if placing fails, or if any chunk order differs from the whole.
pub fn assert_chunks_match_area(
    scatter: &dyn Scatter,
    input: &ScatterInput<'_>,
    grid: ChunkGrid,
) -> usize {
    let area = input.area.bounds();
    let lattice = scatter.lattice();
    let whole: HashMap<CopyAddress, (ScatterCopy, DVec3)> = scatter
        .place(input)
        .expect("placing the whole area")
        .into_iter()
        .map(|copy| {
            let at = world(&copy, input.area.centre);
            (copy.address, (copy, at))
        })
        .collect();

    // Three orders: row by row, reversed, and a fixed shuffle.
    let rows = grid.chunks_over(area);
    let mut reversed = rows.clone();
    reversed.reverse();
    let mut shuffled = rows.clone();
    let n = shuffled.len();
    for i in 0..n {
        let j = (crate::hash::pcg(i as u32) as usize) % n;
        shuffled.swap(i, j);
    }

    for (name, order) in [
        ("row", rows),
        ("reversed", reversed),
        ("shuffled", shuffled),
    ] {
        let mut cache = ChunkCache::new(grid);
        let mut seen: HashMap<CopyAddress, ()> = HashMap::new();
        for chunk in order {
            let centre = grid.area(chunk).centre;
            for copy in cache
                .chunk(scatter, input, chunk)
                .expect("placing a chunk")
                .iter()
            {
                assert!(
                    seen.insert(copy.address, ()).is_none(),
                    "{name} order: {:?} is owned by two chunks",
                    copy.address
                );
                if !area.contains(lattice.corner(copy.address.cell)) {
                    continue;
                }
                let Some((expected, at)) = whole.get(&copy.address) else {
                    panic!("{name} order: chunk {chunk:?} adds {:?}", copy.address);
                };
                let here = world(copy, centre);
                assert!(
                    here.distance(*at) < SAME_PLACE,
                    "{name} order: {:?} stands at {here} in its chunk but {at} in the area",
                    copy.address
                );
                assert_eq!(copy.slot, expected.slot, "{name} order: {:?}", copy.address);
                assert_eq!(
                    copy.footprint, expected.footprint,
                    "{name} order: {:?}",
                    copy.address
                );
                assert!(
                    copy.transform
                        .matrix3
                        .abs_diff_eq(expected.transform.matrix3, 1e-5),
                    "{name} order: {:?} turns differently",
                    copy.address
                );
            }
        }
        let missing = whole.keys().filter(|a| !seen.contains_key(a)).count();
        assert_eq!(
            missing, 0,
            "{name} order: {missing} copies of the area are missing"
        );
    }
    whole.len()
}

/// Checks that every copy of `input`'s area resolves to itself, one at a
/// time and in one batch, and that an address in a cell with no copy, or of
/// another Scatter, resolves to nothing.
///
/// # Panics
///
/// Panics if placing fails, or if any lookup gives the wrong answer.
pub fn assert_resolves(scatter: &dyn Scatter, input: &ScatterInput<'_>) {
    let copies = scatter.place(input).expect("placing the area");
    let addresses: Vec<CopyAddress> = copies.iter().map(|c| c.address).collect();
    let mut cache = ChunkCache::default();
    let batch = cache
        .resolve_all(scatter, input, &addresses)
        .expect("resolving the copies");
    for (copy, found) in copies.iter().zip(batch) {
        let found = found.unwrap_or_else(|| panic!("{:?} does not resolve", copy.address));
        assert_eq!(found.slot, copy.slot);
        let gap = found.transform.translation - copy.transform.translation;
        assert!(
            f64::from(gap.length()) < SAME_PLACE,
            "{:?} resolves elsewhere",
            copy.address
        );
    }
    for copy in copies.iter().take(16) {
        let one = scatter
            .resolve(input, &copy.address)
            .expect("resolving one copy")
            .unwrap_or_else(|| panic!("{:?} does not resolve alone", copy.address));
        assert_eq!(one.footprint, copy.footprint);
    }

    // A cell of the area with no copy in it, and another Scatter's address.
    let lattice = scatter.lattice();
    if let Some(empty) = lattice
        .cells_in(input.area.bounds())
        .find(|cell| copies.iter().all(|c| c.address.cell != *cell))
    {
        let address = input.prefix.copy(empty, 0);
        assert_eq!(scatter.resolve(input, &address).expect("resolving"), None);
    }
    if let Some(copy) = copies.first() {
        let mut other = copy.address;
        other.scatter ^= 1;
        assert_eq!(scatter.resolve(input, &other).expect("resolving"), None);
    }
}

/// Checks that placing `input` while keeping `above` clear leaves no copy
/// whose footprint overlaps `above`, and returns how many copies were placed
/// with and without the exclusion.
///
/// # Panics
///
/// Panics if placing fails, or if a copy overlaps `above`.
pub fn assert_keeps_clear(
    scatter: &dyn Scatter,
    input: &ScatterInput<'_>,
    above: &Footprints,
) -> (usize, usize) {
    let free = scatter.place(input).expect("placing the area").len();
    let kept = scatter
        .place(&input.with_exclusion(above))
        .expect("placing the area with exclusions");
    for copy in &kept {
        assert!(
            !above.overlaps_any(&copy.footprint),
            "{:?} overlaps a footprint it must keep clear of",
            copy.address
        );
    }
    (kept.len(), free)
}
