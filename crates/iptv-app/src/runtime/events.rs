//! Application results applied by the shell through one event sink.
use iptv_core::guide::Schedule;
use iptv_library::SourceId;
use std::rc::Rc;

pub(crate) enum RuntimeEvent {
    ScheduleReady(Rc<dyn Schedule>),
    OrganizationReady {
        schedule: Rc<dyn Schedule>,
        snapshot: super::state::OrganizationSnapshot,
    },
    SourceReady(bool),
    SourceChanged,
    AccountStreams(Option<u32>),
    ForgetSource(SourceId),
    OpenPlaylists,
    ClosePlaylists,
    FirstRun,
    Notify(&'static str),
    ResumeFullscreen,
    GuideChanged,
    LibraryChanged,
    SettingsChanged,
}
