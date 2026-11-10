//! Tolerant base64 for Xtream EPG titles/descriptions.

/// Decodes standard or URL-safe base64 (padding optional, whitespace
/// ignored) into UTF-8. Returns `None` when the input is not base64 or the
/// bytes are not UTF-8; callers then keep the original text, as some panels
/// send EPG fields unencoded.
pub(crate) fn decode_utf8(input: &str) -> Option<String> {
    let mut bits: u32 = 0;
    let mut nbits = 0;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut seen_pad = false;
    let mut symbols = 0usize;
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                seen_pad = true;
                continue;
            }
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        if seen_pad {
            return None;
        }
        symbols += 1;
        bits = (bits << 6) | u32::from(v);
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    if symbols % 4 == 1 {
        return None;
    }
    // Real text has no control characters; plain titles that happen to use
    // only base64 letters usually decode to some.
    if out
        .iter()
        .any(|&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t'))
    {
        return None;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_padded_unpadded_and_unicode() {
        assert_eq!(
            decode_utf8("TW9ybmluZyBOZXdz").as_deref(),
            Some("Morning News")
        );
        assert_eq!(decode_utf8("TW9ybmluZw==").as_deref(), Some("Morning"));
        assert_eq!(decode_utf8("TW9ybmluZw").as_deref(), Some("Morning"));
        assert_eq!(decode_utf8("w7xiZXI=").as_deref(), Some("über"));
        assert_eq!(decode_utf8("").as_deref(), Some(""));
    }

    #[test]
    fn rejects_non_base64() {
        assert_eq!(decode_utf8("Plain title!"), None);
        assert_eq!(decode_utf8("abc=d"), None);
        // Valid alphabet but not UTF-8 once decoded.
        assert_eq!(decode_utf8("Test"), None);
    }
}
