//! Search and add: multiview's tile menu opens the search screen to pick
//! a channel for a new tile. The screen shows over the grid without
//! leaving full screen (leaving it would end multiview); choosing a
//! channel, or a programme (its channel), hands the channel to multiview
//! (`multiview::picked`) and closes the screen, so the grid is back with
//! the new tile. Back (or Close) closes it without one
//! (`multiview::list_closed`, from [`super::close`]).

use super::*;

/// Opens a fresh search screen picking for multiview. A screen waiting
/// for playback to end (`search.stay_on_search`) gives way to it.
pub fn open_to_pick(ui: Ui) {
    if ui.search.with_untracked(Option::is_some) {
        close(ui);
    }
    let history = if options(&crate::settings::get()).show_history {
        load_history()
    } else {
        SearchHistory::default()
    };
    log::info!("iptv search: picking for multiview");
    ui.search.set(Some(Screen {
        state: SearchState::new(history),
        suspended: false,
        programmes: Programmes::Building,
        pick: true,
    }));
    prepare(ui);
}

/// Whether the screen picks for multiview.
pub fn picking(ui: Ui) -> bool {
    ui.search.with(|s| s.as_ref().is_some_and(|s| s.pick))
}

/// The channel a result adds: a channel's own, a programme's channel
/// (whenever it airs: the tile plays the channel live).
pub(super) fn channel_of(id: ResultId) -> usize {
    match id {
        ResultId::Channel(c) | ResultId::Programme { channel: c, .. } => c,
    }
}

/// Chooses result `id` while picking for multiview: its channel becomes a
/// new tile and the screen closes. `false` when not picking.
pub(super) fn choose(ui: Ui, id: ResultId) -> bool {
    if !untracked(|| picking(ui)) {
        return false;
    }
    crate::multiview::picked(ui, channel_of(id));
    close(ui);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_adds_its_channel() {
        assert_eq!(channel_of(ResultId::Channel(4)), 4);
        let programme = ResultId::Programme {
            channel: 7,
            start: 1_000,
        };
        assert_eq!(channel_of(programme), 7);
    }
}
