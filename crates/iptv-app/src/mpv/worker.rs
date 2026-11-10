//! The thread that talks to mpv. GTK's main thread renders mpv's video, and
//! mpv's render API forbids blocking mpv calls on the render thread (the
//! core may wait for a frame while the call waits for the core: the UI
//! froze up to a second when zapping). So every command, property and event
//! goes through this thread; the main thread only queues [`Cmd`]s and reads
//! [`Note`]s.
//!
//! Live streams play on across a connection the server closes ([`Bridge`]):
//! providers cut live HLS (a playlist reload refused once the redirect's
//! token expired) and MPEG-TS (the connection closed) every minute or so.
//! Once a live entry plays, the same address is appended as a spare entry;
//! mpv's `prefetch-playlist` opens it as soon as the current one has been
//! read to its end, while the buffered rest still plays, and goes on to it
//! without a gap. ffmpeg's `reconnect_streamed`/`reconnect_at_eof` are not
//! used for this: they re-request every HLS playlist and segment served
//! without a length, so live HLS falls behind and stalls.

use std::ffi::{CStr, CString, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::ffi;
use super::handle::Mpv;

/// mpv's warning level: what the failure clues read.
const LOG_WARN: c_int = 30;

/// A live entry that played shorter than this before it ended is not
/// bridged: its end goes to the app, whose retries back off.
const MIN_BRIDGED: Duration = Duration::from_secs(5);

/// ffmpeg's options for opening a spare entry ([`Bridge::restarted`]).
const SPARE_OPTIONS: &str = "live_start_index=-2";

/// What the main thread asks of mpv.
pub enum Cmd {
    /// `loadfile <url> replace`, from the default tracks (video and audio
    /// automatic, no subtitles); answered by [`Note::Loading`]. A `live`
    /// stream is bridged across connections the server closes.
    Load {
        url: String,
        live: bool,
    },
    Stop,
    Set(&'static str, String),
    /// The HTTP options for the next load.
    Http {
        user_agent: String,
        fields: Vec<String>,
    },
    /// Answered by [`Note::Position`] when mpv knows it.
    Position,
    /// Answered by [`Note::Fps`] and [`Note::Channels`], as far as known
    /// (an unchanged value is not observed again for the next file).
    Format,
}

/// What mpv said, as owned values (in mpv's order).
pub enum Note {
    /// The playlist entry a [`Cmd::Load`] made (`None`: refused).
    Loading(Option<i64>),
    Started(i64),
    Ended {
        entry: i64,
        reason: c_int,
        error: c_int,
    },
    /// The current live entry was read to its end (the server closed it)
    /// and mpv plays on with this spare entry of the same address, already
    /// open: not an end.
    Bridged(i64),
    /// Playback (re)started; with the audio output in use.
    Restarted {
        audio_out: Option<String>,
    },
    Buffering(bool),
    Width(i64),
    Height(i64),
    Aspect(f64),
    /// A warning or error (read for failure clues, never kept or logged),
    /// or an output format line (logged).
    Log {
        warn: bool,
        text: String,
    },
    /// `time-pos` in seconds.
    Position(f64),
    /// `track-list` changed: mpv's JSON of it.
    Tracks(String),
    /// `container-fps`.
    Fps(f64),
    /// `audio-params/channel-count`.
    Channels(i64),
}

pub struct Worker {
    mpv: Arc<Mpv>,
    queue: Arc<Mutex<Vec<Cmd>>>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub notes: Receiver<Note>,
}

impl Worker {
    /// Starts the thread; `notify` runs (on it) after notes were sent.
    pub fn spawn(mpv: Arc<Mpv>, notify: impl Fn() + Send + 'static) -> Worker {
        let (tx, notes) = channel();
        let queue = Arc::new(Mutex::new(Vec::new()));
        let quit = Arc::new(AtomicBool::new(false));
        let thread = {
            let (mpv, queue, quit) = (mpv.clone(), queue.clone(), quit.clone());
            std::thread::Builder::new()
                .name("iptv-mpv".into())
                .spawn(move || run(&mpv, &queue, &quit, &tx, notify, Bridge::new()))
                .map_err(|e| log::error!("iptv mpv: no control thread ({e})"))
                .ok()
        };
        Worker {
            mpv,
            queue,
            quit,
            thread,
            notes,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(cmd);
        }
        self.mpv.wakeup();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
        self.mpv.wakeup();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    mpv: &Mpv,
    queue: &Mutex<Vec<Cmd>>,
    quit: &AtomicBool,
    tx: &Sender<Note>,
    notify: impl Fn(),
    mut bridge: Bridge,
) {
    loop {
        if quit.load(Ordering::Acquire) {
            return;
        }
        let cmds = queue.lock().map(|mut q| std::mem::take(&mut *q));
        let mut sent = false;
        for cmd in cmds.unwrap_or_default() {
            for note in execute(mpv, cmd, &mut bridge) {
                sent |= tx.send(note).is_ok();
            }
        }
        // Waits for an event or a wake-up (`send`, `drop`), then takes
        // whatever else is queued.
        let mut timeout = -1.0;
        loop {
            // SAFETY: a live handle; the event is valid until the next wait,
            // and only this thread waits.
            let ev = unsafe { &*ffi::mpv_wait_event(mpv.0, timeout) };
            timeout = 0.0;
            match ev.event_id {
                ffi::MPV_EVENT_NONE => break,
                ffi::MPV_EVENT_SHUTDOWN => return,
                _ => {
                    if let Some(note) = bridge.event(mpv, ev).or_else(|| convert(mpv, ev)) {
                        sent |= tx.send(note).is_ok();
                    }
                }
            }
        }
        if sent {
            notify();
        }
    }
}

fn execute(mpv: &Mpv, cmd: Cmd, bridge: &mut Bridge) -> Vec<Note> {
    if let Cmd::Format = cmd {
        let fps = mpv.get_f64(c"container-fps").map(Note::Fps);
        let channels = mpv
            .get_i64(c"audio-params/channel-count")
            .map(Note::Channels);
        return fps.into_iter().chain(channels).collect();
    }
    execute_one(mpv, cmd, bridge).into_iter().collect()
}

fn execute_one(mpv: &Mpv, cmd: Cmd, bridge: &mut Bridge) -> Option<Note> {
    match cmd {
        Cmd::Load { url, live } => {
            // Track choices are options: they would carry over to this file.
            mpv.set("vid", "auto");
            mpv.set("aid", "auto");
            mpv.set("sid", "no");
            mpv.set("demuxer-lavf-o", "");
            let ok = mpv.command(&["loadfile", &url, "replace"]);
            // The new entry is the playlist's last (`replace` may keep the
            // stopping one until it ends).
            let id = ok.then(|| last_entry(mpv)).flatten();
            bridge.load(url, live, id);
            Some(Note::Loading(id))
        }
        Cmd::Stop => {
            mpv.command(&["stop"]);
            bridge.load(String::new(), false, None);
            None
        }
        Cmd::Set(name, value) => {
            mpv.set(name, &value);
            None
        }
        Cmd::Http { user_agent, fields } => {
            mpv.set("user-agent", &user_agent);
            mpv.command(&["change-list", "http-header-fields", "clr", ""]);
            for field in &fields {
                mpv.command(&["change-list", "http-header-fields", "append", field]);
            }
            None
        }
        Cmd::Position => mpv.get_f64(c"time-pos").map(Note::Position),
        Cmd::Format => None,
    }
}

/// The id of the playlist's last entry.
fn last_entry(mpv: &Mpv) -> Option<i64> {
    let n = mpv.get_i64(c"playlist-count")?;
    let name = CString::new(format!("playlist/{}/id", n - 1)).ok()?;
    mpv.get_i64(&name)
}

/// Removes the playlist entry `id`, if it is still there.
fn remove_entry(mpv: &Mpv, id: i64) {
    let n = mpv.get_i64(c"playlist-count").unwrap_or(0);
    let index = (0..n).find(|i| {
        CString::new(format!("playlist/{i}/id"))
            .ok()
            .and_then(|name| mpv.get_i64(&name))
            == Some(id)
    });
    if let Some(i) = index {
        mpv.command(&["playlist-remove", &i.to_string()]);
    }
}

/// The spare entry of a live stream (see the module's docs). Off with
/// `IPTV_RS_LIVE_BRIDGE=0`: every closed connection is then an end the app
/// reconnects.
struct Bridge {
    enabled: bool,
    /// The live stream's address (`None`: nothing live is loaded).
    url: Option<String>,
    /// The entry that plays it, and its spare.
    current: Option<i64>,
    spare: Option<i64>,
    /// The entry mpv started last.
    started: Option<i64>,
    /// When the current entry started playing.
    since: Option<Instant>,
}

impl Bridge {
    fn new() -> Bridge {
        let off = std::env::var("IPTV_RS_LIVE_BRIDGE").is_ok_and(|v| v.trim() == "0");
        if off {
            log::info!("iptv mpv: live bridging off");
        }
        Bridge {
            enabled: !off,
            url: None,
            current: None,
            spare: None,
            started: None,
            since: None,
        }
    }

    /// A load replaced the playlist (or a stop cleared it).
    fn load(&mut self, url: String, live: bool, entry: Option<i64>) {
        self.url = (live && self.enabled && entry.is_some()).then_some(url);
        self.current = entry;
        self.spare = None;
        self.since = None;
    }

    /// Bridging takes these events first; `Some` replaces their note.
    fn event(&mut self, mpv: &Mpv, ev: &ffi::mpv_event) -> Option<Note> {
        match ev.event_id {
            ffi::MPV_EVENT_START_FILE => {
                // SAFETY: the data type mpv documents for the event.
                let s = unsafe { &*(ev.data as *const ffi::mpv_event_start_file) };
                self.started = Some(s.playlist_entry_id);
                None
            }
            ffi::MPV_EVENT_PLAYBACK_RESTART => {
                self.restarted(mpv);
                None
            }
            ffi::MPV_EVENT_END_FILE => {
                // SAFETY: as above.
                let e = unsafe { &*(ev.data as *const ffi::mpv_event_end_file) };
                self.ended(mpv, e.playlist_entry_id, e.reason)
            }
            _ => None,
        }
    }

    /// The current live entry plays: it gets a spare.
    fn restarted(&mut self, mpv: &Mpv) {
        let Some(url) = &self.url else { return };
        if self.current.is_none() || self.started != self.current {
            return;
        }
        self.since.get_or_insert_with(Instant::now);
        if self.spare.is_some() {
            return;
        }
        // The spare of a live HLS stream starts at the newest segment: what
        // the current entry has buffered reaches about there (ffmpeg's
        // default, three segments back, would replay those seconds). A
        // prefetch opens with the global options, not the entry's own: set
        // them now that the current entry is open ([`Cmd::Load`] clears them).
        mpv.set("demuxer-lavf-o", SPARE_OPTIONS);
        if mpv.command(&["loadfile", url, "append"]) {
            self.spare = last_entry(mpv);
        }
    }

    /// The current entry ended: read to its end after it played a while,
    /// it hands over to its spare; any other end drops the spare (mpv
    /// stops instead of going on to it) and is reported as before.
    fn ended(&mut self, mpv: &Mpv, entry: i64, reason: c_int) -> Option<Note> {
        if self.current != Some(entry) {
            return None;
        }
        let spare = self.spare.take()?;
        let played = self.since.take().map(|t| t.elapsed());
        if reason == ffi::MPV_END_FILE_REASON_EOF && played.is_some_and(|p| p >= MIN_BRIDGED) {
            self.current = Some(spare);
            return Some(Note::Bridged(spare));
        }
        remove_entry(mpv, spare);
        None
    }
}

fn convert(mpv: &Mpv, ev: &ffi::mpv_event) -> Option<Note> {
    // SAFETY (each cast): the data type mpv documents for the event id.
    match ev.event_id {
        ffi::MPV_EVENT_LOG_MESSAGE => {
            let m = unsafe { &*(ev.data as *const ffi::mpv_event_log_message) };
            if m.text.is_null() {
                return None;
            }
            let text = unsafe { CStr::from_ptr(m.text) }.to_string_lossy();
            let warn = m.log_level <= LOG_WARN;
            let format = text.starts_with("AO:") || text.starts_with("VO:");
            (warn || format).then(|| Note::Log {
                warn,
                text: text.into_owned(),
            })
        }
        ffi::MPV_EVENT_START_FILE => {
            let s = unsafe { &*(ev.data as *const ffi::mpv_event_start_file) };
            Some(Note::Started(s.playlist_entry_id))
        }
        ffi::MPV_EVENT_END_FILE => {
            let e = unsafe { &*(ev.data as *const ffi::mpv_event_end_file) };
            Some(Note::Ended {
                entry: e.playlist_entry_id,
                reason: e.reason,
                error: e.error,
            })
        }
        ffi::MPV_EVENT_PLAYBACK_RESTART => Some(Note::Restarted {
            audio_out: mpv.get(c"current-ao"),
        }),
        ffi::MPV_EVENT_PROPERTY_CHANGE if ev.reply_userdata == 5 => {
            mpv.get(c"track-list").map(Note::Tracks)
        }
        ffi::MPV_EVENT_PROPERTY_CHANGE => {
            let p = unsafe { &*(ev.data as *const ffi::mpv_event_property) };
            if p.data.is_null() {
                return None; // unavailable (no file, no video)
            }
            match (ev.reply_userdata, p.format) {
                (1, ffi::MPV_FORMAT_FLAG) => {
                    Some(Note::Buffering(unsafe { *(p.data as *const c_int) } != 0))
                }
                (2, ffi::MPV_FORMAT_INT64) => Some(Note::Width(unsafe { *(p.data as *const i64) })),
                (3, ffi::MPV_FORMAT_INT64) => {
                    Some(Note::Height(unsafe { *(p.data as *const i64) }))
                }
                (4, ffi::MPV_FORMAT_DOUBLE) => {
                    Some(Note::Aspect(unsafe { *(p.data as *const f64) }))
                }
                (6, ffi::MPV_FORMAT_DOUBLE) => Some(Note::Fps(unsafe { *(p.data as *const f64) })),
                (7, ffi::MPV_FORMAT_INT64) => {
                    Some(Note::Channels(unsafe { *(p.data as *const i64) }))
                }
                _ => None,
            }
        }
        _ => None,
    }
}
