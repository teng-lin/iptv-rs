//! Tolerant field access over `serde_json::Value`.
//!
//! Real panels send numbers as strings or numbers, `null`, `""`, `"N/A"`,
//! PHP's `[]` where an object is expected, and lists as arrays or as objects
//! keyed by id. Every accessor here returns `None` instead of failing, so a
//! malformed field never aborts an import.

use serde_json::{Map, Value};

/// Placeholder strings providers use for "no value".
const STAND_INS: &[&str] = &["n/a", "na", "-", "--", "null", "none", "undefined"];

pub(crate) fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_owned())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn as_f64(v: &Value) -> Option<f64> {
    let f = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => None,
    }?;
    f.is_finite().then_some(f)
}

pub(crate) fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9e15)
                .map(|f| f as i64)
        }),
        Value::String(s) => {
            let t = s.trim();
            t.parse::<i64>().ok().or_else(|| {
                as_f64(v)
                    .filter(|f| f.fract() == 0.0 && f.abs() < 9e15)
                    .map(|f| f as i64)
            })
        }
        _ => None,
    }
}

pub(crate) fn as_u64(v: &Value) -> Option<u64> {
    as_i64(v).and_then(|n| u64::try_from(n).ok())
}

/// `true`, `1`, `"1"`, `"true"`, `"yes"`, `"on"` → true; their opposites → false.
pub(crate) fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" | "y" => Some(true),
            "0" | "false" | "no" | "off" | "n" | "" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Elements of a list that may be an array or an object keyed by id.
/// Returns `None` for anything else.
pub(crate) fn list_items(v: &Value) -> Option<Vec<&Value>> {
    match v {
        Value::Array(a) => Some(a.iter().collect()),
        Value::Object(o) => Some(o.values().collect()),
        _ => None,
    }
}

/// A borrowed JSON object with tolerant getters.
#[derive(Clone, Copy)]
pub(crate) struct Obj<'a>(pub(crate) &'a Map<String, Value>);

impl<'a> Obj<'a> {
    pub(crate) fn of(v: &'a Value) -> Option<Self> {
        v.as_object().map(Obj)
    }

    pub(crate) fn raw(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key)
    }

    pub(crate) fn has(&self, key: &str) -> bool {
        self.0.get(key).is_some_and(|v| !v.is_null())
    }

    pub(crate) fn str(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(as_string)
    }

    /// Like [`Obj::str`] but treats `N/A`, `-` and friends as missing.
    pub(crate) fn text(&self, key: &str) -> Option<String> {
        self.str(key)
            .filter(|s| !STAND_INS.contains(&s.to_ascii_lowercase().as_str()))
    }

    /// First present, meaningful text among `keys`.
    pub(crate) fn first_text(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| self.text(k))
    }

    pub(crate) fn i64(&self, key: &str) -> Option<i64> {
        self.0.get(key).and_then(as_i64)
    }

    pub(crate) fn u64(&self, key: &str) -> Option<u64> {
        self.0.get(key).and_then(as_u64)
    }

    pub(crate) fn u32(&self, key: &str) -> Option<u32> {
        self.u64(key).and_then(|n| u32::try_from(n).ok())
    }

    pub(crate) fn f64(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(as_f64)
    }

    pub(crate) fn bool(&self, key: &str) -> Option<bool> {
        self.0.get(key).and_then(as_bool)
    }

    pub(crate) fn flag(&self, key: &str) -> bool {
        self.bool(key).unwrap_or(false)
    }

    /// A nested object; PHP's empty `[]` and any non-object are `None`.
    pub(crate) fn obj(&self, key: &str) -> Option<Obj<'a>> {
        self.0.get(key).and_then(Obj::of)
    }

    /// Strings from an array, a single string/number, or a comma list.
    pub(crate) fn strings(&self, key: &str) -> Vec<String> {
        match self.0.get(key) {
            Some(Value::Array(a)) => a.iter().filter_map(as_string).collect(),
            Some(v) => as_string(v)
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

/// A 0–10 rating stored ×10. `0` counts as "unrated" (IPTVnator quirk).
pub(crate) fn rating_x10(value: Option<f64>) -> Option<u16> {
    let r = value?;
    (r > 0.0 && r <= 10.0).then(|| (r * 10.0).round() as u16)
}

/// The first plausible 4-digit year (1900–2100) in a free-form date.
pub(crate) fn year_from(text: Option<&str>) -> Option<u16> {
    let bytes = text?.as_bytes();
    bytes.windows(4).enumerate().find_map(|(i, w)| {
        let boundary_before = i == 0 || !bytes[i - 1].is_ascii_digit();
        let boundary_after = bytes.get(i + 4).is_none_or(|b| !b.is_ascii_digit());
        if boundary_before && boundary_after && w.iter().all(u8::is_ascii_digit) {
            let y: u16 = std::str::from_utf8(w).ok()?.parse().ok()?;
            (1900..=2100).contains(&y).then_some(y)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_as_strings_or_numbers() {
        let v = json!({"a": "12", "b": 12, "c": 12.0, "d": " 7 ", "e": "x", "f": null, "g": "", "h": "1e3", "i": -3});
        let o = Obj::of(&v).unwrap();
        assert_eq!(o.u64("a"), Some(12));
        assert_eq!(o.u64("b"), Some(12));
        assert_eq!(o.u64("c"), Some(12));
        assert_eq!(o.u64("d"), Some(7));
        assert_eq!(o.u64("e"), None);
        assert_eq!(o.u64("f"), None);
        assert_eq!(o.u64("g"), None);
        assert_eq!(o.u64("h"), Some(1000));
        assert_eq!(o.u64("i"), None);
        assert_eq!(o.i64("i"), Some(-3));
        assert_eq!(o.u64("missing"), None);
    }

    #[test]
    fn bools_text_and_objects() {
        let v = json!({"t": "1", "f": 0, "s": "TRUE", "x": "maybe", "na": "N/A", "info": [], "real": {"k": 1}});
        let o = Obj::of(&v).unwrap();
        assert_eq!(o.bool("t"), Some(true));
        assert_eq!(o.bool("f"), Some(false));
        assert_eq!(o.bool("s"), Some(true));
        assert_eq!(o.bool("x"), None);
        assert_eq!(o.text("na"), None);
        assert_eq!(o.str("na").as_deref(), Some("N/A"));
        assert!(o.obj("info").is_none());
        assert_eq!(o.obj("real").and_then(|r| r.u64("k")), Some(1));
    }

    #[test]
    fn years_and_ratings() {
        assert_eq!(year_from(Some("2019-05-01")), Some(2019));
        assert_eq!(year_from(Some("01/02/2004")), Some(2004));
        assert_eq!(year_from(Some("12345")), None);
        assert_eq!(year_from(Some("n/a")), None);
        assert_eq!(rating_x10(Some(7.46)), Some(75));
        assert_eq!(rating_x10(Some(0.0)), None);
        assert_eq!(rating_x10(Some(55.0)), None);
    }
}
