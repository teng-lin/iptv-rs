//! Playback state that does not depend on a player: the resolved stream
//! (URL plus request headers), the player's health with retry/backoff and
//! stall detection, the previous-channel history and zapping within a group.
//!
//! Times are milliseconds on the caller's monotonic clock. Nothing here
//! sleeps: [`Health`] says *when* something is due and the shell sets the
//! timer, then calls back in.

use std::ops::Range;

use crate::guide::Schedule;

/// What the player should open: a URL and the HTTP headers to send with it
/// (User-Agent, Referer, provider identity). Header names keep their case;
/// lookups are case-insensitive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaySource {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl PlaySource {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Sets `name` (replacing any value under the same name, any case).
    /// Blank values are ignored.
    pub fn set_header(&mut self, name: &str, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        match self
            .headers
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            Some(slot) => slot.1 = value.to_owned(),
            None => self.headers.push((name.to_owned(), value.to_owned())),
        }
    }

    /// Adds `name` only when no value is set yet (a fallback).
    pub fn default_header(&mut self, name: &str, value: &str) {
        if self.header(name).is_none() {
            self.set_header(name, value);
        }
    }

    /// Applies `headers` over the current ones (later wins).
    pub fn with_headers<'a>(
        mut self,
        headers: impl IntoIterator<Item = &'a (String, String)>,
    ) -> Self {
        for (k, v) in headers {
            self.set_header(k, v);
        }
        self
    }
}

/// Why playback failed. Decides whether a retry makes sense.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Connection, timeout or I/O failure.
    Network,
    /// The server answered with this HTTP status.
    Http(u16),
    /// Container, manifest or codec the player cannot handle.
    Format,
    /// The stream ended (a live TS connection that closed).
    Ended,
    /// Loading or buffering went on too long.
    Stalled,
    /// The provider would not give a stream address (Stalker `create_link`).
    Resolve,
    /// Nothing to play: no stream address or no archive for this programme.
    Unavailable,
    Other,
}

impl ErrorKind {
    /// Maps a Media3 `PlaybackException.errorCodeName` (and the HTTP status
    /// when the host found one) to a kind.
    pub fn from_player_code(code: &str, http_status: Option<u16>) -> Self {
        if let Some(status) = http_status.filter(|s| *s >= 400) {
            return Self::Http(status);
        }
        let c = code.to_ascii_uppercase();
        if c.contains("PARSING") || c.contains("DECOD") || c.contains("UNSUPPORTED") {
            Self::Format
        } else if c.contains("BAD_HTTP_STATUS") {
            Self::Http(0)
        } else if c.contains("IO_") || c.contains("TIMEOUT") || c.contains("BEHIND_LIVE_WINDOW") {
            Self::Network
        } else {
            Self::Other
        }
    }

    /// Whether an automatic retry can help. Refusals (401/403) and formats
    /// the player cannot open would fail the same way again.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Http(401 | 403) | Self::Format | Self::Unavailable => false,
            Self::Http(_)
            | Self::Network
            | Self::Ended
            | Self::Stalled
            | Self::Resolve
            | Self::Other => true,
        }
    }
}

/// A failure as shown to the viewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: ErrorKind,
    /// The player's or provider's code, for the detail line and logs.
    /// Never contains URLs or credentials.
    pub detail: String,
    /// Automatic attempts made before giving up (0: none were tried).
    pub attempts: u32,
}

/// Retry and stall timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Delay before the first retry; doubles per attempt.
    pub first_delay_ms: u64,
    pub max_delay_ms: u64,
    /// Automatic retries before the error stays on screen.
    pub max_attempts: u32,
    /// Playing this long without trouble resets the attempt count.
    pub stable_ms: u64,
    /// Loading or buffering longer than this is a stall.
    pub stall_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            first_delay_ms: 1_000,
            max_delay_ms: 16_000,
            max_attempts: 5,
            stable_ms: 10_000,
            stall_ms: 15_000,
        }
    }
}

impl RetryPolicy {
    /// The wait before retry number `attempt` (1-based).
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        let shift = attempt.saturating_sub(1).min(20);
        self.first_delay_ms
            .saturating_mul(1 << shift)
            .min(self.max_delay_ms)
    }
}

/// The player state the host reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Buffering,
    Ready,
    Ended,
}

