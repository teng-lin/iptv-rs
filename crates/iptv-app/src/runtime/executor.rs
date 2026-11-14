//! The app's one runtime for network work that is not an import: Stalker
//! link resolves and artwork downloads; its blocking threads also build
//! large schedules off the UI thread. It is made once, on first use, and
//! lives as long as the process.
//!
//! Multi-threaded, with two workers. Its tasks are network waits with
//! short CPU bursts between them (TLS handshakes, a portal's JSON,
//! encoding a 4 MiB logo as a data URI), so a second worker keeps a burst
//! from delaying a zap's link resolve; more workers would only add idle
//! threads on a TV box. A current-thread runtime would need a thread of
//! its own to drive it anyway, and would serialise those bursts.
//!
//! Tasks hand results to the UI through channels or the `JoinHandle`,
//! which wake the NSH task awaiting them; nothing polls. Imports keep
//! their bounded pool (`data/workers.rs`): their pipelines call the store
//! with blocking requests, which must not hold this runtime's workers.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("iptv-net")
            .enable_all()
            .build()
            .expect("start the network runtime")
    })
}

/// Runs `future` on the runtime; await the handle for its output. Dropping
/// the handle detaches the task (cancel it with its `CancelToken`).
pub(crate) fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}

/// Runs CPU-bound `work` on the runtime's blocking threads (building a
/// large schedule); await the handle for its output.
pub(crate) fn spawn_blocking<R: Send + 'static>(
    work: impl FnOnce() -> R + Send + 'static,
) -> JoinHandle<R> {
    runtime().spawn_blocking(work)
}
