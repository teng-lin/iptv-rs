//! What the Android host's props and events mean in mpv's terms: picture
//! sizes as mpv options, HTTP headers as mpv's, and mpv's failures as the
//! Media3 error-code names `iptv_core::playback::ErrorKind` classifies.
//! Pure functions, tested without mpv.

/// How mpv shows a picture-size mode (`pictureSize` prop): the same
/// rectangle `PictureFrame.pictureSize()` gives the Android surface, clipped
/// by the view.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// mpv `keepaspect`: `false` stretches to the view.
    pub keepaspect: bool,
    /// mpv `panscan`: 1 crops to fill the view (cover).
    pub panscan: f64,
    /// mpv `video-aspect-override` (`"no"`: the video's own).
    pub aspect: &'static str,
    /// mpv `video-zoom` (log2 of the scale of the fitted picture).
    pub zoom: f64,
}

/// mpv's options for picture-size `mode` with video of display aspect
/// `video_aspect` (16:9 until known, as on Android).
pub fn picture(mode: &str, video_aspect: Option<f64>) -> Picture {
    let va = video_aspect
        .filter(|a| a.is_finite() && *a > 0.0)
        .unwrap_or(16.0 / 9.0);
    let fit = Picture {
        keepaspect: true,
        panscan: 0.0,
        aspect: "no",
        zoom: 0.0,
    };
    match mode {
        "full" => Picture {
            keepaspect: false,
            ..fit
        },
        "zoom" => Picture {
            panscan: 1.0,
            ..fit
        },
        // A wide film letterboxed in the video: zoom until it fills the frame.
        "cinema" => Picture {
            zoom: (2.35 / va).max(1.0).log2(),
            ..fit
        },
        "reduced" => Picture {
            zoom: 0.85_f64.log2(),
            ..fit
        },
        "4:3" => Picture {
            aspect: "4:3",
            ..fit
        },
        "16:9" => Picture {
            aspect: "16:9",
            ..fit
        },
        "2.35:1" => Picture {
            aspect: "2.35:1",
            ..fit
        },
        _ => fit,
    }
}

/// The HTTP options of one stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http {
    /// mpv `user-agent`: the source's, else the `userAgent` prop's.
    pub user_agent: String,
    /// mpv `http-header-fields` entries (`Name: value`), User-Agent aside.
    pub fields: Vec<String>,
}

/// mpv's HTTP options for a stream's `headers`. mpv (FFmpeg) sends the same
/// fields with every request of the stream, including redirects and
/// playlist segments on other hosts: it cannot keep identity headers
/// (Cookie, Authorization, SN, X-User-Agent) to the stream's own host as the
/// Android host does. Headers that could split a request (CR, LF, NUL) are
/// dropped.
pub fn http(headers: &[(String, String)], fallback_ua: &str) -> Http {
    let clean = |s: &str| !s.contains(['\r', '\n', '\0']);
    let mut user_agent = None;
    let mut fields = Vec::new();
    for (name, value) in headers {
        let name = name.trim();
        if name.is_empty() || name.contains(':') || !clean(name) || !clean(value) {
            continue;
        }
        if name.eq_ignore_ascii_case("user-agent") {
            user_agent = Some(value.clone()).filter(|v| !v.trim().is_empty());
        } else {
            fields.push(format!("{name}: {value}"));
        }
    }
    Http {
        user_agent: user_agent.unwrap_or_else(|| fallback_ua.to_owned()),
        fields,
    }
}

/// Whether mpv may open `url`: network streams only (never local files,
/// FFmpeg's pseudo-protocols or mpv's own `av://`, `lavfi://`...).
pub fn playable(url: &str) -> bool {
    const SCHEMES: &[&str] = &[
        "http", "https", "rtmp", "rtmps", "rtsp", "rtsps", "rtp", "udp", "srt", "mms",
    ];
    url.split_once("://")
        .is_some_and(|(scheme, _)| SCHEMES.iter().any(|s| s.eq_ignore_ascii_case(scheme)))
}

