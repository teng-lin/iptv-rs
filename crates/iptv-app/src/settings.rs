//! The app's settings: one `iptv_settings::Settings`, kept in
//! `settings.json` in [`crate::data::data_dir`] and loaded at startup.
//!
//! - [`get`]: the effective settings, from any thread (the import threads
//!   read the guide's refresh and storage settings).
//! - [`read`]: the same inside a reactive closure; it subscribes to
//!   `Ui::settings`, which [`set`] bumps after every change.
//! - [`set`]: validates, stores and saves. Only the UI thread writes.
//!
//! Every value is global for now. The store resolves playlist and group
//! overrides, but nothing sets them yet, so playlist-scoped settings read
//! their global value.
//!
//! [`offer`] says what the app does with each setting and command: the
//! ones it acts on ([`WIRED`], [`COMMANDS`]), what it shows read-only,
//! what comes later, and what it does not offer at all (device-level
//! features of TV boxes). Some settings apply here ([`bind`]: colours,
//! panel timeouts, [`net_config`]); the others are read where they act
//! (guide data, catch-up keys and steps, search, startup channel, Back,
//! leaving, channel-change info).

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use iptv_core::panels::Timeouts;
use iptv_net::NetConfig;
use iptv_settings::keys::{self, BoolKey, DurationKey, EnumKey};
use iptv_settings::screen::Offer;
use iptv_settings::{Settings, SettingsError, Target, Value};
use nsh_core::prelude::*;

use crate::Ui;
use crate::theme::{ACCENTS, BACKGROUNDS, Theme};

const FILE: &str = "settings.json";
/// Where an unreadable settings file is moved, so a save cannot erase it.
const UNREADABLE: &str = "settings.unreadable.json";

/// The settings the app acts on.
pub const WIRED: &[&str] = &[
    "guide.autoplay",
    "guide.switch_delay",
    "guide.fullscreen_timeout",
    "guide.stay_on_guide",
    "guide.stay_on_guide_overlay",
    "guide.stay_on_list",
    "guide.browse_mode",
    "guide.list_browse_mode",
    "appearance.font_size",
    "guide.show_channel_numbers",
    "guide.show_logos",
    "guide.visible_minutes",
    "general.resume_last_channel",
    "general.confirm_exit",
    "general.user_agent",
    "epg.update_interval",
    "epg.update_on_start",
    "epg.update_on_playlist_change",
    "epg.keep_past",
    "epg.store_descriptions",
    "guide.back_returns_to_now",
    "appearance.accent",
    "appearance.background",
    "appearance.ui_transparency",
    "player.info_panel_timeout",
    "player.controls_panel_timeout",
    "player.info_on_switch",
    "catchup.seek_keys",
    "remote.player_up_down",
    "catchup.forward_step",
    "catchup.rewind_step",
    "catchup.timebar_step",
    "search.favorites_first",
    "search.include_past_without_catchup",
    "search.stay_on_search",
    "search.show_history",
];

/// The commands the app runs (`settings_view::run`).
pub const COMMANDS: &[&str] = &[
    "general.backup",
    "general.restore",
    "epg.add_source",
    "epg.manage_sources",
    "epg.update_now",
    "epg.clear",
    "remote.customize",
    "playlists.manage",
];

/// Shown with their value only: the key list has no editor yet.
const READ_ONLY: &[&str] = &["remote.key_overrides"];

/// Not offered: device-level features of TV boxes (start with the device,
/// frame-rate switching, decoders, audio passthrough, multicast relay,
/// voice input, self-update, usage statistics). Also `general.about`,
/// which the About page shows on its own.
const NOT_OFFERED: &[&str] = &[
    "general.start_on_boot",
    "general.start_on_wake",
    "general.udp_proxy",
    "general.usage_statistics",
    "general.auto_update",
    "general.beta_updates",
    "general.check_updates",
    "general.about",
    "playback.video_decoder",
    "playback.audio_decoder",
    "playback.tunneling",
    "playback.amlogic_workaround",
    "playback.passthrough",
    "playback.afr",
    "playback.afr_refresh_rate",
    "playback.afr_resolution",
    "playback.afr_50_60_only",
    "search.prefer_voice",
];

/// Sections planned for later versions.
const LATER: &[&str] = &["recordings.", "vod.", "parental."];

thread_local! { static REVISION: std::cell::Cell<Option<Signal<u64>>> = const { std::cell::Cell::new(None) }; }

