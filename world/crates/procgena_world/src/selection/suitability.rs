//! How much each Biome free to be drawn likes the field values at a region.

use crate::options::{BiomeSuitability, FieldPreference};
use crate::world::FieldValues;

/// One Biome free to be drawn, ready to score.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    /// The Biome's index in the World's catalogue.
    pub biome: usize,
    /// The natural log of its prevalence.
    ln_prevalence: f64,
    /// Its preferences for elevation (at reference scale), ruggedness and
    /// moisture.
    preferences: [Option<FieldPreference>; 3],
}

impl Candidate {
    /// The candidate for catalogue index `biome` with settings `suits`,
    /// already checked.
    pub fn new(biome: usize, suits: &BiomeSuitability) -> Self {
        Self {
            biome,
            ln_prevalence: suits.prevalence.ln(),
            preferences: [suits.elevation, suits.ruggedness, suits.moisture],
        }
    }

    /// The candidate's score for `fields` in a World whose vertical scale
    /// is `vertical`: the log of its prevalence, less half the sum of each
    /// preferred field's squared distance from its centre in spreads.
    ///
    /// Elevation is divided by `vertical` first, so the preference is
    /// written at reference scale.
    pub fn score(&self, fields: &FieldValues, vertical: f64) -> f64 {
        let values = [
            f64::from(fields.elevation) / vertical,
            f64::from(fields.ruggedness),
            f64::from(fields.moisture),
        ];
        let mut score = self.ln_prevalence;
        for (value, preference) in values.into_iter().zip(&self.preferences) {
            if let Some(FieldPreference { centre, spread }) = preference {
                let z = (value - centre) / spread;
                score -= 0.5 * z * z;
            }
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(elevation: f32, ruggedness: f32, moisture: f32) -> FieldValues {
        FieldValues {
            elevation,
            ruggedness,
            moisture,
        }
    }

    #[test]
    fn omitted_fields_add_nothing() {
        let c = Candidate::new(
            0,
            &BiomeSuitability {
                prevalence: 2.0,
                ..BiomeSuitability::default()
            },
        );
        assert_eq!(c.score(&fields(900.0, -3.0, 5.0), 1.0), 2f64.ln());
    }

    #[test]
    fn each_preference_is_a_gaussian_in_spreads() {
        let prefer = |centre, spread| Some(FieldPreference { centre, spread });
        let c = Candidate::new(
            0,
            &BiomeSuitability {
                prevalence: 1.0,
                elevation: prefer(100.0, 50.0),
                ruggedness: prefer(0.5, 0.25),
                moisture: None,
            },
        );
        // Elevation one spread off, ruggedness two: -0.5 * (1 + 4).
        let s = c.score(&fields(150.0, 0.0, 0.0), 1.0);
        assert!((s - -2.5).abs() < 1e-12, "{s}");
        // At vertical scale 4, E of 600 m is 150 m at reference scale.
        let s = c.score(&fields(600.0, 0.0, 0.0), 4.0);
        assert!((s - -2.5).abs() < 1e-12, "{s}");
    }
}
