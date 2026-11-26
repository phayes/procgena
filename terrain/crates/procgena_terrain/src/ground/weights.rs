//! Ground weights over a grid of texels, such as one terrain chunk: a
//! small palette of materials and how much of each lies at every texel.

use glam::{DVec2, UVec2};

use super::{
    GroundMix, GroundPainter, GroundSite, GroundSource, MaterialCatalogue, MaterialId, SiteDetail,
};
use crate::{CHUNK_CELLS, ChunkError, ChunkKey, FeatureTier};

/// Texels along each side of a chunk's weights, at every level: one on
/// every half cell of the chunk's mesh, the outer ones on its edges.
pub const GROUND_TEXELS: u32 = 2 * CHUNK_CELLS + 1;

/// The most materials one set of weights holds.
pub const PALETTE_SIZE: usize = 8;

/// The ground over a grid of texels as weights: up to [`PALETTE_SIZE`]
/// materials, and at each texel how much of each lies there, in 255ths
/// that add up to 255.
///
/// Build a terrain chunk's with [`chunk`](Self::chunk), or any rectangle's
/// with [`grid`](Self::grid).
///
/// Weights are for drawing only; nothing is placed from them.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundWeights {
    size: UVec2,
    palette: Vec<MaterialId>,
    /// Row by row along x, a weight per palette slot.
    texels: Vec<[u8; PALETTE_SIZE]>,
    dropped: usize,
}

impl GroundWeights {
    /// Paints chunk `key` into a [`GROUND_TEXELS`] × [`GROUND_TEXELS`]
    /// texture, with texel `(i, j)` at [`chunk_point`](Self::chunk_point).
    /// Samples `source` at `tier`, automatically requesting the painter's
    /// [`detail`](GroundPainter::detail) so its conditions have the data
    /// they need, where the source supports it.
    ///
    /// Neighbouring chunks of one level share their edge texels. Where
    /// neither chunk had to drop materials (see [`dropped`](Self::dropped)),
    /// those shared texels have equal weights for every material.
    ///
    /// If more than [`PALETTE_SIZE`] materials lie in the chunk, it keeps
    /// those with the most weight over the chunk, and gives each other
    /// material's weight to the kept material nearest to it in colour.
    ///
    /// # Errors
    ///
    /// [`ChunkError::NoGround`] if any texel has no ground.
    pub fn chunk(
        painter: &(impl GroundPainter + ?Sized),
        source: &(impl GroundSource + ?Sized),
        key: ChunkKey,
        tier: FeatureTier,
    ) -> Result<Self, ChunkError> {
        let size = UVec2::splat(GROUND_TEXELS);
        Self::build(painter, source, size, tier, |i, j| {
            Self::chunk_point(key, i, j)
        })
    }

    /// Paints the rectangle from `min` to `max` (world x and z, metres)
    /// with `size` texels along x and z. Samples `source` at `tier`,
    /// automatically requesting the painter's [`detail`](GroundPainter::detail).
    /// Texels are evenly spaced and include the rectangle's edges:
    /// texel `(i, j)` lies at `min + (max − min) × (i, j) / (size − 1)`.
    ///
    /// Materials past [`PALETTE_SIZE`] are dropped as in
    /// [`chunk`](Self::chunk).
    ///
    /// # Errors
    ///
    /// [`ChunkError::NoGround`] if any texel has no ground.
    ///
    /// # Panics
    ///
    /// If `size` is below 2 along either axis.
    pub fn grid(
        painter: &(impl GroundPainter + ?Sized),
        source: &(impl GroundSource + ?Sized),
        min: DVec2,
        max: DVec2,
        size: UVec2,
        tier: FeatureTier,
    ) -> Result<Self, ChunkError> {
        assert!(size.min_element() >= 2, "a grid needs 2 texels a side");
        let last = (size - 1).as_dvec2();
        Self::build(painter, source, size, tier, |i, j| {
            min + (max - min) * DVec2::new(f64::from(i), f64::from(j)) / last
        })
    }

    /// Paints `size` texels, texel `(i, j)` at `point(i, j)`.
    fn build(
        painter: &(impl GroundPainter + ?Sized),
        source: &(impl GroundSource + ?Sized),
        size: UVec2,
        tier: FeatureTier,
        point: impl Fn(u32, u32) -> DVec2 + Sync,
    ) -> Result<Self, ChunkError> {
        Ok(GroundGrid::sample(source, size, tier, painter.detail(), point)?.paint(painter))
    }

