use super::*;
use crate::guide::{GROUPS, SyntheticSchedule, group_start};

fn policy() -> RetryPolicy {
    RetryPolicy {
        first_delay_ms: 1_000,
        max_delay_ms: 8_000,
        max_attempts: 4,
        stable_ms: 10_000,
        stall_ms: 15_000,
    }
}

fn playing(h: &mut Health, now: u64) {
    h.resolved(now);
    assert_eq!(h.on_state(PlayerState::Buffering, now), None);
    assert_eq!(h.on_state(PlayerState::Ready, now), None);
    assert_eq!(h.phase(), &Phase::Playing);
}

#[test]
fn backoff_doubles_up_to_the_cap() {
    let p = policy();
    let delays: Vec<u64> = (1..=6).map(|a| p.delay_ms(a)).collect();
    assert_eq!(delays, [1_000, 2_000, 4_000, 8_000, 8_000, 8_000]);
    assert_eq!(RetryPolicy::default().delay_ms(100), 16_000, "no overflow");
}

#[test]
fn live_errors_retry_with_backoff_then_stay_failed() {
    let mut h = Health::new(policy());
    let session = h.start(true, 0);
    h.resolved(0);
    let mut now = 100;
    let mut waits = Vec::new();
    for attempt in 1..=4 {
        let r = h
            .on_error(ErrorKind::Network, "ERROR_CODE_IO".into(), now)
            .expect("retry scheduled");
        assert_eq!((r.attempt, r.session), (attempt, session));
        waits.push(r.at_ms - now);
        assert!(h.waiting(), "retrying shows the waiting indicator");
        assert!(!h.due(r, r.at_ms - 1), "not before its time");
        assert!(h.due(r, r.at_ms));
        assert_eq!(h.phase(), &Phase::Resolving);
        h.resolved(r.at_ms);
        now = r.at_ms + 50;
    }
    assert_eq!(waits, [1_000, 2_000, 4_000, 8_000]);
    assert_eq!(h.on_error(ErrorKind::Network, "x".into(), now), None);
    let f = h.failure().expect("gave up");
    assert_eq!((f.kind.clone(), f.attempts), (ErrorKind::Network, 4));
    // Further player noise does not leave the failed state.
    assert_eq!(h.on_state(PlayerState::Buffering, now + 1), None);
    assert!(h.failure().is_some());
}

#[test]
fn a_manual_retry_starts_over() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    h.resolved(0);
    for _ in 0..5 {
        if let Some(r) = h.on_error(ErrorKind::Network, "x".into(), 10) {
            assert!(h.due(r, r.at_ms));
        }
    }
    assert!(h.failure().is_some());
    let old = h.session();
    let new = h.retry_now(20_000);
    assert!(new > old);
    assert_eq!(h.phase(), &Phase::Resolving);
    let r = h.on_error(ErrorKind::Network, "x".into(), 20_001).unwrap();
    assert_eq!(r.attempt, 1, "attempts were reset");
}

#[test]
fn refusals_formats_and_catch_up_do_not_retry() {
    for kind in [
        ErrorKind::Http(403),
        ErrorKind::Http(401),
        ErrorKind::Format,
    ] {
        let mut h = Health::new(policy());
        h.start(true, 0);
        assert_eq!(h.on_error(kind.clone(), "x".into(), 5), None, "{kind:?}");
        assert_eq!(h.failure().map(|f| f.attempts), Some(0));
    }
    let mut h = Health::new(policy());
    h.start(true, 0);
    assert!(
        h.on_error(ErrorKind::Http(503), "x".into(), 5).is_some(),
        "server errors retry"
    );
    let mut archive = Health::new(policy());
    archive.start(false, 0);
    assert_eq!(archive.on_error(ErrorKind::Network, "x".into(), 5), None);
    assert!(archive.failure().is_some());
}