impl PlayerState {
    /// The host's state names (`idle`, `buffering`, `ready`, `ended`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "idle" => Self::Idle,
            "buffering" => Self::Buffering,
            "ready" => Self::Ready,
            "ended" => Self::Ended,
            _ => return None,
        })
    }
}

/// Where playback stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing requested.
    Idle,
    /// Getting the stream address (may need a provider round trip).
    Resolving,
    /// The player opened the stream and has not shown video yet.
    Loading,
    Playing,
    /// Playing stopped to refill the buffer.
    Buffering,
    /// Waiting to retry; `at_ms` is when.
    Retrying {
        attempt: u32,
        at_ms: u64,
        last: ErrorKind,
    },
    /// Stays until the viewer retries or plays something else.
    Failed(Failure),
}

/// A retry the shell should run at `at_ms` (then call [`Health::due`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retry {
    pub at_ms: u64,
    pub attempt: u32,
    /// The playback session it belongs to; a newer session drops it.
    pub session: u64,
}

/// Player health for one playback session: retries live streams with
/// exponential backoff, stops after [`RetryPolicy::max_attempts`], detects
/// stalls. Catch-up never retries on its own (the viewer chose a point in
/// the past; replaying it silently could loop on a dead archive).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub policy: RetryPolicy,
    phase: Phase,
    live: bool,
    paused: bool,
    attempts: u32,
    since_ms: u64,
    ready_since: Option<u64>,
    session: u64,
}

impl Default for Health {
    fn default() -> Self {
        Self::new(RetryPolicy::default())
    }
}

