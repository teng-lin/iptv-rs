//! The wizard's wording (iptv-rs's own; i18n later).

use iptv_core::wizard::{
    FailureKind, Field, FieldError, ImportFailure, ImportProgress, ImportStage, PlaylistKind,
};

pub fn kind_title(kind: PlaylistKind) -> &'static str {
    match kind {
        PlaylistKind::M3uUrl => "Playlist link",
        PlaylistKind::M3uFile => "Playlist file",
        PlaylistKind::Xtream => "Xtream Codes login",
        PlaylistKind::Stalker => "Stalker portal",
    }
}

pub fn kind_detail(kind: PlaylistKind) -> &'static str {
    match kind {
        PlaylistKind::M3uUrl => "An M3U or M3U8 address starting with http:// or https://",
        PlaylistKind::M3uFile => "An M3U file saved on this device",
        PlaylistKind::Xtream => "A server address with a username and password",
        PlaylistKind::Stalker => "A portal address and the MAC address it knows this device by",
    }
}

pub fn label(field: Field) -> &'static str {
    match field {
        Field::PlaylistUrl => "Playlist link",
        Field::PlaylistFile => "File",
        Field::Server => "Server",
        Field::Username => "Username",
        Field::Password => "Password",
        Field::PortalUrl => "Portal",
        Field::Mac => "MAC address",
        Field::Name => "Name",
        Field::EpgUrl => "Guide link",
    }
}

/// Shown in an empty field and as the text entry's placeholder.
pub fn placeholder(field: Field) -> &'static str {
    match field {
        Field::PlaylistUrl => "http://…/playlist.m3u",
        Field::PlaylistFile => "Choose a file…",
        Field::Server => "http://server:port",
        Field::Username | Field::Password => "Required",
        Field::PortalUrl => "http://portal/c/",
        Field::Mac => "00:1A:79:00:00:00",
        Field::Name => "Optional: how the playlist is listed",
        Field::EpgUrl => "Optional: an XMLTV link (the provider's guide otherwise)",
    }
}

pub fn field_error(error: FieldError) -> &'static str {
    match error {
        FieldError::Required => "Fill this in",
        FieldError::ContainsWhitespace => "Remove the spaces",
        FieldError::MissingScheme => "Start with http:// or https://",
        FieldError::UnsupportedScheme => "Only http:// and https:// links work",
        FieldError::InvalidHost => "Check the server name",
        FieldError::InvalidPort => "The port is a number from 1 to 65535",
        FieldError::InvalidMac => "A MAC address has 12 hexadecimal digits",
    }
}

/// Why the last import did not add the playlist.
pub fn failure(kind: PlaylistKind, failure: ImportFailure) -> &'static str {
    if failure.field == Some(Field::EpgUrl) && failure.kind != FailureKind::Cancelled {
        return "The guide link did not load. Correct it, or clear it to use the provider's guide.";
    }
    let m3u = matches!(kind, PlaylistKind::M3uUrl | PlaylistKind::M3uFile);
    match failure.kind {
        FailureKind::Network if kind == PlaylistKind::M3uFile => {
            "The file could not be read. Choose it again."
        }
        FailureKind::Network => {
            "No answer from the server. Check the address and the connection, then try again."
        }
        FailureKind::Auth if m3u => "The server refused access to this playlist.",
        FailureKind::Auth => "The provider refused these login details. Check them and try again.",
        FailureKind::Parse if m3u => "This is not a playlist the app can read. Check the address.",
        FailureKind::Parse => {
            "The server did not answer as this type of service does. Check the address."
        }
        FailureKind::EmptyPlaylist => "The playlist loaded but has nothing to watch in it.",
        FailureKind::Unsupported => {
            "This server is not a service of the chosen type. Try another type."
        }
        FailureKind::Cancelled => {
            "Stopped. Your entries are kept; add the playlist again when ready."
        }
    }
}

pub fn stage(progress: Option<ImportProgress>) -> String {
    let Some(p) = progress else {
        return "Starting…".to_owned();
    };
    let what = match p.stage {
        ImportStage::Connecting => "Connecting…",
        ImportStage::Downloading => "Downloading…",
        ImportStage::Parsing => "Reading the channels…",
        ImportStage::Saving => "Saving…",
    };
    match p.percent() {
        Some(pc) => format!("{what} {pc}%"),
        None => what.to_owned(),
    }
}

pub fn counts(c: iptv_core::wizard::ImportCounts) -> String {
    let plural = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut text = format!(
        "{} in {}",
        plural(c.channels, "channel", "channels"),
        plural(c.groups, "group", "groups")
    );
    if c.movies > 0 {
        text += &format!(" · {}", plural(c.movies, "film", "films"));
    }
    if c.series > 0 {
        text += &format!(" · {}", plural(c.series, "series", "series"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptv_core::wizard::ImportCounts;

    #[test]
    fn every_failure_kind_has_its_own_message() {
        let kinds = [
            FailureKind::Network,
            FailureKind::Auth,
            FailureKind::Parse,
            FailureKind::EmptyPlaylist,
            FailureKind::Unsupported,
            FailureKind::Cancelled,
        ];
        let texts: std::collections::BTreeSet<_> = kinds
            .iter()
            .map(|k| failure(PlaylistKind::Xtream, ImportFailure::new(*k)))
            .collect();
        assert_eq!(texts.len(), kinds.len());
        let epg = ImportFailure {
            kind: FailureKind::Network,
            field: Some(Field::EpgUrl),
        };
        assert!(failure(PlaylistKind::M3uUrl, epg).contains("guide link"));
    }

    #[test]
    fn counts_read_naturally() {
        let c = ImportCounts {
            channels: 1,
            groups: 3,
            movies: 0,
            series: 2,
        };
        assert_eq!(counts(c), "1 channel in 3 groups · 2 series");
    }
}
