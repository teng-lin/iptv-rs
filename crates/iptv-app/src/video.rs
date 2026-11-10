//! The persistent video layer: one `tv.player` native component (registered by
//! the Android host, `hosts/android`, and on Linux by `mpv`), laid out per view mode — the preview
//! rectangle in the info panel, or the whole screen under an overlay or under
//! nothing. Changing modes changes layout only; the player is never recreated.
//! What it plays comes from `player.rs` (resolution, retries); its tracks,
//! format and sound from `media.rs`.
//!
//! The `tv.player` contract (both hosts):
//! - props `source` `{url, headers, token}` or null, `paused`, `userAgent`,
//!   `pictureSize`; `videoTrack` (`auto` or a video track id),
//!   `audioTrack` (`auto` or an audio track id), `textTrack` (`off` or a
//!   text track id) for the open stream (a new source starts again from
//!   `auto`/`auto`/`off`); `volume` (0–100; the desktop's, Android keeps
//!   the system volume), `muted`, `audioDelay` (ms, positive: sound later);
//! - events `state` {state}, `size` {width, height}, `error` {code, http?},
//!   `position` {ms}; `tracks` {video: [{id, label, width, height, fps,
//!   bitrate}], audio: [{id, label, language, channels, codec}], text:
//!   [{id, label, language}], selected: {video, audio, text}} whenever they
//!   change; `format` {fps?, channels?} for what plays.

use nsh_core::prelude::*;
use serde_json::{Value, json};

use crate::panels::{INFO_H, INFO_PAD_X, INFO_PAD_Y, PREVIEW_H, PREVIEW_W};
use crate::{Ui, ViewMode};

/// The local mock server (`tools/mock-server.py`) as the player reaches it:
/// through the Android emulator's host alias, or on this machine.
#[cfg(target_os = "android")]
const MOCK_SERVER: &str = "http://10.0.2.2:8089";
#[cfg(not(target_os = "android"))]
const MOCK_SERVER: &str = "http://127.0.0.1:8089";

/// Synthetic local HLS from `tools/make-hls-fixtures.sh --serve`. Three
/// test patterns cycle by channel.
pub fn stream_url(channel: usize) -> String {
    format!("{MOCK_SERVER}/ch{}/index.m3u8", channel % 3)
}

/// The synthetic archive URL (`tools/mock-server.py` serves `/archive/*`)
/// for the synthetic guide; imported channels resolve through
/// `iptv_guide::playback`.
pub fn catchup_url(channel: usize, start: i32, offset: i32) -> String {
    let s = crate::schedule();
    let p = s.program_at(channel, start);
    let utc = crate::guide::unix_of(&*s, p.start) + i64::from(offset);
    let duration = (p.end - p.start) * 60 - offset;
    format!(
        "{MOCK_SERVER}/archive/ch{}/index.m3u8?utc={utc}&duration={duration}",
        channel % 3
    )
}

/// The player's rectangle (x, y, width, height in px) for the view mode:
/// the preview in the info panel, or the whole window.
pub fn rect(ui: Ui) -> (f32, f32, f32, f32) {
    let w = window();
    if ui.mode.get() != ViewMode::Guide {
        crate::channels_view::preview_rect(ui, w.width.0, w.height.0)
            .unwrap_or((0.0, 0.0, w.width.0, w.height.0))
    } else {
        (
            INFO_PAD_X,
            w.height.0 - INFO_H + INFO_PAD_Y,
            PREVIEW_W,
            PREVIEW_H,
        )
    }
}

/// The stream as the host's `source` prop: `{url, headers, token}`, or
/// null when nothing plays.
fn source_prop(ui: Ui) -> Value {
    if ui.playing.get().is_none() {
        return Value::Null;
    }
    let token = ui.player.token.get();
    ui.player.source.with(|s| {
        s.as_ref().map_or(Value::Null, |s| {
            let headers: serde_json::Map<String, Value> = s
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            json!({ "url": s.url, "headers": headers, "token": token })
        })
    })
}

pub fn layer(ui: Ui) -> impl IntoElement {
    native("tv.player")
        .absolute()
        .left(move || px(rect(ui).0))
        .top(move || px(rect(ui).1))
        .w(move || px(rect(ui).2))
        .h(move || px(rect(ui).3))
        .prop("source", move || source_prop(ui))
        .prop("paused", move || json!(ui.paused.get()))
        .prop("pictureSize", move || {
            let ch = ui.playing.get().unwrap_or(0);
            json!(ui.picture.with(|p| p.for_channel(ch)).key())
        })
        .prop("videoTrack", move || {
            json!(ui.media.choice.with(|c| c.video.clone()))
        })
        .prop("audioTrack", move || {
            json!(ui.media.choice.with(|c| c.audio.clone()))
        })
        .prop("textTrack", move || {
            json!(ui.media.choice.with(|c| c.text.clone()))
        })
        .prop("volume", move || json!(ui.media.volume.with(|v| v.level)))
        .prop("muted", move || json!(ui.media.volume.with(|v| v.muted)))
        .prop("audioDelay", move || json!(ui.media.delay_ms.get()))
        .on_native_event(move |e: &NativeEvent| {
            match e.name.as_ref() {
                // Once a second while playing: not logged.
                "position" => {
                    if let Some(ms) = e.data["ms"].as_u64() {
                        ui.position_ms.set(ms);
                    }
                    return;
                }
                "tracks" => return crate::media::on_tracks(ui, &e.data),
                "format" => return crate::media::on_format(ui, &e.data),
                _ => {}
            }
            let status = match &*e.name {
                "state" => {
                    let state = e.data["state"].as_str().unwrap_or("?");
                    crate::player::on_state(ui, state);
                    state.to_owned()
                }
                "size" => {
                    let (w, h) = (e.data["width"].as_u64(), e.data["height"].as_u64());
                    if let (Some(w), Some(h)) = (w, h) {
                        ui.video_size.set(Some((w as u32, h as u32)));
                    }
                    format!("{}×{}", e.data["width"], e.data["height"])
                }
                "error" => {
                    let code = e.data["code"].as_str().unwrap_or("?");
                    let http = e.data["http"].as_u64().and_then(|h| u16::try_from(h).ok());
                    crate::player::on_error(ui, code, http);
                    format!("error {code}")
                }
                _ => return,
            };
            log::info!("iptv player {}: {status}", e.name);
            ui.player_status.set(status);
        })
}
