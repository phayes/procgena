//! The compatibility weights as a dense table of logs, by catalogue index.

use crate::options::BiomeCompatibility;

/// The log of every pair's compatibility weight, by catalogue index, the
/// same either way round.
#[derive(Debug, Clone)]
pub(crate) struct Affinity {
    /// The number of Biomes in the catalogue.
    n: usize,
    /// `ln_weight[a * n + b]`.
    ln_weight: Vec<f64>,
}

impl Affinity {
    /// The table for a catalogue of `n` Biomes, from checked settings
    /// `compatibility`, with `index` turning a key into its catalogue
    /// index.
    ///
    /// # Errors
    ///
    /// The first key `index` refuses, with its error.
    pub fn new<E>(
        n: usize,
        compatibility: &BiomeCompatibility,
        mut index: impl FnMut(&str) -> Result<usize, E>,
    ) -> Result<Self, E> {
        let mut ln_weight = vec![compatibility.default.ln(); n * n];
        for pair in &compatibility.pairs {
            let (a, b) = (index(&pair.a)?, index(&pair.b)?);
            let w = pair.weight.ln();
            ln_weight[a * n + b] = w;
            ln_weight[b * n + a] = w;
        }
        Ok(Self { n, ln_weight })
    }

    /// The log of the weight of catalogue Biomes `a` and `b` side by side.
    pub fn ln(&self, a: usize, b: usize) -> f64 {
        self.ln_weight[a * self.n + b]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::CompatibilityPair;

    #[test]
    fn pairs_are_symmetric_and_the_rest_default() {
        let keys = ["a", "b", "c"];
        let compatibility = BiomeCompatibility {
            default: 0.5,
            pairs: vec![
                CompatibilityPair {
                    a: "c".into(),
                    b: "a".into(),
                    weight: 2.0,
                },
                CompatibilityPair {
                    a: "b".into(),
                    b: "b".into(),
                    weight: 3.0,
                },
            ],
        };
        let index = |k: &str| keys.iter().position(|&x| x == k).ok_or(k.to_owned());
        let table = Affinity::new(3, &compatibility, index).unwrap();
        assert_eq!(table.ln(0, 2), 2f64.ln());
        assert_eq!(table.ln(2, 0), 2f64.ln());
        assert_eq!(table.ln(1, 1), 3f64.ln());
        for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 2), (2, 1), (2, 2)] {
            assert_eq!(table.ln(a, b), 0.5f64.ln(), "{a} {b}");
        }
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let compatibility = BiomeCompatibility {
            default: 1.0,
            pairs: vec![CompatibilityPair {
                a: "a".into(),
                b: "nope".into(),
                weight: 1.0,
            }],
        };
        let index = |k: &str| if k == "a" { Ok(0) } else { Err(k.to_owned()) };
        assert_eq!(Affinity::new(1, &compatibility, index).unwrap_err(), "nope");
    }
}
