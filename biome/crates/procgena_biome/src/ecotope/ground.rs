//! An Ecotope's ground layers, painted over the ground below it and
//! shaped by its placed Scatters.

use glam::DVec2;
use procgena_terrain::ground::{
    GroundMix, GroundOverlay, GroundPainter, GroundSite, GroundStack, MaterialCatalogue, Ramp,
    Reach, SiteDetail,
};

use super::{Ecotope, EcotopeError, EcotopePlacement, Footprints};

/// The seed kind of an Ecotope's ground layers: layer `key` of an Ecotope
/// placed with seed `seed` gets `child_seed(seed, GROUND_SEED_KIND, key)`
/// for its noise.
pub const GROUND_SEED_KIND: &str = "ecotope.child.ground";

/// One Scatter influence, ready to read.
struct Follow<'a> {
    /// `None` for a Scatter the placement lacks.
    footprints: Option<&'a Footprints>,
    reach: Reach,
    falloff: Ramp,
    invert: bool,
}

/// An Ecotope's ground: its layers laid over a base stack, following the
/// copies of one placement. Get one from [`Ecotope::ground`].
///
/// It paints as a [`GroundPainter`], so it can fill
/// [`GroundWeights`](procgena_terrain::ground::GroundWeights). Only copies
/// in the placement shape the ground, so beyond the placed area layers
/// that follow Scatters are absent.
pub struct EcotopeGround<'a> {
    base: &'a GroundStack,
    overlay: GroundOverlay,
    /// For each overlay layer from the bottom up, the Scatters it follows.
    follows: Vec<Vec<Follow<'a>>>,
}

impl Ecotope {
    /// This Ecotope's ground over `base` (a Biome's base ground, or
    /// [`GroundStack::bare`]), shaped by `placement`, which must be a
    /// placement of this Ecotope.
    ///
    /// Each layer's noise is seeded from the placement's seed and the
    /// layer's key (see [`GROUND_SEED_KIND`]). A layer that follows
    /// several Scatters takes the strongest influence at each point.
    ///
    /// A Scatter that `placement` lacks, such as one removed from the
    /// Ecotope, has no copies: a layer following only it is absent (or,
    /// turned round with `invert`, everywhere).
    ///
    /// The ground is drawing only: nothing is placed from it.
    ///
    /// # Errors
    ///
    /// [`EcotopeError::Ground`] if the layers are invalid or name a
    /// material `base`'s catalogue lacks.
    pub fn ground<'a>(
        &self,
        base: &'a GroundStack,
        placement: &'a EcotopePlacement,
    ) -> Result<EcotopeGround<'a>, EcotopeError> {
        let overlay = GroundOverlay::new(&self.ground, base, GROUND_SEED_KIND, placement.seed())
            .map_err(EcotopeError::Ground)?;
        let follows = overlay
            .layers()
            .map(|(_, layer)| {
                layer
                    .under
                    .iter()
                    .map(|influence| Follow {
                        footprints: placement
                            .get(influence.scatter.as_str())
                            .map(|placed| placed.footprints()),
                        reach: influence.reach,
                        falloff: influence.falloff,
                        invert: influence.invert,
                    })
                    .collect()
            })
            .collect();
        Ok(EcotopeGround {
            base,
            overlay,
            follows,
        })
    }

    /// The mix of materials at World position `p`, whose site is `site`:
    /// [`ground`](Self::ground) for a single point.
    ///
    /// # Errors
    ///
    /// As [`ground`](Self::ground).
    pub fn ground_at(
        &self,
        p: DVec2,
        site: &GroundSite,
        base: &GroundStack,
        placement: &EcotopePlacement,
    ) -> Result<GroundMix, EcotopeError> {
        Ok(self.ground(base, placement)?.paint(p, site))
    }
}

impl EcotopeGround<'_> {
    /// The influence at `p` of the Scatters the `i`th overlay layer follows.
    fn influence(&self, i: usize, p: DVec2) -> f32 {
        self.follows[i]
            .iter()
            .map(|follow| {
                let v = follow.footprints.map_or(0.0, |footprints| {
                    footprints.influence(p, follow.reach, &follow.falloff)
                });
                if follow.invert { 1.0 - v } else { v }
            })
            .fold(0.0, f32::max)
    }
}

impl GroundPainter for EcotopeGround<'_> {
    fn detail(&self) -> SiteDetail {
        self.base.detail().max(self.overlay.detail())
    }

    fn catalogue(&self) -> &MaterialCatalogue {
        self.base.catalogue()
    }

    fn paint(&self, p: DVec2, site: &GroundSite) -> GroundMix {
        self.base
            .paint_with(Some(&self.overlay), |i| self.influence(i, p), p, site)
    }
}
