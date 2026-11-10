//! The settings screen's wording (iptv-rs's own): names of sections,
//! subsections, settings, commands and choices, and how values read. The
//! catalogue's summaries serve as the rows' descriptions.

use iptv_settings::schema::{IntUnit, TimeUnit, ValueType};
use iptv_settings::{Section, Setting, Value};

/// The name of a settings page (`settings_view::PAGES`).
pub fn page(key: &str) -> &'static str {
    match key {
        "general" => "General",
        "playlists" => "Playlists",
        "epg" => "TV guide data",
        "appearance" => "Appearance",
        "playback" => "Playback",
        "remote" => "Remote control",
        "parental" => "Parental control",
        "other" => "Other",
        "about" => "About",
        _ => "",
    }
}

/// A heading: a subsection's name, or (on a page of several sections)
/// the section's.
pub fn heading(section: Section, subsection: Option<&str>) -> &'static str {
    match subsection {
        Some(sub) => self::subsection(section, sub),
        None => self::section(section),
    }
}

pub fn section(section: Section) -> &'static str {
    match section {
        Section::General => "General",
        Section::Playlists => "Playlists",
        Section::Channels => "Channels",
        Section::Epg => "Guide data",
        Section::Appearance => "Appearance",
        Section::Guide => "Guide",
        Section::Logos => "Logos",
        Section::Player => "Player",
        Section::Playback => "Playback",
        Section::Catchup => "Replay",
        Section::History => "History",
        Section::Search => "Search",
        Section::Recordings => "Recordings",
        Section::Parental => "Parental control",
        Section::Remote => "Remote control",
        Section::Vod => "Movies and series",
    }
}

pub fn subsection(section: Section, subsection: &str) -> &'static str {
    match (section, subsection) {
        (Section::General, "startup") => "Starting and leaving",
        (Section::General, "language") => "Language and keyboard",
        (Section::Guide, "browsing") => "Guide layout",
        (Section::Guide, "content") => "In the guide",
        (Section::Playback, "decoding") => "Picture",
        (Section::Catchup, "seeking") => "Replay keys",
        (Section::Catchup, "steps") => "Replay jumps",
        (Section::Vod, "catalogue") => "Movies and series",
        (Section::Vod, "playback") => "Series playback",
        (_, "network") => "Network",
        (_, "privacy") => "Privacy",
        (_, "updates") => "Updates",
        (_, "content") => "Content",
        (_, "sorting") => "Order",
        (_, "categories") => "Categories",
        (_, "names") => "Channel names",
        (_, "storage") => "Storage",
        (_, "matching") => "Matching channels",
        (_, "colours") => "Colours",
        (_, "text") => "Text",
        (_, "motion") => "Motion",

        (_, "emphasis") => "Emphasis",
        (_, "switching") => "Changing channels",
        (_, "panels") => "Panels",
        (_, "info") => "Stream information",
        (_, "clock") => "Clock",
        (_, "buttons") => "Buttons",
        (_, "decoding") => "Decoding",
        (_, "audio") => "Audio",
        (_, "buffering") => "Buffering",
        (_, "frame_rate") => "Frame rate",
        (_, "external") => "Other players",
        (_, "source") => "Replay source",

        (_, "history") => "Watched",
        (_, "recent") => "Recent channels",
        (_, "recording") => "Recording",
        (_, "reminders") => "Reminders",
        (_, "scope") => "Protection",
        (_, "unlock") => "Unlocking",
        (_, "timing") => "Timing",
        (_, "mapping") => "Keys",

        _ => "",
    }
}

