//! The playlists screen's wording (iptv-rs's own; i18n later).

use iptv_core::wizard::{FailureKind, ImportFailure};
use iptv_providers::model::SourceKind;
use iptv_store_sqlite::ImportErrorKind;

use super::details::{Form, Param};
use super::state::{Act, Notice};
use crate::data::{Activity, Playlist};

pub fn kind(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::M3u => "M3U playlist",
        SourceKind::Xtream => "Xtream Codes",
        SourceKind::Stalker => "Stalker portal",
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn channels(n: u32) -> String {
    plural(n, "channel", "channels")
}

/// A time as the guide shows it: today's as `14:05`, others with the day.
fn when(at: i64, now: i64) -> String {
    iptv_core::clock::label(at, now, crate::time::offset_minutes)
}

/// Why a refresh recorded in the store failed (kind only).
pub fn stored_failure(kind: ImportErrorKind) -> &'static str {
    match kind {
        ImportErrorKind::Network => "no answer from the server",
        ImportErrorKind::Timeout => "the server took too long to answer",
        ImportErrorKind::HttpStatus => "the server turned the request down",
        ImportErrorKind::InvalidBody => "the answer was not a playlist",
        ImportErrorKind::InvalidConfig => "the playlist's details are incomplete",
        ImportErrorKind::AccountRejected => "the provider refused the login",
        ImportErrorKind::AuthFailed => "the portal ended the session",
        ImportErrorKind::Blocked => "the portal does not accept this device",
        ImportErrorKind::Cancelled => "it was stopped",
        _ => "something went wrong",
    }
}

/// Why this session's refresh failed.
pub fn failure(failure: ImportFailure) -> &'static str {
    match failure.kind {
        FailureKind::Network => "no answer from the server",
        FailureKind::Auth => "access was refused",
        FailureKind::Parse => "the answer was not a playlist",
        FailureKind::EmptyPlaylist => "the playlist had nothing to watch in it",
        FailureKind::Unsupported => "the server is not a service of this type",
        FailureKind::Cancelled => "it was stopped",
    }
}

/// The row's second line: type and channels.
pub fn summary(p: &Playlist) -> String {
    format!("{} · {}", kind(p.kind), channels(p.channels))
}

/// The row's status: a refresh running or just finished, else what the
/// store has. A failure keeps the channels from the last success.
pub fn status(p: &Playlist, activity: Option<&Activity>, now: i64) -> (String, bool) {
    let kept = || {
        p.updated_at
            .map(|at| format!(" · showing the channels from {}", when(at, now)))
            .unwrap_or_default()
    };
    match activity {
        Some(Activity::Running(progress)) => (crate::wizard_view::words::stage(*progress), false),
        Some(Activity::Checking(progress)) => (
            format!(
                "Checking the new details: {}",
                crate::wizard_view::words::stage(*progress)
            ),
            false,
        ),
        Some(Activity::Rejected(f)) => (rejected(*f), true),
        // The store's kind is the more precise one, once recorded.
        Some(Activity::Done(Err(f))) => {
            let why = match p.failed {
                Some((kind, _)) if kind != ImportErrorKind::Other => stored_failure(kind),
                _ => failure(*f),
            };
            (format!("Refresh failed: {why}{}", kept()), true)
        }
        _ => match (p.failed, p.updated_at) {
            (Some((kind, _)), _) => (
                format!("Last refresh failed: {}{}", stored_failure(kind), kept()),
                true,
            ),
            (None, Some(at)) => (format!("Updated {}", when(at, now)), false),
            (None, None) => (String::new(), false),
        },
    }
}

/// A guide link for display: the address without user info, query values
/// hidden (accounts and tokens go there).
pub fn masked_link(link: &str) -> String {
    let (scheme, rest) = link.split_once("://").unwrap_or(("", link));
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = authority.rsplit('@').next().unwrap_or("");
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let path = path.split('#').next().unwrap_or("");
    let mut shown = if scheme.is_empty() {
        format!("{host}{path}")
    } else {
        format!("{scheme}://{host}{path}")
    };
    if !query.is_empty() {
        let query = query.split('#').next().unwrap_or("");
        let keys: Vec<String> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((k, _)) => format!("{k}=•••"),
                None => "•••".to_owned(),
            })
            .collect();
        shown.push('?');
        shown.push_str(&keys.join("&"));
    }
    shown
}

pub fn guide_line(p: &Playlist) -> String {
    let from = match &p.guide_link {
        Some(link) => masked_link(link.expose()),
        None => "the provider's own".to_owned(),
    };
    let failed = if p.guide_failed {
        " · did not load last time"
    } else {
        ""
    };
    format!("Guide: {from}{failed}")
}

/// New details that did not import.
pub fn rejected(f: ImportFailure) -> String {
    format!("New details not saved: {}. The old ones stay.", failure(f))
}

pub fn act(act: Act, kind: Option<SourceKind>, shown: bool, running: bool) -> &'static str {
    match act {
        Act::Show if shown => "On screen now: back to the guide",
        Act::Show => "Show this playlist",
        Act::Refresh if running => "Refreshing…",
        Act::Refresh => "Refresh now",
        Act::Rename => "Rename",
        Act::Details => kind.map_or("Provider details", details_title),
        Act::UserAgent => "User-Agent",
        Act::GuideLink => "Guide link",
        Act::ProviderGuide => "Use only the provider's guide",
        Act::Delete => "Delete",
    }
}

