//! Percent-encoding helpers shared by the clients.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The set JavaScript's `encodeURIComponent` leaves raw: `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// `encodeURIComponent`-equivalent encoding for one path segment or query value.
pub(crate) fn component(value: &str) -> String {
    utf8_percent_encode(value, COMPONENT).to_string()
}

/// Builds `k=v&k=v` with every key and value component-encoded.
pub(crate) fn query(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&component(k));
        out.push('=');
        out.push_str(&component(v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_encode_uri_component() {
        assert_eq!(component("a b&c/d?e=f"), "a%20b%26c%2Fd%3Fe%3Df");
        assert_eq!(component("x-_.!~*'()"), "x-_.!~*'()");
        assert_eq!(component("ü"), "%C3%BC");
        assert_eq!(component("p+q@r"), "p%2Bq%40r");
    }
}
