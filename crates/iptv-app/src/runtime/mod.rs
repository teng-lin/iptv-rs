//! Application-owned state and events. This module has no UI toolkit imports.
mod events;
pub(crate) mod state;
pub(crate) use events::RuntimeEvent;

use iptv_core::guide::{Schedule, SyntheticSchedule};
use std::{cell::RefCell, rc::Rc};

use iptv_guide::EpgSchedule;
use iptv_import::CancelToken;
use iptv_library::SourceId;
use state::{ImportWork, LibrarySnapshot, SingleSchedule, StatusSnapshot};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Weak;

#[derive(Default)]
pub(crate) struct Slot<T>(T);
impl<T> Slot<T> {
    pub(crate) fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0)
    }
}
impl<T> std::ops::Deref for Slot<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

pub(crate) type LocalTask = std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>;
struct TaskHost {
    spawn: Box<dyn Fn(LocalTask)>,
    delay: Box<dyn Fn(std::time::Duration) -> LocalTask>,
}

type EventSink = Rc<dyn Fn(RuntimeEvent)>;
type PageWaiters = RefCell<Vec<Box<dyn FnOnce()>>>;
type Resolves = RefCell<[Option<(u64, CancelToken)>; iptv_core::multiview::MAX_SURFACES]>;
type SavedSettings = RefCell<Option<(u64, std::sync::Arc<iptv_settings::Settings>)>>;

pub(crate) struct Runtime {
    pub(crate) account_streams: Slot<Cell<Option<u32>>>,
    pub(crate) resolving: Slot<Resolves>,
    pub(crate) wizard_import: Slot<RefCell<Option<(iptv_core::wizard::ImportId, CancelToken)>>>,
    store: std::sync::Arc<store_service::Service>,
    pub(crate) settings_saved: Slot<SavedSettings>,
    pub(crate) settings_changed: Slot<Cell<u64>>,
    pub(crate) backup_busy: Cell<bool>,
    pub(crate) backup_progress: Cell<backup::Progress>,
    pub(crate) search_indexes: Slot<RefCell<search::Indexes>>,
    pub(crate) saved_search: Slot<RefCell<Option<Vec<String>>>>,
    pub(crate) pending_search: Slot<Cell<u64>>,
    pub(crate) favorites: Slot<RefCell<favorites::State>>,
    pub(crate) favorite_writer: Slot<RefCell<Option<std::sync::mpsc::Sender<favorites::Write>>>>,
    pub(crate) favorite_unsaved: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) activity: RefCell<BTreeMap<SourceId, library::Activity>>,
    pub(crate) all: Cell<bool>,
    pub(crate) generation: Slot<Cell<u64>>,
    pub(crate) imports: Slot<RefCell<HashMap<SourceId, (u64, CancelToken)>>>,
    pub(crate) attempts: Slot<RefCell<HashMap<SourceId, i64>>>,
    pub(crate) tickets: Slot<Cell<u64>>,
    pub(crate) completions: Slot<RefCell<HashMap<SourceId, Weak<ImportWork>>>>,
    pub(crate) library: Slot<RefCell<Option<LibrarySnapshot>>>,
    pub(crate) parts: Slot<RefCell<BTreeMap<SourceId, Rc<EpgSchedule>>>>,
    pub(crate) limits: Slot<RefCell<BTreeMap<SourceId, Option<u32>>>>,
    pub(crate) resume: Slot<Cell<bool>>,
    pub(crate) rebuild: Slot<Cell<u64>>,
    pub(crate) epoch: Slot<Cell<i64>>,
    pub(crate) single: Slot<RefCell<Option<SingleSchedule>>>,
    pub(crate) shown: Slot<Cell<Option<SourceId>>>,
    pub(crate) updating: Slot<RefCell<Vec<SourceId>>>,
    pub(crate) edits: Slot<RefCell<BTreeMap<SourceId, u64>>>,
    pub(crate) status: Slot<RefCell<StatusSnapshot>>,
    pub(crate) page_waiters: Slot<PageWaiters>,
    pub(crate) organization_revision: Slot<Cell<u64>>,
    schedule: RefCell<Rc<dyn Schedule>>,
    events: RefCell<Option<EventSink>>,
    tasks: RefCell<Option<Rc<TaskHost>>>,
    pub(crate) foreground: Cell<bool>,
    pub(crate) scheduler_started: Cell<bool>,
}

impl Default for Runtime {
    fn default() -> Self {
        let store = std::sync::Arc::new(store_service::Service::default());
        Self {
            account_streams: Slot::default(),
            resolving: Slot::default(),
            wizard_import: Slot::default(),
            store: store.clone(),
            settings_saved: Slot::default(),
            settings_changed: Slot::default(),
            backup_busy: Cell::new(false),
            backup_progress: Cell::new(backup::Progress::Idle),
            search_indexes: Slot::default(),
            saved_search: Slot::default(),
            pending_search: Slot::default(),
            favorites: Slot::default(),
            favorite_writer: Slot::default(),
            favorite_unsaved: Default::default(),
            activity: RefCell::default(),
            all: Cell::new(false),
            generation: Slot::default(),
            imports: Slot::default(),
            attempts: Slot::default(),
            tickets: Slot::default(),
            completions: Slot::default(),
            library: Slot::default(),
            parts: Slot::default(),
            limits: Slot::default(),
            resume: Slot::default(),
            rebuild: Slot::default(),
            epoch: Slot::default(),
            single: Slot::default(),
            shown: Slot::default(),
            updating: Slot::default(),
            edits: Slot::default(),
            status: Slot::default(),
            page_waiters: Slot::default(),
            organization_revision: Slot::default(),
            schedule: RefCell::new(Rc::new(SyntheticSchedule)),
            events: RefCell::new(None),
            tasks: RefCell::new(None),
            foreground: Cell::new(true),
            scheduler_started: Cell::new(false),
        }
    }
}

