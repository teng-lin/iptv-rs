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

use crate::model::{CatchUpKind, Channel, StreamLocator};
use crate::time::wall_clock;

/// What to replay. All times are Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatchupRequest {
    pub start: i64,
    pub end: i64,
    pub now: i64,
    /// Only for `catchup="xc"`: the panel's UTC offset (see
    /// [`crate::xtream::ServerClock`]); `0` when unknown.
    pub server_offset_minutes: i32,
}

/// The replay URL for a programme on `channel`, or `None` when the channel
/// has no usable catch-up. Does not check that `start` lies inside the
/// archive window; callers gate on `catch_up.days`.
pub fn build_catchup_url(channel: &Channel, req: CatchupRequest) -> Option<String> {
    let catch_up = channel.catch_up.as_ref()?;
    let StreamLocator::Url { url: live, .. } = &channel.stream else {
        return None;
    };
    let source = catch_up
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match &catch_up.kind {
        CatchUpKind::Unsupported(_) | CatchUpKind::Stalker | CatchUpKind::Vod => return None,
        CatchUpKind::Append => return source.map(|s| format!("{live}{}", expand(s, req))),
        _ => {}
    }
    if let Some(src) = source.filter(|s| is_http(s)) {
        let expanded = expand(src, req);
        return if expanded == src {
            set_utc_params(src, req)
        } else {
            Some(expanded)
        };
    }
    match &catch_up.kind {
        CatchUpKind::Flussonic => flussonic(live, req),
        CatchUpKind::Xtream => xtream(live, req),
        CatchUpKind::Shift if is_http(live) => set_utc_params(live, req),
        _ => None,
    }
}

fn is_http(s: &str) -> bool {
    url::Url::parse(s)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

fn set_utc_params(raw: &str, req: CatchupRequest) -> Option<String> {
    let mut url = url::Url::parse(raw.trim()).ok()?;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "utc" && k != "lutc")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("utc", &req.start.to_string())
        .append_pair("lutc", &req.now.to_string());
    Some(url.to_string())
}

fn flussonic(live: &str, req: CatchupRequest) -> Option<String> {
    let mut url = url::Url::parse(live).ok()?;
    let path = url.path().to_owned();
    let duration = (req.end - req.start).max(0);
    let new_path = if let Some(base) = path.strip_suffix("/mpegts") {
        format!("{base}/timeshift_abs-{}.ts", req.start)
    } else {
        let stem = path.strip_suffix(".m3u8")?;
        format!("{stem}-{}-{duration}.m3u8", req.start)
    };
    url.set_path(&new_path);
    Some(url.to_string())
}

fn xtream(live: &str, req: CatchupRequest) -> Option<String> {
    let mut url = url::Url::parse(live).ok()?;
    let segments: Vec<String> = url
        .path_segments()?
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let (user, pass, last) = match segments.as_slice() {
        [kind, u, p, last] if kind == "live" => (u, p, last),
        [u, p, last] => (u, p, last),
        _ => return None,
    };
    let id = last.split_once('.').map_or(last.as_str(), |(s, _)| s);
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes = crate::time::catchup_duration_minutes(req.start, req.end);
    let start = crate::time::format_catchup_start(req.start, req.server_offset_minutes);
    url.set_path(&format!(
        "/timeshift/{user}/{pass}/{minutes}/{start}/{id}.ts"
    ));
    Some(url.to_string())
}

