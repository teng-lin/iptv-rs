//! The persistent video layer: one `tv.player` native component (registered by
//! the Android host, `hosts/android`, and on Linux by `mpv`), laid out per view mode — the preview
//! rectangle in the guide's header, or the whole screen under an overlay or under
//! nothing. Changing modes changes layout only; the player is never recreated.
//! What it plays comes from `player.rs` (resolution, retries); its tracks,
//! format and sound from `media.rs`.
//!
//! Multiview (`multiview.rs`) lays surface 0 out in its tile and mounts one
//! more `tv.player` per further tile (surfaces 1–3), each made when its
//! tile comes and dropped when it goes; all but the active tile's get
//! `muted`. A surface 0 without a tile shrinks to nothing (it is never
//! hidden or detached).
//!
//! The `tv.player` contract (both hosts; every instance is independent: its
//! own player, props and events, several playing at once):
//! - props `source` `{url, headers, token, live, reconnect}` or null (a new
//!   url or token reopens; the picture goes black until the new stream's
//!   first frame, except when `reconnect` says the app reopens the stream
//!   that played, or only the token changed: the last frame stays; `live`
//!   streams play on across connections the server closes where the
//!   player can bridge them), `paused`, `userAgent`, `pictureSize`; `videoTrack` (`auto` or a video track id),
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

/// The full-screen player's rectangle (x, y, width, height in px) for the
/// view mode: the preview in the guide's header, the whole window, or its
/// multiview tile.
pub fn rect(ui: Ui) -> (f32, f32, f32, f32) {
    surface_rect(ui, 0)
}

/// Surface `surface`'s rectangle; empty for a surface with nothing to show.
pub fn surface_rect(ui: Ui, surface: usize) -> (f32, f32, f32, f32) {
    if crate::multiview::is_open(ui) {
        return crate::multiview::surface_rect(ui, surface).unwrap_or((0.0, 0.0, 0.0, 0.0));
    }
    if surface != 0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let w = window();
    if ui.mode.get() != ViewMode::Guide {
        crate::channels_view::preview_rect(ui, w.width.0, w.height.0)
            .unwrap_or((0.0, 0.0, w.width.0, w.height.0))
    } else {
        crate::panels::preview_rect()
    }
}

/// The stream as the host's `source` prop: `{url, headers, token, live,
/// reconnect}`, or null when nothing plays.
fn source_prop(ui: Ui, surface: usize) -> Value {
    let stream = ui.player.stream(surface);
    let playing = if surface == 0 {
        ui.playing.get().is_some()
    } else {
        stream.channel.get().is_some()
    };
    if !playing {
        return Value::Null;
    }
    let token = stream.token.get();
    // Set with the token (`player::load`).
    let live = stream.live.get_untracked();
    let reconnect = stream.reconnect.get_untracked();
    stream.source.with(|s| {
        s.as_ref().map_or(Value::Null, |s| {
            let headers: serde_json::Map<String, Value> = s
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            json!({
                "url": s.url,
                "headers": headers,
                "token": token,
                "live": live,
                "reconnect": reconnect,
            })
        })
    })
}

/// Player surface `surface` (0: the full-screen one).
pub fn layer(ui: Ui, surface: usize) -> impl IntoElement {
    let stream = ui.player.stream(surface);
    native("tv.player")
        .absolute()
        .left(move || px(surface_rect(ui, surface).0))
        .top(move || px(surface_rect(ui, surface).1))
        .w(move || px(surface_rect(ui, surface).2))
        .h(move || px(surface_rect(ui, surface).3))
        .prop("source", move || source_prop(ui, surface))
        // Multiview's other surfaces play on.
        .prop("paused", move || json!(surface == 0 && ui.paused.get()))
        .prop("pictureSize", move || {
            let ch = if surface == 0 {
                ui.playing.get()
            } else {
                stream.channel.get()
            };
            json!(ui.picture.with(|p| p.for_channel(ch.unwrap_or(0))).key())
        })
        .prop("videoTrack", move || {
            json!(stream.choice.with(|c| c.video.clone()))
        })
        .prop("audioTrack", move || {
            json!(stream.choice.with(|c| c.audio.clone()))
        })
        .prop("textTrack", move || {
            json!(stream.choice.with(|c| c.text.clone()))
        })
        .prop("volume", move || json!(ui.media.volume.with(|v| v.level)))
        .prop("muted", move || {
            json!(ui.media.volume.with(|v| v.muted) || !crate::multiview::audible(ui, surface))
        })
        .prop("audioDelay", move || json!(ui.media.delay_ms.get()))
        .on_native_event(move |e: &NativeEvent| {
            match e.name.as_ref() {
                // Once a second while playing: not logged.
                "position" => {
                    if let Some(ms) = e.data["ms"].as_u64() {
                        stream.position_ms.set(ms);
                    }
                    return;
                }
                "tracks" => return crate::media::on_tracks(ui, surface, &e.data),
                "format" => return crate::media::on_format(ui, surface, &e.data),
                _ => {}
            }
            let status = match &*e.name {
                "state" => {
                    let state = e.data["state"].as_str().unwrap_or("?");
                    crate::player::on_state(ui, surface, state);
                    state.to_owned()
                }
                "size" => {
                    let (w, h) = (e.data["width"].as_u64(), e.data["height"].as_u64());
                    if let (Some(w), Some(h)) = (w, h) {
                        stream.size.set(Some((w as u32, h as u32)));
                    }
                    format!("{}×{}", e.data["width"], e.data["height"])
                }
                "error" => {
                    let code = e.data["code"].as_str().unwrap_or("?");
                    let http = e.data["http"].as_u64().and_then(|h| u16::try_from(h).ok());
                    crate::player::on_error(ui, surface, code, http);
                    format!("error {code}")
                }
                _ => return,
            };
            log::info!("iptv player {surface} {}: {status}", e.name);
            if surface == 0 {
                ui.player_status.set(status);
            }
        })
}
