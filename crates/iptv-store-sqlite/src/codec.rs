//! Column encodings. Every format here is persisted: change it only with a
//! migration that rewrites the stored rows.
//!
//! - kinds: [`ContentKind::slug`] / `m3u` `xtream` `stalker`;
//! - `group_ids`: JSON array of group id strings;
//! - `stream`: `{"t":"url","url":…,"headers":[[k,v]…],"properties":[[k,v]…]}`,
//!   `{"t":"xtream","stream_id":n,"ext":s|null}` or
//!   `{"t":"stalker","cmd":s,"tmp":b|null,"lb":b|null}`;
//! - `catch_up`: `{"kind":slug,"other":s?,"days":n|null,"source":s|null,
//!   "correction_secs":n}`. Rows written before `correction_secs` existed
//!   lack it (read as `0`); `days` was always a number there.

use iptv_providers::GroupId;
use iptv_providers::model::{
    CatchUp, CatchUpKind, ContentKind, SourceKind, StalkerLinkFlags, StreamLocator,
};
use serde_json::{Value, json};

use crate::error::{Result, StoreError};

pub(crate) fn source_kind_slug(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::M3u => "m3u",
        SourceKind::Xtream => "xtream",
        SourceKind::Stalker => "stalker",
    }
}

pub(crate) fn source_kind_from(slug: &str) -> Result<SourceKind> {
    Ok(match slug {
        "m3u" => SourceKind::M3u,
        "xtream" => SourceKind::Xtream,
        "stalker" => SourceKind::Stalker,
        _ => return Err(StoreError::InvalidData("sources.kind".into())),
    })
}

pub(crate) fn content_kind_from(slug: &str) -> Result<ContentKind> {
    Ok(match slug {
        "live" => ContentKind::Live,
        "radio" => ContentKind::Radio,
        "movie" => ContentKind::Movie,
        "series" => ContentKind::Series,
        "episode" => ContentKind::Episode,
        _ => return Err(StoreError::InvalidData("content kind".into())),
    })
}

pub(crate) fn group_ids_to_json(groups: &[GroupId]) -> String {
    Value::Array(groups.iter().map(|g| Value::from(g.as_str())).collect()).to_string()
}

pub(crate) fn group_ids_from_json(text: &str) -> Result<Vec<GroupId>> {
    let values: Vec<String> = serde_json::from_str(text)?;
    Ok(values.into_iter().map(GroupId::from_stored).collect())
}

fn pairs_to_json(pairs: &[(String, String)]) -> Value {
    Value::Array(pairs.iter().map(|(k, v)| json!([k, v])).collect())
}

fn pairs_from_json(value: &Value) -> Result<Vec<(String, String)>> {
    let bad = || StoreError::InvalidData("stream header pairs".into());
    let Some(items) = value.as_array() else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| match item.as_array().map(Vec::as_slice) {
            Some([Value::String(k), Value::String(v)]) => Ok((k.clone(), v.clone())),
            _ => Err(bad()),
        })
        .collect()
}

pub(crate) fn stream_to_json(stream: &StreamLocator) -> String {
    match stream {
        StreamLocator::Url {
            url,
            headers,
            properties,
        } => json!({
            "t": "url",
            "url": url,
            "headers": pairs_to_json(headers),
            "properties": pairs_to_json(properties),
        }),
        StreamLocator::Xtream {
            stream_id,
            container_extension,
        } => json!({"t": "xtream", "stream_id": stream_id, "ext": container_extension}),
        StreamLocator::Stalker { cmd, link } => json!({
            "t": "stalker",
            "cmd": cmd,
            "tmp": link.use_http_tmp_link,
            "lb": link.use_load_balancing,
        }),
    }
    .to_string()
}

pub(crate) fn stream_from_json(text: &str) -> Result<StreamLocator> {
    let bad = || StoreError::InvalidData("stream column".into());
    let v: Value = serde_json::from_str(text)?;
    let str_field = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(match v.get("t").and_then(Value::as_str) {
        Some("url") => StreamLocator::Url {
            url: str_field("url").ok_or_else(bad)?,
            headers: pairs_from_json(&v["headers"])?,
            properties: pairs_from_json(&v["properties"])?,
        },
        Some("xtream") => StreamLocator::Xtream {
            stream_id: v.get("stream_id").and_then(Value::as_u64).ok_or_else(bad)?,
            container_extension: str_field("ext"),
        },
        Some("stalker") => StreamLocator::Stalker {
            cmd: str_field("cmd").ok_or_else(bad)?,
            link: StalkerLinkFlags {
                use_http_tmp_link: v.get("tmp").and_then(Value::as_bool),
                use_load_balancing: v.get("lb").and_then(Value::as_bool),
            },
        },
        _ => return Err(bad()),
    })
}

