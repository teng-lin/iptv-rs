//! One store connection on a long-lived worker. UI callers await replies;
//! import workers may wait, but cannot open competing app connections.

use std::cell::Cell;
use std::future::Future;
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

use futures_channel::oneshot;
use iptv_store_sqlite::{Store, StoreError};

type Job = Box<dyn FnOnce(&mut Option<Store>) + Send>;

thread_local! {
    static APP_EPOCH: Cell<Option<u64>> = const { Cell::new(None) };
    static UI_THREAD: Cell<bool> = const { Cell::new(false) };
    static STORE_THREAD: Cell<bool> = const { Cell::new(false) };
    static RETRY_OPEN_AT: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
}

static SERVICE: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
#[derive(Default)]
struct Epochs {
    next: AtomicU64,
    active: AtomicU64,
}
impl Epochs {
    fn begin(&self) -> u64 {
        let epoch = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.active.store(epoch, Ordering::SeqCst);
        epoch
    }
    fn permits(&self, epoch: u64) -> bool {
        epoch != 0 && self.active.load(Ordering::SeqCst) == epoch
    }
    fn close(&self, epoch: u64) -> bool {
        self.active
            .compare_exchange(epoch, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}
static EPOCHS: Epochs = Epochs {
    next: AtomicU64::new(0),
    active: AtomicU64::new(0),
};
fn stale() -> StoreError {
    StoreError::Unavailable("request belongs to a closed application")
}
fn affinity() -> Result<u64, StoreError> {
    APP_EPOCH
        .get()
        .filter(|epoch| EPOCHS.permits(*epoch))
        .ok_or_else(stale)
}
pub(crate) fn is_current() -> bool {
    affinity().is_ok()
}

pub fn register_ui_thread() {
    UI_THREAD.set(true);
    APP_EPOCH.set(Some(EPOCHS.begin()));
}

/// Capture ownership before spawning work. A pooled worker never adopts the
/// epoch of a newer app just because an older network request finishes late.
pub(crate) fn scoped(work: impl FnOnce() + Send + 'static) -> impl FnOnce() + Send + 'static {
    let epoch = APP_EPOCH.get();
    move || {
        struct Restore(Option<u64>);
        impl Drop for Restore {
            fn drop(&mut self) {
                APP_EPOCH.set(self.0);
            }
        }
        let _restore = Restore(APP_EPOCH.replace(epoch));
        work();
    }
}

fn with_epoch<R>(
    epochs: &Epochs,
    epoch: u64,
    store: &mut Option<Store>,
    operation: impl FnOnce(&mut Store) -> Result<R, StoreError>,
) -> Result<R, StoreError> {
    if !epochs.permits(epoch) {
        return Err(stale());
    }
    with_store(store, |store| {
        // Opening the OS broker can wait; a newer app may have started while
        // an old open was waiting, so recheck before touching catalog rows.
        if !epochs.permits(epoch) {
            return Err(stale());
        }
        operation(store)
    })
}

fn open_store() -> Result<Store, StoreError> {
    let directory = crate::data::data_dir();
    std::fs::create_dir_all(&directory)
        .map_err(|_| StoreError::Unavailable("cannot create data directory"))?;
    crate::credential_key::open_database(&directory.join("iptv.sqlite"))
}

fn sender() -> Result<&'static mpsc::Sender<Job>, StoreError> {
    if STORE_THREAD.get() {
        return Err(StoreError::Unavailable("recursive store request"));
    }
    Ok(SERVICE.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("iptv-store".into())
            .spawn(move || {
                STORE_THREAD.set(true);
                let mut store = None;
                while let Ok(job) = receiver.recv() {
                    run_job(&mut store, job);
                }
            })
            .expect("start store service");
        sender
    }))
}

fn queue(job: Job) -> Result<(), StoreError> {
    sender()?
        .send(job)
        .map_err(|_| StoreError::Unavailable("store worker stopped"))
}

fn run_job(store: &mut Option<Store>, job: Job) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(store))).is_err() {
        // Discard the connection so an interrupted operation cannot leak its
        // transaction or statement state into the next request.
        *store = None;
        log::error!("iptv store: request panicked; connection discarded");
    }
}

fn with_store<R>(
    store: &mut Option<Store>,
    operation: impl FnOnce(&mut Store) -> Result<R, StoreError>,
) -> Result<R, StoreError> {
    if store.is_none() {
        if RETRY_OPEN_AT
            .get()
            .is_some_and(|at| std::time::Instant::now() < at)
        {
            return Err(StoreError::Unavailable(
                "credential storage is temporarily unavailable",
            ));
        }
        match open_store() {
            Ok(opened) => {
                *store = Some(opened);
                RETRY_OPEN_AT.set(None);
            }
            Err(error) => {
                RETRY_OPEN_AT.set(Some(
                    std::time::Instant::now() + std::time::Duration::from_secs(1),
                ));
                return Err(error);
            }
        }
    }
    operation(store.as_mut().expect("opened store"))
}