#[test]
fn stable_playback_resets_the_attempt_count() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    h.resolved(0);
    let r = h.on_error(ErrorKind::Network, "x".into(), 10).unwrap();
    assert_eq!(r.attempt, 1);
    assert!(h.due(r, r.at_ms));
    playing(&mut h, r.at_ms);
    // Short playback: the next error is attempt 2.
    let r = h
        .on_error(ErrorKind::Network, "x".into(), r.at_ms + 500)
        .unwrap();
    assert_eq!(r.attempt, 2);
    assert!(h.due(r, r.at_ms));
    playing(&mut h, r.at_ms);
    // A rebuffer does not restart the stability clock.
    assert_eq!(h.on_state(PlayerState::Buffering, r.at_ms + 4_000), None);
    assert_eq!(h.on_state(PlayerState::Ready, r.at_ms + 5_000), None);
    let r = h
        .on_error(ErrorKind::Network, "x".into(), r.at_ms + 10_000)
        .unwrap();
    assert_eq!(r.attempt, 1, "played 10 s: counts afresh");
}

#[test]
fn a_live_stream_that_ends_reconnects_at_once() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    playing(&mut h, 0);
    let r = h.on_state(PlayerState::Ended, 30_000).expect("reconnect");
    assert_eq!(r.at_ms, 30_000);
    // An archive that ends is simply over.
    let mut a = Health::new(policy());
    a.start(false, 0);
    playing(&mut a, 0);
    assert_eq!(a.on_state(PlayerState::Ended, 60_000), None);
    assert_eq!(a.phase(), &Phase::Idle);
    assert!(a.failure().is_none());
}

#[test]
fn loading_or_buffering_too_long_is_a_stall() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    h.resolved(100);
    assert_eq!(h.stall_deadline(), Some(15_100));
    assert_eq!(h.tick(15_099), None);
    let r = h.tick(15_100).expect("stall retries");
    assert_eq!(
        h.phase(),
        &Phase::Retrying {
            attempt: 1,
            at_ms: r.at_ms,
            last: ErrorKind::Stalled
        }
    );
    assert!(h.due(r, r.at_ms));
    playing(&mut h, r.at_ms);
    assert_eq!(h.stall_deadline(), None, "no deadline while playing");
    h.on_state(PlayerState::Buffering, 30_000);
    assert_eq!(h.stall_deadline(), Some(45_000));
    // Paused: waiting is expected, the deadline restarts on resume.
    h.set_paused(true, 31_000);
    assert_eq!(h.stall_deadline(), None);
    assert_eq!(h.tick(50_000), None);
    h.set_paused(false, 50_000);
    assert_eq!(h.stall_deadline(), Some(65_000));
}

#[test]
fn stale_retries_and_sessions_are_ignored() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    let r = h.on_error(ErrorKind::Network, "x".into(), 1).unwrap();
    // The viewer zapped meanwhile.
    h.start(true, 500);
    assert!(!h.due(r, r.at_ms), "older session");
    h.stop();
    assert_eq!(h.phase(), &Phase::Idle);
    assert_eq!(h.on_error(ErrorKind::Network, "x".into(), 2), None);
    assert_eq!(h.on_state(PlayerState::Ready, 3), None);
    assert_eq!(h.phase(), &Phase::Idle);
}

#[test]
fn player_codes_map_to_kinds() {
    use ErrorKind as K;
    let k = ErrorKind::from_player_code;
    assert_eq!(
        k("ERROR_CODE_IO_NETWORK_CONNECTION_FAILED", None),
        K::Network
    );
    assert_eq!(k("ERROR_CODE_IO_BAD_HTTP_STATUS", Some(403)), K::Http(403));
    assert_eq!(k("ERROR_CODE_IO_BAD_HTTP_STATUS", None), K::Http(0));
    assert_eq!(
        k("ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED", None),
        K::Format
    );
    assert_eq!(k("ERROR_CODE_DECODING_FAILED", None), K::Format);
    assert_eq!(k("ERROR_CODE_BEHIND_LIVE_WINDOW", None), K::Network);
    assert_eq!(k("ERROR_CODE_UNSPECIFIED", None), K::Other);
}

