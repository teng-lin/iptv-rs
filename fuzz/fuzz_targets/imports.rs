#![no_main]
//! One linked executable covers seven independent parser boundaries.
//! The first byte selects the parser; the remainder is its arbitrary input.
use libfuzzer_sys::fuzz_target;
fuzz_target!(|input: &[u8]| {
    let Some((&selector, data)) = input.split_first() else {
        return;
    };
    match selector % 7 {
        0 => m3u(data),
        1 => xmltv(data),
        2 => gzip(data),
        3 => xtream(data),
        4 => stalker(data),
        5 => catchup(data),
        _ => backup_manifest(data),
    }
});
fn m3u(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let mut p = iptv_providers::m3u::M3uParser::new();
    for c in data.chunks(1 + data.len() % 97) {
        p.feed(c);
    }
    let _ = p.finish();
}
fn xmltv(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let mut p = iptv_epg::XmltvParser::new(Default::default());
    for c in data.chunks(1 + data.len() % 97) {
        if p.feed(c, &mut |_| {}).is_err() {
            return;
        }
    }
    let _ = p.finish(&mut |_| {});
}
fn gzip(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let _ = iptv_import::import_m3u_text(
        data,
        &mut iptv_import::NoProgress,
        &iptv_import::CancelToken::new(),
    );
    let _ = iptv_epg::parse_xmltv(data, Default::default());
}
fn xtream(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    use iptv_providers::xtream::{ListKind, XtreamListParser};
    for kind in [ListKind::Live, ListKind::Vod, ListKind::Series] {
        let mut p = XtreamListParser::new(kind, Default::default());
        for c in data.chunks(1 + data.len() % 97) {
            if p.feed(c, &mut |_| {}).is_err() {
                break;
            }
        }
        let _ = p.finish(&mut |_| {});
    }
}
fn stalker(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let response = iptv_providers::HttpResponse::new(200, data);
    let _ = iptv_providers::stalker::parse_channel_page(&response);
    let _ = iptv_providers::stalker::parse_vod_page(&response);
}
fn catchup(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let now = data
        .get(..8)
        .map(|b| i64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0);
    let _ = iptv_providers::m3u::expand_live_url(&text, now, 0);
}
fn backup_manifest(data: &[u8]) {
    if data.len() > 65536 {
        return;
    }
    let _ = iptv_import::backup_manifest::parse(data);
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = iptv_import::backup_manifest::allowed(s);
    }
}
