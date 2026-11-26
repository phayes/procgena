//! The one background thread that reads terrain: every stream's chunk
//! reads, and the region builds inside them, run on it one at a time.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// A job queued on the terrain thread.
type Job = Box<dyn FnOnce() + Send>;

/// Where a job leaves its result, or its panic.
type Outcome<T> = Arc<Mutex<Option<std::thread::Result<T>>>>;

/// A job started with [`spawn`], whose result arrives later. Dropping it
/// before the job starts cancels the job.
pub(crate) struct Pending<T> {
    outcome: Outcome<T>,
    cancelled: Arc<AtomicBool>,
}

impl<T> Pending<T> {
    /// The job's result once it has finished, else `None`. Gives the
    /// result once.
    ///
    /// # Panics
    ///
    /// If the job panicked, with the job's panic.
    pub(crate) fn poll(&self) -> Option<T> {
        let taken = self
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        taken.map(|outcome| outcome.unwrap_or_else(|panic| resume_unwind(panic)))
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Queues `job` on the terrain thread, behind every job queued before it,
/// and returns its pending result. Starts the thread on first use.
pub(crate) fn spawn<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    let outcome: Outcome<T> = Arc::default();
    let cancelled = Arc::new(AtomicBool::new(false));
    let (out, skip) = (Arc::clone(&outcome), Arc::clone(&cancelled));
    let job: Job = Box::new(move || {
        if !skip.load(Ordering::Relaxed) {
            let result = catch_unwind(AssertUnwindSafe(job));
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        }
    });
    queue()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .send(job)
        .expect("the terrain thread runs as long as the program");
    Pending { outcome, cancelled }
}

/// The terrain thread's queue, starting the thread the first time.
fn queue() -> &'static Mutex<Sender<Job>> {
    static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, jobs) = channel::<Job>();
        std::thread::Builder::new()
            .name("terrain-worker".to_owned())
            .spawn(move || {
                for job in jobs {
                    job();
                }
            })
            .expect("the terrain thread starts");
        Mutex::new(sender)
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// Polls `pending` until it finishes, for at most ten seconds.
    fn wait<T>(pending: &Pending<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(value) = pending.poll() {
                return value;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the job never finished"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn jobs_run_one_at_a_time_in_order_on_one_thread() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let jobs: Vec<_> = (0..20)
            .map(|i| {
                let order = Arc::clone(&order);
                spawn(move || {
                    order.lock().unwrap().push(i);
                    std::thread::current().name().map(str::to_owned)
                })
            })
            .collect();
        for job in &jobs {
            assert_eq!(wait(job).as_deref(), Some("terrain-worker"));
        }
        let order = order.lock().unwrap();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    }

    #[test]
    fn a_dropped_job_that_has_not_started_never_runs() {
        let (hold, release) = channel::<()>();
        let blocker = spawn(move || release.recv().ok());
        let ran = Arc::new(AtomicBool::new(false));
        let dropped = {
            let ran = Arc::clone(&ran);
            spawn(move || ran.store(true, Ordering::Relaxed))
        };
        drop(dropped);
        hold.send(()).unwrap();
        wait(&blocker);
        // A job queued after the dropped one has finished, so the dropped
        // one had its turn.
        wait(&spawn(|| ()));
        assert!(!ran.load(Ordering::Relaxed));
    }

    #[test]
    fn a_panic_reaches_whoever_polls() {
        let job = spawn(|| -> u32 { panic!("the read failed") });
        let polled = catch_unwind(AssertUnwindSafe(|| wait(&job)));
        assert!(polled.is_err());
        // The thread carries on with later jobs.
        assert_eq!(wait(&spawn(|| 7)), 7);
    }
}
