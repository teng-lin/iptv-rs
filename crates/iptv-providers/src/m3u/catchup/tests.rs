use super::*;
use crate::m3u::parse_m3u;

const REQ: CatchupRequest = CatchupRequest::new(1_700_000_000, 1_700_003_600, 1_700_010_000);

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
        Ok("http://live.example.com/ch/1.m3u8?x=y&utc=1700000000&lutc=1700010000")
    );
}

#[test]
fn archive_days_without_type_fall_back_to_shift() {
    let ch = channel(r#"tvg-rec="7""#, "http://live.example.com/ch/2.ts");
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://live.example.com/ch/2.ts?utc=1700000000&lutc=1700010000")
    );
    // Non-HTTP stream: no replay target.
    let ch = channel(r#"tvg-rec="7""#, "rtp://239.0.0.1:5000");
    assert_eq!(build_catchup_url(&ch, REQ), Err(CatchupError::MalformedUrl));
}

#[test]
fn no_days_keeps_catchup() {
    let ch = channel(r#"catchup="shift""#, "http://live.example.com/ch/3.ts");
    assert_eq!(ch.catch_up.as_ref().unwrap().days, None);
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://live.example.com/ch/3.ts?utc=1700000000&lutc=1700010000")
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
        Ok(
            "http://arc.example.com/ch/4/1700000000-60.m3u8?s=1700000000&e=1700003600&d=20231114&o=10000"
        )
    );
    let ch = channel(
        r#"catchup="default" catchup-days="2" catchup-source="http://arc.example.com/{utc:Y-m-d_H}/{unknown}""#,
        "http://l.example.com/x",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://arc.example.com/2023-11-14_22/{unknown}")
    );
    // A plain HTTP source gets utc/lutc (IPTVnator).
    let ch = channel(
        r#"catchup-days="2" catchup-source="http://arc.example.com/ch/5""#,
        "http://l.example.com/x",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://arc.example.com/ch/5?utc=1700000000&lutc=1700010000")
    );
    // Default without a source: nothing to replay.
    let ch = channel(
        r#"catchup="default" catchup-days="2""#,
        "http://l.example.com/x",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ),
        Err(CatchupError::MissingTemplate)
    );
}

#[test]
fn append_flussonic_xc_and_unsupported() {
    let ch = channel(
        r#"catchup="append" catchup-days="1" catchup-source="?utc={utc}&lutc={lutc}""#,
        "http://l.example.com/c.m3u8",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://l.example.com/c.m3u8?utc=1700000000&lutc=1700010000")
    );

    let ch = channel(
        r#"catchup="flussonic" catchup-days="1""#,
        "http://f.example.com/chan/index.m3u8?token=t",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://f.example.com/chan/index-1700000000-3600.m3u8?token=t")
    );
    let ch = channel(
        r#"catchup="fs" catchup-days="1""#,
        "http://f.example.com/chan/mpegts",
    );
    assert_eq!(
        build_catchup_url(&ch, REQ).as_deref(),
        Ok("http://f.example.com/chan/timeshift_abs-1700000000.ts")
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
        Ok("http://xc.example.com:8080/timeshift/demo/secret/60/2023-11-14:23-13/1001.ts")
    );

    let ch = channel(
        r#"catchup="vod" catchup-days="1""#,
        "http://l.example.com/x",
    );
    assert_eq!(build_catchup_url(&ch, REQ), Err(CatchupError::Unsupported));
    let ch = channel(
        r#"catchup="rtsp-magic" catchup-days="1""#,
        "http://l.example.com/x",
    );
    assert!(matches!(
        ch.catch_up.as_ref().unwrap().kind,
        CatchUpKind::Unsupported(_)
    ));
    assert_eq!(build_catchup_url(&ch, REQ), Err(CatchupError::Unsupported));
}

#[test]
fn header_catchup_defaults_apply() {
    let text = "#EXTM3U catchup=\"shift\" catchup-days=\"5\"\n#EXTINF:-1,A\nhttp://l.example.com/a\n#EXTINF:-1 catchup-days=\"1\",B\nhttp://l.example.com/b\n";
    let items = parse_m3u(text).channels.items;
    assert_eq!(items[0].catch_up.as_ref().map(|c| c.days), Some(Some(5)));
    assert_eq!(items[1].catch_up.as_ref().map(|c| c.days), Some(Some(1)));
}

