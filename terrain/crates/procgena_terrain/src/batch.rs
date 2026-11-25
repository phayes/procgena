//! Ordered sampling, with the same results with or without Rayon.

pub(crate) fn map<T: Sync, U: Send>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    #[cfg(feature = "rayon")]
    {
        use rayon::prelude::*;
        items.par_iter().map(&f).collect()
    }
    #[cfg(not(feature = "rayon"))]
    {
        items.iter().map(f).collect()
    }
}
