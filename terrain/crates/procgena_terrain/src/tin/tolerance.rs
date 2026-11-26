//! How closely a canonical mesh follows the ground.

use procgena_core::ContentHasher;

/// How closely a canonical mesh follows one kind of ground, such as open
/// land or a river's bed and banks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DomainTolerance {
    /// The most the mesh may differ from the ground in height at a probe,
    /// in metres. Positive.
    pub height: f64,
    /// The most the mesh's slope may differ from the ground's at a probe,
    /// as rise over run, or `None` to leave slope unchecked. Positive.
    pub gradient: Option<f64>,
    /// The longest a triangle edge may be, in metres. Positive.
    pub max_edge: f64,
}

impl DomainTolerance {
    /// Whether every value is in range.
    pub fn is_valid(&self) -> bool {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        positive(self.height) && self.gradient.is_none_or(positive) && positive(self.max_edge)
    }
}

/// How densely the ground is sampled to build a canonical mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TinSampling {
    /// Only as densely as the ground needs: every 8 m where it is close to
    /// flat, down to every metre where it bends, and every metre near
    /// water and roads. Far fewer reads, but ground that bends sharply
    /// within a few metres and looks flat at the samples can be missed by
    /// a little more than the height tolerance.
    Adaptive,
    /// Every metre: the mesh is within its height tolerance at every 1 m
    /// lattice point, at several times the cost on gentle ground.
    EveryMetre,
}

/// How closely a canonical mesh follows the ground, and how many vertices
/// it may spend doing so.
///
/// Neighbouring chunks meet exactly only if they were built with the same
/// tolerance, so a world uses one tolerance for all its chunks. Named
/// levels, from the closest to the coarsest: [`DENSE`](Self::DENSE),
/// [`GENERAL`](Self::GENERAL) (the default), [`LOW_POLY`](Self::LOW_POLY)
/// and [`VERY_LOW_POLY`](Self::VERY_LOW_POLY). All of them hold river and
/// pond beds and banks to the same water tolerance.
///
/// Ground within [`water_band`](Self::water_band) of a river's or pond's
/// water follows [`water`](Self::water); all other ground follows
/// [`general`](Self::general).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TinTolerance {
    /// Open land.
    pub general: DomainTolerance,
    /// River and pond beds and banks.
    pub water: DomainTolerance,
    /// How far beyond the water's edge the banks reach, in metres: ground
    /// this close to water follows [`water`](Self::water). Not negative.
    pub water_band: f64,
    /// The furthest a curved feature line (such as a river's bank on a
    /// bend, or a pond's shore) may stray from the straight edges that
    /// follow it in the mesh, in metres. Positive.
    pub chord: f64,
    /// The most vertices one chunk may have, border included. Where it runs
    /// out, the chunk records the places it misses its height tolerance.
    /// The border and the feature lines are always kept, even past the
    /// budget.
    pub vertex_budget: usize,
    /// How densely the ground is sampled.
    pub sampling: TinSampling,
}

impl TinTolerance {
    /// Open land within half a metre with edges up to 32 m, and river and
    /// pond beds and banks (5 m beyond the water) within 10 cm and a slope
    /// of 0.03 with edges up to 4 m; curved lines within 5 cm, and at most
    /// 4,096 vertices a chunk. Sampled [`Adaptive`](TinSampling::Adaptive)ly.
    pub const GENERAL: Self = Self {
        general: DomainTolerance {
            height: 0.5,
            gradient: None,
            max_edge: 32.0,
        },
        water: DomainTolerance {
            height: 0.1,
            gradient: Some(0.03),
            max_edge: 4.0,
        },
        water_band: 5.0,
        chord: 0.05,
        vertex_budget: 4096,
        sampling: TinSampling::Adaptive,
    };

    /// [`GENERAL`](Self::GENERAL) sampled every metre: every 1 m lattice
    /// point is within the tolerance, at several times the cost on gentle
    /// ground.
    pub const DENSE: Self = Self {
        sampling: TinSampling::EveryMetre,
        ..Self::GENERAL
    };

    /// Open land within 1 m with edges up to 64 m (a chunk's width), for
    /// a low-poly look; water as in [`GENERAL`](Self::GENERAL).
    pub const LOW_POLY: Self = Self {
        general: DomainTolerance {
            height: 1.0,
            gradient: None,
            max_edge: 64.0,
        },
        ..Self::GENERAL
    };

    /// Open land within 3 m with edges up to 64 m, for a strongly faceted
    /// look; water as in [`GENERAL`](Self::GENERAL).
    pub const VERY_LOW_POLY: Self = Self {
        general: DomainTolerance {
            height: 3.0,
            gradient: None,
            max_edge: 64.0,
        },
        ..Self::GENERAL
    };

    /// The named level `name`: `"dense"`, `"general"`, `"low_poly"` or
    /// `"very_low_poly"`, or `None` for any other name.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "dense" => Some(Self::DENSE),
            "general" => Some(Self::GENERAL),
            "low_poly" => Some(Self::LOW_POLY),
            "very_low_poly" => Some(Self::VERY_LOW_POLY),
            _ => None,
        }
    }

    /// A hash of every value, equal on every platform for equal
    /// tolerances. Different tolerances hash differently.
    pub fn content_hash(&self) -> u64 {
        let mut h = ContentHasher::new();
        h.write_str("procgena_terrain::TinTolerance");
        for domain in [&self.general, &self.water] {
            h.write_f64(domain.height);
            h.write_bool(domain.gradient.is_some());
            h.write_f64(domain.gradient.unwrap_or(0.0));
            h.write_f64(domain.max_edge);
        }
        h.write_f64(self.water_band);
        h.write_f64(self.chord);
        h.write_usize(self.vertex_budget);
        h.write_u32(match self.sampling {
            TinSampling::Adaptive => 0,
            TinSampling::EveryMetre => 1,
        });
        h.finish()
    }

    /// Whether every value is in range.
    pub fn is_valid(&self) -> bool {
        self.general.is_valid()
            && self.water.is_valid()
            && self.water_band.is_finite()
            && self.water_band >= 0.0
            && self.chord.is_finite()
            && self.chord > 0.0
            && self.vertex_budget >= 4
    }
}

impl Default for TinTolerance {
    fn default() -> Self {
        Self::GENERAL
    }
}