#[test]
fn rejects_future_programmes_and_positions() {
    let ch = channel(
        r#"catchup="shift" catchup-days="1""#,
        "http://l.example.com/x",
    );
    let at = |start, end, now| CatchupRequest::new(start, end, now);
    assert_eq!(
        build_catchup_url(&ch, at(2_000, 3_000, 1_999)),
        Err(CatchupError::FutureProgramme)
    );
    // On air: playable from its start, but not seeking past now.
    assert!(build_catchup_url(&ch, at(2_000, 3_000, 2_500)).is_ok());
    let seek = |offset_secs| CatchupRequest {
        offset_secs,
        ..at(2_000, 3_000, 2_500)
    };
    assert_eq!(
        build_catchup_url(&ch, seek(400)).as_deref(),
        Ok("http://l.example.com/x?utc=2400&lutc=2500")
    );
    assert_eq!(
        build_catchup_url(&ch, seek(600)),
        Err(CatchupError::FutureProgramme)
    );
}

#[test]
fn checks_the_archive_window() {
    let day = 86_400;
    let now = 100 * day;
    let old =
        |days_ago: i64| CatchupRequest::new(now - days_ago * day, now - days_ago * day + 3600, now);
    let declared = channel(
        r#"catchup="shift" catchup-days="2""#,
        "http://l.example.com/x",
    );
    assert!(build_catchup_url(&declared, old(2)).is_ok());
    assert_eq!(
        build_catchup_url(&declared, old(3)),
        Err(CatchupError::OutsideArchive)
    );
    // No catchup-days: the caller's default window applies.
    let unknown = channel(r#"catchup="shift""#, "http://l.example.com/x");
    assert!(build_catchup_url(&unknown, old(7)).is_ok());
    assert_eq!(
        build_catchup_url(&unknown, old(8)),
        Err(CatchupError::OutsideArchive)
    );
    let wide = CatchupRequest {
        default_window_days: 14,
        ..old(8)
    };
    assert!(build_catchup_url(&unknown, wide).is_ok());
    // Seeking past the programme end.
    let past_end = CatchupRequest {
        offset_secs: 3600,
        ..old(1)
    };
    assert_eq!(
        build_catchup_url(&declared, past_end),
        Err(CatchupError::OutsideArchive)
    );
}

#[test]
fn buffers_widen_the_replay() {
    let ch = channel(
        r#"catchup="default" catchup-days="1" catchup-source="http://arc.example.com/{utc}/{duration}/{utcend}""#,
        "http://l.example.com/x",
    );
    let req = CatchupRequest {
        buffer_before_secs: 120,
        buffer_after_secs: 300,
        ..CatchupRequest::new(10_000, 13_600, 20_000)
    };
    assert_eq!(
        build_catchup_url(&ch, req).as_deref(),
        Ok("http://arc.example.com/9880/4020/13900")
    );
    // The end buffer stops at now.
    let req = CatchupRequest { now: 13_700, ..req };
    assert_eq!(
        build_catchup_url(&ch, req).as_deref(),
        Ok("http://arc.example.com/9880/3820/13700")
    );
    // Seeking is measured from the buffered start.
    let req = CatchupRequest {
        offset_secs: 600,
        ..req
    };
    assert_eq!(
        build_catchup_url(&ch, req).as_deref(),
        Ok("http://arc.example.com/10480/3220/13700")
    );
}

#[test]
fn errors_do_not_quote_urls() {
    let ch = channel(
        r#"catchup="xc" catchup-days="1""#,
        "http://xc.example.com/live/demo/hunter2/not-an-id.m3u8?token=tok3n",
    );
    let err = build_catchup_url(&ch, REQ).unwrap_err();
    assert_eq!(err, CatchupError::MalformedUrl);
    for text in [format!("{err:?}"), err.to_string()] {
        assert!(!text.contains("hunter2") && !text.contains("tok3n") && !text.contains("demo"));
    }
}
