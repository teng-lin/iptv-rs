//! Catch-up (archive) URLs for M3U channels.
//!
//! Rules per [`CatchUpKind`] (Kodi pvr.iptvsimple's modes; IPTVnator's
//! `shift` fallback for archive days without a type):
//!
//! - `default`: the expanded `catchup-source` is the replay URL. A source
//!   without placeholders is used **verbatim** (Kodi). IPTVnator instead sets
//!   `utc`/`lutc` on such a source; we follow Kodi so a fixed archive URL is
//!   never altered. A source without `scheme://` is appended to the live URL.
//! - `append`: live URL + expanded `catchup-source`.
//! - `shift`, `flussonic`, `xc`: an absolute HTTP(S) `catchup-source`, when
//!   declared, wins and is expanded as for `default`; otherwise the URL is
//!   derived from the live URL:
//!   - `shift`: `utc`/`lutc` query parameters on the live URL; other query
//!     bytes are kept exactly as written.
//!   - `flussonic`: `…/NAME.m3u8` → `…/NAME-{start}-{duration}.m3u8`,
//!     `…/mpegts` → `…/timeshift_abs-{start}.ts`.
//!   - `xc`: Xtream-shaped live URL → `/timeshift/U/P/{minutes}/{Y-m-d:H-M}/{id}.EXT`,
//!     EXT `m3u8` for an `.m3u8` live URL, else `ts`.
//! - Unknown types and Stalker channels: [`CatchupError::Unsupported`].
//!
//! Template placeholders, in `{x}` or `${x}` form: `utc`/`start` (programme
//! start), `utcend`/`end`, `lutc`/`now`/`timestamp`, `duration` (seconds),
//! `offset` (now − start, seconds), `duration:N`/`offset:N` (divided by N),
//! `Y m d H M S` (start, viewer's clock) and `utc:FMT`/`start:FMT`/`end:FMT`/`utcend:FMT`
//! where FMT uses the same letters (`{utc:Y-m-d}`, viewer's clock: see
//! [`CatchupRequest::local_offset_secs`]). Unknown placeholders are left
//! untouched.
//!
//! ## Timing
//!
//! The replay runs from `start − buffer_before_secs + offset_secs` to
//! `end + buffer_after_secs` (the end buffer stops at `now`). "Start" in the
//! rules above is that replay start. A programme that starts after `now`,
//! or a replay start after `now`, is [`CatchupError::FutureProgramme`]; a
//! replay start at or past the replay end, or before the archive window
//! (`catchup-days`, else [`CatchupRequest::default_window_days`]), is
//! [`CatchupError::OutsideArchive`].

mod template;

use template::{Vars, expand};

pub use crate::error::CatchupError;
use crate::model::{CatchUpKind, Channel, StreamLocator};

/// Archive window assumed when the playlist declares catch-up without
/// `catchup-days` (our default; Kodi leaves it to a user setting).
pub const DEFAULT_CATCHUP_WINDOW_DAYS: u32 = 7;

/// What to replay. All times are Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatchupRequest {
    /// Programme start and end from the guide.
    pub start: i64,
    pub end: i64,
    pub now: i64,
    /// Where to begin inside the replay, from its start (the programme start
    /// minus `buffer_before_secs`), for seeking. `0` plays from the start.
    pub offset_secs: u32,
    /// The viewer's UTC offset in seconds at `start`. Date-part placeholders
    /// (`{Y}`…`{S}`, `{utc:FMT}`) render in this clock, like Kodi's
    /// locale-based formatting; Unix-number placeholders stay UTC. The host
    /// resolves its zone (and DST) to this number; no tz database here.
    pub local_offset_secs: i32,
    /// Only for `catchup="xc"`: the panel's UTC offset (see
    /// [`crate::xtream::ServerClock`]); `0` when unknown.
    pub server_offset_minutes: i32,
    /// Archive window used when the channel declares none.
    pub default_window_days: u32,
    /// Extra time before the programme start and after its end (Kodi's
    /// "buffer before/after"); included in `{duration}`.
    pub buffer_before_secs: u32,
    pub buffer_after_secs: u32,
}

impl CatchupRequest {
    /// A request for `[start, end)` at `now` with no seek, no buffers and
    /// the [`DEFAULT_CATCHUP_WINDOW_DAYS`] window.
    pub const fn new(start: i64, end: i64, now: i64) -> Self {
        Self {
            start,
            end,
            now,
            offset_secs: 0,
            local_offset_secs: 0,
            server_offset_minutes: 0,
            default_window_days: DEFAULT_CATCHUP_WINDOW_DAYS,
            buffer_before_secs: 0,
            buffer_after_secs: 0,
        }
    }
}

