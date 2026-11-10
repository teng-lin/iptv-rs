//! Tolerant Xtream response parsing.
//!
//! - Category/content lists must be JSON arrays (or objects keyed by id whose
//!   values are all objects). An account object (`{"user_info":{"auth":0}}`)
//!   is [`ProviderError::AccountRejected`]; anything else is
//!   [`ProviderError::InvalidBody`]. Only an actual empty array proves an
//!   empty catalog.
//! - Entries without a usable id are skipped with a warning; every other
//!   malformed field becomes `None`.
//! - Detail stand-ins (`N/A`, `-`, rating `0`) are treated as missing.

use serde_json::Value;

use super::ServerClock;
use crate::b64;
use crate::error::ProviderError;
use crate::http::{HttpResponse, json_body};
use crate::id::{ContentId, GroupId};
use crate::lenient::{Obj, list_items, rating_x10, year_from};
use crate::model::{
    CatchUp, CatchUpKind, Channel, ContentKind, Episode, Group, Import, Programme, Season, Series,
    SeriesDetails, SourceKind, StreamLocator, VodItem, WarningLocation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    Active,
    Inactive,
    Expired,
    /// No `user_info` in the response (or an unknown state).
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerInfo {
    pub url: Option<String>,
    pub port: Option<u16>,
    pub https_port: Option<u16>,
    pub protocol: Option<String>,
    pub clock: ServerClock,
}

/// `user_info` + `server_info`. The echoed password is never kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub status: AccountStatus,
    /// Unix seconds; `None` = no expiry (`0`, negative, missing, invalid).
    pub expires_at: Option<i64>,
    pub created_at: Option<i64>,
    pub is_trial: bool,
    pub active_connections: Option<u32>,
    pub max_connections: Option<u32>,
    /// Lowercased, e.g. `["m3u8", "ts"]`.
    pub allowed_output_formats: Vec<String>,
    pub message: Option<String>,
    pub server: ServerInfo,
}

/// IPTVnator's status rules: status text is case-insensitive; `auth` 1/true
/// means active when there is no status text; `auth` 0/false is inactive;
/// a past positive `exp_date` is expired even when the status says active.
pub fn resolve_account_status(user_info: Option<&Value>, now_unix: i64) -> AccountStatus {
    let Some(info) = user_info.and_then(Obj::of) else {
        return AccountStatus::Unavailable;
    };
    let auth = info.bool("auth");
    if auth == Some(false) {
        return AccountStatus::Inactive;
    }
    let status = info
        .str("status")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if status == "expired" {
        return AccountStatus::Expired;
    }
    let active = status == "active" || (auth == Some(true) && status.is_empty());
    if !active {
        return if status.is_empty() {
            AccountStatus::Unavailable
        } else {
            AccountStatus::Inactive
        };
    }
    match expiry(info) {
        Some(exp) if exp < now_unix => AccountStatus::Expired,
        _ => AccountStatus::Active,
    }
}

fn expiry(info: Obj<'_>) -> Option<i64> {
    info.i64("exp_date").filter(|e| *e > 0)
}