    /// Weights from one mix per texel, row by row along x.
    fn from_mixes(size: UVec2, mixes: &[GroundMix], catalogue: &MaterialCatalogue) -> Self {
        // Each material's total over the chunk, added in texel order.
        let mut totals = vec![0.0_f64; catalogue.len()];
        for mix in mixes {
            for (id, w) in mix.iter() {
                totals[usize::from(id.0)] += f64::from(w);
            }
        }
        let present: Vec<MaterialId> = (0..totals.len())
            .filter(|&i| totals[i] > 0.0)
            .map(|i| MaterialId(i as u16))
            .collect();

        // Keep the heaviest materials, ties to the lower id, and send each
        // dropped one to the kept one nearest in colour.
        let mut ranked = present.clone();
        ranked.sort_by(|a, b| {
            totals[usize::from(b.0)]
                .total_cmp(&totals[usize::from(a.0)])
                .then(a.cmp(b))
        });
        ranked.truncate(PALETTE_SIZE);
        let mut palette = ranked;
        palette.sort();
        let dropped = present.len() - palette.len();
        let colour = |id: MaterialId| catalogue.get(id).1.colour;
        let slot_of = |id: MaterialId| -> usize {
            if let Ok(slot) = palette.binary_search(&id) {
                return slot;
            }
            let c = colour(id);
            let distance = |k: MaterialId| {
                colour(k)
                    .iter()
                    .zip(c)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>()
            };
            (0..palette.len())
                .min_by(|&a, &b| distance(palette[a]).total_cmp(&distance(palette[b])))
                .expect("a chunk with materials keeps at least one")
        };
        let slots: Vec<usize> = (0..catalogue.len())
            .map(|i| {
                let id = MaterialId(i as u16);
                if totals[i] > 0.0 { slot_of(id) } else { 0 }
            })
            .collect();

        // Quantise each texel.
        let texels = mixes
            .iter()
            .map(|mix| {
                let mut weights = [0.0_f32; PALETTE_SIZE];
                for (id, w) in mix.iter() {
                    weights[slots[usize::from(id.0)]] += w;
                }
                quantise(&weights[..palette.len()])
            })
            .collect();
        Self {
            size,
            palette,
            texels,
            dropped,
        }
    }

    /// The number of texels along x and z.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// The materials, by id, one per weight slot.
    pub fn palette(&self) -> &[MaterialId] {
        &self.palette
    }

    /// How many materials lay on the texels beyond the [`PALETTE_SIZE`]
    /// kept, 0 if none were dropped.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// The weights at every texel, row by row along x, as
    /// [`texel`](Self::texel) gives them.
    pub fn texels(&self) -> &[[u8; PALETTE_SIZE]] {
        &self.texels
    }

    /// The weights at texel `(i, j)`, in 255ths, one per palette slot; the
    /// slots past the palette's length are 0.
    ///
    /// # Panics
    ///
    /// If `(i, j)` is outside [`size`](Self::size).
    pub fn texel(&self, i: u32, j: u32) -> [u8; PALETTE_SIZE] {
        assert!(i < self.size.x && j < self.size.y, "texel out of range");
        self.texels[(j * self.size.x + i) as usize]
    }

    /// The weight of `material` at texel `(i, j)`, in 255ths; 0 if it isn't
    /// in the palette.
    ///
    /// # Panics
    ///
    /// If `(i, j)` is outside [`size`](Self::size).
    pub fn weight(&self, i: u32, j: u32, material: MaterialId) -> u8 {
        self.palette
            .binary_search(&material)
            .map_or(0, |slot| self.texel(i, j)[slot])
    }

    /// The colour at every texel, row by row along x, as linear RGB from
    /// `catalogue` (the painter's).
    pub fn colours(&self, catalogue: &MaterialCatalogue) -> Vec<[f32; 3]> {
        let palette: Vec<[f32; 3]> = self
            .palette
            .iter()
            .map(|&id| catalogue.get(id).1.colour)
            .collect();
        self.texels
            .iter()
            .map(|texel| {
                let mut colour = [0.0; 3];
                for (w, c) in texel.iter().zip(&palette) {
                    for k in 0..3 {
                        colour[k] += c[k] * f32::from(*w) / 255.0;
                    }
                }
                colour
            })
            .collect()
    }

