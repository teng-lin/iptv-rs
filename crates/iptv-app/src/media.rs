//! What the open streams offer and how they sound: each surface's tracks
//! and format as the `tv.player` host reports them (`tracks` and `format`
//! events, kept in its [`crate::player::Stream`]), the viewer's track
//! choices, and the app's volume, mute and audio delay (props). A new
//! stream forgets the old one's tracks and choices; the sound settings
//! stay. `video.rs` binds these to the hosts; the panels and choosers read
//! and change the full-screen player's (surface 0).

use iptv_core::player_info::StreamFormat;
use iptv_core::sound::{VOLUME_STEP, Volume};
use iptv_core::tracks::{AudioTrack, Choice, Kind, TextTrack, Tracks, VideoTrack};
use nsh_core::prelude::*;
use serde_json::Value;

use crate::Ui;

/// Sound state beside [`Ui`]'s (signals are `Copy`), the same for every
/// surface; multiview mutes all but its active one.
#[derive(Clone, Copy)]
pub struct Media {
    pub volume: Signal<Volume>,
    /// Audio delay in milliseconds (positive: sound later).
    pub delay_ms: Signal<i32>,
}

impl Media {
    pub fn new() -> Self {
        Media {
            volume: signal(Volume::default()),
            delay_ms: signal(0),
        }
    }
}

/// Whether this host can delay the sound (`audioDelay`): mpv on the
/// desktop, Media3 through the host's delayed audio sink.
pub const AUDIO_DELAY: bool = true;

/// `surface` opened another stream: its tracks are not known yet and the
/// choices made for the last one do not apply.
pub fn reset(ui: Ui, surface: usize) {
    let m = ui.player.stream(surface);
    if m.tracks.with_untracked(|t| *t != Tracks::default()) {
        m.tracks.set(Tracks::default());
    }
    if m.choice.with_untracked(|c| *c != Choice::default()) {
        m.choice.set(Choice::default());
    }
    if m.format.with_untracked(|f| *f != StreamFormat::default()) {
        m.format.set(StreamFormat::default());
    }
}

/// The viewer picked `value` (a track id, `auto` or `off`) for `kind` of
/// the full-screen player.
pub fn choose(ui: Ui, kind: Kind, value: String) {
    log::info!("iptv tracks: {kind:?} → {value}");
    ui.player.main().choice.update(|c| match kind {
        Kind::Video => c.video = value,
        Kind::Audio => c.audio = value,
        Kind::Text => c.text = value,
    });
}

pub fn step_volume(ui: Ui, up: bool) {
    let delta = if up { VOLUME_STEP } else { -VOLUME_STEP };
    ui.media.volume.update(|v| v.step(delta));
    log_volume(ui);
}

pub fn toggle_mute(ui: Ui) {
    ui.media.volume.update(Volume::toggle_mute);
    log_volume(ui);
}

fn log_volume(ui: Ui) {
    let v = ui.media.volume.get_untracked();
    log::info!(
        "iptv volume: {}%{}",
        v.level,
        if v.muted { ", muted" } else { "" }
    );
}

/// The host's `tracks` event from `surface`.
pub fn on_tracks(ui: Ui, surface: usize, data: &Value) {
    let tracks = parse_tracks(data);
    log::info!(
        "iptv tracks {surface}: {} video, {} audio, {} subtitles",
        tracks.video.len(),
        tracks.audio.len(),
        tracks.text.len()
    );
    let m = ui.player.stream(surface);
    if m.tracks.with_untracked(|t| *t != tracks) {
        m.tracks.set(tracks);
    }
}

/// The host's `format` event from `surface`: whichever of `fps` and
/// `channels` it knows.
pub fn on_format(ui: Ui, surface: usize, data: &Value) {
    let m = ui.player.stream(surface);
    let mut f = m.format.get_untracked();
    if let Some(fps) = data["fps"].as_f64() {
        f.fps = fps as f32;
    }
    if let Some(ch) = data["channels"].as_u64() {
        f.channels = u32::try_from(ch).unwrap_or(0);
    }
    if m.format.with_untracked(|old| *old != f) {
        m.format.set(f);
    }
}

fn text_of(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn num(v: &Value, key: &str) -> u32 {
    v[key]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.0)
        .map_or(0, |n| n.min(f64::from(u32::MAX)) as u32)
}

fn list<T>(v: &Value, key: &str, f: impl Fn(&Value) -> T) -> Vec<T> {
    v[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| !text_of(t, "id").is_empty())
                .map(f)
                .collect()
        })
        .unwrap_or_default()
}

/// The `tracks` event's data: `{video: [{id, label, width, height, fps,
/// bitrate}], audio: [{id, label, language, channels, codec}], text: [{id,
/// label, language}], selected: {video: "auto"|id, audio: id|null, text:
/// "off"|id}}`. Missing fields read as unknown.
pub fn parse_tracks(data: &Value) -> Tracks {
    let selected = &data["selected"];
    let pick =
        |key: &str, none: &str| Some(text_of(selected, key)).filter(|s| !s.is_empty() && s != none);
    Tracks {
        video: list(data, "video", |t| VideoTrack {
            id: text_of(t, "id"),
            label: text_of(t, "label"),
            width: num(t, "width"),
            height: num(t, "height"),
            fps: t["fps"]
                .as_f64()
                .filter(|f| f.is_finite() && *f > 0.0)
                .unwrap_or(0.0) as f32,
            bitrate: num(t, "bitrate"),
        }),
        audio: list(data, "audio", |t| AudioTrack {
            id: text_of(t, "id"),
            label: text_of(t, "label"),
            language: text_of(t, "language"),
            channels: num(t, "channels"),
            codec: text_of(t, "codec"),
        }),
        text: list(data, "text", |t| TextTrack {
            id: text_of(t, "id"),
            label: text_of(t, "label"),
            language: text_of(t, "language"),
        }),
        selected_video: pick("video", iptv_core::tracks::AUTO),
        selected_audio: pick("audio", ""),
        selected_text: pick("text", iptv_core::tracks::OFF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_hosts_tracks_event_reads_into_the_model() {
        let t = parse_tracks(&json!({
            "video": [
                {"id": "0:0", "label": "", "width": 640, "height": 360, "fps": 25.0, "bitrate": 800000},
                {"id": "0:1", "width": 1280, "height": 720, "fps": -1, "bitrate": -1}
            ],
            "audio": [
                {"id": 2, "label": "Commentary", "language": "eng", "channels": 2, "codec": "aac"},
                {"label": "no id"}
            ],
            "text": [{"id": "3", "language": "fre"}],
            "selected": {"video": "auto", "audio": "2", "text": "off"}
        }));
        assert_eq!(t.video.len(), 2);
        assert_eq!(
            (t.video[1].height, t.video[1].fps, t.video[1].bitrate),
            (720, 0.0, 0)
        );
        assert_eq!(t.audio.len(), 1, "a track without an id is dropped");
        assert_eq!(t.audio[0].id, "2", "numeric ids read as text");
        assert_eq!(t.text[0].language, "fre");
        assert_eq!(t.selected_video, None, "automatic");
        assert_eq!(t.selected_audio.as_deref(), Some("2"));
        assert_eq!(t.selected_text, None, "off");

        let chosen = parse_tracks(&json!({"selected": {"video": "0:1", "text": "3"}}));
        assert_eq!(chosen.selected_video.as_deref(), Some("0:1"));
        assert_eq!(chosen.selected_text.as_deref(), Some("3"));
        assert_eq!(chosen.selected_audio, None);
        assert_eq!(parse_tracks(&Value::Null), Tracks::default());
    }
}