#[test]
fn play_source_headers_are_case_insensitive_and_override() {
    let channel = vec![
        ("user-agent".to_owned(), "ChannelAgent/1".to_owned()),
        ("Referer".to_owned(), "http://ref.example.com/".to_owned()),
    ];
    let mut s = PlaySource::new("http://tv.example.com/1.m3u8");
    s.set_header("User-Agent", "SourceAgent/2");
    s.set_header("X-Blank", "  ");
    let s = s.with_headers(&channel);
    assert_eq!(s.header("USER-AGENT"), Some("ChannelAgent/1"));
    assert_eq!(s.header("referer"), Some("http://ref.example.com/"));
    assert_eq!(s.headers.len(), 2, "replaced, blank skipped");
    let mut s = s;
    s.default_header("User-Agent", "Fallback/3");
    assert_eq!(s.header("User-Agent"), Some("ChannelAgent/1"));
}

#[test]
fn previous_channel_toggles() {
    let mut h = ChannelHistory::default();
    assert_eq!(h.previous(), None);
    h.played("a");
    h.played("a");
    assert_eq!(h.previous(), None, "replaying is not a change");
    h.played("b");
    h.played("c");
    assert_eq!((h.current(), h.previous()), (Some("c"), Some("b")));
    let prev = h.previous().unwrap().to_owned();
    h.played(&prev);
    assert_eq!((h.current(), h.previous()), (Some("b"), Some("c")));
}

#[test]
fn zapping_wraps_within_the_group() {
    let s = SyntheticSchedule;
    let (g0, g1) = (group_start(0), group_start(1));
    assert_eq!(group_range(&s, g0), g0..g1);
    assert_eq!(group_range(&s, g1 - 1), g0..g1);
    assert_eq!(
        zap_target(&s, g1 - 1, 1),
        g0,
        "down from the last wraps to the first"
    );
    assert_eq!(
        zap_target(&s, g0, -1),
        g1 - 1,
        "up from the first wraps to the last"
    );
    assert_eq!(zap_target(&s, g0, 1), g0 + 1);
    let last = group_start(GROUPS - 1);
    let end = crate::guide::CHANNELS;
    assert_eq!(group_range(&s, end - 1), last..end);
    assert_eq!(zap_target(&s, end - 1, 1), last);
}

/// A schedule with uneven groups, an empty group and one channel groups.
struct Groups(Vec<usize>, usize);

impl Schedule for Groups {
    fn channel_count(&self) -> usize {
        self.1
    }
    fn group_count(&self) -> usize {
        self.0.len()
    }
    fn group_start(&self, group: usize) -> usize {
        self.0[group]
    }
    fn program_at(&self, _: usize, minute: i32) -> crate::guide::Program {
        crate::guide::Program {
            start: minute,
            end: minute + 1,
        }
    }
    fn programs_between(&self, _: usize, _: i32, _: i32) -> Vec<crate::guide::Program> {
        Vec::new()
    }
    fn catchup_days(&self, _: usize) -> u8 {
        0
    }
    fn channel_name(&self, _: usize) -> String {
        String::new()
    }
    fn group_name(&self, _: usize) -> String {
        String::new()
    }
    fn title(&self, _: usize, _: crate::guide::Program) -> String {
        String::new()
    }
}

#[test]
fn zapping_handles_uneven_and_single_channel_groups() {
    // Groups: [0,3) [3,4) [4,8)
    let s = Groups(vec![0, 3, 4], 8);
    assert_eq!(group_range(&s, 3), 3..4);
    assert_eq!(zap_target(&s, 3, 1), 3, "a one-channel group stays");
    assert_eq!(zap_target(&s, 7, 1), 4);
    assert_eq!(zap_target(&s, 2, 1), 0);
    assert_eq!(zap_target(&s, 4, -3), 5);
    let none = Groups(vec![], 5);
    assert_eq!(zap_target(&none, 4, 1), 0, "no groups: the whole list");
    let empty = Groups(vec![], 0);
    assert_eq!(zap_target(&empty, 0, 1), 0);
}

#[test]
fn the_previous_stream_reports_nothing_while_resolving() {
    let mut h = Health::new(policy());
    h.start(true, 0);
    assert_eq!(h.on_state(PlayerState::Ready, 5), None);
    assert_eq!(h.on_state(PlayerState::Ended, 6), None);
    assert_eq!(
        h.phase(),
        &Phase::Resolving,
        "still waiting for the address"
    );
    // A resolver failure is an error like any other.
    assert!(
        h.on_error(ErrorKind::Resolve, "Network".into(), 7)
            .is_some()
    );
}
