//! The `Scatter` trait every Scatter implements, and the copies it places.

use glam::{Affine3A, DVec2, Vec3A};

use super::ScatterInput;
use super::footprint::{Disc, Footprints};
use super::lattice::{Fixed2, FixedRect, Lattice, to_fixed};
use super::mask::{FOOTPRINT_TEXEL_SHIFT, MaskLayer};
use crate::address::CopyAddress;
use crate::specimen_id::SpecimenId;

/// Why a Scatter could not place its copies, such as invalid settings or an
/// area that asks for too many copies.
pub type ScatterError = Box<dyn std::error::Error + Send + Sync>;

/// How far a Scatter's placement reaches across chunk borders, in metres.
///
/// To place the copies of an area exactly as the whole World would, a
/// Scatter looks at candidates up to [`margin`](Self::margin) beyond it. The
/// Scatters below it in an Ecotope need its copies up to
/// [`footprint`](Self::footprint) plus [`displacement`](Self::displacement)
/// beyond their own area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    /// Rounds of spacing, `d`: odd, 5 by default.
    pub rounds: u32,
    /// The largest spacing between two copies, `R`.
    pub spacing: f32,
    /// How far a copy can stand from its lattice cell's corner, `J`: the
    /// cell's width plus any offset, such as a cluster member's.
    pub displacement: f32,
    /// How far a copy's footprint reaches from where it stands, `F`.
    pub footprint: f32,
}

impl Reach {
    /// The spacing dependency `S = d × R`: a copy's survival depends only on
    /// candidates this close.
    pub fn dependency(&self) -> f32 {
        self.rounds as f32 * self.spacing
    }

    /// How far beyond an area a Scatter must look at candidates to place
    /// that area exactly: `S + 2J`.
    pub fn margin(&self) -> f32 {
        self.dependency() + 2.0 * self.displacement
    }

    /// The cells to look at to place `area` exactly: `area` grown by
    /// [`margin`](Self::margin).
    pub fn window(&self, area: FixedRect) -> FixedRect {
        area.grow(to_fixed(f64::from(self.margin())))
    }

    /// [`spacing`](Self::spacing) in fixed-point units, the `reach` to pass
    /// to [`keep_spaced`](super::keep_spaced).
    pub fn spacing_fixed(&self) -> i64 {
        to_fixed(f64::from(self.spacing))
    }
}

/// Where a Scatter's mask for the Scatters below it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskSource {
    /// From its placed copies' footprints, the same at every distance.
    Copies,
    /// From its density field, without placing copies: for Scatters too
    /// dense to place out to a long range, such as grass.
    Density,
}

/// One specimen a Scatter draws its copies from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScatterSlot {
    /// The mix entry it belongs to: its position in the Scatter's list of
    /// species or classes, 0 for a Scatter of one kind.
    pub entry: u16,
    /// The specimen. Copies with equal ids share one built specimen.
    pub specimen: SpecimenId,
}

/// One copy a Scatter placed, in the form every Scatter shares.
#[derive(Clone, Debug, PartialEq)]
pub struct ScatterCopy {
    /// Names the copy anywhere in the World.
    pub address: CopyAddress,
    /// Which specimen to draw: its position in the Scatter's
    /// [`slots`](Scatter::slots).
    pub slot: u32,
    /// Puts the specimen in place, in metres, relative to the centre of the
    /// area it was placed over.
    pub transform: Affine3A,
    /// The ground the copy takes up, which Scatters below it keep clear of.
    pub footprint: Vec<Disc>,
}

impl ScatterCopy {
    /// The same copy, with its transform made relative to World position
    /// `to` instead of `from`.
    pub fn moved(mut self, from: DVec2, to: DVec2) -> Self {
        let shift = (from - to).as_vec2();
        self.transform.translation += Vec3A::new(shift.x, 0.0, shift.y);
        self
    }
}

