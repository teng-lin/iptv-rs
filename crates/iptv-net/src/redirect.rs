//! Redirects, followed here rather than by reqwest.
//!
//! reqwest's automatic policy adds a `Referer` with the previous URL's
//! query (Xtream credentials) and keeps non-standard secret headers such
//! as Stalker's `SN` on another host. Here a hop that changes origin
//! (scheme, host or port) drops every header in [`CROSS_ORIGIN_STRIP`],
//! and no `Referer` is ever added. HTTPS-to-HTTP downgrades are refused.

use iptv_import::{FetchError, FetchErrorKind};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, LOCATION};
use reqwest::{Method, Url};

use crate::error::from_reqwest;

/// Identity headers that must not follow a redirect to another origin.
const CROSS_ORIGIN_STRIP: &[&str] = &[
    "authorization",
    "cookie",
    "proxy-authorization",
    "sn",
    "x-user-agent",
];

/// A request that can be re-sent to a redirect target.
#[derive(Clone)]
pub(crate) struct Prepared {
    pub public_only: bool,
    #[cfg(debug_assertions)]
    pub allow_private_fixture: bool,
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
}

impl Prepared {
    fn request(&self) -> reqwest::Request {
        let mut req = reqwest::Request::new(self.method.clone(), self.url.clone());
        *req.headers_mut() = self.headers.clone();
        if let Some(body) = &self.body {
            *req.body_mut() = Some(body.clone().into());
        }
        req
    }
}

/// `scheme://host:port` of a URL, for logs.
fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// Sends `p`, following up to `max` redirects. `shown` is the redacted
/// original URL for logs.
pub(crate) async fn execute(
    client: &reqwest::Client,
    public_client: &reqwest::Client,
    mut p: Prepared,
    max: usize,
    shown: &str,
) -> Result<reqwest::Response, FetchError> {
    let redirect = |what: String| FetchError::new(FetchErrorKind::Redirect, what);
    let mut hops = 0;
    let initial_origin = p.url.origin();
    loop {
        let restricted = p.public_only || p.url.origin() != initial_origin;
        #[cfg(debug_assertions)]
        let restricted = restricted && !p.allow_private_fixture;
        if restricted {
            iptv_providers::content_url::validate(p.url.as_str()).map_err(|_| {
                FetchError::new(
                    FetchErrorKind::InvalidRequest,
                    "content destination must be public HTTP(S)",
                )
            })?;
        }
        let transport = if restricted { public_client } else { client };
        let resp = transport.execute(p.request()).await.map_err(from_reqwest)?;
        let status = resp.status().as_u16();
        let location = match status {
            301 | 302 | 303 | 307 | 308 => resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_owned()),
            _ => None,
        };
        let Some(location) = location else {
            return Ok(resp);
        };
        drop(resp);
        if hops >= max {
            return Err(redirect(format!("more than {max} redirects")));
        }
        hops += 1;
        let next = p
            .url
            .join(&location)
            .map_err(|_| redirect("invalid redirect location".into()))?;
        if !matches!(next.scheme(), "http" | "https") {
            return Err(redirect("redirect to an unsupported scheme".into()));
        }
        if next.origin() != initial_origin {
            p.public_only = true;
        }
        if p.url.scheme() == "https" && next.scheme() == "http" {
            return Err(redirect(
                "HTTPS redirect would use an unencrypted connection".into(),
            ));
        }
        if next.origin() != p.url.origin() {
            for h in CROSS_ORIGIN_STRIP {
                p.headers.remove(*h);
            }
        }
        if status == 303 || (matches!(status, 301 | 302) && p.method == Method::POST) {
            p.method = Method::GET;
            p.body = None;
            p.headers.remove(CONTENT_TYPE);
            p.headers.remove(CONTENT_LENGTH);
        }
        log::debug!("{shown}: HTTP {status} -> {}", origin(&next));
        p.url = next;
    }
}
