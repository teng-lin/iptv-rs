//! Owned application state; all types here are independent of the shell.
use iptv_guide::EpgSchedule;
use iptv_import::CancelToken;
use iptv_library::{Playlist, SourceId, SourceLine};
use iptv_store_sqlite::{StoredGuide, StoredSource};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};

pub(crate) struct ImportWork {
    pub(crate) finished: Cell<bool>,
    pub(crate) catalog_done: Cell<bool>,
    pub(crate) cancel: CancelToken,
    /// Tasks waiting for [`Self::finish`].
    pub(crate) waiting: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
}
impl ImportWork {
    pub(crate) fn finished(&self) -> bool {
        self.finished.get()
    }
    pub(crate) fn finish(&self) {
        self.finished.set(true);
        for waiting in self.waiting.take() {
            let _ = waiting.send(());
        }
    }
    /// Resolves when the work has finished (at once if it has).
    pub(crate) fn until_finished(&self) -> impl std::future::Future<Output = ()> + 'static {
        let wait = (!self.finished.get()).then(|| {
            let (sender, receiver) = futures_channel::oneshot::channel();
            self.waiting.borrow_mut().push(sender);
            receiver
        });
        async move {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
        }
    }
    pub(crate) fn playlist_done(&self) {
        self.catalog_done.set(true);
    }
    pub(crate) fn cancel_guide(&self) {
        if self.catalog_done.get() {
            self.cancel.cancel();
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LibrarySnapshot {
    pub(crate) playlists: Vec<Playlist>,
    pub(crate) details: BTreeMap<SourceId, StoredSource>,
    pub(crate) imports: BTreeMap<SourceId, iptv_store_sqlite::ImportStatus>,
    pub(crate) active: Option<SourceId>,
}

/// The current single-source schedule can enter the combined catalogue without
/// dropping a portal session, account format, connection limit or loaded guide.
#[derive(Clone)]
pub(crate) struct SingleSchedule {
    pub(crate) source: SourceId,
    pub(crate) schedule: Rc<EpgSchedule>,
    pub(crate) limit: Option<u32>,
    pub(crate) pending: Option<Rc<ImportWork>>,
    pub(crate) revision: Option<Weak<ImportWork>>,
}

#[derive(Default)]
pub(crate) struct StatusSnapshot {
    pub(crate) library: Option<LibrarySnapshot>,
    pub(crate) combined: bool,
    pub(crate) enabled: Vec<SourceId>,
    pub(crate) guides: BTreeMap<SourceId, StoredGuide>,
    pub(crate) lines: BTreeMap<SourceId, Vec<SourceLine>>,
}