pub fn parse_account_info(
    response: &HttpResponse,
    now_unix: i64,
) -> Result<AccountInfo, ProviderError> {
    let body = json_body(response)?;
    let root = Obj::of(&body)
        .ok_or_else(|| ProviderError::InvalidBody("account info is not an object".into()))?;
    let user = root.raw("user_info");
    let info = user.and_then(Obj::of);
    let server = root.obj("server_info");
    let clock = server.map(|s| {
        ServerClock::new(
            s.str("timezone").as_deref(),
            s.str("time_now").as_deref(),
            s.i64("timestamp_now"),
        )
    });
    Ok(AccountInfo {
        status: resolve_account_status(user, now_unix),
        expires_at: info.and_then(expiry),
        created_at: info.and_then(|i| i.i64("created_at")).filter(|c| *c > 0),
        is_trial: info.is_some_and(|i| i.flag("is_trial")),
        active_connections: info.and_then(|i| i.u32("active_cons")),
        max_connections: info.and_then(|i| i.u32("max_connections")),
        allowed_output_formats: info
            .map(|i| {
                i.strings("allowed_output_formats")
                    .into_iter()
                    .map(|f| f.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default(),
        message: info.and_then(|i| i.text("message")),
        server: ServerInfo {
            url: server.and_then(|s| s.text("url")),
            port: server
                .and_then(|s| s.u64("port"))
                .and_then(|p| u16::try_from(p).ok()),
            https_port: server
                .and_then(|s| s.u64("https_port"))
                .and_then(|p| u16::try_from(p).ok()),
            protocol: server.and_then(|s| s.text("server_protocol")),
            clock: clock.unwrap_or_default(),
        },
    })
}

/// The list items of a catalog response.
fn list(body: &Value) -> Result<Vec<&Value>, ProviderError> {
    match body {
        Value::Array(items) => Ok(items.iter().collect()),
        Value::Object(map) if map.contains_key("user_info") => Err(ProviderError::AccountRejected),
        Value::Object(map) if !map.is_empty() && map.values().all(Value::is_object) => {
            Ok(map.values().collect())
        }
        _ => Err(ProviderError::InvalidBody("expected a list".into())),
    }
}

fn group_ids(o: Obj<'_>, kind: ContentKind) -> Vec<GroupId> {
    let mut ids: Vec<String> = o.str("category_id").into_iter().collect();
    for extra in o.strings("category_ids") {
        if !ids.contains(&extra) {
            ids.push(extra);
        }
    }
    ids.iter()
        .map(|id| GroupId::from_provider(SourceKind::Xtream, kind, id))
        .collect()
}

fn positive_id(o: Obj<'_>, key: &str) -> Option<u64> {
    o.u64(key).filter(|id| *id > 0)
}

/// `get_live_categories` / `get_vod_categories` / `get_series_categories`.
pub fn parse_categories(
    response: &HttpResponse,
    kind: ContentKind,
) -> Result<Import<Group>, ProviderError> {
    let body = json_body(response)?;
    let mut out = Import::default();
    for (i, item) in list(&body)?.into_iter().enumerate() {
        let Some(o) = Obj::of(item) else {
            out.skip(WarningLocation::Index(i), "category is not an object");
            continue;
        };
        let Some(id) = o.str("category_id") else {
            out.skip(WarningLocation::Index(i), "category without category_id");
            continue;
        };
        let name = o
            .first_text(&["category_name", "name", "title"])
            .unwrap_or_else(|| format!("Category {id}"));
        out.items.push(Group {
            id: GroupId::from_provider(SourceKind::Xtream, kind, &id),
            adult: o.flag("is_adult"),
            provider_id: Some(id),
            name,
            kind,
        });
    }
    Ok(out)
}

/// `get_live_streams`. Ids use [`ContentKind::Live`] for radio too, so a
/// panel flipping `stream_type` does not orphan customisations.
pub fn parse_live_streams(response: &HttpResponse) -> Result<Import<Channel>, ProviderError> {
    let body = json_body(response)?;
    let mut out = Import::default();
    for (i, item) in list(&body)?.into_iter().enumerate() {
        let Some(o) = Obj::of(item) else {
            out.skip(WarningLocation::Index(i), "stream is not an object");
            continue;
        };
        let Some(stream_id) = positive_id(o, "stream_id") else {
            out.skip(
                WarningLocation::Index(i),
                "stream without a positive stream_id",
            );
            continue;
        };
        let name = o.text("name").unwrap_or_else(|| {
            out.warn(WarningLocation::Index(i), "stream without a name");
            format!("Channel {stream_id}")
        });
        let days = o.u32("tv_archive_duration").unwrap_or(0);
        let catch_up = (o.flag("tv_archive") && days > 0).then_some(CatchUp {
            kind: CatchUpKind::Xtream,
            days: Some(days),
            source: None,
            correction_secs: 0,
        });
        out.items.push(Channel {
            id: ContentId::from_provider(
                SourceKind::Xtream,
                ContentKind::Live,
                &stream_id.to_string(),
            ),
            provider_id: Some(stream_id.to_string()),
            name,
            number: o.u32("num"),
            logo: o.text("stream_icon"),
            groups: group_ids(o, ContentKind::Live),
            stream: StreamLocator::Xtream {
                stream_id,
                container_extension: None,
            },
            epg_id: o.text("epg_channel_id"),
            epg_name: None,
            epg_shift_minutes: None,
            catch_up,
            radio: o
                .str("stream_type")
                .is_some_and(|t| t.eq_ignore_ascii_case("radio_streams")),
        });
    }
    Ok(out)
}

/// An absolute HTTP(S) `direct_source`; other schemes and relative values
/// are ignored.
fn direct_source(o: Obj<'_>) -> Option<String> {
    let s = o.text("direct_source")?;
    url::Url::parse(&s)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .map(|_| s)
}

fn vod_locator(o: Obj<'_>, stream_id: u64) -> StreamLocator {
    match direct_source(o) {
        Some(url) => StreamLocator::Url {
            url,
            headers: Vec::new(),
            properties: Vec::new(),
        },
        None => StreamLocator::Xtream {
            stream_id,
            container_extension: o.text("container_extension"),
        },
    }
}

fn rating(o: Obj<'_>) -> Option<u16> {
    rating_x10(o.f64("rating")).or_else(|| rating_x10(o.f64("rating_5based").map(|r| r * 2.0)))
}

fn year(o: Obj<'_>) -> Option<u16> {
    o.u64("year")
        .and_then(|y| u16::try_from(y).ok())
        .filter(|y| (1900..=2100).contains(y))
        .or_else(|| {
            year_from(
                o.first_text(&["releaseDate", "release_date", "releasedate"])
                    .as_deref(),
            )
        })
}

fn duration(o: Obj<'_>) -> Option<u32> {
    o.u32("duration_secs").filter(|d| *d > 0).or_else(|| {
        // "HH:MM:SS" fallback.
        let text = o.text("duration")?;
        let parts: Vec<u32> = text
            .split(':')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        let secs = parts
            .iter()
            .fold(0u32, |acc, p| acc.saturating_mul(60).saturating_add(*p));
        (secs > 0).then_some(secs)
    })
}

fn vod_item(o: Obj<'_>, stream_id: u64, name: String) -> VodItem {
    VodItem {
        id: ContentId::from_provider(
            SourceKind::Xtream,
            ContentKind::Movie,
            &stream_id.to_string(),
        ),
        provider_id: Some(stream_id.to_string()),
        name,
        groups: group_ids(o, ContentKind::Movie),
        poster: o.first_text(&["stream_icon", "movie_image", "cover_big", "cover"]),
        plot: o.first_text(&["plot", "description"]),
        year: year(o),
        rating_x10: rating(o),
        added: o.i64("added").filter(|a| *a > 0),
        duration_secs: duration(o),
        stream: vod_locator(o, stream_id),
        is_series: false,
    }
}

/// `get_vod_streams`.
pub fn parse_vod_streams(response: &HttpResponse) -> Result<Import<VodItem>, ProviderError> {
    let body = json_body(response)?;
    let mut out = Import::default();
    for (i, item) in list(&body)?.into_iter().enumerate() {
        let Some(o) = Obj::of(item) else {
            out.skip(WarningLocation::Index(i), "movie is not an object");
            continue;
        };
        let Some(id) = positive_id(o, "stream_id") else {
            out.skip(
                WarningLocation::Index(i),
                "movie without a positive stream_id",
            );
            continue;
        };
        let name = o
            .text("name")
            .or_else(|| o.text("title"))
            .unwrap_or_else(|| format!("Movie {id}"));
        out.items.push(vod_item(o, id, name));
    }
    Ok(out)
}

fn series_item(o: Obj<'_>, series_id: u64) -> Series {
    Series {
        id: ContentId::from_provider(
            SourceKind::Xtream,
            ContentKind::Series,
            &series_id.to_string(),
        ),
        provider_id: Some(series_id.to_string()),
        name: o
            .text("name")
            .or_else(|| o.text("title"))
            .unwrap_or_else(|| format!("Series {series_id}")),
        groups: group_ids(o, ContentKind::Series),
        poster: o.first_text(&["cover", "cover_big", "movie_image"]),
        plot: o.first_text(&["plot", "description"]),
        year: year(o),
        rating_x10: rating(o),
        last_modified: o.i64("last_modified").filter(|t| *t > 0),
    }
}

/// `get_series`.
pub fn parse_series_list(response: &HttpResponse) -> Result<Import<Series>, ProviderError> {
    let body = json_body(response)?;
    let mut out = Import::default();
    for (i, item) in list(&body)?.into_iter().enumerate() {
        let Some(o) = Obj::of(item) else {
            out.skip(WarningLocation::Index(i), "series is not an object");
            continue;
        };
        let Some(id) = positive_id(o, "series_id") else {
            out.skip(
                WarningLocation::Index(i),
                "series without a positive series_id",
            );
            continue;
        };
        out.items.push(series_item(o, id));
    }
    Ok(out)
}

/// `get_series_info`. `episodes` may be an object keyed by season, an array
/// of per-season arrays, or a flat array; `info` may be PHP's `[]`.
pub fn parse_series_info(
    response: &HttpResponse,
    series_id: u64,
) -> Result<SeriesDetails, ProviderError> {
    let body = json_body(response)?;
    let root = Obj::of(&body)
        .ok_or_else(|| ProviderError::InvalidBody("series info is not an object".into()))?;
    let mut out: Import<Episode> = Import::default();
    let empty = serde_json::Map::new();
    let info = root.obj("info").unwrap_or(Obj(&empty));
    let series = series_item(info, series_id);

    let mut raw: Vec<(Option<u32>, &Value)> = Vec::new();
    match root.raw("episodes") {
        Some(Value::Object(by_season)) => {
            for (key, eps) in by_season {
                let season = key.trim().parse::<u32>().ok();
                match list_items(eps) {
                    Some(items) => raw.extend(items.into_iter().map(|e| (season, e))),
                    None => out.warn(WarningLocation::Index(0), "season episodes are not a list"),
                }
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                match item {
                    Value::Array(inner) => raw.extend(inner.iter().map(|e| (None, e))),
                    other => raw.push((None, other)),
                }
            }
        }
        _ => {}
    }
    for (i, (season_key, value)) in raw.into_iter().enumerate() {
        let Some(e) = Obj::of(value) else {
            out.skip(WarningLocation::Index(i), "episode is not an object");
            continue;
        };
        let Some(id) = positive_id(e, "id") else {
            out.skip(WarningLocation::Index(i), "episode without a positive id");
            continue;
        };
        let einfo = e.obj("info").unwrap_or(Obj(&empty));
        let season = e.u32("season").or(season_key).unwrap_or(1);
        let number = e.u32("episode_num").unwrap_or(0);
        let stream = match direct_source(e) {
            Some(url) => StreamLocator::Url {
                url,
                headers: Vec::new(),
                properties: Vec::new(),
            },
            None => StreamLocator::Xtream {
                stream_id: id,
                container_extension: e.text("container_extension"),
            },
        };
        out.items.push(Episode {
            id: ContentId::from_provider(SourceKind::Xtream, ContentKind::Episode, &id.to_string()),
            provider_id: Some(id.to_string()),
            season,
            number,
            title: e
                .text("title")
                .unwrap_or_else(|| format!("Episode {number}")),
            plot: einfo.first_text(&["plot", "description"]),
            image: einfo.first_text(&["movie_image", "cover_big"]),
            duration_secs: duration(einfo),
            stream,
        });
    }
    out.items.sort_by_key(|e| (e.season, e.number));

    let mut seasons: Vec<Season> = root
        .raw("seasons")
        .and_then(list_items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(Obj::of)
        .filter_map(|s| {
            Some(Season {
                number: s.u32("season_number")?,
                name: s.text("name"),
                episode_count: s.u32("episode_count"),
                poster: s.first_text(&["cover", "cover_big"]),
            })
        })
        .collect();
    for e in &out.items {
        if !seasons.iter().any(|s| s.number == e.season) {
            seasons.push(Season {
                number: e.season,
                name: None,
                episode_count: None,
                poster: None,
            });
        }
    }
    seasons.sort_by_key(|s| s.number);
    seasons.dedup_by_key(|s| s.number);
    Ok(SeriesDetails {
        series,
        seasons,
        episodes: out,
    })
}

/// `get_vod_info` result: the item plus detail-only fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VodDetails {
    pub item: VodItem,
    pub cast: Option<String>,
    pub director: Option<String>,
    pub genre: Option<String>,
    pub trailer: Option<String>,
    pub tmdb_id: Option<u64>,
    pub backdrops: Vec<String>,
    /// Whether the panel returned a playable source (`movie_data` first,
    /// top-level fallback; positive id plus container or direct source).
    pub playable: bool,
}

/// `get_vod_info`. Null/absent payloads normalise to empty metadata.
pub fn parse_vod_info(response: &HttpResponse, vod_id: u64) -> Result<VodDetails, ProviderError> {
    let body = json_body(response)?;
    let empty = serde_json::Map::new();
    let root = Obj::of(&body).unwrap_or(Obj(&empty));
    let info = root.obj("info").unwrap_or(Obj(&empty));
    let data = root.obj("movie_data");

    // Playback source: movie_data wins as one unit, then the top level.
    fn candidate(o: Obj<'_>) -> Option<(Obj<'_>, u64)> {
        let id = positive_id(o, "stream_id")?;
        (o.text("container_extension").is_some() || direct_source(o).is_some()).then_some((o, id))
    }
    let source = data.and_then(candidate).or_else(|| candidate(root));
    let meta = data.unwrap_or(root);
    let id = source
        .map(|(_, id)| id)
        .or_else(|| positive_id(meta, "stream_id"))
        .unwrap_or(vod_id);
    let name = meta
        .text("name")
        .or_else(|| info.text("name"))
        .or_else(|| info.text("o_name"))
        .unwrap_or_else(|| format!("Movie {id}"));
    let mut item = vod_item(meta, id, name);
    if let Some((src, sid)) = source {
        item.stream = vod_locator(src, sid);
    }
    item.poster = info
        .first_text(&["movie_image", "cover_big"])
        .or(item.poster);
    item.plot = info.first_text(&["plot", "description"]).or(item.plot);
    item.year = year(info).or(item.year);
    item.rating_x10 = rating(info).or(item.rating_x10);
    item.duration_secs = duration(info).or(item.duration_secs);
    let mut backdrops = info.strings("backdrop_path");
    backdrops.retain(|b| !b.eq_ignore_ascii_case("n/a"));
    Ok(VodDetails {
        cast: info.first_text(&["cast", "actors"]),
        director: info.text("director"),
        genre: info.text("genre"),
        trailer: info.first_text(&["youtube_trailer", "trailer"]),
        tmdb_id: info
            .u64("tmdb_id")
            .filter(|t| *t > 0)
            .or_else(|| info.u64("tmdb").filter(|t| *t > 0)),
        backdrops,
        playable: source.is_some(),
        item,
    })
}

/// `get_short_epg` and `get_simple_data_table`. Titles and descriptions are
/// base64 (kept as-is when they are not). `start_timestamp`/`stop_timestamp`
/// win; otherwise `start`/`end` (or `stop`) are read in the panel's clock
/// (`server_offset_minutes`, `0` when unknown). Sorted by start.
pub fn parse_epg(
    response: &HttpResponse,
    server_offset_minutes: i32,
) -> Result<Import<Programme>, ProviderError> {
    let body = json_body(response)?;
    let listings = match &body {
        Value::Object(map) => map
            .get("epg_listings")
            .and_then(list_items)
            .unwrap_or_default(),
        Value::Array(items) => items.iter().collect(),
        _ => return Err(ProviderError::InvalidBody("expected epg_listings".into())),
    };
    let mut out = Import::default();
    let decode = |s: String| {
        b64::decode_utf8(&s)
            .map(|d| d.trim().to_owned())
            .unwrap_or(s)
    };
    for (i, item) in listings.into_iter().enumerate() {
        let Some(o) = Obj::of(item) else {
            out.skip(WarningLocation::Index(i), "programme is not an object");
            continue;
        };
        let local = |key: &str| {
            o.str(key)
                .and_then(|t| crate::time::parse_local_datetime(&t, server_offset_minutes))
        };
        let start = o
            .i64("start_timestamp")
            .filter(|t| *t > 0)
            .or_else(|| local("start"));
        let end = o
            .i64("stop_timestamp")
            .filter(|t| *t > 0)
            .or_else(|| local("stop"))
            .or_else(|| local("end"));
        let (Some(start), Some(end)) = (start, end) else {
            out.skip(WarningLocation::Index(i), "programme without start/end");
            continue;
        };
        if end <= start {
            out.skip(WarningLocation::Index(i), "programme ends before it starts");
            continue;
        }
        out.items.push(Programme {
            epg_id: o.text("epg_id").or_else(|| o.text("channel_id")),
            title: o.str("title").map(decode).unwrap_or_default(),
            description: o.str("description").map(decode).filter(|d| !d.is_empty()),
            start,
            end,
            has_archive: o.flag("has_archive"),
        });
    }
    out.items.sort_by_key(|p| p.start);
    Ok(out)
}