impl Runtime {
    pub(crate) fn bind_tasks(
        &self,
        spawn: impl Fn(LocalTask) + 'static,
        delay: impl Fn(std::time::Duration) -> LocalTask + 'static,
    ) {
        *self.tasks.borrow_mut() = Some(Rc::new(TaskHost {
            spawn: Box::new(spawn),
            delay: Box::new(delay),
        }));
    }
    pub(crate) fn spawn(&self, task: impl std::future::Future<Output = ()> + 'static) {
        let host = self
            .tasks
            .borrow()
            .clone()
            .expect("runtime task host installed");
        (host.spawn)(Box::pin(task));
    }
    pub(crate) async fn delay(&self, duration: std::time::Duration) {
        let host = self
            .tasks
            .borrow()
            .clone()
            .expect("runtime task host installed");
        (host.delay)(duration).await;
    }
    pub(crate) fn with_context<R>(&self, run: impl FnOnce() -> R) -> R {
        store_service::with_service(self.store.clone(), run)
    }
    pub(crate) fn scope_future<F: std::future::Future + 'static>(
        &self,
        future: F,
    ) -> impl std::future::Future<Output = F::Output> + 'static + use<F> {
        store_service::scope_future(self.store.clone(), future)
    }
    pub(crate) fn flush_files(&self, limit: std::time::Duration) -> bool {
        self.with_context(|| file_writer::flush(limit))
    }

    pub(crate) fn activate(&self) {
        store_service::activate(self.store.clone());
    }
    pub(crate) fn start_services(&self) {
        store_service::with_service(self.store.clone(), store_service::register_ui_thread);
    }
    pub(crate) fn drain_services(&self) -> bool {
        let drained =
            store_service::with_service(self.store.clone(), store_service::drain_on_shutdown);
        // Detached local tasks can retain this Runtime after the root closes.
        // Retire services now, rather than waiting for its last Rc to drop.
        self.store.retire();
        drained
    }

    pub(crate) fn bind(&self, sink: impl Fn(RuntimeEvent) + 'static) {
        *self.events.borrow_mut() = Some(Rc::new(sink));
    }

    pub(crate) fn emit(&self, event: RuntimeEvent) {
        if let RuntimeEvent::AccountStreams(limit) = &event {
            self.account_streams.set(*limit);
        }
        let sink = self.events.borrow().clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }

    pub(crate) fn schedule(&self) -> Rc<dyn Schedule> {
        self.schedule.borrow().clone()
    }

    pub(crate) fn set_schedule(&self, schedule: Rc<dyn Schedule>) {
        *self.schedule.borrow_mut() = schedule;
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.store.retire();
        if let Some((_, cancel)) = self.wizard_import.borrow().as_ref() {
            cancel.cancel();
        }
        for (_, cancel) in self.resolving.borrow().iter().flatten() {
            cancel.cancel();
        }
        for (_, cancel) in self.imports.borrow().values() {
            cancel.cancel();
        }
        for completion in self.completions.borrow().values().filter_map(Weak::upgrade) {
            completion.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_runtimes_can_apply_events_during_delivery() {
        let first = Rc::new(Runtime::default());
        let second = Runtime::default();
        let original_second = second.schedule();
        let target: Rc<dyn Schedule> = Rc::new(SyntheticSchedule);
        let weak = Rc::downgrade(&first);
        first.bind(move |event| {
            if let RuntimeEvent::ScheduleReady(schedule) = event {
                weak.upgrade().unwrap().set_schedule(schedule);
            }
        });
        first.emit(RuntimeEvent::ScheduleReady(target.clone()));
        assert!(Rc::ptr_eq(&first.schedule(), &target));
        assert!(Rc::ptr_eq(&second.schedule(), &original_second));
        assert!(!Rc::ptr_eq(&second.schedule(), &target));
    }

    #[test]
    fn closing_one_library_cancels_only_its_own_imports() {
        let first = Runtime::default();
        let second = Runtime::default();
        let first_cancel = CancelToken::new();
        let second_cancel = CancelToken::new();
        first
            .imports
            .borrow_mut()
            .insert(SourceId(1), (1, first_cancel.clone()));
        second
            .imports
            .borrow_mut()
            .insert(SourceId(1), (1, second_cancel.clone()));
        drop(first);
        assert!(first_cancel.is_cancelled());
        assert!(!second_cancel.is_cancelled());
    }
}

mod store_service;

pub(crate) mod executor;

pub(crate) mod file_writer;

pub(crate) mod commands;

pub(crate) mod imports;

pub(crate) mod stored;

pub(crate) mod opening;

pub(crate) mod paging;

pub(crate) mod playback;

pub(crate) mod guides;

pub(crate) mod workers;

pub(crate) mod wizard_map;

pub(crate) mod private_files;

pub(crate) mod library;

pub(crate) mod favorites;

pub(crate) mod search;

pub(crate) mod backup;

pub(crate) mod assign;

pub(crate) mod credential_store;
pub(crate) mod model;

pub(crate) mod settings;

#[cfg(feature = "test-support")]
pub use store_service::occupy_writer;

mod paths;