pub fn observe() {
    REVISION.with(|v| {
        if let Some(s) = v.get() {
            s.get();
        }
    });
}

pub fn font_factor() -> f32 {
    observe();
    match choice(keys::FONT_SIZE) {
        "small" => 0.9,
        "large" => 1.15,
        "extra_large" => 1.3,
        _ => 1.0,
    }
}

static CURRENT: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

/// What the app does with the setting or command `key`. The long-press
/// time matters only where keys report their release (Android), and
/// picture in picture on leaving only on Android.
pub fn offer(key: &str) -> Offer {
    // Leaving the app into a picture-in-picture window: Android only.
    let android = cfg!(target_os = "android");
    if matches!(key, "general.backup" | "general.restore") && cfg!(target_os = "ios") {
        return Offer::NotYet;
    }
    if WIRED.contains(&key)
        || COMMANDS.contains(&key)
        || (key == keys::LONG_PRESS.0 && crate::input::LONG_PRESS)
        || (key == keys::PIP_ON_LEAVE.0 && android)
    {
        Offer::Active
    } else if key == keys::PIP_ON_LEAVE.0 {
        Offer::Hidden
    } else if READ_ONLY.contains(&key) {
        Offer::ReadOnly
    } else if NOT_OFFERED.contains(&key) {
        Offer::Hidden
    } else if LATER.iter().any(|p| key.starts_with(p)) {
        Offer::Later
    } else {
        Offer::NotYet
    }
}

/// The effective settings (defaults until [`load`]).
pub fn get() -> Arc<Settings> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// [`get`], subscribing the calling reactive closure to changes.
pub fn read(ui: Ui) -> Arc<Settings> {
    ui.settings.get();
    get()
}

fn replace(settings: Settings) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(settings));
}

fn path() -> PathBuf {
    crate::data::data_dir().join(FILE)
}

/// Loads the saved settings (startup).
pub fn load() {
    replace(read_file(&path()));
}

/// The settings saved at `path`; defaults when there are none. A file
/// that cannot be read as settings is moved aside (never overwritten).
fn read_file(path: &Path) -> Settings {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::new(),
        Err(e) => {
            log::warn!(
                "iptv settings: cannot read the settings ({}); using defaults",
                e.kind()
            );
            return Settings::new();
        }
    };
    match Settings::from_json(&text) {
        Ok((settings, report)) => {
            if !report.is_clean() {
                log::info!(
                    "iptv settings: loaded with {} unknown, {} adjusted and {} rejected entries",
                    report.unknown.len(),
                    report.clamped.len(),
                    report.rejected.len()
                );
            }
            settings
        }
        Err(e) => {
            log::warn!("iptv settings: {e}; using defaults and keeping the file aside");
            if let Some(dir) = path.parent() {
                let _ = std::fs::rename(path, dir.join(UNREADABLE));
            }
            Settings::new()
        }
    }
}

/// Writes `settings` to `path` through a temporary file.
fn write_file(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, settings.to_json_string())?;
    std::fs::rename(&tmp, path)
}

/// Stores `value` for `key` (globally), saves, and tells the views.
pub fn set(ui: Ui, key: &str, value: Value) -> Result<(), SettingsError> {
    let mut next = (*get()).clone();
    let changes = next.set(key, value)?;
    if let Err(e) = write_file(&path(), &next) {
        log::warn!("iptv settings: could not save ({})", e.kind());
    }
    replace(next);
    if !changes.is_empty() {
        log::info!("iptv settings: {key} changed");
        ui.settings.update(|v| *v += 1);
    }
    Ok(())
}

pub fn flag(key: BoolKey) -> bool {
    get().bool(key, &Target::Global)
}

pub fn duration_ms(key: DurationKey) -> i64 {
    get().duration_ms(key, &Target::Global)
}

pub fn browse_mode() -> crate::ViewMode {
    if choice(keys::BROWSE_MODE) == "overlay" {
        crate::ViewMode::Overlay
    } else {
        crate::ViewMode::Guide
    }
}

pub fn choice(key: EnumKey) -> &'static str {
    get().choice(key, &Target::Global)
}

/// `general.user_agent`, when set.
pub fn user_agent() -> Option<String> {
    let ua = get().text(keys::USER_AGENT, &Target::Global);
    (!ua.trim().is_empty()).then(|| ua.trim().to_owned())
}

