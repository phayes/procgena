//! Turns the single decay value into the separate amounts it controls: bark
//! loss, twig and stub loss, flattening, sag, burial, how worn the breaks
//! are, and how rotten and weathered the colours look.
//!
//! Each amount rises smoothly over its own range of decay classes. Where an
//! amount has its own optional setting, a `Some` value is used as given and
//! the decay value is ignored for that amount.

use crate::options::DeadwoodOptions;
use crate::rng::smoothstep;

/// The effective amounts for one set of options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Decay {
    /// Fraction of bark peeled off, 0 to 1.
    pub stripped: f32,
    /// Fraction of stubs still attached, 0 to 1.
    pub stub_keep: f32,
    /// Fraction of side branches still attached, 0 to 1.
    pub child_keep: f32,
    /// Fraction of twigs still attached, 0 to 1.
    pub twig_keep: f32,
    /// Flattening toward the ground, 0 to 0.6.
    pub ovality: f32,
    /// Sag between supports, 0 (stiff) to 1 (lies on the ground).
    pub sag: f32,
    /// Suggested burial depth as a fraction of the diameter, 0 to 0.5.
    pub burial: f32,
    /// How much breaks and splinters are worn down and rounded, 0 to 1.
    pub soften: f32,
    /// How rotten the wood looks, 0 to 1.
    pub rot: f32,
    /// How far bark has weathered toward its weathered colour, 0 to 1.
    pub weathering: f32,
}

impl Decay {
    /// Works out the amounts for `o`, using its overrides where given.
    pub(crate) fn new(o: &DeadwoodOptions) -> Self {
        let d = o.decay;
        Self {
            stripped: o.bark.stripped.unwrap_or(smoothstep(1.5, 4.0, d)),
            stub_keep: o.stubs.keep.unwrap_or(1.0 - 0.85 * smoothstep(2.0, 4.5, d)),
            child_keep: o
                .branching
                .keep
                .unwrap_or(1.0 - 0.8 * smoothstep(2.0, 4.0, d)),
            twig_keep: o
                .branching
                .twig_keep
                .unwrap_or(1.0 - smoothstep(0.8, 2.2, d)),
            ovality: o.shape.ovality.unwrap_or(0.45 * smoothstep(3.0, 5.0, d)),
            sag: o.settle.sag.unwrap_or(smoothstep(1.0, 4.0, d)),
            burial: o.settle.burial.unwrap_or(0.3 * smoothstep(3.5, 5.0, d)),
            soften: smoothstep(2.0, 5.0, d),
            rot: smoothstep(2.5, 5.0, d),
            weathering: smoothstep(1.0, 4.0, d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_wood_is_whole_and_rotten_wood_is_not() {
        let fresh = Decay::new(&DeadwoodOptions {
            decay: 0.0,
            ..Default::default()
        });
        assert_eq!(fresh.stripped, 0.0);
        assert_eq!(fresh.twig_keep, 1.0);
        assert_eq!(fresh.sag, 0.0);
        assert_eq!(fresh.burial, 0.0);

        let rotten = Decay::new(&DeadwoodOptions {
            decay: 5.0,
            ..Default::default()
        });
        assert_eq!(rotten.stripped, 1.0);
        assert_eq!(rotten.twig_keep, 0.0);
        assert_eq!(rotten.sag, 1.0);
        assert!(rotten.burial > 0.29);
        assert!(rotten.ovality > 0.4);
    }

    #[test]
    fn overrides_win() {
        let mut o = DeadwoodOptions {
            decay: 5.0,
            ..Default::default()
        };
        o.bark.stripped = Some(0.0);
        o.settle.sag = Some(0.25);
        let d = Decay::new(&o);
        assert_eq!(d.stripped, 0.0);
        assert_eq!(d.sag, 0.25);
    }
}
