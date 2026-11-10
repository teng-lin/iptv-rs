//! Playback state that does not depend on a player: the resolved stream
//! (URL plus request headers), the player's health with retry/backoff and
//! stall detection ([`Health`]), what a failure means ([`Cause`]), the
//! previous-channel history and zapping within a group.
//!
//! Times are milliseconds on the caller's monotonic clock. Nothing here
//! sleeps: [`Health`] says *when* something is due and the shell sets the
//! timer, then calls back in.

use std::ops::Range;

use crate::guide::Schedule;

mod error;
mod health;
mod states;

pub use error::{Cause, ErrorKind, Failure};
pub use health::Health;
pub use states::{Phase, PlayerState, Retry, RetryPolicy, Waiting};

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

/// How many channels the recent-channels list keeps.
pub const RECENT_MAX: usize = 30;

/// The channels played live, newest first, by content identity (stable
/// keys): "previous channel" and the player's History list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelHistory {
    recent: Vec<String>,
}

impl ChannelHistory {
    /// `key` started playing: it moves to the front. Replaying the current
    /// channel changes nothing.
    pub fn played(&mut self, key: &str) {
        if self.current() == Some(key) {
            return;
        }
        self.recent.retain(|k| k != key);
        self.recent.insert(0, key.to_owned());
        self.recent.truncate(RECENT_MAX);
    }

    pub fn current(&self) -> Option<&str> {
        self.recent.first().map(String::as_str)
    }

    /// The channel before the current one (pressing it again comes back).
    pub fn previous(&self) -> Option<&str> {
        self.recent.get(1).map(String::as_str)
    }

    /// Every channel played, newest (the current one) first.
    pub fn recent(&self) -> &[String] {
        &self.recent
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
mod live_tests;
#[cfg(test)]
mod tests;
