//! The worker pool for heavy background generation (terrain tiles, planet maps, the sky).
//!
//! Bevy's `AsyncComputeTaskPool` has at most 4 threads by default and Bevy uses it for its own
//! work, such as compiling render pipelines. Queuing seconds of generation there starved that
//! work and stalled frames (a 2.2 s startup frame on Windows), so generation runs on this
//! separate pool instead: half the logical cores, at least two threads.

use std::sync::OnceLock;

use bevy::tasks::{TaskPool, TaskPoolBuilder};

/// The shared generation worker pool.
pub fn workers() -> &'static TaskPool {
    static POOL: OnceLock<TaskPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        TaskPoolBuilder::new()
            .num_threads((cores / 2).max(2))
            .thread_name("generation".into())
            .build()
    })
}
