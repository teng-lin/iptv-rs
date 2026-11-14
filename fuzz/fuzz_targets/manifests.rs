#![no_main]
//! The playback relay's manifest rewriters: an HLS playlist or a DASH MPD
//! from a stream's server, rewritten so that the player fetches every
//! resource through the relay, where origins, public addresses and headers
//! are checked. The first byte selects the rewriter and the manifest's own
//! address; the remainder is the manifest.
//!
//! Neither rewriter may panic. A rewritten HLS playlist must also leave a
//! player nothing to fetch from anywhere but the relay: every URI line and
//! every `URI="…"` attribute starts with the relay's address, also for a
//! player that ends lines at a lone carriage return or trims control
//! characters.
use iptv_net::playback::fuzzing::{rewrite_dash, rewrite_hls};
use libfuzzer_sys::fuzz_target;

/// The relay's capability prefix; no manifest can contain it by chance in a
/// position where the checks below look for it.
const PREFIX: &str = "http://127.0.0.1:1/0123456789abcdef/";
const BASES: [&str; 4] = [
    "https://media.example.test/live/main.m3u8",
    "http://media.example.test:8080/a/b/main.mpd?token=synthetic",
    "http://media.example.test",
    "https://[2001:db8::1]:8443/v/",
];

fuzz_target!(|input: &[u8]| {
    let Some((&selector, data)) = input.split_first() else {
        return;
    };
    if data.len() > 65536 {
        return;
    }
    let base = BASES[usize::from(selector >> 1) % BASES.len()];
    if selector & 1 == 0 {
        hls(data, base);
    } else {
        dash(data, base);
    }
});

fn hls(data: &[u8], base: &str) {
    // The relay drops a byte order mark before rewriting.
    let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data);
    let Some(out) = rewrite_hls(data, base, PREFIX) else {
        return;
    };
    let text = std::str::from_utf8(&out).expect("a rewritten playlist is text");
    for line in text.split(['\n', '\r']) {
        // As Rust and as Java trim a line: a player may do either.
        for line in [line.trim(), line.trim_matches(|c: char| c <= ' ')] {
            if line.is_empty() {
                continue;
            }
            if !line.starts_with('#') {
                assert!(line.starts_with(PREFIX), "URI line off the relay: {line:?}");
                continue;
            }
            let mut rest = line;
            while let Some(at) = rest.find("URI=\"") {
                let value = &rest[at + 5..];
                assert!(
                    value.starts_with(PREFIX),
                    "URI attribute off the relay: {line:?}"
                );
                let end = value
                    .find('"')
                    .expect("a rewritten URI attribute is closed");
                rest = &value[end + 1..];
            }
        }
    }
}

fn dash(data: &[u8], base: &str) {
    let Some(out) = rewrite_dash(data, base, PREFIX) else {
        return;
    };
    // What was written is read again by a player's XML parser: it must at
    // least not trip this one.
    let _ = rewrite_dash(&out, base, PREFIX);
}
