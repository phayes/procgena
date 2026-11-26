//! A stream's chunk reads waiting for the terrain thread. The thread takes
//! the next waiting read as soon as it finishes one, so it never waits for
//! a frame, and the stream can drop and reorder the waiting reads at any
//! time.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::worker;

/// The work of one read.
pub(crate) type Job<T> = Box<dyn FnOnce() -> T + Send>;

/// Reads waiting for the terrain thread, each under a key, with the
/// results of those it has finished. The thread takes them one at a time
/// in the order they were queued. Between two reads it lets every other
/// job on the thread have a turn (another stream's reads among them).
/// Dropping the queue drops the reads still waiting.
pub(crate) struct ReadQueue<K, T> {
    shared: Arc<Mutex<Shared<K, T>>>,
}

/// What the stream and the terrain thread share.
struct Shared<K, T> {
    /// Reads not started, next first.
    waiting: VecDeque<(K, Job<T>)>,
    /// Reads finished and not yet collected, with their result or panic.
    done: Vec<(K, std::thread::Result<T>)>,
    /// Whether the terrain thread has a turn coming that takes the next
    /// waiting read.
    scheduled: bool,
}

impl<K, T> Default for ReadQueue<K, T> {
    fn default() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                waiting: VecDeque::new(),
                done: Vec::new(),
                scheduled: false,
            })),
        }
    }
}

impl<K: Send + 'static, T: Send + 'static> ReadQueue<K, T> {
    /// Queues `jobs` behind the reads already waiting, in order.
    pub(crate) fn push(&self, jobs: impl IntoIterator<Item = (K, Job<T>)>) {
        let mut shared = lock(&self.shared);
        shared.waiting.extend(jobs);
        if !shared.scheduled && !shared.waiting.is_empty() {
            shared.scheduled = true;
            take_turn(Arc::clone(&self.shared));
        }
    }

    /// Drops every read not started yet, and returns their keys. A read
    /// already started still finishes, and its result comes from
    /// [`finished`](Self::finished).
    pub(crate) fn cancel(&self) -> Vec<K> {
        lock(&self.shared)
            .waiting
            .drain(..)
            .map(|(key, _)| key)
            .collect()
    }

    /// The reads finished since the last call, in the order they finished,
    /// with their results.
    ///
    /// # Panics
    ///
    /// If one of them panicked, with its panic. The other results
    /// collected in the same call are lost.
    pub(crate) fn finished(&self) -> Vec<(K, T)> {
        let done = std::mem::take(&mut lock(&self.shared).done);
        done.into_iter()
            .map(|(key, result)| (key, result.unwrap_or_else(|panic| resume_unwind(panic))))
            .collect()
    }
}

impl<K, T> Drop for ReadQueue<K, T> {
    fn drop(&mut self) {
        lock(&self.shared).waiting.clear();
    }
}

