//! Application-owned state and events. This module has no UI toolkit imports.
mod events;
pub(crate) use events::RuntimeEvent;

use iptv_core::guide::{Schedule, SyntheticSchedule};
use std::{cell::RefCell, rc::Rc};

type EventSink = Rc<dyn Fn(RuntimeEvent)>;

pub(crate) struct Runtime {
    schedule: RefCell<Rc<dyn Schedule>>,
    events: RefCell<Option<EventSink>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
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
}
