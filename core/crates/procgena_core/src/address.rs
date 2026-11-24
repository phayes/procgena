//! Addresses that name one copy anywhere in a World.
//!
//! A [`CopyAddress`] is plain integers: the region, the Ecotope and the
//! Scatter that placed the copy, the Scatter's lattice cell and the copy's
//! number within that cell. It stays the same when chunks load in another
//! order, when other Scatters are added, and after a reload, so it is what
//! edits, picking and promotion use.

/// Names one region of a World: the region whose Biome placed a copy.
///
/// A Scatter built on its own, in a viewer or a studio tab, uses
/// `RegionId(0)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(pub u64);

/// One cell of a Scatter's lattice, fixed to the World: column `x` along X
/// and row `z` along Z, packed into one integer.
///
/// A cell only means something together with the Scatter's cell size (see
/// [`crate::scatter::Lattice`]): the same `CellId` in two Scatters with
/// different cell sizes names different ground.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(pub u64);

impl CellId {
    /// The cell in column `x` and row `z`.
    pub const fn new(x: i32, z: i32) -> Self {
        Self(((x as u32 as u64) << 32) | z as u32 as u64)
    }

    /// The cell's column, along X.
    pub const fn x(self) -> i32 {
        (self.0 >> 32) as u32 as i32
    }

    /// The cell's row, along Z.
    pub const fn z(self) -> i32 {
        self.0 as u32 as i32
    }
}

/// The part of a [`CopyAddress`] that names one Scatter in one Ecotope of
/// one region. Every copy a Scatter places carries its prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AddressPrefix {
    /// The region whose Biome placed the copies.
    pub region: RegionId,
    /// The salt of the Ecotope's key ([`crate::ChildKey::salt`]).
    pub ecotope: u32,
    /// The salt of the Scatter's key within its Ecotope.
    pub scatter: u32,
}

impl AddressPrefix {
    /// The prefix of a Scatter built on its own: region 0 and zero salts.
    pub const ALONE: Self = Self {
        region: RegionId(0),
        ecotope: 0,
        scatter: 0,
    };

    /// The address of copy `index` in lattice cell `cell` under this prefix.
    pub const fn copy(self, cell: CellId, index: u16) -> CopyAddress {
        CopyAddress {
            region: self.region,
            ecotope: self.ecotope,
            scatter: self.scatter,
            cell,
            index,
        }
    }
}

/// Names one copy anywhere in a World. Stable across loads, chunking, build
/// order and the insertion of other Scatters.
///
/// Addresses sort by region, then Ecotope, then Scatter, then cell, then
/// index, so sorting a list of them groups the copies of one Scatter.
/// An address names a spot, not a guarantee: a spot can be empty (masked
/// out or thinned), which is what [`crate::scatter::Scatter::resolve`]
/// reports as `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CopyAddress {
    /// The region whose Biome placed the copy.
    pub region: RegionId,
    /// The salt of the Ecotope's key.
    pub ecotope: u32,
    /// The salt of the Scatter's key within its Ecotope.
    pub scatter: u32,
    /// The Scatter's lattice cell that the copy came from.
    pub cell: CellId,
    /// The copy's number within its cell: 0 when a cell holds one copy, and
    /// the member's number when a Scatter places several in one cell.
    pub index: u16,
}

impl CopyAddress {
    /// The region, Ecotope and Scatter part of the address.
    pub const fn prefix(&self) -> AddressPrefix {
        AddressPrefix {
            region: self.region,
            ecotope: self.ecotope,
            scatter: self.scatter,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_pack_both_signs_without_collisions() {
        for (x, z) in [
            (0, 0),
            (-1, 0),
            (0, -1),
            (-1, -1),
            (i32::MIN, i32::MAX),
            (7, -3),
        ] {
            let cell = CellId::new(x, z);
            assert_eq!((cell.x(), cell.z()), (x, z));
        }
        assert_ne!(CellId::new(-1, 0), CellId::new(0, -1));
        assert_eq!(CellId::new(4031, -77).0, 0x0000_0fbf_ffff_ffb3);
    }

    #[test]
    fn addresses_sort_by_scatter_before_cell() {
        let a = AddressPrefix {
            scatter: 1,
            ..AddressPrefix::ALONE
        };
        let b = AddressPrefix {
            scatter: 2,
            ..AddressPrefix::ALONE
        };
        assert!(a.copy(CellId::new(9, 9), 3) < b.copy(CellId::new(0, 0), 0));
        assert_eq!(a.copy(CellId::new(1, 2), 0).prefix(), a);
    }
}