/// A Scatter: many copies of one kind of asset spread over the ground.
///
/// Every Scatter places the same way: candidates on a [`Lattice`] fixed to
/// the World, kept by rank below the local density, cleared from the
/// footprints of the Scatters above it, stood on the ground and spaced out.
/// Placing an area gives the copies whose lattice cell's corner lies in the
/// area, so the chunks of an area give the same copies, with the same
/// addresses, in any order.
///
/// The trait can be used as `dyn Scatter`, so an Ecotope can hold Scatters
/// of any kind, including your own.
pub trait Scatter: Send + Sync {
    /// The lattice its candidates sit on.
    fn lattice(&self) -> Lattice;

    /// How far its placement reaches across chunk borders.
    fn reach(&self) -> Reach;

    /// How far away, in metres, its largest copies are still drawn. It is
    /// placed no further than the Scatters below it need, and drawn no
    /// further than this.
    fn draw_range(&self) -> f32;

    /// Where its mask for the Scatters below comes from. The provided
    /// [`mask`](Self::mask) builds it from copies; a Scatter whose source is
    /// [`MaskSource::Density`] overrides it.
    fn mask_source(&self) -> MaskSource {
        MaskSource::Copies
    }

    /// The specimens its copies are drawn from, in a World with seed
    /// `world_seed`. [`ScatterCopy::slot`] indexes this list.
    fn slots(&self, world_seed: u32) -> Vec<ScatterSlot>;

    /// Places the copies whose lattice cell's corner lies in `input`'s area.
    ///
    /// The same input always gives the same copies, and an area split into
    /// smaller areas gives the same copies, with the same addresses and
    /// places, as the whole.
    ///
    /// # Errors
    ///
    /// Returns an error when the Scatter's settings are invalid, or the area
    /// asks for more copies than the Scatter allows.
    fn place(&self, input: &ScatterInput<'_>) -> Result<Vec<ScatterCopy>, ScatterError>;

    /// The copy at `address`, or `None` where its spot is empty (masked
    /// out, thinned or too steep) or the address names another Scatter.
    ///
    /// The copy's transform is relative to `input.area.centre`; the area's
    /// size is not used. This places the address's cell on its own, which
    /// costs about as much as a small area; to look up many addresses, use a
    /// [`ChunkCache`](super::ChunkCache).
    ///
    /// # Errors
    ///
    /// As [`place`](Self::place).
    fn resolve(
        &self,
        input: &ScatterInput<'_>,
        address: &CopyAddress,
    ) -> Result<Option<ScatterCopy>, ScatterError> {
        if address.prefix() != input.prefix {
            return Ok(None);
        }
        let corner = self.lattice().corner(address.cell);
        let cell = FixedRect::new(corner, Fixed2::new(corner.x + 1, corner.z + 1)).area();
        let copies = self.place(&input.over(cell))?;
        Ok(copies
            .into_iter()
            .find(|copy| copy.address == *address)
            .map(|copy| copy.moved(cell.centre, input.area.centre)))
    }

    /// The footprints of the copies placed over `input`'s area grown by
    /// [`footprint`](Reach::footprint) plus
    /// [`displacement`](Reach::displacement): every footprint that reaches
    /// into the area, and some near it. Draws nothing.
    ///
    /// # Errors
    ///
    /// As [`place`](Self::place).
    fn footprints(&self, input: &ScatterInput<'_>) -> Result<Footprints, ScatterError> {
        let reach = self.reach();
        let grow = to_fixed(f64::from(reach.footprint + reach.displacement));
        let copies = self.place(&input.over(input.area.bounds().grow(grow).area()))?;
        Ok(copies
            .iter()
            .flat_map(|copy| copy.footprint.iter().copied())
            .collect())
    }

    /// The mask it leaves over `input`'s area for the Scatters below it: 0
    /// on footprint texels (0.5 m), 255 elsewhere. Draws nothing.
    ///
    /// # Errors
    ///
    /// As [`place`](Self::place).
    fn mask(&self, input: &ScatterInput<'_>) -> Result<MaskLayer, ScatterError> {
        let footprints = self.footprints(input)?;
        let mut layer = MaskLayer::full(input.area.bounds(), FOOTPRINT_TEXEL_SHIFT);
        layer.stamp(footprints.discs());
        Ok(layer)
    }
}
