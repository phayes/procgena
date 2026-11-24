//! Exact footprints: the discs of ground that placed copies take up, so
//! Scatters lower in an Ecotope never stand inside them.

use std::collections::HashMap;

use glam::DVec2;

use super::lattice::{Fixed2, FixedRect, to_fixed};

/// A disc of ground a copy takes up, such as a trunk, a boulder or a stretch
/// of a log, in fixed point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Disc {
    /// The disc's centre.
    pub centre: Fixed2,
    /// Its radius in fixed-point units (1/256 m), 0 or more.
    pub radius: i64,
}

impl Disc {
    /// A disc around World position `centre` (metres, X then Z), `radius`
    /// metres across from its centre. A negative radius counts as 0.
    pub fn new(centre: DVec2, radius: f32) -> Self {
        Self {
            centre: Fixed2::from_world(centre),
            radius: to_fixed(f64::from(radius)).max(0),
        }
    }

    /// Whether this disc and `other` overlap. Discs that only touch do not.
    pub fn overlaps(&self, other: &Disc) -> bool {
        let reach = i128::from(self.radius + other.radius);
        self.centre.distance_squared(other.centre) < reach * reach
    }

    /// Whether `p` lies inside the disc (not on its edge).
    pub fn contains(&self, p: Fixed2) -> bool {
        let r = i128::from(self.radius);
        self.centre.distance_squared(p) < r * r
    }
}

/// Width of the buckets [`Footprints`] sorts its discs into, in fixed-point
/// units (4 m).
const BUCKET: i64 = 4 * 256;

/// A set of footprint discs, quick to test new discs against.
///
/// An Ecotope gathers the footprints of the Scatters above one Scatter and
/// passes them in its [`ScatterInput`](super::ScatterInput), and that
/// Scatter then drops every candidate whose own footprint overlaps one of
/// them.
#[derive(Clone, Debug, Default)]
pub struct Footprints {
    discs: Vec<Disc>,
    buckets: HashMap<(i64, i64), Vec<u32>>,
    largest: i64,
}

impl Footprints {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// The discs, in the order they were added.
    pub fn discs(&self) -> &[Disc] {
        &self.discs
    }

    /// Whether the set holds no discs.
    pub fn is_empty(&self) -> bool {
        self.discs.is_empty()
    }

    /// Adds `disc`.
    pub fn insert(&mut self, disc: Disc) {
        let key = bucket(disc.centre);
        self.buckets
            .entry(key)
            .or_default()
            .push(self.discs.len() as u32);
        self.largest = self.largest.max(disc.radius);
        self.discs.push(disc);
    }

    /// Whether `disc` overlaps any disc in the set.
    pub fn overlaps(&self, disc: &Disc) -> bool {
        let reach = disc.radius + self.largest;
        let lo = bucket(Fixed2::new(disc.centre.x - reach, disc.centre.z - reach));
        let hi = bucket(Fixed2::new(disc.centre.x + reach, disc.centre.z + reach));
        (lo.0..=hi.0).any(|x| {
            (lo.1..=hi.1).any(|z| {
                self.buckets
                    .get(&(x, z))
                    .is_some_and(|list| list.iter().any(|&i| self.discs[i as usize].overlaps(disc)))
            })
        })
    }

    /// Whether any of `discs` overlaps a disc in the set.
    pub fn overlaps_any(&self, discs: &[Disc]) -> bool {
        discs.iter().any(|d| self.overlaps(d))
    }

    /// Whether `p` lies inside any disc in the set.
    pub fn covers(&self, p: Fixed2) -> bool {
        self.overlaps(&Disc {
            centre: p,
            radius: 0,
        })
    }

    /// The discs that reach into `rect`, in the order they were added.
    pub fn within(&self, rect: FixedRect) -> impl Iterator<Item = &Disc> {
        self.discs.iter().filter(move |d| {
            let grown = rect.grow(d.radius);
            grown.contains(d.centre)
        })
    }
}

impl Extend<Disc> for Footprints {
    fn extend<I: IntoIterator<Item = Disc>>(&mut self, iter: I) {
        for disc in iter {
            self.insert(disc);
        }
    }
}

impl FromIterator<Disc> for Footprints {
    fn from_iter<I: IntoIterator<Item = Disc>>(iter: I) -> Self {
        let mut set = Self::new();
        set.extend(iter);
        set
    }
}

fn bucket(p: Fixed2) -> (i64, i64) {
    (p.x.div_euclid(BUCKET), p.z.div_euclid(BUCKET))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(x: i64, z: i64, radius: i64) -> Disc {
        Disc {
            centre: Fixed2::new(x, z),
            radius,
        }
    }

    #[test]
    fn discs_that_touch_do_not_overlap() {
        // 3-4-5: centres 5 units apart.
        let a = disc(0, 0, 2);
        assert!(!a.overlaps(&disc(3, 4, 3)));
        assert!(a.overlaps(&disc(3, 4, 4)));
        assert!(a.contains(Fixed2::new(1, 1)));
        assert!(!a.contains(Fixed2::new(2, 0)));
    }

    #[test]
    fn the_set_finds_overlaps_across_buckets() {
        let mut set = Footprints::new();
        set.insert(disc(BUCKET - 1, 0, 3 * BUCKET));
        set.insert(disc(-10 * BUCKET, 0, 1));
        // Far from the big disc's bucket, but inside its radius.
        assert!(set.overlaps(&disc(3 * BUCKET, 0, 1)));
        assert!(!set.overlaps(&disc(5 * BUCKET, 0, 1)));
        assert!(set.covers(Fixed2::new(-10 * BUCKET, 0)));
        assert!(!set.covers(Fixed2::new(-10 * BUCKET, 1)));
        assert!(set.overlaps_any(&[disc(9 * BUCKET, 0, 1), disc(0, 0, 1)]));
        let rect = FixedRect::new(Fixed2::new(-10 * BUCKET + 1, -5), Fixed2::new(-5, 5));
        // The small disc's centre is inside; the big one reaches in.
        assert_eq!(set.within(rect).count(), 2);
        let beyond = FixedRect::new(Fixed2::new(-20 * BUCKET, -5), Fixed2::new(-11 * BUCKET, 5));
        assert_eq!(set.within(beyond).count(), 0);
    }

    #[test]
    fn disc_tests_are_exact_far_from_the_origin() {
        let far = 1_000_000_000_000i64;
        let a = disc(far, -far, 256);
        assert!(!a.overlaps(&disc(far + 512, -far, 256)));
        assert!(a.overlaps(&disc(far + 511, -far, 256)));
        assert_eq!(Disc::new(DVec2::new(2.0, -1.0), 0.5), disc(512, -256, 128));
    }
}
