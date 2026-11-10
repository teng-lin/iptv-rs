//! A playlist's guide sources: the provider's own guides (playlist header,
//! Xtream `xmltv.php`) and any the user added, in the user's order, each
//! switched on or off and with its time shift (`iptv_store_sqlite::
//! GuideSource`, schema v6). Pure: what changes when the user edits the
//! list or a playlist import lists the provider's guides, and the configs a
//! guide download takes from it.
//!
//! Feed ids are stable per playlist: the provider's n-th guide is feed
//! `2 + n`; the first guide the user adds is feed 1 (as a v5 build's typed
//! guide), later ones count up from 1001. Priorities follow the list
//! (first wins) and are the smallest that keep that order above each
//! source's natural one (the user's 1000, the provider's in its own order),
//! so a guide arranged as a v5 build had it is not downloaded again.

use iptv_epg::{SourceId, TimePolicy};
use iptv_import::EpgSourceConfig;
use iptv_providers::SecretUrl;
use iptv_store_sqlite::GuideSource;

/// The feed of the first guide the user adds.
pub const FIRST_USER_FEED: SourceId = SourceId(1);
/// The provider's n-th guide is feed `PROVIDER_FEEDS + n`.
const PROVIDER_FEEDS: u32 = 2;
/// Later guides the user adds: above this.
const MORE_USER_FEEDS: u32 = 1000;
/// The natural priority of a guide the user added (above the provider's).
const USER_PRIORITY: i32 = 1000;
/// Time shifts the user can choose, in minutes either way.
pub const MAX_SHIFT_MINUTES: i32 = 12 * 60;

/// A guide source the user adds.
pub fn user_source(feed: SourceId, url: SecretUrl, name: Option<String>) -> GuideSource {
    GuideSource {
        feed,
        provider_index: None,
        url,
        name: name.filter(|n| !n.trim().is_empty()),
        enabled: true,
        shift_minutes: 0,
    }
}

/// The feed id for a guide the user adds to `sources`.
pub fn next_user_feed(sources: &[GuideSource]) -> SourceId {
    let highest = sources
        .iter()
        .filter(|s| !s.is_provider())
        .map(|s| s.feed.0)
        .max();
    match highest {
        None => FIRST_USER_FEED,
        Some(n) => SourceId(n.max(MORE_USER_FEEDS) + 1),
    }
}

/// The provider's guides `urls` (from a playlist import) into `sources`:
/// a known one keeps its place, switch, shift and name with its address of
/// now; a new one goes last, switched on; one the provider no longer lists
/// goes.
pub fn sync_provider(sources: &[GuideSource], urls: &[SecretUrl]) -> Vec<GuideSource> {
    let mut out: Vec<GuideSource> = sources
        .iter()
        .filter_map(|s| match s.provider_index {
            None => Some(s.clone()),
            Some(n) => urls.get(n as usize).map(|url| GuideSource {
                url: url.clone(),
                ..s.clone()
            }),
        })
        .collect();
    for (n, url) in urls.iter().enumerate() {
        let n = n as u32;
        if !out.iter().any(|s| s.provider_index == Some(n)) {
            out.push(GuideSource {
                feed: SourceId(PROVIDER_FEEDS + n),
                provider_index: Some(n),
                url: url.clone(),
                name: None,
                enabled: true,
                shift_minutes: 0,
            });
        }
    }
    out
}

/// The configs of a guide download: the sources switched on, first one
/// highest. `template` carries the storage options.
pub fn configs_of(sources: &[GuideSource], template: &EpgSourceConfig) -> Vec<EpgSourceConfig> {
    let providers = sources.iter().filter(|s| s.is_provider()).count() as i32;
    let on: Vec<&GuideSource> = sources
        .iter()
        .filter(|s| s.enabled && !s.url.expose().trim().is_empty())
        .collect();
    let mut priorities = vec![0; on.len()];
    let mut below: Option<i32> = None;
    for (i, s) in on.iter().enumerate().rev() {
        let natural = match s.provider_index {
            None => USER_PRIORITY,
            Some(n) => providers - 1 - n as i32,
        };
        let p = below.map_or(natural, |b| natural.max(b + 1));
        priorities[i] = p;
        below = Some(p);
    }
    on.iter()
        .zip(priorities)
        .map(|(s, priority)| EpgSourceConfig {
            id: s.feed,
            url: s.url.clone(),
            priority,
            time: TimePolicy {
                shift_minutes: s.shift_minutes,
                ..template.time
            },
            ..template.clone()
        })
        .collect()
}