impl Health {
    pub fn new(policy: RetryPolicy) -> Self {
        Self {
            policy,
            phase: Phase::Idle,
            live: true,
            paused: false,
            attempts: 0,
            since_ms: 0,
            ready_since: None,
            session: 0,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Bumped by every [`Health::start`]; results of older sessions are stale.
    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn failure(&self) -> Option<&Failure> {
        match &self.phase {
            Phase::Failed(f) => Some(f),
            _ => None,
        }
    }

    /// Resolving, loading, buffering or waiting to retry: the viewer sees
    /// a waiting indicator.
    pub fn waiting(&self) -> bool {
        matches!(
            self.phase,
            Phase::Resolving | Phase::Loading | Phase::Buffering | Phase::Retrying { .. }
        )
    }

    /// A new stream was requested (channel change, live/catch-up switch or a
    /// manual retry): fresh attempt count, new session.
    pub fn start(&mut self, live: bool, now: u64) -> u64 {
        self.session += 1;
        self.live = live;
        self.attempts = 0;
        self.paused = false;
        self.enter(Phase::Resolving, now);
        self.session
    }

    /// Playback stopped (nothing playing).
    pub fn stop(&mut self) {
        self.session += 1;
        self.attempts = 0;
        self.phase = Phase::Idle;
        self.ready_since = None;
    }

    /// The manual Retry: like [`Health::start`] with the same live flag.
    pub fn retry_now(&mut self, now: u64) -> u64 {
        let live = self.live;
        self.start(live, now)
    }

    /// The stream address is known and handed to the player.
    pub fn resolved(&mut self, now: u64) {
        if self.phase == Phase::Resolving {
            self.enter(Phase::Loading, now);
        }
    }

    pub fn set_paused(&mut self, paused: bool, now: u64) {
        if self.paused && !paused {
            // Waiting while paused is not a stall: the clock starts again.
            self.since_ms = now;
        }
        self.paused = paused;
    }

    /// A scheduled retry fired: `true` if it still applies (then re-resolve
    /// and reopen the stream; the phase is `Resolving` again).
    pub fn due(&mut self, retry: Retry, now: u64) -> bool {
        let current =
            matches!(self.phase, Phase::Retrying { attempt, .. } if attempt == retry.attempt);
        if retry.session != self.session || !current || now < retry.at_ms {
            return false;
        }
        self.enter(Phase::Resolving, now);
        true
    }

    /// The host reported a state change.
    pub fn on_state(&mut self, state: PlayerState, now: u64) -> Option<Retry> {
        match (state, &self.phase) {
            (_, Phase::Idle | Phase::Failed(_) | Phase::Retrying { .. }) => None,
            (PlayerState::Ready, _) => {
                if self.phase != Phase::Playing {
                    // A rebuffer keeps the stability clock running.
                    self.ready_since.get_or_insert(now);
                    self.enter(Phase::Playing, now);
                }
                None
            }
            (PlayerState::Buffering, Phase::Playing) => {
                self.enter(Phase::Buffering, now);
                None
            }
            (PlayerState::Buffering, Phase::Resolving) => {
                self.enter(Phase::Loading, now);
                None
            }
            (PlayerState::Ended, _) if self.live => {
                self.on_error(ErrorKind::Ended, "ended".into(), now)
            }
            (PlayerState::Ended, _) => {
                // A finished archive: nothing more to play, not an error.
                self.enter(Phase::Idle, now);
                None
            }
            _ => None,
        }
    }

    /// The host or the resolver reported an error. Returns the retry to
    /// schedule, or `None` when the failure stays on screen.
    pub fn on_error(&mut self, kind: ErrorKind, detail: String, now: u64) -> Option<Retry> {
        if matches!(self.phase, Phase::Idle | Phase::Failed(_)) {
            return None;
        }
        if self
            .ready_since
            .is_some_and(|t| now.saturating_sub(t) >= self.policy.stable_ms)
        {
            self.attempts = 0;
        }
        self.ready_since = None;
        if !self.live || !kind.retryable() || self.attempts >= self.policy.max_attempts {
            let attempts = self.attempts;
            self.enter(
                Phase::Failed(Failure {
                    kind,
                    detail,
                    attempts,
                }),
                now,
            );
            return None;
        }
        self.attempts += 1;
        let attempt = self.attempts;
        // A live stream that closed after playing reconnects at once.
        let delay = if kind == ErrorKind::Ended && attempt == 1 {
            0
        } else {
            self.policy.delay_ms(attempt)
        };
        let at_ms = now + delay;
        self.enter(
            Phase::Retrying {
                attempt,
                at_ms,
                last: kind,
            },
            now,
        );
        Some(Retry {
            at_ms,
            attempt,
            session: self.session,
        })
    }

    /// When [`Health::tick`] should next run (a stall deadline), if ever.
    pub fn stall_deadline(&self) -> Option<u64> {
        match self.phase {
            Phase::Resolving | Phase::Loading | Phase::Buffering if !self.paused => {
                Some(self.since_ms + self.policy.stall_ms)
            }
            _ => None,
        }
    }

    /// Checks for a stall: loading or buffering past the deadline counts as
    /// an error ([`ErrorKind::Stalled`]).
    pub fn tick(&mut self, now: u64) -> Option<Retry> {
        match self.stall_deadline() {
            Some(at) if now >= at => self.on_error(ErrorKind::Stalled, "stalled".into(), now),
            _ => None,
        }
    }

    fn enter(&mut self, phase: Phase, now: u64) {
        self.phase = phase;
        self.since_ms = now;
    }
}

/// The last two channels played, by content identity (stable keys), for
/// "previous channel".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelHistory {
    current: Option<String>,
    previous: Option<String>,
}

impl ChannelHistory {
    /// `key` started playing. Replaying the current channel changes nothing.
    pub fn played(&mut self, key: &str) {
        if self.current.as_deref() == Some(key) {
            return;
        }
        self.previous = self.current.replace(key.to_owned());
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// The channel before the current one (pressing it again comes back).
    pub fn previous(&self) -> Option<&str> {
        self.previous.as_deref()
    }
}

/// The channels of the group containing `channel` (channels are stored
/// grouped; the last group runs to the end).
pub fn group_range(schedule: &dyn Schedule, channel: usize) -> Range<usize> {
    let count = schedule.channel_count();
    let groups = schedule.group_count();
    if groups == 0 || count == 0 {
        return 0..count;
    }
    // Group starts ascend: binary search for the last start <= channel.
    let (mut lo, mut hi) = (0, groups);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if schedule.group_start(mid) <= channel {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let start = schedule.group_start(lo).min(count);
    let end = if lo + 1 < groups {
        schedule.group_start(lo + 1).clamp(start, count)
    } else {
        count
    };
    if start < end && (start..end).contains(&channel) {
        start..end
    } else {
        0..count
    }
}

/// Channel up/down from `channel` by `step`, wrapping within its group.
pub fn zap_target(schedule: &dyn Schedule, channel: usize, step: isize) -> usize {
    let range = group_range(schedule, channel);
    let len = range.len();
    if len == 0 {
        return channel;
    }
    let at = channel.clamp(range.start, range.end - 1) - range.start;
    range.start + (at as isize + step).rem_euclid(len as isize) as usize
}

#[cfg(test)]
mod tests;