/// The name of a setting or command.
pub fn label(key: &str) -> &'static str {
    match key {
        "general.start_on_boot" => "Start when the device turns on",
        "general.start_on_wake" => "Start when the device wakes",
        "general.resume_last_channel" => "Play the last channel on start",
        "general.confirm_exit" => "Press Back twice to leave",
        "general.confirm_exit_window" => "Time for the second Back",
        "general.language" => "Language",
        "general.keyboard" => "On-screen keyboard",
        "general.user_agent" => "User-Agent",
        "general.udp_proxy" => "UDP relay",
        "general.usage_statistics" => "Share anonymous usage counts",
        "general.auto_update" => "Update by itself",
        "general.beta_updates" => "Offer test versions",
        "general.backup" => "Back up",
        "general.restore" => "Restore a backup",
        "general.reset_watch_time" => "Forget viewing time",
        "general.restart" => "Restart the app",
        "general.send_logs" => "Send a diagnostic log",
        "general.check_updates" => "Look for a new version",
        "general.about" => "About this app",
        "playlists.include_live" => "Live channels",
        "playlists.include_vod" => "Movies and series",
        "playlists.max_connections" => "Streams at once",
        "playlists.sort" => "Playlist order",
        "playlists.update_interval" => "Refresh playlists every",
        "playlists.update_on_start" => "Refresh when the app starts",
        "playlists.manage" => "Manage playlists",
        "playlists.add" => "Add a playlist",
        "playlists.reorder" => "Arrange playlists",
        "playlists.update_all" => "Refresh all playlists now",
        "channels.group_sort" => "Group order",
        "channels.channel_sort" => "Channel order",
        "channels.show_new_groups" => "Show groups that appear later",
        "channels.show_all_channels" => "All channels category",
        "channels.show_favorites" => "Favourites category",
        "channels.show_all_playlists" => "All playlists category",
        "channels.group_all_playlists_by_playlist" => "Keep each playlist together",
        "channels.playlist_opens" => "A playlist opens on",
        "channels.show_hidden_group_channels" => "Channels of hidden groups",
        "channels.strip_prefixes" => "Remove from the start of names",
        "channels.strip_suffixes" => "Remove from the end of names",
        "channels.manage_groups" => "Manage groups",
        "epg.update_interval" => "Download the guide every",
        "epg.update_on_start" => "Update when the app starts",
        "epg.update_on_playlist_change" => "Update after a playlist changes",
        "epg.keep_past" => "Keep finished programmes for",
        "epg.store_descriptions" => "Keep programme descriptions",
        "epg.full_scan" => "Thorough channel matching",
        "epg.time_offset" => "Shift guide times by",
        "epg.add_source" => "Add a guide source",
        "epg.manage_sources" => "Guide sources",
        "epg.update_now" => "Update the guide now",
        "epg.clear" => "Delete stored guide data",
        "appearance.accent" => "Accent colour",
        "appearance.background" => "Background colour",
        "appearance.selection_style" => "Focus fill",
        "appearance.ui_transparency" => "Video showing through screens",
        "appearance.panel_transparency" => "See-through player panels",
        "appearance.font_size" => "Text size",
        "appearance.two_line_channel_names" => "Channel names on two lines",
        "appearance.two_line_program_titles" => "Programme titles on two lines",
        "appearance.animated_scrolling" => "Animated scrolling",
        "appearance.animated_transitions" => "Animated transitions",
        "guide.browse_mode" => "Guide layout",
        "guide.list_browse_mode" => "Channel list layout",
        "guide.show_channel_names" => "Channel names",
        "guide.show_channel_numbers" => "Channel numbers",
        "guide.show_now_playing" => "Programme on air in the list",
        "guide.show_programmes" => "Programme grid",
        "guide.show_description" => "Programme description",
        "guide.show_source_labels" => "Playlist and group of channels",
        "guide.show_catchup_badge" => "Replay mark",
        "guide.dim_past" => "Dim finished programmes",
        "guide.highlight_playing" => "Colour the playing channel",
        "guide.highlight_now" => "Emphasise programmes on air",
        "guide.now_highlight_style" => "On-air emphasis",
        "guide.now_line_full_height" => "Time line across the grid",
        "guide.autoplay" => "Play the focused channel by itself",
        "guide.switch_delay" => "Wait before tuning",
        "guide.fullscreen_timeout" => "Go full screen after",
        "guide.stay_on_list" => "Keep the channel list after choosing",
        "guide.stay_on_guide" => "Keep the guide after choosing",
        "guide.stay_on_guide_overlay" => "Keep the guide over video after choosing",
        "guide.back_returns_to_now" => "Back returns to the programmes on air",
        "logos.source" => "Preferred logos",
        "logos.folder" => "Logo folder",
        "logos.inexact_match" => "Near matches of file names",
        "logos.clear_cache" => "Forget downloaded logos",
        "player.info_panel_timeout" => "Hide the info panel after",
        "player.controls_panel_timeout" => "Hide the controls after",
        "player.info_on_switch" => "Show info on a channel change",
        "player.switch_info_at_bottom" => "That info along the bottom",
        "player.description_on_switch_only" => "Description only on a channel change",
        "player.show_media_info" => "Stream properties",
        "player.numeric_resolution" => "Resolution in pixels",
        "player.clock" => "Clock",
        "player.clock_date" => "Date with the clock",
        "player.clock_position" => "Clock corner",
        "player.clock_size" => "Clock size",
        "player.history_button" => "History button",
        "player.guide_button" => "Guide button",
        "playback.video_decoder" => "Video decoding",
        "playback.audio_decoder" => "Audio decoding",
        "playback.tunneling" => "Tunnelled video",
        "playback.amlogic_workaround" => "Amlogic workaround",
        "playback.black_frame_on_switch" => "Black picture while tuning",
        "playback.passthrough" => "Surround audio to the receiver",
        "playback.prefer_surround" => "Prefer surround audio tracks",
        "playback.audio_offset" => "Audio delay",
        "playback.buffer" => "Buffer",
        "playback.afr" => "Match the content's frame rate",
        "playback.afr_refresh_rate" => "May change the refresh rate",
        "playback.afr_resolution" => "May change the resolution",
        "playback.afr_50_60_only" => "Only for 50 and 60 fps",
        "playback.external_player" => "Play in another app",
        "catchup.scheme" => "Replay addresses",
        "catchup.archive_days" => "Replay window",
        "catchup.start_offset" => "Correct replay start by",
        "catchup.rewind_live_key" => "Rewind live channels with",
        "catchup.seek_keys" => "Seek in replays with",
        "catchup.forward_step" => "Fast-forward jumps",
        "catchup.rewind_step" => "Rewind jumps",
        "catchup.timebar_step" => "Time bar jumps",
        "history.keep_days" => "Keep history for",
        "history.add_after" => "Add to history after",
        "history.recent_count" => "Recent channels kept",
        "history.recent_add_after" => "Add to recent channels after",
        "history.clear" => "Clear history",
        "search.prefer_voice" => "Start with voice input",
        "search.favorites_first" => "Favourites first",
        "search.show_history" => "Show recent searches",
        "search.include_past_without_catchup" => "Finished programmes without replay",
        "search.stay_on_search" => "Back to search after watching",
        "search.clear_history" => "Clear recent searches",
        "recordings.folder" => "Recording folder",
        "recordings.format" => "File type",
        "recordings.lead_padding" => "Start early by",
        "recordings.trail_padding" => "Stop late by",
        "recordings.reminder_lead" => "Remind me before the start",
        "recordings.reminder_action" => "When a reminder is due",
        "recordings.reminder_popup_timeout" => "Reminder closes after",
        "recordings.reminder_wake" => "Wake the device for reminders",
        "parental.live_enabled" => "Protect live channels",
        "parental.vod_enabled" => "Protect movies and series",
        "parental.require_pin_for" => "The PIN guards",
        "parental.pin_input" => "Enter the PIN with",
        "parental.unlock_window" => "Stay unlocked",
        "parental.change_pin" => "Set the PIN",
        "remote.long_press" => "Long press after",
        "remote.digit_timeout" => "Channel number pause",
        "remote.key_overrides" => "Changed keys",
        "remote.customize" => "Keys and what they do",
        "remote.restore_defaults" => "Restore the original keys",
        "vod.movies_by_category" => "Movies by category",
        "vod.shows_by_category" => "Series by category",
        "vod.card_style" => "Tile shape",
        "vod.sort" => "Movie and series order",
        "vod.autoplay_next_episode" => "Play the next episode",
        _ => "",
    }
}

