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
//! `muted`, and a paused tile's `paused`. A surface 0 without a tile
//! shrinks to nothing (it is never hidden or detached).
//!
//! The `tv.player` contract (both hosts; every instance is independent: its
//! own player, props and events, several playing at once):
//! - props `source` `{url, headers, token, live, reconnect}` or null (a new
//!   url or token reopens; the picture goes black until the new stream's
//!   first frame, except when `reconnect` says the app reopens the stream
//!   that played, or only the token changed: the last frame stays; `live`
//!   streams play on across connections the server closes where the
//!   player can bridge them, keep their last frame after an error (the app
//!   reconnects them) and are buffered to ride out short outages), `paused`, `userAgent`, `pictureSize`; `videoTrack` (`auto` or a video track id),
//!   `audioTrack` (`auto` or an audio track id), `textTrack` (`off` or a
//!   text track id) for the open stream (a new source starts again from
//!   `auto`/`auto`/`off`); `volume` (0–100; the desktop's, Android keeps
//!   the system volume), `muted`, `audioDelay` (ms, positive: sound later);
//! - events `state` {state}, `size` {width, height}, `error` {code, http?},
//!   `position` {ms}; `tracks` {video: [{id, label, width, height, fps,
//!   bitrate}], audio: [{id, label, language, channels, codec}], text:
//!   [{id, label, language}], selected: {video, audio, text}} whenever they
//!   change; `format` {fps?, channels?} for what plays.
//! - picture in picture (surface 0): prop `pip` {auto, enter, window},
//!   events `pip` {supported, tv, active} and `pipAction` {action}
//!   (`pip.rs`; Android's host; the desktops' players read `window`).
//!
//! In the background (`lifecycle()` Background) Android's host stops every
//! player, closing its connections, and reopens it when the app comes back
//! (live at the live edge, catch-up where it was) with no events between:
//! the app only pauses its stall checks (`player::bind`). Desktop players
//! play on in a minimised window.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nsh_core::prelude::*;
use serde_json::{Value, json};

use crate::{Ui, ViewMode};

/// The local mock server (`tools/mock-server.py`) as the player reaches it:
/// through the Android emulator's host alias, or on this machine. Debug
/// builds only: the synthetic guide's channels have no streams in a release.
#[cfg(all(debug_assertions, target_os = "android"))]
const MOCK_SERVER: &str = "http://10.0.2.2:8089";
#[cfg(all(debug_assertions, not(target_os = "android")))]
const MOCK_SERVER: &str = "http://127.0.0.1:8089";

/// What a channel of the synthetic guide (shown before any playlist is
/// imported) plays: in debug builds the mock server's test streams
/// (`tools/make-hls-fixtures.sh --serve`; three test patterns cycle by
/// channel, `/archive/*` for catch-up), else nothing.
#[cfg(debug_assertions)]
pub fn synthetic_url(channel: usize, playback: crate::Playback) -> Option<String> {
    Some(match playback {
        crate::Playback::Live => format!("{MOCK_SERVER}/ch{}/index.m3u8", channel % 3),
        crate::Playback::CatchUp { start, offset } => {
            let s = crate::schedule();
            let p = s.program_at(channel, start);
            let utc = crate::guide::unix_of(&*s, p.start) + i64::from(offset);
            let duration = (p.end - p.start) * 60 - offset;
            format!(
                "{MOCK_SERVER}/archive/ch{}/index.m3u8?utc={utc}&duration={duration}",
                channel % 3
            )
        }
    })
}

#[cfg(not(debug_assertions))]
pub fn synthetic_url(_channel: usize, _playback: crate::Playback) -> Option<String> {
    None
}

/// The full-screen player's rectangle (x, y, width, height in px) for the
/// view mode: the preview in the guide's header, the whole window, or its
/// multiview tile.
pub fn rect(ui: Ui) -> (f32, f32, f32, f32) {
    surface_rect(ui, 0)
}

/// Surface `surface`'s rectangle; empty for a surface with nothing to show.
/// While the desktops' picture-in-picture window shows surface 0 it
/// shrinks to nothing in the main window, where a placeholder takes its
/// place (`desktop/pip_window.rs`).
pub fn surface_rect(ui: Ui, surface: usize) -> (f32, f32, f32, f32) {
    if surface == 0 && crate::pip::in_window(ui) {
        return (0.0, 0.0, 0.0, 0.0);
    }
    layout_rect(ui, surface)
}

/// Where the view mode puts surface `surface`.
pub fn layout_rect(ui: Ui, surface: usize) -> (f32, f32, f32, f32) {
    // Android's picture-in-picture window: surface 0 fills it (`pip.rs`).
    if crate::pip::fills_app(ui) {
        let w = window();
        return match surface {
            0 => (0.0, 0.0, w.width.0, w.height.0),
            _ => (0.0, 0.0, 0.0, 0.0),
        };
    }
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

/// Each surface's opens: `source` values handed to its player with a new
/// url or token (what makes a host open a stream).
static OPENS: [AtomicU64; crate::player::SURFACES] =
    [const { AtomicU64::new(0) }; crate::player::SURFACES];

/// How many streams surface `surface`'s player was told to open so far
/// (for tests: one change of channel must open it once).
pub fn opens(surface: usize) -> u64 {
    OPENS.get(surface).map_or(0, |n| n.load(Ordering::Relaxed))
}

/// Whether surface `surface`'s player has a stream to play now (for
/// tests: stopping playback takes it away).
pub fn has_source(surface: usize) -> bool {
    SOURCED
        .get(surface)
        .is_some_and(|s| s.load(Ordering::Relaxed))
}

static SOURCED: [AtomicBool; crate::player::SURFACES] =
    [const { AtomicBool::new(false) }; crate::player::SURFACES];

/// Counts `value` as an open when its url or token differs from `last`'s.
fn count_open(surface: usize, last: &RefCell<Option<(String, u64)>>, value: &Value) {
    let now = value["url"]
        .as_str()
        .map(|url| (url.to_owned(), value["token"].as_u64().unwrap_or(0)));
    let mut last = last.borrow_mut();
    if now.is_some() && *last != now {
        OPENS[surface].fetch_add(1, Ordering::Relaxed);
    }
    SOURCED[surface].store(now.is_some(), Ordering::Relaxed);
    *last = now;
}

/// The stream as the host's `source` prop: `{url, headers, token, live,
/// reconnect}`, or null when nothing plays.
fn source_prop(ui: Ui, surface: usize) -> Value {
    let stream = ui.player.stream(surface);
    let playing = if surface == 0 {
        ui.playing.get().is_some() || crate::player::resume::playing(ui)
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
                "publicOnly": s.public_only,
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
    let sent = RefCell::new(None);
    native("tv.player")
        .absolute()
        .left(move || px(surface_rect(ui, surface).0))
        .top(move || px(surface_rect(ui, surface).1))
        .w(move || px(surface_rect(ui, surface).2))
        .h(move || px(surface_rect(ui, surface).3))
        .prop("source", move || {
            let value = source_prop(ui, surface);
            count_open(surface, &sent, &value);
            value
        })
        // Full screen's pause holds surface 0; in multiview each tile has
        // its own (the menu's Pause).
        .prop("paused", move || {
            json!((surface == 0 && ui.paused.get()) || crate::multiview::paused(ui, surface))
        })
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
        .prop("pip", move || crate::pip::prop(ui, surface))
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
                "pip" => return crate::pip::on_state(ui, &e.data),
                "pipAction" => return crate::pip::on_action(ui, &e.data),
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
                    if surface == 0 {
                        crate::startup::mark("video frame");
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