pub(crate) fn catch_up_to_json(catch_up: &CatchUp) -> String {
    let (kind, other) = match &catch_up.kind {
        CatchUpKind::Default => ("default", None),
        CatchUpKind::Append => ("append", None),
        CatchUpKind::Shift => ("shift", None),
        CatchUpKind::Flussonic => ("flussonic", None),
        CatchUpKind::Xtream => ("xtream", None),
        CatchUpKind::Stalker => ("stalker", None),
        CatchUpKind::Vod => ("vod", None),
        CatchUpKind::Unsupported(name) => ("unsupported", Some(name.as_str())),
    };
    let mut v = json!({
        "kind": kind,
        "days": catch_up.days,
        "source": catch_up.source,
        "correction_secs": catch_up.correction_secs,
    });
    if let Some(other) = other {
        v["other"] = Value::from(other);
    }
    v.to_string()
}

pub(crate) fn catch_up_from_json(text: &str) -> Result<CatchUp> {
    let bad = || StoreError::InvalidData("catch_up column".into());
    let v: Value = serde_json::from_str(text)?;
    let kind = match v.get("kind").and_then(Value::as_str) {
        Some("default") => CatchUpKind::Default,
        Some("append") => CatchUpKind::Append,
        Some("shift") => CatchUpKind::Shift,
        Some("flussonic") => CatchUpKind::Flussonic,
        Some("xtream") => CatchUpKind::Xtream,
        Some("stalker") => CatchUpKind::Stalker,
        Some("vod") => CatchUpKind::Vod,
        Some("unsupported") => CatchUpKind::Unsupported(
            v.get("other")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        _ => return Err(bad()),
    };
    let days = match v.get("days") {
        None | Some(Value::Null) => None,
        Some(d) => Some(
            d.as_u64()
                .and_then(|d| u32::try_from(d).ok())
                .ok_or_else(bad)?,
        ),
    };
    let correction_secs = match v.get("correction_secs") {
        None | Some(Value::Null) => 0,
        Some(c) => c
            .as_i64()
            .and_then(|c| i32::try_from(c).ok())
            .ok_or_else(bad)?,
    };
    Ok(CatchUp {
        kind,
        days,
        source: v.get("source").and_then(Value::as_str).map(str::to_owned),
        correction_secs,
    })
}

/// Narrows a stored integer, reporting the column on overflow.
pub(crate) fn narrow<T: TryFrom<i64>>(value: Option<i64>, column: &str) -> Result<Option<T>> {
    value
        .map(|v| T::try_from(v).map_err(|_| StoreError::InvalidData(column.to_owned())))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_round_trip() {
        let streams = [
            StreamLocator::Url {
                url: "http://stream.example.com/a.m3u8".into(),
                headers: vec![("User-Agent".into(), "Synthetic/1".into())],
                properties: vec![("inputstream".into(), "adaptive".into())],
            },
            StreamLocator::Xtream {
                stream_id: 42,
                container_extension: Some("mkv".into()),
            },
            StreamLocator::Xtream {
                stream_id: 7,
                container_extension: None,
            },
            StreamLocator::Stalker {
                cmd: "ffrt http://localhost/ch/1".into(),
                link: StalkerLinkFlags {
                    use_http_tmp_link: Some(true),
                    use_load_balancing: None,
                },
            },
        ];
        for s in streams {
            assert_eq!(stream_from_json(&stream_to_json(&s)).unwrap(), s);
        }
    }

    #[test]
    fn catch_up_round_trips() {
        for kind in [
            CatchUpKind::Default,
            CatchUpKind::Append,
            CatchUpKind::Shift,
            CatchUpKind::Flussonic,
            CatchUpKind::Xtream,
            CatchUpKind::Stalker,
            CatchUpKind::Vod,
            CatchUpKind::Unsupported("archive-x".into()),
        ] {
            for (days, correction_secs) in [(Some(3), 0), (None, -5400), (Some(1), 1800)] {
                let c = CatchUp {
                    kind: kind.clone(),
                    days,
                    source: Some("?utc={utc}".into()),
                    correction_secs,
                };
                assert_eq!(catch_up_from_json(&catch_up_to_json(&c)).unwrap(), c);
            }
        }
    }

    /// Rows written before `days` became optional and `correction_secs`
    /// existed still decode.
    #[test]
    fn catch_up_reads_the_earlier_encoding() {
        let old = catch_up_from_json(r#"{"kind":"xtream","days":2,"source":null}"#).unwrap();
        assert_eq!(
            old,
            CatchUp {
                kind: CatchUpKind::Xtream,
                days: Some(2),
                source: None,
                correction_secs: 0,
            }
        );
        assert!(catch_up_from_json(r#"{"kind":"shift","days":"x"}"#).is_err());
        assert!(catch_up_from_json(r#"{"kind":"shift","correction_secs":1.5}"#).is_err());
    }

    #[test]
    fn json_errors_do_not_quote_values() {
        let err = stream_from_json(r#"{"t": "url", "url": 5, "x": "http://u:pw@h"}"#)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("pw"), "{err}");
        let err = group_ids_from_json(r#"["ok", 1, "secret-value"]"#)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("secret"), "{err}");
    }
}