/// What mpv's log said about the current file's failure (its messages carry
/// no codes the API reports: FFmpeg's HTTP status and socket errors come
/// only as text). Holds no text, so no address can leak from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clues {
    http: Option<u16>,
    network: Option<&'static str>,
    format: bool,
}

pub const HTTP_STATUS: &str = "ERROR_CODE_IO_BAD_HTTP_STATUS";
pub const CONNECTION_FAILED: &str = "ERROR_CODE_IO_NETWORK_CONNECTION_FAILED";
pub const CONNECTION_TIMEOUT: &str = "ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT";
pub const CONTAINER_UNSUPPORTED: &str = "ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED";
pub const FORMAT_UNSUPPORTED: &str = "ERROR_CODE_DECODING_FORMAT_UNSUPPORTED";
pub const DECODER_INIT_FAILED: &str = "ERROR_CODE_DECODER_INIT_FAILED";
pub const IO_UNSPECIFIED: &str = "ERROR_CODE_IO_UNSPECIFIED";
pub const UNSPECIFIED: &str = "ERROR_CODE_UNSPECIFIED";
/// Not one of Media3's: an address mpv is not allowed to open
/// ([`playable`]); classified as a format failure (no retry).
pub const UNSUPPORTED_SCHEME: &str = "ERROR_CODE_UNSUPPORTED_SCHEME";

/// The 3-digit status after `marker` in `text`.
fn status_after(text: &str, marker: &str) -> Option<u16> {
    let rest = &text[text.find(marker)? + marker.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 3).then(|| digits.parse().ok()).flatten()
}

impl Clues {
    /// Reads one log message.
    pub fn hear(&mut self, text: &str) {
        if let Some(status) =
            status_after(text, "HTTP error ").or_else(|| status_after(text, "Server returned "))
            && status >= 400
        {
            self.http = Some(status);
            return;
        }
        let t = text.to_ascii_lowercase();
        if t.contains("timed out") {
            self.network = Some(CONNECTION_TIMEOUT);
        } else if [
            "connection refused",
            "failed to resolve",
            "network is unreachable",
            "host is unreachable",
            "no route to host",
            "connection reset",
        ]
        .iter()
        .any(|m| t.contains(m))
        {
            self.network = Some(CONNECTION_FAILED);
        } else if t.contains("failed to recognize file format") {
            self.format = true;
        }
    }

