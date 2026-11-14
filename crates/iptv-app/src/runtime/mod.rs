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

type EventSink = Rc<dyn Fn(RuntimeEvent)>;
type PageWaiters = RefCell<Vec<Box<dyn FnOnce()>>>;

pub(crate) struct Runtime {
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
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl Runtime {
    pub(crate) fn bind(&self, sink: impl Fn(RuntimeEvent) + 'static) {
        *self.events.borrow_mut() = Some(Rc::new(sink));
    }

    pub(crate) fn emit(&self, event: RuntimeEvent) {
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
