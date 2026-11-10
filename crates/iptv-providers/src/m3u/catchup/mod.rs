//! Catch-up (archive) URLs for M3U channels.
//!
//! Precedence (IPTVnator's, extended with templates and Flussonic/XC):
//!
//! 1. `Unsupported` type → none.
//! 2. `append`: live URL + expanded `catchup-source`.
//! 3. `catchup-source` is an absolute HTTP(S) URL (any type): expanded; if it
//!    has no placeholders, `utc`/`lutc` query parameters are set on it.
//! 4. `flussonic`: `…/index.m3u8` → `…/index-{start}-{duration}.m3u8`,
//!    `…/mpegts` → `…/timeshift_abs-{start}.ts`.
//! 5. `xc`: Xtream-shaped live URL → `/timeshift/U/P/{minutes}/{Y-m-d:H-M}/{id}.ts`.
//! 6. `shift` (also archive days without a type): `utc`/`lutc` on the live URL.
//! 7. `default` without a usable source → none.
//!
//! Template placeholders, in `{x}` or `${x}` form: `utc`/`start` (programme
//! start), `utcend`/`end`, `lutc`/`now`/`timestamp`, `duration` (seconds),
//! `offset` (now − start, seconds), `duration:N`/`offset:N` (divided by N),
//! `Y m d H M S` (start, UTC) and `utc:FMT`/`start:FMT`/`end:FMT`/`utcend:FMT`
//! where FMT uses the same letters (`{utc:Y-m-d}`). Unknown placeholders are
//! left untouched.
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
    if catch_up.kind == CatchUpKind::Append {
        let source = source.ok_or(CatchupError::MissingTemplate)?;
        return Ok(format!("{live}{}", expand(source, &vars)));
    }
    if let Some(src) = source.filter(|s| is_http(s)) {
        let expanded = expand(src, &vars);
        return if expanded == src {
            set_utc_params(src, &vars)
        } else {
            Ok(expanded)
        };
    }
    match &catch_up.kind {
        CatchUpKind::Flussonic => flussonic(live, &vars),
        CatchUpKind::Xtream => xtream(live, &vars, req.server_offset_minutes),
        CatchUpKind::Shift if is_http(live) => set_utc_params(live, &vars),
        CatchUpKind::Shift => Err(CatchupError::MalformedUrl),
        _ => Err(CatchupError::MissingTemplate),
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
    Ok(Vars { from, to, now })
}

fn is_http(s: &str) -> bool {
    url::Url::parse(s)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

fn set_utc_params(raw: &str, vars: &Vars) -> Result<String, CatchupError> {
    let mut url = url::Url::parse(raw.trim()).map_err(|_| CatchupError::MalformedUrl)?;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "utc" && k != "lutc")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("utc", &vars.from.to_string())
        .append_pair("lutc", &vars.now.to_string());
    Ok(url.to_string())
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
    let id = last.split_once('.').map_or(last.as_str(), |(s, _)| s);
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CatchupError::MalformedUrl);
    }
    let minutes = crate::time::catchup_duration_minutes(vars.from, vars.to);
    let start = crate::time::format_catchup_start(vars.from, server_offset_minutes);
    url.set_path(&format!(
        "/timeshift/{user}/{pass}/{minutes}/{start}/{id}.ts"
    ));
    Ok(url.to_string())
}

#[cfg(test)]
mod tests;
