//! Addresses that name one copy anywhere in a World.
//!
//! A [`CopyAddress`] is plain integers: the region, the Ecotope and the
//! Scatter that placed the copy, the Scatter's lattice cell and the copy's
//! number within that cell. It stays the same when chunks load in another
//! order, when other Scatters are added, and after a reload, so it is what
//! edits, picking and promotion use.
//!
//! An [`Address`] goes further, into the copy's structure: a copy address
//! followed by up to four [`PartStep`]s, such as `fruit 17 / item 2`, each
//! pinned to the specimen it counts parts of.

use crate::SpecimenId;
use crate::hash::salt;

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

    /// The address of this copy as a whole, with no part steps.
    pub const fn whole(self) -> Address {
        Address::new(self)
    }
}

/// The most part steps an [`Address`] holds.
pub const MAX_PART_STEPS: usize = 4;

/// One step into a specimen's structure, such as `fruit 17` on a tree or
/// `item 2` on a fruit cluster, pinned to the specimen it indexes.
///
/// `index` counts the specimen's structure, not its mesh, so a step names
/// the same part at every level of detail. If the specimen at that point
/// of a path is no longer `pin` (its pool's budget or its options
/// changed), the step and every step after it no longer apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartStep {
    /// The salt of the part kind's frozen code key, such as
    /// `"tree.part.fruit"`; see [`PartStep::kind_of`].
    pub kind: u32,
    /// The part's number within its kind on the specimen.
    pub index: u32,
    /// The specimen whose structure `index` counts.
    pub pin: SpecimenId,
}

impl PartStep {
    /// Part `index` of kind `kind` (a code key such as `"fruit.part.item"`)
    /// on the specimen `pin`.
    pub const fn new(kind: &str, index: u32, pin: SpecimenId) -> Self {
        Self {
            kind: Self::kind_of(kind),
            index,
            pin,
        }
    }

    /// The salt that names part kind `key` in a step. Each asset has one
    /// frozen key per kind of part, such as `"tree.part.fruit"`.
    pub const fn kind_of(key: &str) -> u32 {
        salt(key)
    }
}

/// Names a copy, or a part of one: a [`CopyAddress`] followed by up to
/// [`MAX_PART_STEPS`] part steps, such as
/// `(orchard, apples, cell (4031, -77) #0) / fruit 17 / item 2`.
///
/// Addresses compare and sort by the copy first, then by the steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address {
    /// The copy the path starts from.
    pub copy: CopyAddress,
    len: u8,
    parts: [Option<PartStep>; MAX_PART_STEPS],
}

impl Address {
    /// The copy `copy` as a whole.
    pub const fn new(copy: CopyAddress) -> Self {
        Self {
            copy,
            len: 0,
            parts: [None; MAX_PART_STEPS],
        }
    }

    /// This address followed by `step`, or `None` when it already holds
    /// [`MAX_PART_STEPS`] steps.
    #[must_use]
    pub const fn then(mut self, step: PartStep) -> Option<Self> {
        if self.len as usize == MAX_PART_STEPS {
            return None;
        }
        self.parts[self.len as usize] = Some(step);
        self.len += 1;
        Some(self)
    }

    /// The part steps, in order from the copy inward.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = PartStep> + '_ {
        self.parts[..self.len as usize]
            .iter()
            .map(|step| step.expect("steps below len are set"))
    }

    /// The address with its last step removed, or `None` for a whole copy.
    #[must_use]
    pub const fn parent(mut self) -> Option<Self> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.parts[self.len as usize] = None;
        Some(self)
    }
}

impl From<CopyAddress> for Address {
    fn from(copy: CopyAddress) -> Self {
        Self::new(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GeneratorKind, OptionsHash, PoolId};

    fn pin(slot: u32) -> SpecimenId {
        let kind = GeneratorKind::from_key("tree.generator.kind");
        SpecimenId::new(1, PoolId::new(kind, OptionsHash(7)), slot)
    }

    #[test]
    fn part_paths_compose_onto_a_copy() {
        let copy = AddressPrefix::ALONE.copy(CellId::new(4031, -77), 0);
        let fruit = PartStep::new("tree.part.fruit", 17, pin(0));
        let item = PartStep::new("fruit.part.item", 2, pin(1));
        let address = copy.whole().then(fruit).unwrap().then(item).unwrap();
        assert_eq!(address.copy, copy);
        assert_eq!(address.parts().collect::<Vec<_>>(), [fruit, item]);
        assert_eq!(address.parent().unwrap().parts().len(), 1);
        assert_eq!(address.parent().unwrap().parent(), Some(copy.whole()));
        assert_eq!(copy.whole().parent(), None);
        assert_eq!(fruit.kind, crate::hash::salt("tree.part.fruit"));
    }

    #[test]
    fn a_path_holds_at_most_four_steps() {
        let step = PartStep::new("tree.part.fruit", 0, pin(0));
        let mut address = Address::from(CopyAddress::default());
        for _ in 0..MAX_PART_STEPS {
            address = address.then(step).unwrap();
        }
        assert_eq!(address.then(step), None);
    }

    #[test]
    fn addresses_with_parts_sort_after_their_copy() {
        let copy = CopyAddress::default();
        let part = copy
            .whole()
            .then(PartStep::new("tree.part.fruit", 0, pin(0)))
            .unwrap();
        assert!(copy.whole() < part);
        assert_ne!(copy.whole(), part);
    }

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
