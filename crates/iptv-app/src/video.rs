//! The persistent video layer: one `tv.player` native component (registered by
//! the Android host, `hosts/android`), laid out per view mode — the preview
//! rectangle in the info panel, or the whole screen under an overlay or under
//! nothing. Changing modes changes layout only; the player is never recreated.

use nsh_core::prelude::*;
use serde_json::{Value, json};

use crate::panels::{INFO_H, INFO_PAD_X, INFO_PAD_Y, PREVIEW_H, PREVIEW_W};
use crate::{Ui, ViewMode};

/// Synthetic local HLS from `tools/make-hls-fixtures.sh --serve`, reached
/// through the emulator's host alias. Three test patterns cycle by channel.
pub fn stream_url(channel: usize) -> String {
    format!("http://10.0.2.2:8089/ch{}/index.m3u8", channel % 3)
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let full = move || ui.mode.get() != ViewMode::Guide;
    native("tv.player")
        .absolute()
        .left(move || px(if full() { 0.0 } else { INFO_PAD_X }))
        .top(move || px(if full() { 0.0 } else { window().height.0 - INFO_H + INFO_PAD_Y }))
        .w(move || if full() { window().width } else { px(PREVIEW_W) })
        .h(move || if full() { window().height } else { px(PREVIEW_H) })
        .prop("url", move || ui.playing.get().map_or(Value::Null, |ch| json!(stream_url(ch))))
        .on_native_event(move |e: &NativeEvent| {
            let status = match &*e.name {
                "state" => e.data["state"].as_str().unwrap_or("?").to_owned(),
                "size" => format!("{}×{}", e.data["width"], e.data["height"]),
                "error" => format!("error {}", e.data["code"].as_str().unwrap_or("?")),
                _ => return,
            };
            log::info!("iptv player {}: {status}", e.name);
            ui.player_status.set(status);
        })
}
