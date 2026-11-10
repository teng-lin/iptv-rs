//! `tv.player` on the Linux desktop: one persistent libmpv player drawing
//! through mpv's render API (OpenGL) into a `GtkGLArea`, the native widget
//! NSH hosts in the video layer's view (`video.rs`). In-process and
//! compositor-independent (Wayland and X11); the app's UI draws above it
//! as above any view. Layout changes resize the area; the mpv instance
//! lives as long as the component.
//!
//! Same contract as the Android host's `PlayerComponent.kt`:
//! - props `source` `{url, headers, token}` or null (a new url or token
//!   reopens), `paused`, `userAgent` (fallback when the source sends none),
//!   `pictureSize` (see [`map::picture`]);
//! - events `state` {state: idle|buffering|ready|ended}, `size` {width,
//!   height}, `error` {code, http?} with Media3's code names
//!   ([`map::Clues::error`]), never addresses or messages.
//!
//! The picture is black from a new source or an error until the stream
//! plays again. mpv sends a stream's headers with all of its requests
//! ([`map::http`]). `IPTV_RS_AUDIO` names mpv's audio output (`null`
//! mutes, for tests); `IPTV_RS_HWDEC` overrides `hwdec`.

mod ffi;
mod gl;
mod handle;
mod map;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, c_int, c_void};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nsh_platform_linux::gtk4::{self as gtk, glib, prelude::*};
use nsh_platform_linux::{ComponentContext, Components, NativeComponent};
use serde_json::{Value, json};

use handle::Mpv;

pub const NAME: &str = "tv.player";
pub(crate) const DEFAULT_UA: &str = "iptv-rs/0.1 (mpv)";
/// mpv's warning level: what the failure clues read.
const LOG_WARN: c_int = 30;

/// Registers `tv.player`.
pub fn register(components: &Components) {
    components.register(
        NAME,
        Rc::new(|ctx| Box::new(Player::new(ctx)) as Box<dyn NativeComponent>),
    );
}

thread_local! {
    /// The players by id, for mpv's callbacks (which come on mpv's threads
    /// and reach the main loop with only an id).
    static PLAYERS: RefCell<HashMap<u64, Weak<Inner>>> = RefCell::default();
}

fn find(id: u64) -> Option<Rc<Inner>> {
    PLAYERS.with(|p| p.borrow().get(&id).and_then(Weak::upgrade))
}

/// What mpv's callbacks hold: the player's id and whether a main-loop
/// call is already queued.
struct Waker {
    id: u64,
    events: AtomicBool,
    frame: AtomicBool,
}

/// Runs `f` on the main loop once per burst of `flag` wake-ups (never
/// synchronously: mpv may call back from inside its API on this thread).
fn wake(flag: &AtomicBool, id: u64, f: fn(&Inner)) {
    if !flag.swap(true, Ordering::AcqRel) {
        glib::idle_add_full(glib::Priority::DEFAULT, move || {
            if let Some(p) = find(id) {
                f(&p);
            }
            glib::ControlFlow::Break
        });
    }
}

unsafe extern "C" fn on_events(data: *mut c_void) {
    // SAFETY: `data` is the player's boxed `Waker`, alive until mpv is gone.
    let w = unsafe { &*(data as *const Waker) };
    wake(&w.events, w.id, Inner::drain);
}

unsafe extern "C" fn on_frame(data: *mut c_void) {
    // SAFETY: as in `on_events`; the render context is freed first.
    let w = unsafe { &*(data as *const Waker) };
    wake(&w.frame, w.id, |p| {
        p.waker.frame.store(false, Ordering::Release);
        p.area.queue_render();
    });
}

/// What the player has been told and has seen.
struct State {
    url: Option<String>,
    token: i64,
    headers: Vec<(String, String)>,
    user_agent: String,
    picture: String,
    aspect: Option<f64>,
    width: i64,
    height: i64,
    /// The size last reported.
    reported: (i64, i64),
    /// Black over the picture until the stream plays.
    shutter: bool,
    /// Loads mpv has not started yet: older files' events are ignored.
    pending_loads: u32,
    /// The playlist entry of the file now playing.
    entry: Option<i64>,
    /// The current file started playing (later buffering is a rebuffer).
    playing: bool,
    buffering: bool,
    last_state: &'static str,
    clues: map::Clues,
    /// A source that waits for the GL context.
    deferred: bool,
    audio_out: Option<String>,
}

