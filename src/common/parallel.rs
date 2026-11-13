//! Running independent per-page work on several threads.
//!
//! The PDF caches of the hayro crates are not thread-safe, so every worker
//! thread builds its own state (renderer, font registry, ...) with `init`
//! and reuses it for all the items it takes. Items are handed out one at a
//! time, so a few slow pages do not hold up the others, and results come
//! back in item order.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use std::sync::atomic::{AtomicUsize, Ordering};

/// Stack size of worker threads: PDF interpretation recurses into nested
/// forms and patterns, so give workers as much room as the main thread.
const STACK_SIZE: usize = 16 << 20;

/// Number of worker threads to use: `requested`, or one per CPU when
/// `requested` is 0.
pub fn resolve_jobs(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// Computes `func(state, i)` for every `i` in `0..n` on up to `jobs` threads
/// (0 = one per CPU) and returns the results in order.
///
/// `init` builds the per-thread state once per worker. With one job (or at
/// most one item) everything runs on the calling thread and no thread is
/// spawned. A panic in a worker is propagated to the caller once all
/// workers have finished.
///
/// # Panics
/// When a worker thread cannot be started, or when `init` or `func` panics.
pub fn map<S, R, I, F>(n: usize, jobs: usize, init: I, func: F) -> Vec<R>
where
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize) -> R + Sync,
{
    let jobs = resolve_jobs(jobs).min(n);
    if jobs <= 1 {
        let mut state = init();
        return (0..n).map(|i| func(&mut state, i)).collect();
    }
    let next = AtomicUsize::new(0);
    let worker = || {
        let mut state = init();
        let mut done = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            done.push((i, func(&mut state, i)));
        }
        done
    };
    let mut slots: Vec<Option<R>> = (0..n).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs)
            .map(|_| {
                std::thread::Builder::new()
                    .stack_size(STACK_SIZE)
                    .spawn_scoped(scope, worker)
                    .expect("cannot start a worker thread")
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(done) => done.into_iter().for_each(|(i, result)| slots[i] = Some(result)),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.expect("every item is computed"))
        .collect()
}

#[cfg(test)]
mod tests {
    /// Results come back in item order for any number of jobs, and an empty
    /// input gives an empty output.
    #[test]
    fn keeps_order() {
        for jobs in [0, 1, 3, 16] {
            let out = super::map(
                50,
                jobs,
                || 0usize,
                |calls, i| {
                    *calls += 1;
                    i * i
                },
            );
            assert_eq!(out, (0..50).map(|i| i * i).collect::<Vec<_>>());
        }
        assert!(super::map(0, 4, || (), |_, i| i).is_empty());
    }
}
