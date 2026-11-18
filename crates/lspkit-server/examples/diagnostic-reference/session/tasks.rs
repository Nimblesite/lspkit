//! The session's request tasks. [MEM-060-EXAMPLE-TASKS]
//!
//! Every request task is owned: finished tasks are reaped as new ones start,
//! and dropping the owner at session end aborts any still in flight, so a
//! stuck handler never pins a `Server` clone (and every open document).

use std::future::Future;

use tokio::task::JoinSet;

/// Owns every request task the session spawns.
#[derive(Default)]
pub(super) struct Tasks(JoinSet<()>);

impl Tasks {
    /// Spawn one request task, first releasing the ones that finished.
    pub(super) fn spawn(&mut self, task: impl Future<Output = ()> + Send + 'static) {
        while self.0.try_join_next().is_some() {}
        let _abort = self.0.spawn(task);
    }
}