struct Inner {
    id: u64,
    area: gtk::GLArea,
    ctx: ComponentContext,
    mpv: Option<Mpv>,
    render: Cell<*mut ffi::mpv_render_context>,
    gl: Cell<Option<gl::Gl>>,
    state: RefCell<State>,
    waker: Box<Waker>,
}

/// The component NSH holds.
struct Player(Rc<Inner>);

impl Player {
    fn new(ctx: ComponentContext) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let area = gtk::GLArea::new();
        area.set_auto_render(false);
        area.set_has_depth_buffer(false);
        area.set_has_stencil_buffer(false);
        // Touches go to the view.
        area.set_can_target(false);
        area.set_focusable(false);
        let mpv = Mpv::new();
        if mpv.is_none() {
            log::error!("iptv mpv: no player (is libmpv installed?)");
        }
        let inner = Rc::new(Inner {
            id,
            area: area.clone(),
            ctx,
            mpv,
            render: Cell::new(std::ptr::null_mut()),
            gl: Cell::new(None),
            state: RefCell::new(State {
                url: None,
                token: 0,
                headers: Vec::new(),
                user_agent: DEFAULT_UA.to_owned(),
                picture: "normal".to_owned(),
                aspect: None,
                width: 0,
                height: 0,
                reported: (0, 0),
                shutter: true,
                pending_loads: 0,
                entry: None,
                playing: false,
                buffering: false,
                last_state: "idle",
                clues: map::Clues::default(),
                deferred: false,
                audio_out: None,
            }),
            waker: Box::new(Waker {
                id,
                events: AtomicBool::new(false),
                frame: AtomicBool::new(false),
            }),
        });
        PLAYERS.with(|p| p.borrow_mut().insert(id, Rc::downgrade(&inner)));
        if let Some(mpv) = &inner.mpv {
            let data = &*inner.waker as *const Waker as *mut c_void;
            // SAFETY: the waker outlives the handle (`Inner::drop`).
            unsafe { ffi::mpv_set_wakeup_callback(mpv.0, Some(on_events), data) };
        }
        let weak = Rc::downgrade(&inner);
        area.connect_realize(move |_| {
            if let Some(p) = weak.upgrade() {
                p.realize();
            }
        });
        let weak = Rc::downgrade(&inner);
        area.connect_unrealize(move |_| {
            if let Some(p) = weak.upgrade() {
                p.unrealize();
            }
        });
        let weak = Rc::downgrade(&inner);
        area.connect_render(move |_, _| {
            if let Some(p) = weak.upgrade() {
                p.draw();
            }
            glib::Propagation::Stop
        });
        Player(inner)
    }
}

impl NativeComponent for Player {
    fn widget(&self) -> gtk::Widget {
        self.0.area.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        let p = &self.0;
        match name {
            "source" => p.set_source(value),
            "paused" => {
                let paused = value.and_then(Value::as_bool).unwrap_or(false);
                if let Some(mpv) = &p.mpv {
                    mpv.set("pause", if paused { "yes" } else { "no" });
                }
            }
            "pictureSize" => {
                let mode = value.and_then(Value::as_str).unwrap_or("normal");
                p.state.borrow_mut().picture = mode.to_owned();
                p.apply_picture();
            }
            "userAgent" => {
                let ua = value
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                p.state.borrow_mut().user_agent = ua.unwrap_or(DEFAULT_UA).to_owned();
                p.apply_http();
            }
            _ => {}
        }
    }
}

