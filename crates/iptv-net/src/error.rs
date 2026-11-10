//! reqwest errors → [`FetchError`] without URLs or secrets.

use iptv_import::{FetchError, FetchErrorKind};

pub(crate) fn from_reqwest(e: reqwest::Error) -> FetchError {
    // The URL (and with it Xtream credentials) is part of reqwest's Display.
    let e = e.without_url();
    let kind = if e.is_timeout() || chain_has_timeout(&e) {
        FetchErrorKind::Timeout
    } else if e.is_redirect() {
        FetchErrorKind::Redirect
    } else if chain_has_tls(&e) {
        FetchErrorKind::Tls
    } else if e.is_connect() {
        FetchErrorKind::Connect
    } else if e.is_body() || e.is_decode() {
        FetchErrorKind::Body
    } else if e.is_builder() {
        FetchErrorKind::InvalidRequest
    } else if e.is_request() {
        FetchErrorKind::Connect
    } else {
        FetchErrorKind::Other
    };
    FetchError::new(kind, describe(&e))
}

/// The error and its causes, de-duplicated, joined with `: `.
fn describe(e: &reqwest::Error) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = cur {
        let text = err.to_string();
        if !text.is_empty() && !parts.iter().any(|p| p.contains(&text)) {
            parts.push(text);
        }
        cur = err.source();
    }
    parts.join(": ")
}

fn chain_has_tls(e: &reqwest::Error) -> bool {
    any_cause(e, &|err| err.is::<rustls::Error>())
}

fn chain_has_timeout(e: &reqwest::Error) -> bool {
    any_cause(e, &|err| {
        err.downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
    })
}

/// Walks the causes of `e`, descending into `io::Error` payloads, which
/// `io::Error::source` skips (hyper wraps rustls errors in two of them).
fn any_cause(
    e: &(dyn std::error::Error + 'static),
    pred: &dyn Fn(&(dyn std::error::Error + 'static)) -> bool,
) -> bool {
    let mut cur = e.source();
    while let Some(err) = cur {
        if pred(err) {
            return true;
        }
        if let Some(io) = err.downcast_ref::<std::io::Error>()
            && let Some(inner) = io.get_ref()
        {
            let inner: &(dyn std::error::Error + 'static) = inner;
            if pred(inner) || any_inner(inner, pred) {
                return true;
            }
        }
        cur = err.source();
    }
    false
}

fn any_inner(
    e: &(dyn std::error::Error + 'static),
    pred: &dyn Fn(&(dyn std::error::Error + 'static)) -> bool,
) -> bool {
    match e
        .downcast_ref::<std::io::Error>()
        .and_then(|io| io.get_ref())
    {
        Some(inner) => pred(inner) || any_inner(inner, pred),
        None => any_cause(e, pred),
    }
}
