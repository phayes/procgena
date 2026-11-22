//! Helpers for running independent work on one or more CPU threads.
//!
//! Parallel execution requires the `rayon` feature and a true `parallel` argument.
//! Otherwise, callbacks run on the calling thread. Parallel callbacks may run in
//! any order; `map` still returns values in input order.

#[cfg(feature = "rayon")]
use rayon::prelude::*;

/// Whether this build enables the `rayon` feature.
pub(crate) const PARALLEL: bool = cfg!(feature = "rayon");

/// Calls `f` with each chunk's zero-based index and mutable slice.
///
/// The last chunk may contain fewer than `chunk_len` elements. Set `parallel` to
/// false to run callbacks in order on the calling thread.
///
/// # Panics
///
/// Panics if `chunk_len` is zero.
pub(crate) fn for_each_chunk_mut<T, F>(data: &mut [T], chunk_len: usize, parallel: bool, f: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync + Send,
{
    #[cfg(feature = "rayon")]
    if parallel {
        data.par_chunks_mut(chunk_len)
            .enumerate()
            .for_each(|(i, chunk)| f(i, chunk));
        return;
    }
    let _ = parallel;
    data.chunks_mut(chunk_len)
        .enumerate()
        .for_each(|(i, chunk)| f(i, chunk));
}

/// Applies `f` to each item and returns the values in input order.
///
/// Set `parallel` to false to run callbacks on the calling thread. With parallel
/// execution, callback order and any side effects are not ordered.
pub(crate) fn map<T, U, F>(items: &[T], parallel: bool, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync + Send,
{
    #[cfg(feature = "rayon")]
    if parallel {
        return items.par_iter().map(f).collect();
    }
    let _ = parallel;
    items.iter().map(f).collect()
}