/// Transfers ownership immediately in FIFO order, even if the caller drops its
/// reply future. Bulk producers are bounded by the refresh pool; no SQLite work
/// or enqueue wait runs on the UI thread.
pub fn call<R: Send + 'static>(
    operation: impl FnOnce(&mut Store) -> Result<R, StoreError> + Send + 'static,
) -> impl Future<Output = Result<R, StoreError>> {
    let (sender, receiver) = oneshot::channel();
    let queued = affinity().and_then(|epoch| {
        queue(Box::new(move |store| {
            let _ = sender.send(with_epoch(&EPOCHS, epoch, store, operation));
        }))
    });
    async move {
        queued?;
        receiver
            .await
            .map_err(|_| StoreError::Unavailable("store reply lost"))?
    }
}

/// For import/background workers only. Calling this from the UI is a bug.
pub fn blocking<R: Send + 'static>(
    operation: impl FnOnce(&mut Store) -> Result<R, StoreError> + Send + 'static,
) -> Result<R, StoreError> {
    assert!(!UI_THREAD.get(), "blocking store request on the UI thread");
    let epoch = affinity()?;
    let (sender, receiver) = mpsc::sync_channel(1);
    self::sender()?
        .send(Box::new(move |store| {
            let _ = sender.send(with_epoch(&EPOCHS, epoch, store, operation));
        }))
        .map_err(|_| StoreError::Unavailable("store worker stopped"))?;
    receiver
        .recv()
        .map_err(|_| StoreError::Unavailable("store reply lost"))?
}

/// Root teardown only, after the favourites writer has submitted its last work.
/// Accepted requests already belong to this worker, independent of UI futures.
pub(crate) fn drain_on_shutdown() -> bool {
    let Some(epoch) = APP_EPOCH.get() else {
        return true;
    };
    let Some(service) = SERVICE.get() else {
        EPOCHS.close(epoch);
        return true;
    };
    let (done, reply) = mpsc::channel();
    if service
        .send(Box::new(move |store| {
            // Release SQLite handles before a new app instance opens or restores
            // its library in the same process.
            EPOCHS.close(epoch);
            // Even if a timed-out teardown was followed by a newer start, this
            // FIFO barrier still retires the connection held by preceding work.
            *store = None;
            RETRY_OPEN_AT.set(None);
            let _ = done.send(());
        }))
        .is_err()
    {
        return false;
    }
    reply
        .recv_timeout(std::time::Duration::from_secs(10))
        .is_ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn delayed_old_worker_cannot_write_a_replacement_catalog() {
        use std::sync::{Arc, mpsc};
        let epochs = Arc::new(super::Epochs::default());
        let old = epochs.begin();
        let mut store = Some(iptv_store_sqlite::Store::open_in_memory().unwrap());
        super::with_epoch(&epochs, old, &mut store, |store| {
            store.set_combined_last_channel("accepted-before-close")
        })
        .unwrap();
        assert_eq!(
            store
                .as_ref()
                .unwrap()
                .combined_last_channel()
                .unwrap()
                .as_deref(),
            Some("accepted-before-close")
        );
        let previous = super::APP_EPOCH.replace(Some(old));
        let (release, wait) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let worker_epochs = epochs.clone();
        let delayed = super::scoped(move || {
            wait.recv().unwrap();
            let captured = super::APP_EPOCH.get().unwrap();
            let mut replacement = Some(iptv_store_sqlite::Store::open_in_memory().unwrap());
            replacement
                .as_mut()
                .unwrap()
                .set_combined_last_channel("new-app")
                .unwrap();
            let write = super::with_epoch(&worker_epochs, captured, &mut replacement, |store| {
                store.set_combined_last_channel("late-old-worker")
            });
            done.send((
                write.is_err(),
                replacement.unwrap().combined_last_channel().unwrap(),
            ))
            .unwrap();
        });
        super::APP_EPOCH.set(previous);
        let thread = std::thread::spawn(delayed);
        assert!(epochs.close(old));
        let current = epochs.begin();
        assert_ne!(current, old);
        release.send(()).unwrap();
        let (rejected, value) = result.recv().unwrap();
        assert!(rejected);
        assert_eq!(value.as_deref(), Some("new-app"));
        thread.join().unwrap();
    }
    #[test]
    fn a_panicking_request_does_not_prevent_the_next_reply() {
        let mut store = None;
        super::run_job(&mut store, Box::new(|_| panic!("synthetic store failure")));
        let (tx, rx) = std::sync::mpsc::channel();
        super::run_job(
            &mut store,
            Box::new(move |_| {
                tx.send(()).unwrap();
            }),
        );
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn application_database_opening_is_confined_to_the_service() {
        fn inspect(directory: &std::path::Path) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    inspect(&path);
                } else if path.extension().is_some_and(|extension| extension == "rs")
                    && path
                        .file_name()
                        .is_none_or(|name| name != "store_service.rs")
                {
                    let source = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !source.contains("open_store("),
                        "database opened outside service: {}",
                        path.display()
                    );
                }
            }
        }
        inspect(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    }
}
