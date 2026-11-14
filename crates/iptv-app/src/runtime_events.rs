//! The shell's single application-event entry point.
use crate::{Ui, runtime::RuntimeEvent};

pub(crate) fn apply(ui: Ui, event: RuntimeEvent) {
    match event {
        RuntimeEvent::ScheduleReady(schedule) => crate::set_schedule(&ui, schedule),
        RuntimeEvent::OrganizationReady { schedule, snapshot } => {
            if std::rc::Rc::ptr_eq(&schedule, &crate::schedule())
                && let Some(epg) = schedule
                    .as_any()
                    .and_then(|s| s.downcast_ref::<iptv_guide::EpgSchedule>())
            {
                let kept = crate::lists::keep(ui);
                snapshot.install(epg);
                crate::lists::restore(ui, kept);
                ui.data_version.update(|version| *version += 1);
            }
        }
        RuntimeEvent::SourceReady(resume) => crate::player::source_ready(ui, resume),
        RuntimeEvent::SourceChanged => crate::player::source_changed(ui),
        RuntimeEvent::AccountStreams(limit) => crate::multiview::set_account_streams(limit),
        RuntimeEvent::ForgetSource(source) => crate::player::forget_source(source),
        RuntimeEvent::OpenPlaylists => crate::playlists_view::open(ui),
        RuntimeEvent::ClosePlaylists => crate::playlists_view::close(ui),
        RuntimeEvent::FirstRun => crate::wizard_view::open_first_run(ui),
        RuntimeEvent::Notify(message) => crate::settings_view::notify(message),
        RuntimeEvent::ResumeFullscreen => {
            ui.mode.set(crate::ViewMode::Fullscreen);
            ui.show_playing();
        }
        RuntimeEvent::GuideChanged => ui.guide_pages.update(|version| *version += 1),
        RuntimeEvent::LibraryChanged => ui.library.version.update(|version| *version += 1),
        RuntimeEvent::SettingsChanged => ui.settings.update(|version| *version += 1),
    }
}
