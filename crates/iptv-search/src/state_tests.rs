use iptv_core::guide::Airing;

use super::SearchAction::*;
use super::*;
use crate::text::MatchQuality;

fn ch(channel: usize) -> ChannelHit {
    ChannelHit {
        channel,
        score: MatchQuality::Prefix,
    }
}

fn pr(channel: usize, start: UnixTime) -> ProgrammeHit {
    ProgrammeHit {
        channel,
        start,
        end: start + 1800,
        title: format!("Show {channel}-{start}"),
        airing: Airing::Upcoming,
        catchup: false,
        score: MatchQuality::Word,
    }
}

fn typed(q: &str) -> SearchState {
    let mut s = SearchState::new(SearchHistory::default());
    assert_eq!(s.apply(Type(q.into())), SearchEffect::Search(q.into()));
    s
}

const P: fn(usize, UnixTime) -> ResultId = |channel, start| ResultId::Programme { channel, start };

#[test]
fn idle_shows_history_and_picks_an_entry() {
    let history = SearchHistory::from_list(["kestrel", "orchard"], 20);
    let mut s = SearchState::new(history);
    assert_eq!(s.phase(), Phase::Idle);
    assert!(s.keyboard_visible());
    s.apply(Down);
    s.apply(Down);
    assert_eq!(s.focus(), &Focus::History("orchard".into()));
    s.apply(Down);
    assert_eq!(
        s.focus(),
        &Focus::History("orchard".into()),
        "stays on the last"
    );
    assert_eq!(s.apply(Ok), SearchEffect::Search("orchard".into()));
    assert_eq!((s.query(), s.phase()), ("orchard", Phase::Typing));
    assert!(!s.keyboard_visible());
    assert_eq!(s.history().entries(), ["orchard", "kestrel"]);
    // Keyboard hidden: results take focus.
    assert!(s.set_results("orchard", vec![ch(5)], vec![]));
    assert_eq!(s.focus(), &Focus::Result(ResultId::Channel(5)));
    // Up from history's first entry goes back to the input.
    let mut s = SearchState::new(SearchHistory::from_list(["one"], 20));
    s.apply(Down);
    s.apply(Up);
    assert_eq!(s.focus(), &Focus::Input);
}

#[test]
fn typing_drops_stale_results_and_submit_moves_focus() {
    let mut s = typed("ke");
    assert_eq!(
        s.apply(Type("kes".into())),
        SearchEffect::Search("kes".into())
    );
    assert!(!s.set_results("ke", vec![ch(1)], vec![]), "stale");
    assert_eq!(s.phase(), Phase::Typing);
    assert!(s.set_results("kes", vec![ch(1), ch(7)], vec![pr(2, 100)]));
    assert_eq!(s.phase(), Phase::Results);
    assert_eq!(s.focus(), &Focus::Input, "keyboard still up");
    assert_eq!(s.apply(Ok), SearchEffect::None);
    assert!(!s.keyboard_visible());
    assert_eq!(s.focus(), &Focus::Result(ResultId::Channel(1)));
    assert_eq!(s.history().entries(), ["kes"]);
}

#[test]
fn moves_between_input_tabs_and_results_keeping_focus_per_tab() {
    let mut s = typed("owl");
    s.set_results("owl", vec![ch(1), ch(7)], vec![pr(2, 100), pr(3, 50)]);
    s.apply(Down);
    assert_eq!(s.focus(), &Focus::Tabs);
    s.apply(Down);
    s.apply(Down);
    s.apply(Down);
    assert_eq!(s.focus(), &Focus::Result(ResultId::Channel(7)));
    s.apply(Right);
    assert_eq!(s.tab(), Tab::Programs);
    assert_eq!(s.focus(), &Focus::Result(P(2, 100)));
    s.apply(Down);
    s.apply(Left);
    assert_eq!(
        s.focus(),
        &Focus::Result(ResultId::Channel(7)),
        "remembered"
    );
    s.apply(Right);
    assert_eq!(s.focus(), &Focus::Result(P(3, 50)), "remembered");
    s.apply(Up);
    s.apply(Up);
    assert_eq!(s.focus(), &Focus::Tabs);
    s.apply(Left);
    assert_eq!((s.tab(), s.focus()), (Tab::Channels, &Focus::Tabs));
    s.apply(Up);
    assert_eq!(s.focus(), &Focus::Input);
}

#[test]
fn play_and_come_back_keeps_query_and_focused_result() {
    let mut s = typed("owl");
    s.set_results("owl", vec![ch(1)], vec![pr(2, 100), pr(3, 50), pr(3, 900)]);
    s.apply(Ok);
    s.apply(Right);
    s.apply(Down);
    assert_eq!(s.focus(), &Focus::Result(P(3, 50)));
    assert_eq!(s.apply(Ok), SearchEffect::Play(P(3, 50)));
    // Back from the player: refresh, results come back in another order.
    assert_eq!(s.resume(), SearchEffect::Search("owl".into()));
    s.set_results("owl", vec![ch(1)], vec![pr(3, 50), pr(9, 10), pr(2, 100)]);
    assert_eq!((s.query(), s.tab()), ("owl", Tab::Programs));
    assert_eq!(
        s.focus(),
        &Focus::Result(P(3, 50)),
        "same item, not same row"
    );
    // The programme ended and dropped out: the same channel's next one.
    s.resume();
    s.set_results("owl", vec![ch(1)], vec![pr(2, 100), pr(3, 900)]);
    assert_eq!(s.focus(), &Focus::Result(P(3, 900)));
    // Nothing left on that channel: the first result.
    s.resume();
    s.set_results("owl", vec![ch(1)], vec![pr(4, 1), pr(2, 100)]);
    assert_eq!(s.focus(), &Focus::Result(P(4, 1)));
    assert_eq!(s.history().entries(), ["owl"]);
}

#[test]
fn switches_to_the_tab_with_results_and_handles_no_results() {
    let mut s = typed("nuit");
    s.set_results("nuit", vec![], vec![pr(1, 0)]);
    assert_eq!(s.tab(), Tab::Programs);
    s.apply(Ok);
    assert_eq!(s.focus(), &Focus::Result(P(1, 0)));
    s.apply(Left);
    assert_eq!(s.tab(), Tab::Programs, "no empty tab to switch to");
    s.apply(Type("nuitx".into()));
    s.set_results("nuitx", vec![], vec![]);
    assert_eq!((s.phase(), s.focus()), (Phase::NoResults, &Focus::Input));
    s.apply(Down);
    assert_eq!(s.focus(), &Focus::Input);
}

#[test]
fn back_hides_keyboard_then_returns_to_input_then_closes() {
    let mut s = typed("owl");
    s.set_results("owl", vec![ch(1)], vec![]);
    assert_eq!(s.apply(Back), SearchEffect::None);
    assert!(!s.keyboard_visible());
    assert_eq!(s.phase(), Phase::Results, "results stay");
    s.apply(Down);
    s.apply(Down);
    assert_eq!(s.apply(Back), SearchEffect::None);
    assert_eq!(s.focus(), &Focus::Input);
    assert_eq!(s.apply(Back), SearchEffect::Close);
    s.apply(ShowKeyboard);
    assert!(s.keyboard_visible());
    assert_eq!(s.apply(Clear), SearchEffect::None);
    assert_eq!((s.phase(), s.query()), (Phase::Idle, ""));
    assert!(s.channels().is_empty());
    assert!(
        !s.set_results("", vec![ch(1)], vec![]),
        "idle ignores results"
    );
    assert_eq!(s.apply(Back), SearchEffect::Close);
    assert_eq!(s.resume(), SearchEffect::None);
}