/// An edit of the list on the guide sources screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Add {
        url: SecretUrl,
        name: Option<String>,
    },
    /// Only a guide the user added; the provider's are switched off.
    Remove(SourceId),
    SetEnabled(SourceId, bool),
    /// One place up (`-1`) or down (`1`).
    Move(SourceId, i32),
    Shift(SourceId, i32),
    Rename(SourceId, Option<String>),
    /// A new address for a guide the user added.
    Relink(SourceId, SecretUrl),
}

/// `sources` after `edit`; `None` when it changes nothing.
pub fn apply(sources: &[GuideSource], edit: &Edit) -> Option<Vec<GuideSource>> {
    let mut out = sources.to_vec();
    let at = |id: SourceId| out.iter().position(|s| s.feed == id);
    match edit {
        Edit::Add { url, name } => {
            let feed = next_user_feed(&out);
            out.push(user_source(feed, url.clone(), name.clone()));
        }
        Edit::Remove(id) => {
            let i = at(*id).filter(|&i| !out[i].is_provider())?;
            out.remove(i);
        }
        Edit::SetEnabled(id, on) => {
            let i = at(*id).filter(|&i| out[i].enabled != *on)?;
            out[i].enabled = *on;
        }
        Edit::Move(id, step) => {
            let i = at(*id)?;
            let to = usize::try_from(i as i64 + i64::from(*step))
                .ok()
                .filter(|&to| to < out.len() && to != i)?;
            let s = out.remove(i);
            out.insert(to, s);
        }
        Edit::Shift(id, minutes) => {
            let i = at(*id)?;
            let m = (*minutes).clamp(-MAX_SHIFT_MINUTES, MAX_SHIFT_MINUTES);
            (out[i].shift_minutes != m).then_some(())?;
            out[i].shift_minutes = m;
        }
        Edit::Rename(id, name) => {
            let i = at(*id)?;
            let name = name.clone().filter(|n| !n.trim().is_empty());
            (out[i].name != name).then_some(())?;
            out[i].name = name;
        }
        Edit::Relink(id, url) => {
            let i = at(*id).filter(|&i| !out[i].is_provider())?;
            (out[i].url != *url).then_some(())?;
            out[i].url = url.clone();
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> SecretUrl {
        SecretUrl::new(format!("http://{s}.example.test/guide.xml"))
    }

    fn template() -> EpgSourceConfig {
        EpgSourceConfig::new(SourceId(0), url("template"))
    }

    fn feeds(sources: &[GuideSource]) -> Vec<u32> {
        sources.iter().map(|s| s.feed.0).collect()
    }

    #[test]
    fn the_providers_guides_follow_each_import() {
        let typed = user_source(FIRST_USER_FEED, url("typed"), None);
        let first = sync_provider(&[typed.clone()], &[url("a"), url("b")]);
        assert_eq!(feeds(&first), [1, 2, 3]);
        assert_eq!(first[1].provider_index, Some(0));
        // The user's arrangement stays; a changed address is taken over.
        let mut arranged = first.clone();
        arranged.swap(0, 2);
        arranged[0].enabled = false;
        arranged[0].shift_minutes = 60;
        let again = sync_provider(&arranged, &[url("a2"), url("b")]);
        assert_eq!(feeds(&again), [3, 2, 1]);
        assert_eq!((again[0].enabled, again[0].shift_minutes), (false, 60));
        assert_eq!(again[1].url, url("a2"));
        // One the provider dropped goes; the user's stay.
        let fewer = sync_provider(&again, &[url("a2")]);
        assert_eq!(feeds(&fewer), [2, 1]);
        assert_eq!(sync_provider(&fewer, &[]), [typed]);
    }

    #[test]
    fn feeds_the_user_adds_never_clash_with_the_providers() {
        assert_eq!(next_user_feed(&[]), FIRST_USER_FEED);
        let mut s = sync_provider(&[], &[url("a"), url("b")]);
        assert_eq!(next_user_feed(&s), FIRST_USER_FEED);
        s = apply(
            &s,
            &Edit::Add {
                url: url("u1"),
                name: None,
            },
        )
        .unwrap();
        s = apply(
            &s,
            &Edit::Add {
                url: url("u2"),
                name: Some(" ".into()),
            },
        )
        .unwrap();
        s = apply(
            &s,
            &Edit::Add {
                url: url("u3"),
                name: Some("Mine".into()),
            },
        )
        .unwrap();
        assert_eq!(feeds(&s), [2, 3, 1, 1001, 1002]);
        assert_eq!(
            (s[3].name.as_deref(), s[4].name.as_deref()),
            (None, Some("Mine"))
        );
    }

    #[test]
    fn priorities_follow_the_list_and_keep_a_v5_arrangement() {
        let typed = user_source(FIRST_USER_FEED, url("typed"), None);
        // As a v5 build had an Xtream source with a typed guide.
        let v5 = sync_provider(&[typed], &[url("xtream")]);
        let c = configs_of(&v5, &template());
        assert_eq!(
            c.iter().map(|c| (c.id.0, c.priority)).collect::<Vec<_>>(),
            [(1, 1000), (2, 0)]
        );
        // The provider's guide first: above the user's.
        let moved = apply(&v5, &Edit::Move(SourceId(2), -1)).unwrap();
        let c = configs_of(&moved, &template());
        assert_eq!(
            c.iter().map(|c| (c.id.0, c.priority)).collect::<Vec<_>>(),
            [(2, 1001), (1, 1000)]
        );
        // Switched off: not downloaded; the shift travels with the config.
        let off = apply(&moved, &Edit::SetEnabled(SourceId(1), false)).unwrap();
        let shifted = apply(&off, &Edit::Shift(SourceId(2), -120)).unwrap();
        let c = configs_of(&shifted, &template());
        assert_eq!(c.len(), 1);
        assert_eq!(
            (c[0].id.0, c[0].priority, c[0].time.shift_minutes),
            (2, 0, -120)
        );
    }

    #[test]
    fn edits_change_only_what_they_name() {
        let s = sync_provider(&[user_source(FIRST_USER_FEED, url("u"), None)], &[url("p")]);
        let provider = SourceId(2);
        assert_eq!(
            apply(&s, &Edit::Remove(provider)),
            None,
            "switched off instead"
        );
        assert_eq!(apply(&s, &Edit::Relink(provider, url("x"))), None);
        assert_eq!(
            apply(&s, &Edit::Move(FIRST_USER_FEED, -1)),
            None,
            "already first"
        );
        assert_eq!(apply(&s, &Edit::Move(provider, 1)), None, "already last");
        assert_eq!(apply(&s, &Edit::SetEnabled(provider, true)), None);
        assert_eq!(apply(&s, &Edit::Shift(provider, 0)), None);
        let far = apply(&s, &Edit::Shift(provider, 100 * 60)).unwrap();
        assert_eq!(far[1].shift_minutes, MAX_SHIFT_MINUTES);
        let removed = apply(&s, &Edit::Remove(FIRST_USER_FEED)).unwrap();
        assert_eq!(feeds(&removed), [2]);
        let renamed = apply(&s, &Edit::Rename(provider, Some("Provider".into()))).unwrap();
        assert_eq!(renamed[1].name.as_deref(), Some("Provider"));
        assert_eq!(
            apply(&renamed, &Edit::Rename(provider, Some("".into()))).unwrap()[1].name,
            None
        );
    }
}
