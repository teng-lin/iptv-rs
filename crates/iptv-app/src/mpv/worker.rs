//! The thread that talks to mpv. GTK's main thread renders mpv's video, and
//! mpv's render API forbids blocking mpv calls on the render thread (the
//! core may wait for a frame while the call waits for the core: the UI
//! froze up to a second when zapping). So every command, property and event
//! goes through this thread; the main thread only queues [`Cmd`]s and reads
//! [`Note`]s.

use std::ffi::{CStr, CString, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::ffi;
use super::handle::Mpv;

/// mpv's warning level: what the failure clues read.
const LOG_WARN: c_int = 30;

/// What the main thread asks of mpv.
pub enum Cmd {
    /// `loadfile <url> replace`, from the default tracks (video and audio
    /// automatic, no subtitles); answered by [`Note::Loading`].
    Load(String),
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
                .spawn(move || run(&mpv, &queue, &quit, &tx, notify))
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
) {
    loop {
        if quit.load(Ordering::Acquire) {
            return;
        }
        let cmds = queue.lock().map(|mut q| std::mem::take(&mut *q));
        let mut sent = false;
        for cmd in cmds.unwrap_or_default() {
            for note in execute(mpv, cmd) {
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
                    if let Some(note) = convert(mpv, ev) {
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

fn execute(mpv: &Mpv, cmd: Cmd) -> Vec<Note> {
    if let Cmd::Format = cmd {
        let fps = mpv.get_f64(c"container-fps").map(Note::Fps);
        let channels = mpv
            .get_i64(c"audio-params/channel-count")
            .map(Note::Channels);
        return fps.into_iter().chain(channels).collect();
    }
    execute_one(mpv, cmd).into_iter().collect()
}

fn execute_one(mpv: &Mpv, cmd: Cmd) -> Option<Note> {
    match cmd {
        Cmd::Load(url) => {
            // Track choices are options: they would carry over to this file.
            mpv.set("vid", "auto");
            mpv.set("aid", "auto");
            mpv.set("sid", "no");
            let ok = mpv.command(&["loadfile", &url, "replace"]);
            // The new entry is the playlist's last (`replace` may keep the
            // stopping one until it ends).
            let id = ok
                .then(|| mpv.get_i64(c"playlist-count"))
                .flatten()
                .and_then(|n| {
                    let name = CString::new(format!("playlist/{}/id", n - 1)).ok()?;
                    mpv.get_i64(&name)
                });
            Some(Note::Loading(id))
        }
        Cmd::Stop => {
            mpv.command(&["stop"]);
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