/// Events to send once the state is no longer borrowed (an app handler
/// may set props right away).
type Out = Vec<(&'static str, Value)>;

impl State {
    fn state(&mut self, out: &mut Out, name: &'static str) {
        if self.last_state != name {
            self.last_state = name;
            out.push(("state", json!({ "state": name })));
        }
    }
}

impl Inner {
    fn emit(&self, out: Out) {
        for (name, data) in out {
            self.ctx.emit(name, data);
        }
    }

    fn set_source(&self, value: Option<&Value>) {
        let source = value.filter(|v| v.is_object());
        let url = source
            .and_then(|s| s["url"].as_str())
            .map(str::to_owned)
            .filter(|u| !u.is_empty());
        let token = source.and_then(|s| s["token"].as_i64()).unwrap_or(0);
        let headers = source
            .and_then(|s| s["headers"].as_object())
            .map(|h| {
                h.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let changed = {
            let mut st = self.state.borrow_mut();
            let changed = url != st.url || token != st.token;
            st.url = url;
            st.token = token;
            st.headers = headers;
            changed
        };
        self.apply_http();
        if changed {
            self.open();
        }
    }

    /// Opens the current source (or stops), behind the black shutter.
    fn open(&self) {
        let mut out = Out::new();
        let url = {
            let mut st = self.state.borrow_mut();
            st.shutter = true;
            st.playing = false;
            st.buffering = false;
            st.clues = map::Clues::default();
            st.deferred = false;
            st.url.clone()
        };
        self.area.queue_render();
        match (url, &self.mpv) {
            (None, Some(mpv)) => {
                mpv.command(&["stop"]);
                self.state.borrow_mut().state(&mut out, "idle");
            }
            (None, None) => {}
            (Some(_), None) => fail(&self.state, &mut out, map::DECODER_INIT_FAILED, None),
            (Some(url), Some(_)) if !map::playable(&url) => {
                if let Some(mpv) = &self.mpv {
                    mpv.command(&["stop"]);
                }
                fail(&self.state, &mut out, map::UNSUPPORTED_SCHEME, None);
            }
            // mpv's video output needs the render context: load once realized.
            (Some(_), Some(_)) if self.render.get().is_null() => {
                self.state.borrow_mut().deferred = true;
            }
            (Some(url), Some(mpv)) => {
                if mpv.command(&["loadfile", &url, "replace"]) {
                    let mut st = self.state.borrow_mut();
                    st.pending_loads += 1;
                    st.state(&mut out, "buffering");
                } else {
                    fail(&self.state, &mut out, map::IO_UNSPECIFIED, None);
                }
            }
        }
        self.emit(out);
    }

    fn apply_http(&self) {
        let Some(mpv) = &self.mpv else { return };
        let http = {
            let st = self.state.borrow();
            map::http(&st.headers, &st.user_agent)
        };
        mpv.set("user-agent", &http.user_agent);
        mpv.command(&["change-list", "http-header-fields", "clr", ""]);
        for field in &http.fields {
            mpv.command(&["change-list", "http-header-fields", "append", field]);
        }
    }

    fn apply_picture(&self) {
        let Some(mpv) = &self.mpv else { return };
        let p = {
            let st = self.state.borrow();
            map::picture(&st.picture, st.aspect)
        };
        mpv.set("keepaspect", if p.keepaspect { "yes" } else { "no" });
        mpv.set("panscan", &format!("{:.4}", p.panscan));
        mpv.set("video-aspect-override", p.aspect);
        mpv.set("video-zoom", &format!("{:.6}", p.zoom));
    }

    fn realize(&self) {
        let Some(mpv) = &self.mpv else { return };
        self.area.make_current();
        if let Some(e) = self.area.error() {
            log::error!("iptv mpv: no GL context: {e}");
            return;
        }
        let Some(gl) = gl::Gl::load() else {
            log::error!("iptv mpv: GL functions not found");
            return;
        };
        self.gl.set(Some(gl));
        let mut init = ffi::mpv_opengl_init_params {
            get_proc_address: Some(gl::get_proc_address),
            get_proc_address_ctx: std::ptr::null_mut(),
        };
        let mut params = [
            param(
                ffi::MPV_RENDER_PARAM_API_TYPE,
                c"opengl".as_ptr().cast_mut(),
            ),
            param(ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &mut init),
            param(
                ffi::MPV_RENDER_PARAM_INVALID,
                std::ptr::null_mut::<c_void>(),
            ),
        ];
        let mut render = std::ptr::null_mut();
        // SAFETY: GTK's context is current; params end with INVALID.
        let r = unsafe { ffi::mpv_render_context_create(&mut render, mpv.0, params.as_mut_ptr()) };
        if r < 0 || render.is_null() {
            log::error!("iptv mpv: no render context ({r})");
            return;
        }
        let data = &*self.waker as *const Waker as *mut c_void;
        // SAFETY: the waker outlives the render context.
        unsafe { ffi::mpv_render_context_set_update_callback(render, Some(on_frame), data) };
        self.render.set(render);
        log::info!("iptv mpv: rendering through GTK's GL context");
        if self.state.borrow().deferred {
            self.open();
        }
    }

    fn unrealize(&self) {
        self.free_render();
        // The video output went with the context: reopen when realized again.
        let mut st = self.state.borrow_mut();
        st.deferred = st.url.is_some();
    }

    fn free_render(&self) {
        let render = self.render.replace(std::ptr::null_mut());
        if render.is_null() {
            return;
        }
        if self.area.is_realized() {
            self.area.make_current();
        }
        // SAFETY: ours, freed once, with its GL context current.
        unsafe {
            ffi::mpv_render_context_set_update_callback(render, None, std::ptr::null_mut());
            ffi::mpv_render_context_free(render);
        }
        self.gl.set(None);
    }

    fn draw(&self) {
        let Some(gl) = self.gl.get() else { return };
        let render = self.render.get();
        if render.is_null() || self.state.borrow().shutter {
            gl.black();
            return;
        }
        let scale = self.area.scale_factor();
        let mut fbo = ffi::mpv_opengl_fbo {
            fbo: gl.framebuffer(),
            w: self.area.width() * scale,
            h: self.area.height() * scale,
            internal_format: 0,
        };
        let (mut flip, mut block): (c_int, c_int) = (1, 0);
        let mut params = [
            param(ffi::MPV_RENDER_PARAM_OPENGL_FBO, &mut fbo),
            param(ffi::MPV_RENDER_PARAM_FLIP_Y, &mut flip),
            param(ffi::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &mut block),
            param(
                ffi::MPV_RENDER_PARAM_INVALID,
                std::ptr::null_mut::<c_void>(),
            ),
        ];
        // SAFETY: GTK made its context current for the render signal.
        unsafe { ffi::mpv_render_context_render(render, params.as_mut_ptr()) };
    }

    /// mpv's queued events (on the main loop).
    fn drain(&self) {
        self.waker.events.store(false, Ordering::Release);
        let Some(mpv) = &self.mpv else { return };
        let mut out = Out::new();
        loop {
            // SAFETY: a live handle; the event is valid until the next wait.
            let ev = unsafe { &*ffi::mpv_wait_event(mpv.0, 0.0) };
            match ev.event_id {
                ffi::MPV_EVENT_NONE => break,
                ffi::MPV_EVENT_LOG_MESSAGE => {
                    // SAFETY: this event's data.
                    let m = unsafe { &*(ev.data as *const ffi::mpv_event_log_message) };
                    self.log_message(m);
                }
                ffi::MPV_EVENT_START_FILE => {
                    // SAFETY: this event's data.
                    let s = unsafe { &*(ev.data as *const ffi::mpv_event_start_file) };
                    let mut st = self.state.borrow_mut();
                    st.pending_loads = st.pending_loads.saturating_sub(1);
                    st.entry = Some(s.playlist_entry_id);
                    st.clues = map::Clues::default();
                }
                ffi::MPV_EVENT_END_FILE => {
                    // SAFETY: this event's data.
                    let e = unsafe { &*(ev.data as *const ffi::mpv_event_end_file) };
                    self.end_file(e, &mut out);
                }
                ffi::MPV_EVENT_PLAYBACK_RESTART => self.restarted(mpv, &mut out),
                ffi::MPV_EVENT_PROPERTY_CHANGE => {
                    // SAFETY: this event's data.
                    let p = unsafe { &*(ev.data as *const ffi::mpv_event_property) };
                    self.property(ev.reply_userdata, p, &mut out);
                }
                _ => {}
            }
        }
        self.emit(out);
    }

    fn current(&self, st: &State) -> bool {
        st.pending_loads == 0 && st.url.is_some()
    }

    fn log_message(&self, m: &ffi::mpv_event_log_message) {
        if m.text.is_null() {
            return;
        }
        // SAFETY: mpv's NUL-terminated message text.
        let text = unsafe { CStr::from_ptr(m.text) }.to_string_lossy();
        // Output formats carry no addresses; everything else may.
        if text.starts_with("AO:") || text.starts_with("VO:") {
            log::info!("iptv mpv: {}", text.trim_end());
        }
        if m.log_level <= LOG_WARN {
            self.state.borrow_mut().clues.hear(&text);
        }
    }

    fn end_file(&self, e: &ffi::mpv_event_end_file, out: &mut Out) {
        let mut st = self.state.borrow_mut();
        if !self.current(&st) || st.entry != Some(e.playlist_entry_id) {
            return; // a file the viewer already left
        }
        st.playing = false;
        match e.reason {
            ffi::MPV_END_FILE_REASON_EOF => st.state(out, "ended"),
            ffi::MPV_END_FILE_REASON_ERROR => {
                let (code, http) = st.clues.error(e.error);
                drop(st);
                fail(&self.state, out, code, http);
                self.area.queue_render();
            }
            _ => {}
        }
    }

    fn restarted(&self, mpv: &Mpv, out: &mut Out) {
        let mut st = self.state.borrow_mut();
        if !self.current(&st) {
            return;
        }
        st.playing = true;
        st.shutter = false;
        if !st.buffering {
            st.state(out, "ready");
        }
        let ao = mpv.get(c"current-ao");
        if ao.is_some() && ao != st.audio_out {
            log::info!("iptv mpv: audio output {}", ao.as_deref().unwrap_or("?"));
            st.audio_out = ao;
        }
        drop(st);
        self.area.queue_render();
    }

    fn property(&self, id: u64, p: &ffi::mpv_event_property, out: &mut Out) {
        if p.data.is_null() {
            return; // unavailable (no file, no video)
        }
        let mut st = self.state.borrow_mut();
        match (id, p.format) {
            (1, ffi::MPV_FORMAT_FLAG) => {
                // SAFETY: a flag property's data is an int.
                st.buffering = unsafe { *(p.data as *const c_int) } != 0;
                if st.playing && self.current(&st) {
                    let name = if st.buffering { "buffering" } else { "ready" };
                    st.state(out, name);
                }
            }
            (2 | 3, ffi::MPV_FORMAT_INT64) => {
                // SAFETY: an int64 property's data.
                let v = unsafe { *(p.data as *const i64) };
                if id == 2 {
                    st.width = v;
                } else {
                    st.height = v;
                }
                let size = (st.width, st.height);
                if size.0 > 0 && size.1 > 0 && size != st.reported {
                    st.reported = size;
                    out.push(("size", json!({ "width": size.0, "height": size.1 })));
                }
            }
            (4, ffi::MPV_FORMAT_DOUBLE) => {
                // SAFETY: a double property's data.
                let a = unsafe { *(p.data as *const f64) };
                if st.aspect != Some(a) {
                    st.aspect = Some(a);
                    let cinema = st.picture == "cinema";
                    drop(st);
                    if cinema {
                        self.apply_picture();
                    }
                }
            }
            _ => {}
        }
    }
}

/// The current file failed: black picture, `error`, then `idle`.
fn fail(state: &RefCell<State>, out: &mut Out, code: &'static str, http: Option<u16>) {
    let mut st = state.borrow_mut();
    st.shutter = true;
    st.playing = false;
    let mut data = json!({ "code": code });
    if let Some(status) = http {
        data["http"] = json!(status);
    }
    out.push(("error", data));
    st.state(out, "idle");
}

fn param<T>(type_: c_int, data: *mut T) -> ffi::mpv_render_param {
    ffi::mpv_render_param {
        type_,
        data: data.cast(),
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        PLAYERS.with(|p| p.borrow_mut().remove(&self.id));
        if let Some(mpv) = &self.mpv {
            // SAFETY: a live handle; no callback runs after this returns.
            unsafe { ffi::mpv_set_wakeup_callback(mpv.0, None, std::ptr::null_mut()) };
        }
        // The render context goes before the handle; the waker after both.
        self.free_render();
        self.mpv.take();
    }
}