/// What a command row does, where the catalogue's summary says more than
/// this version does.
pub fn description(key: &str) -> Option<&'static str> {
    Some(match key {
        "remote.customize" => "What each key of the remote does, screen by screen.",
        "general.about" => "The version of this app.",
        "playlists.manage" => {
            "Add, choose, refresh, rename and delete playlists, and arrange their guide sources."
        }
        "epg.add_source" => "Add an XMLTV guide link to the playlist on screen.",
        "epg.manage_sources" => {
            "The playlist's guide sources: switch on or off, order (the first with a channel \
             wins), time shift, how each last downloaded."
        }
        "epg.update_now" => "Download every guide source of the playlist on screen now.",
        _ => return None,
    })
}

/// The name of a choice (enum variant or palette key).
pub fn choice(family: &str, variant: &str) -> String {
    let name = match (family, variant) {
        ("sort", "manual") => "Arranged by hand",
        ("sort", "name") => "By name",
        ("sort", "playlist_order") => "As in the playlist",
        ("sort", "watch_time") => "Most watched first",
        ("sort", "date_added") => "Newest first",
        ("sort", "rating") => "Best rated first",
        ("sort", "last_modified") => "Recently changed first",
        (_, "small") => "Small",
        (_, "medium") => "Medium",
        (_, "large") => "Large",
        (_, "extra_large") => "Extra large",
        ("level", "none") => "None",
        ("level", "low") => "Low",
        ("level", "high") => "High",
        ("selection_style", "accent_fill") => "Accent colour",
        ("selection_style", "neutral_fill") => "Neutral",
        ("browse_mode", "preview") => "Video beside the list",
        ("browse_mode", "overlay") => "List over the video",
        ("now_highlight", "plain") => "Plain",
        ("now_highlight", "colour") => "Tinted",
        ("now_highlight", "progress_only") => "Progress bar only",
        ("corner", "top_left") => "Top left",
        ("corner", "top_right") => "Top right",
        ("corner", "bottom_left") => "Bottom left",
        ("corner", "bottom_right") => "Bottom right",
        ("logo_source", "playlist") => "From the playlist",
        ("logo_source", "epg") => "From the guide data",
        ("logo_source", "folder") => "From the logo folder",
        ("decoder", "hardware") => "Hardware",
        ("decoder", "software") => "Software",
        ("external_player", "never") => "Never",
        ("external_player", "vod_only") => "Movies and series only",
        ("external_player", "always") => "Always",
        ("catchup_scheme", "auto") => "As the playlist says",
        ("catchup_scheme", "shift") => "Time shift",
        ("catchup_scheme", "append") => "Appended",
        ("catchup_scheme", "flussonic") => "Flussonic",
        ("catchup_scheme", "xtream") => "Xtream Codes",
        ("catchup_scheme", "disabled") => "No replay",
        ("rewind_live_key", "off") => "Nothing",
        ("rewind_live_key", "left") => "Left",
        ("rewind_live_key", "down") => "Down",
        ("rewind_live_key", "rewind") => "The rewind key",
        ("seek_keys", "left_right") => "Left and Right",
        ("seek_keys", "down_up") => "Down and Up",
        ("seek_keys", "transport") => "Rewind and fast-forward keys",
        ("recording_format", "ts") => "MPEG-TS",
        ("recording_format", "mkv") => "Matroska",
        ("reminder_action", "ask") => "Ask",
        ("reminder_action", "switch_channel") => "Switch to the channel",
        ("reminder_action", "notify_only") => "Only tell me",
        ("pin_scope", "blocked_content") => "Blocked content",
        ("pin_scope", "blocked_content_and_settings") => "Blocked content and settings",
        ("unlock_window", "every_time") => "Ask every time",
        ("unlock_window", "min_10") => "10 minutes",
        ("unlock_window", "min_30") => "30 minutes",
        ("unlock_window", "hour_1") => "1 hour",
        ("unlock_window", "hour_2") => "2 hours",
        ("unlock_window", "until_restart") => "Until the app restarts",
        ("unlock_window", "until_unblocked_channel") => "Until an open channel plays",
        ("pin_input", "keypad") => "On-screen keypad",
        ("pin_input", "number_keys") => "Number keys",
        ("keyboard", "system") => "The device's",
        ("keyboard", "built_in") => "The app's own",
        ("playlist_opens", "all_channels") => "All channels",
        ("playlist_opens", "favorites") => "Favourites",
        ("card_style", "poster") => "Poster",
        ("card_style", "landscape") => "Wide",
        ("card_style", "compact") => "Compact",
        // Palettes: the theme's names (`blue_grey` → "Blue grey").
        ("accent" | "background", v) => return title(v),
        _ => "",
    };
    name.to_owned()
}