/// The network settings of provider requests: `general.user_agent` for
/// requests that do not name their own.
pub fn net_config() -> NetConfig {
    let mut config = NetConfig::default();
    #[cfg(debug_assertions)]
    {
        config.allow_private_fixture =
            cfg!(feature = "test-support") || crate::data::DEV_PLAYLIST.is_some();
    }
    if let Some(ua) = user_agent() {
        config.user_agent = ua;
    }
    config
}

/// Adds `general.user_agent` to a stream's headers unless the stream
/// names its own (a playlist's per-channel agent wins).
pub fn add_user_agent(headers: &mut Vec<(String, String)>, ua: Option<String>) {
    let named = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
    if let (Some(ua), false) = (ua, named) {
        headers.push(("User-Agent".to_owned(), ua));
    }
}

/// `remote.long_press`, in ms.
pub fn long_press_ms() -> u64 {
    u64::try_from(duration_ms(keys::LONG_PRESS)).unwrap_or(0)
}

/// A theme bundle's palette key (`"Blue grey"` → `blue_grey`).
pub fn palette_key(name: &str) -> String {
    name.to_lowercase().replace(' ', "_")
}

/// The colours chosen in `appearance.*`.
pub fn theme_of(s: &Settings) -> Theme {
    let at = &Target::Global;
    let (accent, background) = (s.colour(keys::ACCENT, at), s.colour(keys::BACKGROUND, at));
    let fallback = Theme::default();
    Theme {
        accent: ACCENTS
            .iter()
            .find(|a| palette_key(a.name) == accent)
            .copied()
            .unwrap_or(fallback.accent),
        background: BACKGROUNDS
            .iter()
            .find(|b| palette_key(b.name) == background)
            .copied()
            .unwrap_or(fallback.background),
    }
}

/// The player panels' auto-hide delays.
pub fn timeouts_of(s: &Settings) -> Timeouts {
    let ms = |key| u64::try_from(s.duration_ms(key, &Target::Global)).unwrap_or(0);
    Timeouts {
        info_ms: ms(keys::INFO_PANEL_TIMEOUT),
        controls_ms: ms(keys::CONTROLS_PANEL_TIMEOUT),
        // Not a setting: the desktop's controls hide soon after the mouse rests.
        ..Timeouts::default()
    }
}

/// Alpha of browsing surfaces drawn over full-screen video, from
/// `appearance.ui_transparency` (percent of the video showing through).
pub fn overlay_alpha(s: &Settings) -> u8 {
    let percent = s.int(keys::UI_TRANSPARENCY, &Target::Global).clamp(0, 100);
    u8::try_from(255 - percent * 255 / 100).unwrap_or(u8::MAX)
}

