//! The one background thread that reads terrain: every stream's chunk
//! reads, and the region builds inside them, run on it one at a time.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock, PoisonError};

/// A job queued on the terrain thread.
type Job = Box<dyn FnOnce() + Send>;

/// Queues `job` on the terrain thread, behind every job queued before it.
/// Starts the thread on first use.
///
/// The job must not panic: a panic ends the terrain thread, and every job
/// queued after it never runs.
pub(crate) fn submit(job: impl FnOnce() + Send + 'static) {
    queue()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .send(Box::new(job))
        .expect("the terrain thread runs as long as the program");
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
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn jobs_run_one_at_a_time_in_order_on_one_thread() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let (done, finished) = channel();
        for i in 0..20 {
            let order = Arc::clone(&order);
            let done = done.clone();
            submit(move || {
                let name = std::thread::current().name().map(str::to_owned);
                order.lock().unwrap().push(i);
                done.send(name).unwrap();
            });
        }
        for _ in 0..20 {
            let name = finished.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(name.as_deref(), Some("terrain-worker"));
        }
        let order = order.lock().unwrap();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    }
}