/// Expands `{x}` and `${x}` placeholders.
fn expand(template: &str, req: CatchupRequest) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let dollar = open > 0 && rest.as_bytes()[open - 1] == b'$';
        let Some(close) = rest[open..].find('}').map(|c| open + c) else {
            break;
        };
        let token = &rest[open + 1..close];
        match placeholder(token, req) {
            Some(value) => {
                out.push_str(&rest[..if dollar { open - 1 } else { open }]);
                out.push_str(&value);
            }
            None => out.push_str(&rest[..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

fn placeholder(token: &str, req: CatchupRequest) -> Option<String> {
    let duration = (req.end - req.start).max(0);
    let offset = (req.now - req.start).max(0);
    let (name, arg) = match token.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (token, None),
    };
    let divided = |v: i64| match arg {
        None => Some(v.to_string()),
        Some(a) => a
            .parse::<i64>()
            .ok()
            .filter(|d| *d > 0)
            .map(|d| (v / d).to_string()),
    };
    let formatted = |t: i64| match arg {
        None => Some(t.to_string()),
        Some(fmt) => Some(format_time(fmt, t)),
    };
    match name {
        "utc" | "start" => formatted(req.start),
        "utcend" | "end" => formatted(req.end),
        "lutc" | "now" | "timestamp" => formatted(req.now),
        "duration" => divided(duration),
        "offset" => divided(offset),
        "Y" | "m" | "d" | "H" | "M" | "S" if arg.is_none() => Some(format_time(name, req.start)),
        _ => None,
    }
}

/// Formats `t` (UTC) with `Y m d H M S` letters; other characters are copied.
fn format_time(fmt: &str, t: i64) -> String {
    let w = wall_clock(t, 0);
    let mut out = String::with_capacity(fmt.len() * 2);
    for c in fmt.chars() {
        match c {
            'Y' => out.push_str(&format!("{:04}", w.year)),
            'm' => out.push_str(&format!("{:02}", w.month)),
            'd' => out.push_str(&format!("{:02}", w.day)),
            'H' => out.push_str(&format!("{:02}", w.hour)),
            'M' => out.push_str(&format!("{:02}", w.minute)),
            'S' => out.push_str(&format!("{:02}", w.second)),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m3u::parse_m3u;

    const REQ: CatchupRequest = CatchupRequest {
        start: 1_700_000_000,
        end: 1_700_003_600,
        now: 1_700_010_000,
        server_offset_minutes: 0,
    };

    fn channel(extinf_attrs: &str, url: &str) -> Channel {
        let text = format!("#EXTM3U\n#EXTINF:-1 {extinf_attrs},Test\n{url}\n");
        parse_m3u(&text).channels.items.remove(0)
    }

    #[test]
    fn shift_sets_utc_and_lutc() {
        let ch = channel(
            r#"catchup="shift" catchup-days="3""#,
            "http://live.example.com/ch/1.m3u8?utc=1&x=y",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://live.example.com/ch/1.m3u8?x=y&utc=1700000000&lutc=1700010000")
        );
    }

    #[test]
    fn archive_days_without_type_fall_back_to_shift() {
        let ch = channel(r#"tvg-rec="7""#, "http://live.example.com/ch/2.ts");
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://live.example.com/ch/2.ts?utc=1700000000&lutc=1700010000")
        );
        // Non-HTTP stream: no replay target.
        let ch = channel(r#"tvg-rec="7""#, "rtp://239.0.0.1:5000");
        assert_eq!(build_catchup_url(&ch, REQ), None);
    }

    #[test]
    fn no_days_keeps_catchup() {
        let ch = channel(r#"catchup="shift""#, "http://live.example.com/ch/3.ts");
        assert_eq!(ch.catch_up.as_ref().unwrap().days, None);
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://live.example.com/ch/3.ts?utc=1700000000&lutc=1700010000")
        );
    }

    #[test]
    fn default_source_templates() {
        let ch = channel(
            r#"catchup="default" catchup-days="2" catchup-source="http://arc.example.com/ch/4/{utc}-{duration:60}.m3u8?s=${start}&e={utcend}&d={Y}{m}{d}&o={offset}""#,
            "http://live.example.com/ch/4.m3u8",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some(
                "http://arc.example.com/ch/4/1700000000-60.m3u8?s=1700000000&e=1700003600&d=20231114&o=10000"
            )
        );
        let ch = channel(
            r#"catchup="default" catchup-days="2" catchup-source="http://arc.example.com/{utc:Y-m-d_H}/{unknown}""#,
            "http://l.example.com/x",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://arc.example.com/2023-11-14_22/{unknown}")
        );
        // A plain HTTP source gets utc/lutc (IPTVnator).
        let ch = channel(
            r#"catchup-days="2" catchup-source="http://arc.example.com/ch/5""#,
            "http://l.example.com/x",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://arc.example.com/ch/5?utc=1700000000&lutc=1700010000")
        );
        // Default without a source: nothing to replay.
        let ch = channel(
            r#"catchup="default" catchup-days="2""#,
            "http://l.example.com/x",
        );
        assert_eq!(build_catchup_url(&ch, REQ), None);
    }

    #[test]
    fn append_flussonic_xc_and_unsupported() {
        let ch = channel(
            r#"catchup="append" catchup-days="1" catchup-source="?utc={utc}&lutc={lutc}""#,
            "http://l.example.com/c.m3u8",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://l.example.com/c.m3u8?utc=1700000000&lutc=1700010000")
        );

        let ch = channel(
            r#"catchup="flussonic" catchup-days="1""#,
            "http://f.example.com/chan/index.m3u8?token=t",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://f.example.com/chan/index-1700000000-3600.m3u8?token=t")
        );
        let ch = channel(
            r#"catchup="fs" catchup-days="1""#,
            "http://f.example.com/chan/mpegts",
        );
        assert_eq!(
            build_catchup_url(&ch, REQ).as_deref(),
            Some("http://f.example.com/chan/timeshift_abs-1700000000.ts")
        );

        let ch = channel(
            r#"catchup="xc" catchup-days="1""#,
            "http://xc.example.com:8080/live/demo/secret/1001.m3u8",
        );
        assert_eq!(
            build_catchup_url(
                &ch,
                CatchupRequest {
                    server_offset_minutes: 60,
                    ..REQ
                }
            )
            .as_deref(),
            Some("http://xc.example.com:8080/timeshift/demo/secret/60/2023-11-14:23-13/1001.ts")
        );

        let ch = channel(
            r#"catchup="vod" catchup-days="1""#,
            "http://l.example.com/x",
        );
        assert_eq!(build_catchup_url(&ch, REQ), None);
        let ch = channel(
            r#"catchup="rtsp-magic" catchup-days="1""#,
            "http://l.example.com/x",
        );
        assert!(matches!(
            ch.catch_up.as_ref().unwrap().kind,
            CatchUpKind::Unsupported(_)
        ));
        assert_eq!(build_catchup_url(&ch, REQ), None);
    }

    #[test]
    fn header_catchup_defaults_apply() {
        let text = "#EXTM3U catchup=\"shift\" catchup-days=\"5\"\n#EXTINF:-1,A\nhttp://l.example.com/a\n#EXTINF:-1 catchup-days=\"1\",B\nhttp://l.example.com/b\n";
        let items = parse_m3u(text).channels.items;
        assert_eq!(items[0].catch_up.as_ref().map(|c| c.days), Some(Some(5)));
        assert_eq!(items[1].catch_up.as_ref().map(|c| c.days), Some(Some(1)));
    }
}