/// Applies the settings that live in `Ui` state: colours and panel
/// timeouts, now and after every change.
pub fn bind(ui: Ui) {
    REVISION.with(|r| r.set(Some(ui.settings)));
    let last_mode = std::cell::Cell::new(None);
    effect(move || {
        let s = read(ui);
        let (theme, timeouts) = (theme_of(&s), timeouts_of(&s));
        let mode = s.choice(keys::BROWSE_MODE, &Target::Global);
        let changed = last_mode.replace(Some(mode)) != Some(mode);
        let span = s.int(keys::IntKey("guide.visible_minutes"), &Target::Global) as i32;
        untracked(|| {
            ui.theme.set(theme);
            if ui.focus.with_untracked(|f| f.visible) != span {
                let view = crate::lists::view(ui);
                ui.focus.update(|f| f.set_visible(span, &view));
            }
            if changed && ui.mode.get_untracked() != crate::ViewMode::Fullscreen {
                ui.mode.set(if mode == "overlay" {
                    crate::ViewMode::Overlay
                } else {
                    crate::ViewMode::Guide
                });
            }
            if ui.panels.with(|p| p.timeouts) != timeouts {
                ui.panels.update(|p| p.timeouts = timeouts);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use iptv_settings::catalogue;

    use super::*;

    #[test]
    fn listed_keys_exist() {
        for key in WIRED.iter().chain(READ_ONLY) {
            assert!(catalogue::setting(key).is_some(), "{key}");
        }
        for key in COMMANDS {
            assert!(catalogue::command(key).is_some(), "{key}");
        }
        for key in NOT_OFFERED {
            assert!(
                catalogue::setting(key).is_some() || catalogue::command(key).is_some(),
                "{key}"
            );
        }
    }

    #[test]
    fn the_user_agent_goes_where_none_is_named() {
        let mut headers = Vec::new();
        add_user_agent(&mut headers, None);
        assert!(headers.is_empty());
        add_user_agent(&mut headers, Some("SyntheticPlayer/1.0".into()));
        assert_eq!(
            headers,
            [("User-Agent".to_owned(), "SyntheticPlayer/1.0".to_owned())]
        );
        let mut own = vec![("user-agent".to_owned(), "ChannelAgent".to_owned())];
        add_user_agent(&mut own, Some("SyntheticPlayer/1.0".into()));
        assert_eq!(own.len(), 1, "the channel's own agent wins");
    }

    #[test]
    fn what_the_app_offers() {
        assert_eq!(offer("epg.keep_past"), Offer::Active);
        assert_eq!(offer("epg.update_now"), Offer::Active);
        assert_eq!(offer("epg.manage_sources"), Offer::Active);
        assert_eq!(offer("epg.add_source"), Offer::Active);
        assert_eq!(offer("playback.afr"), Offer::Hidden);
        assert_eq!(offer("vod.sort"), Offer::Later);
        assert_eq!(offer("parental.live_enabled"), Offer::Later);
        assert_eq!(offer("history.keep_days"), Offer::NotYet);
        assert_eq!(offer("remote.key_overrides"), Offer::ReadOnly);
        let pip = if cfg!(target_os = "android") {
            Offer::Active
        } else {
            Offer::Hidden
        };
        assert_eq!(offer("player.pip_on_leave"), pip);
    }

    #[test]
    fn colours_follow_the_appearance_settings() {
        let mut s = Settings::new();
        assert_eq!(theme_of(&s), Theme::default());
        s.set("appearance.accent", Value::colour("red")).unwrap();
        s.set("appearance.background", Value::colour("blue_grey"))
            .unwrap();
        let theme = theme_of(&s);
        assert_eq!(theme.accent.name, "Red");
        assert_eq!(theme.background.name, "Blue grey");
        // Every palette key names a bundle.
        for name in catalogue::choices::ACCENT.variants {
            s.set("appearance.accent", Value::colour(name)).unwrap();
            assert_eq!(palette_key(theme_of(&s).accent.name), *name);
        }
    }

    #[test]
    fn panel_timeouts_follow_the_player_settings() {
        let mut s = Settings::new();
        assert_eq!(timeouts_of(&s), Timeouts::default());
        s.set("player.info_panel_timeout", Value::secs(9)).unwrap();
        s.set("player.controls_panel_timeout", Value::secs(20))
            .unwrap();
        assert_eq!(
            timeouts_of(&s),
            Timeouts {
                info_ms: 9_000,
                controls_ms: 20_000,
                ..Timeouts::default()
            }
        );
    }

    #[test]
    fn transparency_sets_the_overlay_alpha() {
        let mut s = Settings::new();
        assert_eq!(overlay_alpha(&s), 204, "20 % by default");
        s.set("appearance.ui_transparency", Value::Int(0)).unwrap();
        assert_eq!(overlay_alpha(&s), 255);
        s.set("appearance.ui_transparency", Value::Int(80)).unwrap();
        assert_eq!(overlay_alpha(&s), 51);
    }

    #[test]
    fn settings_survive_a_save_and_an_unreadable_file_is_kept() {
        let dir = std::env::temp_dir().join(format!("iptv-settings-test-{}", std::process::id()));
        let path = dir.join(FILE);
        assert_eq!(read_file(&path), Settings::new(), "nothing saved yet");
        let mut s = Settings::new();
        s.set("catchup.forward_step", Value::secs(60)).unwrap();
        write_file(&path, &s).unwrap();
        assert_eq!(read_file(&path), s);
        std::fs::write(&path, "not settings").unwrap();
        assert_eq!(read_file(&path), Settings::new());
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join(UNREADABLE)).unwrap(),
            "not settings"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_entries_are_skipped_and_an_unreadable_path_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("iptv-settings-extra-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let mut s = Settings::new();
        s.set("catchup.forward_step", Value::secs(60)).unwrap();
        let mut json = s.to_json();
        json["global"]["synthetic.unknown"] = serde_json::Value::from(1);
        std::fs::write(&path, json.to_string()).unwrap();
        let read = read_file(&path);
        assert_eq!(read.get("catchup.forward_step"), Ok(Value::secs(60)));
        assert!(path.exists(), "a readable file stays");
        // A directory where the file should be: defaults, nothing moved.
        assert_eq!(read_file(&dir), Settings::new());
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
