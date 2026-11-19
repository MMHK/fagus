//! External tools (the rasterizer, Chrome) run with a time limit, and work spread over threads.

use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Runs `command` without input or output until it ends, or kills it once `limit` has passed.
/// Returns its exit status, or `None` if it was killed.
pub fn run(command: &mut Command, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() > limit {
            // It may have ended in the meantime; either way it is reaped.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Calls `work` for every item on `jobs` threads and returns the results in the order of the
/// items. `progress` is called after each item with the number done so far.
pub fn map<T: Sync, R: Send>(
    items: &[T],
    jobs: usize,
    work: impl Fn(&T) -> R + Sync,
    progress: impl Fn(usize) + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = work(item);
                    results.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(result);
                    progress(done.fetch_add(1, Ordering::Relaxed) + 1);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .map(|result| result.expect("every item was worked on"))
        .collect()
}

/// The number of threads to use by default: one per processor.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(2, std::num::NonZero::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_keeps_the_order_of_the_items() {
        let items: Vec<u32> = (0..100).collect();
        let squares = map(&items, 4, |item| item * item, |_| {});
        assert_eq!(squares, items.iter().map(|item| item * item).collect::<Vec<_>>());
        assert_eq!(map(&[] as &[u32], 4, |item| *item, |_| {}), Vec::<u32>::new());
    }

    #[cfg(unix)]
    #[test]
    fn run_kills_a_command_that_takes_too_long() {
        let status = run(Command::new("sleep").arg("5"), Duration::from_millis(50)).unwrap();
        assert!(status.is_none());
        let status = run(&mut Command::new("true"), Duration::from_secs(10)).unwrap();
        assert!(status.is_some_and(|status| status.success()));
    }
}