    /// The world position (x and z, metres) of texel `(i, j)` of chunk
    /// `key`'s weights: `i` half steps along x and `j` along z from the
    /// chunk's smallest corner (see [`ChunkKey::step`]).
    pub fn chunk_point(key: ChunkKey, i: u32, j: u32) -> DVec2 {
        key.half_point(i64::from(i), i64::from(j))
    }
}

/// Sampled ground independent of its paint. Retain this across material edits
/// to repaint without querying terrain, or upload its sites for GPU painting.
#[derive(Debug, Clone)]
pub struct GroundGrid {
    size: UVec2,
    detail: SiteDetail,
    points: Vec<DVec2>,
    sites: Vec<GroundSite>,
}

impl GroundGrid {
    /// Samples chunk `key` at `tier` for later painting, using the same
    /// texel positions as [`GroundWeights::chunk`]. Request at least the
    /// [`detail`](GroundPainter::detail) of every painter that will use
    /// the grid. Higher detail costs more to sample but allows painters
    /// to use additional data without sampling again.
    /// Sources may leave unsupported data unknown; see [`GroundSource::site_at`].
    ///
    /// # Errors
    /// Returns [`ChunkError::NoGround`] if any site has a non-finite height.
    pub fn chunk(
        source: &(impl GroundSource + ?Sized),
        key: ChunkKey,
        tier: FeatureTier,
        detail: SiteDetail,
    ) -> Result<Self, ChunkError> {
        Self::sample(source, UVec2::splat(GROUND_TEXELS), tier, detail, |i, j| {
            GroundWeights::chunk_point(key, i, j)
        })
    }

    fn sample(
        source: &(impl GroundSource + ?Sized),
        size: UVec2,
        tier: FeatureTier,
        detail: SiteDetail,
        point: impl Fn(u32, u32) -> DVec2,
    ) -> Result<Self, ChunkError> {
        let points: Vec<_> = (0..size.y)
            .flat_map(|j| (0..size.x).map(move |i| (i, j)))
            .map(|(i, j)| point(i, j))
            .collect();
        let sites = source.sites_at(&points, tier, detail);
        assert_eq!(sites.len(), points.len(), "one site per point");
        for (p, site) in points.iter().zip(&sites) {
            if !site.height.is_finite() {
                return Err(ChunkError::NoGround(*p));
            }
        }
        Ok(Self {
            size,
            detail,
            points,
            sites,
        })
    }

    /// The detail requested when this grid was sampled. This does not
    /// guarantee that the source supplied every field. Sample a new grid
    /// before using a painter that requires higher [`detail`](GroundPainter::detail).
    pub fn detail(&self) -> SiteDetail {
        self.detail
    }

    /// Paints the cached sites without querying terrain again, using the
    /// same palette selection and weight rounding as [`GroundWeights::chunk`].
    /// Does not check or upgrade the grid's [`detail`](Self::detail).
    /// Missing data stays unknown, so layer conditions that need it have
    /// no effect.
    pub fn paint(&self, painter: &(impl GroundPainter + ?Sized)) -> GroundWeights {
        let located: Vec<_> = self.points.iter().zip(&self.sites).collect();
        let mixes: Vec<_> = located
            .iter()
            .map(|&(p, site)| painter.paint(*p, site))
            .collect();
        GroundWeights::from_mixes(self.size, &mixes, painter.catalogue())
    }

    /// Number of samples along x and z.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// World positions, row by row along x.
    pub fn points(&self) -> &[DVec2] {
        &self.points
    }

    /// Sites in the same order as [`points`](Self::points).
    pub fn sites(&self) -> &[GroundSite] {
        &self.sites
    }
}

/// `weights` (adding up to 1) in 255ths adding up to 255: each rounded
/// down, then the leftover 255ths to the largest remainders, ties to the
/// lower slot.
fn quantise(weights: &[f32]) -> [u8; PALETTE_SIZE] {
    let mut out = [0u8; PALETTE_SIZE];
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return out;
    }
    let scaled: Vec<f32> = weights.iter().map(|w| w / total * 255.0).collect();
    let mut sum = 0u32;
    for (o, s) in out.iter_mut().zip(&scaled) {
        *o = s.floor().min(255.0) as u8;
        sum += u32::from(*o);
    }
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| {
        (scaled[b] - scaled[b].floor())
            .total_cmp(&(scaled[a] - scaled[a].floor()))
            .then(a.cmp(&b))
    });
    for &slot in order
        .iter()
        .cycle()
        .take(255usize.saturating_sub(sum as usize))
    {
        out[slot] += 1;
    }
    out
}