/// Locks `shared`, whatever a panic elsewhere left it as.
fn lock<K, T>(shared: &Mutex<Shared<K, T>>) -> MutexGuard<'_, Shared<K, T>> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Queues one turn on the terrain thread: it runs the next waiting read,
/// if any, and queues the next turn while reads are left.
fn take_turn<K: Send + 'static, T: Send + 'static>(shared: Arc<Mutex<Shared<K, T>>>) {
    worker::submit(move || {
        let Some((key, job)) = ({
            let mut state = lock(&shared);
            let next = state.waiting.pop_front();
            state.scheduled = next.is_some();
            next
        }) else {
            return;
        };
        let result = catch_unwind(AssertUnwindSafe(job));
        let more = {
            let mut state = lock(&shared);
            state.done.push((key, result));
            state.scheduled = !state.waiting.is_empty();
            state.scheduled
        };
        if more {
            take_turn(shared);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use super::*;

    /// Collects finished reads from `queue` until `count` have come, for at
    /// most ten seconds.
    fn collect<K: Send + 'static, T: Send + 'static>(
        queue: &ReadQueue<K, T>,
        count: usize,
    ) -> Vec<(K, T)> {
        let start = Instant::now();
        let mut all = Vec::new();
        while all.len() < count {
            all.extend(queue.finished());
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "only {} of {count} reads finished",
                all.len()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        all
    }

    /// A job that returns `value`.
    fn job<T: Send + 'static>(value: T) -> Job<T> {
        Box::new(move || value)
    }

    #[test]
    fn reads_run_on_the_terrain_thread_in_the_order_queued() {
        let queue = ReadQueue::default();
        queue.push((0..10).map(|i| {
            (
                i,
                Box::new(move || (i * 10, std::thread::current().name().map(str::to_owned)))
                    as Job<_>,
            )
        }));
        queue.push([(10, job((100, None)))]);
        let done = collect(&queue, 11);
        let keys: Vec<i32> = done.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, (0..11).collect::<Vec<_>>());
        for (key, (value, thread)) in &done[..10] {
            assert_eq!(*value, key * 10);
            assert_eq!(thread.as_deref(), Some("terrain-worker"));
        }
    }

    #[test]
    fn cancelled_reads_never_run_and_a_started_one_still_finishes() {
        let queue = ReadQueue::default();
        let (started, has_started) = channel::<()>();
        let (release, hold) = channel::<()>();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_late = Arc::clone(&ran);
        queue.push([
            (
                "held",
                Box::new(move || {
                    started.send(()).unwrap();
                    hold.recv().unwrap();
                }) as Job<()>,
            ),
            (
                "late",
                Box::new(move || ran_late.store(true, Ordering::Relaxed)),
            ),
        ]);
        has_started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(queue.cancel(), vec!["late"]);
        release.send(()).unwrap();
        let done = collect(&queue, 1);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, "held");
        // A read queued after the cancelled one has run, so the cancelled
        // one would have had its turn.
        queue.push([("after", job(()))]);
        collect(&queue, 1);
        assert!(!ran.load(Ordering::Relaxed));
    }

    #[test]
    fn a_dropped_queue_drops_its_waiting_reads() {
        let (started, has_started) = channel::<()>();
        let (release, hold) = channel::<()>();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_late = Arc::clone(&ran);
        let queue = ReadQueue::default();
        queue.push([
            (
                0,
                Box::new(move || {
                    started.send(()).unwrap();
                    hold.recv().unwrap();
                }) as Job<()>,
            ),
            (1, Box::new(move || ran_late.store(true, Ordering::Relaxed))),
        ]);
        has_started.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(queue);
        release.send(()).unwrap();
        // A job queued on the thread after the dropped reads runs after
        // them.
        let (done, finished) = channel();
        worker::submit(move || done.send(()).unwrap());
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!ran.load(Ordering::Relaxed));
    }

    #[test]
    fn a_panicking_read_reaches_the_collector_and_the_rest_carry_on() {
        let queue = ReadQueue::default();
        queue.push([(
            0,
            Box::new(|| -> u32 { panic!("the read failed") }) as Job<u32>,
        )]);
        let collected = catch_unwind(AssertUnwindSafe(|| collect(&queue, 1)));
        assert!(collected.is_err());
        queue.push([(1, job(7))]);
        assert_eq!(collect(&queue, 1), vec![(1, 7)]);
    }

    #[test]
    fn two_queues_take_turns_on_the_thread() {
        let (first, second) = (ReadQueue::default(), ReadQueue::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let (release, hold) = channel::<()>();
        // Hold the thread so both queues are filled before either runs.
        worker::submit(move || hold.recv().unwrap());
        for (name, queue) in [("a", &first), ("b", &second)] {
            queue.push((0..3).map(|i| {
                let order = Arc::clone(&order);
                (
                    i,
                    Box::new(move || order.lock().unwrap().push((name, i))) as Job<()>,
                )
            }));
        }
        release.send(()).unwrap();
        collect(&first, 3);
        collect(&second, 3);
        let order = order.lock().unwrap();
        let names: Vec<&str> = order.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["a", "b", "a", "b", "a", "b"], "{order:?}");
    }
}
