//! Decodes the shared native player track event.
use iptv_core::tracks::{AudioTrack, TextTrack, Tracks, VideoTrack};
use serde_json::Value;

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