/// The replay URL for a programme on `channel`. See the module docs for the
/// rules and the checks.
pub fn build_catchup_url(channel: &Channel, req: CatchupRequest) -> Result<String, CatchupError> {
    let catch_up = channel.catch_up.as_ref().ok_or(CatchupError::Unsupported)?;
    let StreamLocator::Url { url: live, .. } = &channel.stream else {
        return Err(CatchupError::Unsupported);
    };
    if matches!(
        catch_up.kind,
        CatchUpKind::Unsupported(_) | CatchUpKind::Stalker | CatchUpKind::Vod
    ) {
        return Err(CatchupError::Unsupported);
    }
    let vars = window(&req, catch_up.window_days(req.default_window_days))?;
    let source = catch_up
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let appended = |src: &str| checked(format!("{live}{}", expand(src, &vars)));
    match &catch_up.kind {
        CatchUpKind::Append => appended(source.ok_or(CatchupError::MissingTemplate)?),
        CatchUpKind::Default => match source.ok_or(CatchupError::MissingTemplate)? {
            src if has_scheme(src) => checked(expand(src, &vars)),
            src => appended(src),
        },
        _ => match source.filter(|s| is_http(s)) {
            Some(src) => checked(expand(src, &vars)),
            None => match &catch_up.kind {
                CatchUpKind::Flussonic => flussonic(live, &vars),
                CatchUpKind::Xtream => xtream(live, &vars, req.server_offset_minutes),
                _ => set_utc_params(live, &vars),
            },
        },
    }
}

/// `scheme://…`: a complete URL rather than a suffix for the live URL.
fn has_scheme(s: &str) -> bool {
    s.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    })
}

/// The finished URL must parse; it is returned as built, not re-serialised.
fn checked(url: String) -> Result<String, CatchupError> {
    match url::Url::parse(&url) {
        Ok(_) => Ok(url),
        Err(_) => Err(CatchupError::MalformedUrl),
    }
}

/// Applies buffers and the seek offset, and checks the result against now
/// and the archive window.
fn window(req: &CatchupRequest, days: u32) -> Result<Vars, CatchupError> {
    let (start, end, now) = (req.start, req.end, req.now);
    if start > now {
        return Err(CatchupError::FutureProgramme);
    }
    let from = start - i64::from(req.buffer_before_secs) + i64::from(req.offset_secs);
    let to = (end + i64::from(req.buffer_after_secs)).min(end.max(now));
    if from > now {
        return Err(CatchupError::FutureProgramme);
    }
    if from >= to || from.max(start) < now - i64::from(days) * 86_400 {
        return Err(CatchupError::OutsideArchive);
    }
    Ok(Vars {
        from,
        to,
        now,
        local_offset_secs: req.local_offset_secs,
    })
}

fn is_http(s: &str) -> bool {
    url::Url::parse(s)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

/// Sets `utc`/`lutc` on `raw` by editing the string: earlier `utc`/`lutc`
/// pairs are dropped, every other query byte is kept as written (signed
/// tokens break when a URL library re-encodes them).
fn set_utc_params(raw: &str, vars: &Vars) -> Result<String, CatchupError> {
    let raw = raw.trim();
    if !is_http(raw) {
        return Err(CatchupError::MalformedUrl);
    }
    let (head, fragment) = match raw.split_once('#') {
        Some((h, f)) => (h, Some(f)),
        None => (raw, None),
    };
    let (base, query) = head.split_once('?').unwrap_or((head, ""));
    let mut out = String::with_capacity(raw.len() + 32);
    out.push_str(base);
    out.push('?');
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let key = pair.split_once('=').map_or(pair, |(k, _)| k);
        if key != "utc" && key != "lutc" {
            out.push_str(pair);
            out.push('&');
        }
    }
    out.push_str(&format!("utc={}&lutc={}", vars.from, vars.now));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    Ok(out)
}

fn flussonic(live: &str, vars: &Vars) -> Result<String, CatchupError> {
    let mut url = url::Url::parse(live).map_err(|_| CatchupError::MalformedUrl)?;
    let path = url.path().to_owned();
    let new_path = if let Some(base) = path.strip_suffix("/mpegts") {
        format!("{base}/timeshift_abs-{}.ts", vars.from)
    } else {
        let stem = path
            .strip_suffix(".m3u8")
            .ok_or(CatchupError::MalformedUrl)?;
        format!("{stem}-{}-{}.m3u8", vars.from, vars.duration())
    };
    url.set_path(&new_path);
    Ok(url.to_string())
}

fn xtream(live: &str, vars: &Vars, server_offset_minutes: i32) -> Result<String, CatchupError> {
    let mut url = url::Url::parse(live).map_err(|_| CatchupError::MalformedUrl)?;
    let segments: Vec<String> = url
        .path_segments()
        .ok_or(CatchupError::MalformedUrl)?
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let (user, pass, last) = match segments.as_slice() {
        [kind, u, p, last] if kind == "live" => (u, p, last),
        [u, p, last] => (u, p, last),
        _ => return Err(CatchupError::MalformedUrl),
    };
    let (id, ext) = last.split_once('.').unwrap_or((last.as_str(), ""));
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CatchupError::MalformedUrl);
    }
    // Kodi builds xc archives for both containers; keep the live one.
    let ext = if ext.eq_ignore_ascii_case("m3u8") {
        "m3u8"
    } else {
        "ts"
    };
    let minutes = crate::time::catchup_duration_minutes(vars.from, vars.to);
    let start = crate::time::format_catchup_start(vars.from, server_offset_minutes);
    url.set_path(&format!(
        "/timeshift/{user}/{pass}/{minutes}/{start}/{id}.{ext}"
    ));
    Ok(url.to_string())
}

#[cfg(test)]
mod tests;
