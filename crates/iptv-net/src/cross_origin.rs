//! Which request headers may leave the origin they were configured for.
//!
//! Headers reach this crate from the provider clients (portal identity),
//! from playlists (`#KODIPROP`, `url|Name=value`: any name, for example an
//! API key) and from the user (User-Agent). All of them belong to the
//! origin (scheme, host and port) of the URL they came with. A redirect, an
//! HLS segment or key, or a DASH resource may name another origin; that
//! origin gets only the headers in [`KEEP`], and a `Referer` cut down to
//! its own origin.
//!
//! This is an allowlist because a playlist can call a credential header
//! anything: a list of known identity headers (`Authorization`, `Cookie`,
//! `SN`…) lets `X-Api-Key` or `X-Auth-Token` through.
//!
//! The `Referer` rule is the browsers' default,
//! `strict-origin-when-cross-origin`: a CDN that checks which site sent the
//! viewer still sees the host, while a path or query (often a session
//! token) stays with the first origin.
//!
//! [`crate::redirect`] applies it on every hop that changes origin, the
//! playback relay to every sub-resource outside the stream's origin.

use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, REFERER};

/// Headers another origin may receive unchanged: content negotiation and
/// range requests, which say nothing about the account, and `Origin`, which
/// by definition carries no path or query.
pub(crate) const KEEP: &[&str] = &[
    "user-agent",
    "accept",
    "accept-encoding",
    "accept-language",
    "range",
    "if-range",
    "origin",
];

/// Reduces `headers`, configured for `from`, to what `to` (another origin)
/// may receive. Callers decide that the origin changed.
pub(crate) fn restrict(headers: &mut HeaderMap, from: &Url, to: &Url) {
    // Callers refuse an HTTPS to HTTP change before sending anything; should
    // one ever be allowed, a secure page is not named over cleartext.
    let downgrade = from.scheme() == "https" && to.scheme() != "https";
    let referer = headers
        .get(REFERER)
        .filter(|_| !downgrade)
        .and_then(origin_only);
    let dropped: Vec<HeaderName> = headers
        .keys()
        .filter(|name| !KEEP.contains(&name.as_str()))
        .cloned()
        .collect();
    for name in dropped {
        headers.remove(name);
    }
    if let Some(referer) = referer {
        headers.insert(REFERER, referer);
    }
}

/// `scheme://host[:port]/` of a `Referer` value; `None` when it is not an
/// HTTP(S) URL (then nothing of it is sent).
fn origin_only(referer: &HeaderValue) -> Option<HeaderValue> {
    let url = Url::parse(referer.to_str().ok()?.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    // The origin has no userinfo, path, query or fragment.
    HeaderValue::from_str(&format!("{}/", url.origin().ascii_serialization())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn restricted(pairs: &[(&str, &str)], from: &str, to: &str) -> Vec<(String, String)> {
        let mut map = headers(pairs);
        restrict(
            &mut map,
            &Url::parse(from).unwrap(),
            &Url::parse(to).unwrap(),
        );
        let mut left: Vec<(String, String)> = map
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap().to_owned()))
            .collect();
        left.sort();
        left
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn only_transport_headers_origin_and_the_referer_origin_remain() {
        let left = restricted(
            &[
                ("User-Agent", "SyntheticPlayer/1.0"),
                ("Accept", "*/*"),
                ("Accept-Encoding", "gzip"),
                ("Accept-Language", "en"),
                ("Range", "bytes=0-9"),
                ("If-Range", "\"synthetic-etag\""),
                ("Origin", "https://app.example.test"),
                (
                    "Referer",
                    "https://user:pw@portal.example.test:8443/watch/7?token=synthetic#frag",
                ),
                ("Authorization", "Bearer synthetic"),
                ("Cookie", "mac=00:1A:79:00:00:01"),
                ("Proxy-Authorization", "Basic synthetic"),
                ("SN", "SERIAL-SYNTHETIC"),
                ("X-User-Agent", "Model: Synthetic"),
                ("X-Api-Key", "synthetic-key"),
                ("X-Auth-Token", "synthetic-token"),
                ("X-Forwarded-For", "192.0.2.1"),
                ("Content-Type", "application/json"),
            ],
            "http://first.example.test/a",
            "http://second.example.test/b",
        );
        assert_eq!(
            left,
            pairs(&[
                ("accept", "*/*"),
                ("accept-encoding", "gzip"),
                ("accept-language", "en"),
                ("if-range", "\"synthetic-etag\""),
                ("origin", "https://app.example.test"),
                ("range", "bytes=0-9"),
                ("referer", "https://portal.example.test:8443/"),
                ("user-agent", "SyntheticPlayer/1.0"),
            ])
        );
    }

    #[test]
    fn referer_is_dropped_when_it_is_not_a_web_origin_or_on_a_downgrade() {
        for referer in [
            "not a url",
            "android-app://tv.example.test/watch",
            "file:///home/synthetic/list.m3u",
            "/watch/7?token=synthetic",
        ] {
            assert_eq!(
                restricted(
                    &[("Referer", referer)],
                    "https://first.example.test/",
                    "https://second.example.test/"
                ),
                Vec::new(),
                "{referer}"
            );
        }
        // Default ports are not spelled out, as in a browser.
        assert_eq!(
            restricted(
                &[("Referer", "http://portal.example.test:80/c/index.html")],
                "http://first.example.test/",
                "https://second.example.test/"
            ),
            pairs(&[("referer", "http://portal.example.test/")])
        );
        assert_eq!(
            restricted(
                &[
                    ("Referer", "https://portal.example.test/c/"),
                    ("Origin", "https://portal.example.test"),
                ],
                "https://first.example.test/",
                "http://second.example.test/"
            ),
            pairs(&[("origin", "https://portal.example.test")])
        );
    }

    #[test]
    fn repeated_headers_are_removed_with_every_value() {
        let left = restricted(
            &[
                ("X-Api-Key", "synthetic-one"),
                ("X-Api-Key", "synthetic-two"),
                ("Referer", "https://portal.example.test/a?x=1"),
                ("Referer", "https://other.example.test/b?y=2"),
            ],
            "http://first.example.test/",
            "http://second.example.test/",
        );
        assert_eq!(left, pairs(&[("referer", "https://portal.example.test/")]));
    }
}