pub fn notice(notice: Notice) -> &'static str {
    match notice {
        Notice::NameEmpty => "A playlist needs a name. The old one is kept.",
        Notice::Link(e) => crate::wizard_view::words::field_error(e),
        Notice::Unchanged => "Nothing was changed.",
    }
}

/// The "Refresh all" row's status.
pub fn refreshing(running: usize) -> String {
    match running {
        0 => String::new(),
        1 => "Refreshing 1 playlist…".to_owned(),
        n => format!("Refreshing {n} playlists…"),
    }
}

/// The provider details a kind of playlist has.
pub fn details_title(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::M3u => "Playlist link",
        SourceKind::Xtream => "Server and login",
        SourceKind::Stalker => "Portal and MAC address",
    }
}

pub fn param(p: Param) -> &'static str {
    crate::wizard_view::words::label(p.field())
}

/// Text entry's placeholder: a value that is not shown stays when the
/// field is left empty.
pub fn param_placeholder(p: Param, prefilled: bool) -> &'static str {
    if prefilled {
        crate::wizard_view::words::placeholder(p.field())
    } else {
        "Type the new value; left empty, the current one stays"
    }
}

/// A detail as the form shows it: logins and MAC addresses hidden, links
/// without accounts or tokens.
pub fn param_value(form: &Form, p: Param) -> String {
    let value = form.value(p);
    if p.secret() {
        "••••••••".to_owned()
    } else if p.is_link() {
        masked_link(&value)
    } else {
        value
    }
}

/// What deleting `p` takes away, and what the guide does next.
pub fn delete_consequences(p: &Playlist, next: Option<&str>, shown: bool) -> String {
    let mut text = format!(
        "Its {}, its guide and your favourites from it will be gone from the app. \
         The playlist at your provider is not affected.",
        channels(p.channels)
    );
    match (shown, next) {
        (true, Some(next)) => text += &format!(" The guide switches to “{next}”."),
        (_, None) => text += " No playlists will be left.",
        (false, Some(_)) => {}
    }
    text
}

#[cfg(test)]
mod tests {
    use iptv_store_sqlite::SourceId;

    use super::*;

    fn playlist() -> Playlist {
        Playlist {
            id: SourceId(1),
            name: "Synthetic".into(),
            kind: SourceKind::Xtream,
            channels: 1,
            updated_at: None,
            failed: None,
            guide_link: None,
            guide_failed: false,
            user_agent: None,
            from_file: false,
        }
    }

    #[test]
    fn links_never_show_credentials() {
        assert_eq!(
            masked_link("http://guide.example.test:8080/xmltv.php?username=u1&password=p1"),
            "http://guide.example.test:8080/xmltv.php?username=•••&password=•••"
        );
        assert_eq!(
            masked_link("https://user:secret@guide.example.test/epg.xml.gz#x"),
            "https://guide.example.test/epg.xml.gz"
        );
        assert_eq!(
            masked_link("http://guide.example.test/e.xml?token"),
            "http://guide.example.test/e.xml?•••"
        );
        assert_eq!(
            masked_link("http://guide.example.test"),
            "http://guide.example.test"
        );
    }

    #[test]
    fn statuses() {
        let mut p = playlist();
        assert_eq!(summary(&p), "Xtream Codes · 1 channel");
        assert_eq!(status(&p, None, 0), (String::new(), false));
        p.failed = Some((ImportErrorKind::Timeout, Some(5)));
        let (text, error) = status(&p, None, 0);
        assert!(error && text.contains("took too long"));
        let done = Activity::Done(Err(ImportFailure::new(FailureKind::Network)));
        // The kind the store recorded is the more precise one.
        let (text, error) = status(&p, Some(&done), 0);
        assert!(error && text.starts_with("Refresh failed: the server took too long"));
        p.failed = None;
        let (text, _) = status(&p, Some(&done), 0);
        assert!(text.starts_with("Refresh failed: no answer"));
        let (text, error) = status(&p, Some(&Activity::Running(None)), 0);
        assert!(!error && text == "Starting…");
    }

    #[test]
    fn the_guide_line_names_its_source_and_a_failure() {
        let mut p = playlist();
        assert_eq!(guide_line(&p), "Guide: the provider's own");
        p.guide_link = Some(iptv_providers::SecretUrl::new(
            "http://guide.example.test/x.xml?token=t",
        ));
        p.guide_failed = true;
        assert_eq!(
            guide_line(&p),
            "Guide: http://guide.example.test/x.xml?token=••• · did not load last time"
        );
    }

    #[test]
    fn deleting_states_its_consequences() {
        let p = playlist();
        let text = delete_consequences(&p, Some("Other"), true);
        assert!(text.contains("1 channel,") && text.contains("favourites"));
        assert!(text.contains("switches to “Other”"));
        assert!(delete_consequences(&p, None, true).contains("No playlists will be left"));
        assert!(!delete_consequences(&p, Some("Other"), false).contains("switches"));
    }
}