    /// The `error` event for a file that ended with mpv error `error`:
    /// Media3's code name and the HTTP status, if a server answered one.
    pub fn error(&self, error: i32) -> (&'static str, Option<u16>) {
        use super::ffi::{
            MPV_ERROR_LOADING_FAILED, MPV_ERROR_NOTHING_TO_PLAY, MPV_ERROR_UNKNOWN_FORMAT,
        };
        if let Some(status) = self.http {
            return (HTTP_STATUS, Some(status));
        }
        if let Some(code) = self.network {
            return (code, None);
        }
        let code = match error {
            _ if self.format => CONTAINER_UNSUPPORTED,
            MPV_ERROR_UNKNOWN_FORMAT => CONTAINER_UNSUPPORTED,
            MPV_ERROR_NOTHING_TO_PLAY => FORMAT_UNSUPPORTED,
            MPV_ERROR_LOADING_FAILED => IO_UNSPECIFIED,
            _ => UNSPECIFIED,
        };
        (code, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptv_core::playback::ErrorKind;

    fn kind(clues: &Clues, error: i32) -> ErrorKind {
        let (code, http) = clues.error(error);
        ErrorKind::from_player_code(code, http)
    }

    #[test]
    fn picture_sizes_match_the_android_frame() {
        assert_eq!(picture("normal", Some(1.5)), picture("anything", None));
        assert!(!picture("full", None).keepaspect);
        assert_eq!(picture("zoom", None).panscan, 1.0);
        assert_eq!(picture("4:3", None).aspect, "4:3");
        assert_eq!(picture("2.35:1", None).aspect, "2.35:1");
        assert!((picture("reduced", None).zoom.exp2() - 0.85).abs() < 1e-9);
        // Cinema zooms a 16:9 picture by 2.35 / (16/9); never shrinks.
        let cinema = picture("cinema", Some(16.0 / 9.0)).zoom.exp2();
        assert!((cinema - 2.35 * 9.0 / 16.0).abs() < 1e-9);
        assert_eq!(picture("cinema", Some(2.4)).zoom, 0.0);
    }

    #[test]
    fn headers_become_mpv_options() {
        let h = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<Vec<_>>()
        };
        let got = http(
            &h(&[
                ("user-agent", "Box/1"),
                ("Referer", "http://example.test/"),
                ("Cookie", "a=1, b=2"),
                ("X-Bad", "v\r\nInjected: 1"),
                ("", "empty"),
            ]),
            "fallback",
        );
        assert_eq!(got.user_agent, "Box/1");
        assert_eq!(
            got.fields,
            ["Referer: http://example.test/", "Cookie: a=1, b=2"]
        );
        assert_eq!(http(&[], "fallback").user_agent, "fallback");
    }

    #[test]
    fn only_network_streams_open() {
        assert!(playable("http://127.0.0.1:8089/ch0/index.m3u8"));
        assert!(playable("HTTPS://example.test/live.ts"));
        assert!(playable("udp://@239.0.0.1:1234"));
        for url in [
            "file:///etc/hosts",
            "/etc/hosts",
            "av://lavfi:sine",
            "concat:a|b",
            "",
        ] {
            assert!(!playable(url), "{url}");
        }
    }

    #[test]
    fn failures_classify_as_on_android() {
        let mut c = Clues::default();
        c.hear("http: HTTP error 404 Not Found");
        assert_eq!(c.error(MPV_LOADING), (HTTP_STATUS, Some(404)));
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Http(404));

        let mut c = Clues::default();
        c.hear("Server returned 403 Forbidden (access denied)");
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Http(403));
        assert!(!kind(&c, MPV_LOADING).retryable());

        let mut c = Clues::default();
        c.hear("tcp: Connection to tcp://127.0.0.1:9 failed: Connection refused");
        assert_eq!(c.error(MPV_LOADING).0, CONNECTION_FAILED);
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Network);

        let mut c = Clues::default();
        c.hear("tcp: Connection to tcp://192.0.2.1:80 failed: Connection timed out");
        assert_eq!(c.error(MPV_LOADING).0, CONNECTION_TIMEOUT);
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Network);

        let mut c = Clues::default();
        c.hear("Failed to recognize file format.");
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Format);

        let c = Clues::default();
        assert_eq!(
            kind(&c, super::super::ffi::MPV_ERROR_UNKNOWN_FORMAT),
            ErrorKind::Format
        );
        assert_eq!(
            kind(&c, super::super::ffi::MPV_ERROR_NOTHING_TO_PLAY),
            ErrorKind::Format
        );
        assert_eq!(kind(&c, MPV_LOADING), ErrorKind::Network);
        assert_eq!(kind(&c, -20), ErrorKind::Other);
        assert_eq!(
            ErrorKind::from_player_code(UNSUPPORTED_SCHEME, None),
            ErrorKind::Format
        );
        assert_eq!(
            ErrorKind::from_player_code(DECODER_INIT_FAILED, None),
            ErrorKind::Format
        );
        // A 2xx/3xx in a message is no failure status.
        let mut c = Clues::default();
        c.hear("HTTP error 302 Found");
        assert_eq!(c.error(MPV_LOADING).1, None);
    }

    const MPV_LOADING: i32 = super::super::ffi::MPV_ERROR_LOADING_FAILED;
}
