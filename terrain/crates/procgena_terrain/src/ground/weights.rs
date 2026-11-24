//! Ground weights for one terrain chunk: a small palette of materials and
//! how much of each lies at every texel.

use glam::DVec2;

use super::{GroundMix, GroundPainter, GroundSource, MaterialCatalogue, MaterialId};
use crate::chunk::rows;
use crate::{CHUNK_CELLS, ChunkError, ChunkKey, ChunkStage};

/// Texels along each side of a chunk's weights, at every level: one on
/// every half cell of the chunk's mesh, the outer ones on its edges.
pub const GROUND_TEXELS: u32 = 2 * CHUNK_CELLS + 1;

/// The most materials one chunk's weights hold.
pub const PALETTE_SIZE: usize = 8;

/// The ground of one chunk as weights: up to [`PALETTE_SIZE`] materials,
/// and at each of [`GROUND_TEXELS`]² texels how much of each lies there,
/// in 255ths that add up to 255.
///
/// Texel `(i, j)` lies `i` half steps along x and `j` along z from the
/// chunk's smallest corner (see [`ChunkKey::step`]), so neighbouring chunks
/// of one level share their edge texels. Where neither chunk had to drop
/// materials (see [`dropped`](Self::dropped)), those shared texels have
/// equal weights for every material.
///
/// Weights are for drawing only; nothing is placed from them.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkWeights {
    key: ChunkKey,
    palette: Vec<MaterialId>,
    /// Row by row along x, a weight per palette slot.
    texels: Vec<[u8; PALETTE_SIZE]>,
    dropped: usize,
}

impl ChunkWeights {
    /// Paints chunk `key` with `painter`, reading sites from `source` at
    /// `stage`.
    ///
    /// If more than [`PALETTE_SIZE`] materials lie in the chunk, it keeps
    /// those with the most weight over the chunk, and gives each other
    /// material's weight to the kept material nearest to it in colour.
    ///
    /// # Errors
    ///
    /// [`ChunkError::NoGround`] if any texel has no ground.
    pub fn build(
        painter: &(impl GroundPainter + ?Sized),
        source: &(impl GroundSource + ?Sized),
        key: ChunkKey,
        stage: ChunkStage,
    ) -> Result<Self, ChunkError> {
        let n = i64::from(GROUND_TEXELS);

        // Paint every texel, row by row.
        let row = |j: i64| -> Result<Vec<GroundMix>, ChunkError> {
            (0..n)
                .map(|i| {
                    let p = key.half_point(i, j);
                    let site = source.site(p, stage);
                    if site.height.is_finite() {
                        Ok(painter.paint(p, &site))
                    } else {
                        Err(ChunkError::NoGround(p))
                    }
                })
                .collect()
        };
        let mixes = rows(n, row)?.concat();
        Ok(Self::from_mixes(key, &mixes, painter.catalogue()))
    }

    /// Weights from one mix per texel, row by row along x.
    fn from_mixes(key: ChunkKey, mixes: &[GroundMix], catalogue: &MaterialCatalogue) -> Self {
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
            key,
            palette,
            texels,
            dropped,
        }
    }

    /// The chunk the weights are for.
    pub fn key(&self) -> ChunkKey {
        self.key
    }

    /// The chunk's materials, by id, one per weight slot.
    pub fn palette(&self) -> &[MaterialId] {
        &self.palette
    }

    /// How many materials lay in the chunk beyond the [`PALETTE_SIZE`]
    /// kept, 0 if none were dropped.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// The weights at texel `(i, j)`, in 255ths, one per palette slot; the
    /// slots past the palette's length are 0.
    ///
    /// # Panics
    ///
    /// If `i` or `j` is not below [`GROUND_TEXELS`].
    pub fn texel(&self, i: u32, j: u32) -> [u8; PALETTE_SIZE] {
        assert!(i < GROUND_TEXELS && j < GROUND_TEXELS, "texel out of range");
        self.texels[(j * GROUND_TEXELS + i) as usize]
    }

    /// The weight of `material` at texel `(i, j)`, in 255ths; 0 if it isn't
    /// in the palette.
    ///
    /// # Panics
    ///
    /// If `i` or `j` is not below [`GROUND_TEXELS`].
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
    /// `key`.
    pub fn texel_point(key: ChunkKey, i: u32, j: u32) -> DVec2 {
        key.half_point(i64::from(i), i64::from(j))
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