fn title(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// How a zero reads where it means something else than an amount.
fn zero(key: &str) -> Option<&'static str> {
    Some(match key {
        "epg.update_interval" | "playlists.update_interval" => "Only on request",
        "guide.fullscreen_timeout" => "Never",
        "catchup.archive_days" => "As the provider says",
        "history.add_after" | "history.recent_add_after" => "At once",
        "recordings.reminder_lead" => "At the start",
        "epg.keep_past" => "None",
        _ => return None,
    })
}

/// How an empty text reads.
fn empty(key: &str) -> &'static str {
    match key {
        "general.user_agent" => "The app's own",
        "recordings.folder" => "App storage",
        "channels.strip_prefixes" | "channels.strip_suffixes" | "logos.folder" => "None",
        _ => "Not set",
    }
}

fn plural(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A signed offset as `h:mm` or `h:mm:ss`.
fn offset(ms: i64, seconds: bool) -> String {
    let sign = if ms < 0 {
        "−"
    } else if ms > 0 {
        "+"
    } else {
        ""
    };
    let s = ms.abs() / 1000;
    if seconds {
        format!("{sign}{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{sign}{}:{:02}", s / 3600, s / 60 % 60)
    }
}

/// A number of a setting's type, in its unit.
pub fn amount(ty: &ValueType, n: i64) -> String {
    match *ty {
        ValueType::Int { unit, .. } => match unit {
            IntUnit::Percent => format!("{n} %"),
            IntUnit::Count | IntUnit::None => n.to_string(),
        },
        ValueType::Duration { unit, min_ms, .. } => match unit {
            TimeUnit::Millis if min_ms < 0 && n > 0 => format!("+{n} ms"),
            TimeUnit::Millis if n < 0 => format!("−{} ms", -n),
            TimeUnit::Millis => format!("{n} ms"),
            TimeUnit::Seconds => {
                let secs = format!("{:.3}", n as f64 / 1000.0);
                format!("{} s", secs.trim_end_matches('0').trim_end_matches('.'))
            }
            TimeUnit::Minutes => format!("{} min", n / 60_000),
            TimeUnit::Hours => format!("{} h", n / 3_600_000),
            TimeUnit::Days => plural(n / 86_400_000, "day", "days"),
            TimeUnit::HoursMinutes => offset(n, false),
            TimeUnit::HoursMinutesSeconds => offset(n, true),
        },
        _ => n.to_string(),
    }
}

/// How `value` of `setting` reads.
pub fn value(setting: &Setting, value: &Value) -> String {
    match value {
        Value::Bool(true) => "On".to_owned(),
        Value::Bool(false) => "Off".to_owned(),
        Value::Int(0) | Value::Duration(0) if zero(setting.key).is_some() => {
            zero(setting.key).unwrap_or_default().to_owned()
        }
        Value::Int(n) | Value::Duration(n) => amount(&setting.ty, *n),
        Value::Enum(v) | Value::Colour(v) => setting
            .ty
            .choices()
            .map(|c| choice(c.family, v))
            .unwrap_or_else(|| v.clone()),
        Value::Text(t) if t.is_empty() => empty(setting.key).to_owned(),
        Value::Text(t) if t == "system" && setting.key == "general.language" => {
            "The device's".to_owned()
        }
        Value::Text(t) => t.clone(),
        Value::KeyBindings(list) if list.is_empty() => "None".to_owned(),
        Value::KeyBindings(list) => plural(list.len() as i64, "change", "changes"),
    }
}

#[cfg(test)]
mod tests {
    use iptv_settings::catalogue;

    use super::*;

    #[test]
    fn everything_has_a_name() {
        for sec in Section::ALL {
            assert!(!heading(sec, None).is_empty(), "{sec:?}");
            for sub in sec.subsections() {
                assert!(!heading(sec, Some(sub)).is_empty(), "{sec:?}.{sub}");
            }
        }
        for s in catalogue::settings() {
            assert!(!label(s.key).is_empty(), "{}", s.key);
            if let Some(choices) = s.ty.choices() {
                for v in choices.variants {
                    assert!(!choice(choices.family, v).is_empty(), "{}: {v}", s.key);
                }
            }
        }
        for c in catalogue::commands() {
            assert!(!label(c.key).is_empty(), "{}", c.key);
        }
    }

    #[test]
    fn values_read_in_their_units() {
        let read = |key: &str, v: Value| value(catalogue::setting(key).unwrap(), &v);
        assert_eq!(read("general.resume_last_channel", Value::Bool(true)), "On");
        assert_eq!(
            read("epg.update_interval", Value::Duration(0)),
            "Only on request"
        );
        assert_eq!(read("epg.update_interval", Value::secs(24 * 3600)), "24 h");
        assert_eq!(read("epg.keep_past", Value::secs(86_400)), "1 day");
        assert_eq!(read("epg.keep_past", Value::secs(7 * 86_400)), "7 days");
        assert_eq!(read("catchup.forward_step", Value::secs(30)), "30 s");
        assert_eq!(read("catchup.timebar_step", Value::secs(300)), "5 min");
        assert_eq!(read("guide.switch_delay", Value::ms(500)), "0.5 s");
        assert_eq!(read("guide.switch_delay", Value::ms(1250)), "1.25 s");
        assert_eq!(read("remote.long_press", Value::ms(400)), "400 ms");
        assert_eq!(read("playback.audio_offset", Value::ms(-120)), "−120 ms");
        assert_eq!(read("playback.audio_offset", Value::ms(40)), "+40 ms");
        assert_eq!(read("epg.time_offset", Value::secs(-5400)), "−1:30");
        assert_eq!(read("catchup.start_offset", Value::secs(75)), "+0:01:15");
        assert_eq!(read("appearance.ui_transparency", Value::Int(20)), "20 %");
        assert_eq!(
            read("appearance.accent", Value::colour("blue_grey")),
            "Blue grey"
        );
        assert_eq!(
            read("catchup.seek_keys", Value::choice("down_up")),
            "Down and Up"
        );
        assert_eq!(read("general.user_agent", Value::text("")), "The app's own");
        assert_eq!(
            read("remote.key_overrides", Value::KeyBindings(Vec::new())),
            "None"
        );
    }
}
