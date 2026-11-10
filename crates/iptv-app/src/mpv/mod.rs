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
//!   ([`map::Clues::error`]), never addresses or messages, and `position`
//!   {ms} once a second while playing (from where the source opened).
//!
//! The picture is black from a new source or an error until the stream
//! plays again. mpv sends a stream's headers with all of its requests
//! ([`map::http`]). `IPTV_RS_AUDIO` names mpv's audio output (`null`
//! mutes, for tests); `IPTV_RS_HWDEC` overrides `hwdec`.
//!
//! GTK's main thread renders; every other mpv call runs on a control
//! thread ([`worker`]), as mpv's render API requires.

mod ffi;
mod gl;
mod handle;
mod map;
mod worker;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nsh_platform_linux::gtk4::{self as gtk, glib, prelude::*};
use nsh_platform_linux::{ComponentContext, Components, NativeComponent};
use serde_json::{Value, json};

use handle::Mpv;
use worker::{Cmd, Note, Worker};

pub const NAME: &str = "tv.player";
pub(crate) const DEFAULT_UA: &str = "iptv-rs/0.1 (mpv)";

/// Registers `tv.player`.
pub fn register(components: &Components) {
    components.register(
        NAME,
        Rc::new(|ctx| Box::new(Player::new(ctx)) as Box<dyn NativeComponent>),
    );
}

thread_local! {
    /// The players by id, for mpv's threads (which reach the main loop with
    /// only an id).
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

unsafe extern "C" fn on_frame(data: *mut c_void) {
    // SAFETY: `data` is the player's `Waker`, alive until the render context
    // is freed.
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
    /// The playlist entry of the source last loaded (`None` while a load is
    /// on its way): other files' events (one the viewer left, or a load mpv
    /// dropped for a newer one) are ignored.
    expected: Option<i64>,
    /// Loads queued and not yet answered ([`Note::Loading`]).
    loads: u32,
    /// The playlist entry mpv started last.
    entry: Option<i64>,
    /// The current file started playing (later buffering is a rebuffer).
    playing: bool,
    buffering: bool,
    last_state: &'static str,
    clues: map::Clues,
    /// A source that waits for the GL context.
    deferred: bool,
    audio_out: Option<String>,
    /// The `paused` prop.
    paused: bool,
    /// mpv's error for the current file, reported after the log is read.
    failed: Option<i32>,
}

struct Inner {
    id: u64,
    area: gtk::GLArea,
    ctx: ComponentContext,
    mpv: Option<Arc<Mpv>>,
    worker: Option<Worker>,
    render: Cell<*mut ffi::mpv_render_context>,
    gl: Cell<Option<gl::Gl>>,
    state: RefCell<State>,
    waker: Arc<Waker>,
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
        let mpv = Mpv::new().map(Arc::new);
        if mpv.is_none() {
            log::error!("iptv mpv: no player (is libmpv installed?)");
        }
        let waker = Arc::new(Waker {
            id,
            events: AtomicBool::new(false),
            frame: AtomicBool::new(false),
        });
        let worker = mpv.as_ref().map(|mpv| {
            let waker = waker.clone();
            Worker::spawn(mpv.clone(), move || {
                wake(&waker.events, waker.id, Inner::drain)
            })
        });
        let inner = Rc::new(Inner {
            id,
            area: area.clone(),
            ctx,
            mpv,
            worker,
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
                expected: None,
                loads: 0,
                entry: None,
                playing: false,
                buffering: false,
                last_state: "idle",
                clues: map::Clues::default(),
                deferred: false,
                audio_out: None,
                paused: false,
                failed: None,
            }),
            waker,
        });
        PLAYERS.with(|p| p.borrow_mut().insert(id, Rc::downgrade(&inner)));
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
        // `position` once a second while playing (catch-up seeking).
        let weak = Rc::downgrade(&inner);
        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let Some(p) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            p.tick();
            glib::ControlFlow::Continue
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
                p.state.borrow_mut().paused = paused;
                p.send(Cmd::Set("pause", if paused { "yes" } else { "no" }.into()));
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
    fn send(&self, cmd: Cmd) {
        if let Some(w) = &self.worker {
            w.send(cmd);
        }
    }

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
            st.failed = None;
            st.expected = None;
            st.deferred = false;
            st.url.clone()
        };
        self.area.queue_render();
        match (url, &self.mpv) {
            (None, Some(_)) => {
                self.send(Cmd::Stop);
                self.state.borrow_mut().state(&mut out, "idle");
            }
            (None, None) => {}
            (Some(_), None) => fail(&self.state, &mut out, map::DECODER_INIT_FAILED, None),
            (Some(url), Some(_)) if !map::playable(&url) => {
                self.send(Cmd::Stop);
                fail(&self.state, &mut out, map::UNSUPPORTED_SCHEME, None);
            }
            // mpv's video output needs the render context: load once realized.
            (Some(_), Some(_)) if self.render.get().is_null() => {
                self.state.borrow_mut().deferred = true;
            }
            (Some(url), Some(_)) => {
                let mut st = self.state.borrow_mut();
                st.loads += 1;
                st.state(&mut out, "buffering");
                drop(st);
                self.send(Cmd::Load(url));
            }
        }
        self.emit(out);
    }

    fn apply_http(&self) {
        let http = {
            let st = self.state.borrow();
            map::http(&st.headers, &st.user_agent)
        };
        self.send(Cmd::Http {
            user_agent: http.user_agent,
            fields: http.fields,
        });
    }

    fn apply_picture(&self) {
        let p = {
            let st = self.state.borrow();
            map::picture(&st.picture, st.aspect)
        };
        let keep = if p.keepaspect { "yes" } else { "no" };
        self.send(Cmd::Set("keepaspect", keep.into()));
        self.send(Cmd::Set("panscan", format!("{:.4}", p.panscan)));
        self.send(Cmd::Set("video-aspect-override", p.aspect.into()));
        self.send(Cmd::Set("video-zoom", format!("{:.6}", p.zoom)));
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
        let data = Arc::as_ptr(&self.waker) as *mut c_void;
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

    /// What the control thread reported (on the main loop).
    fn drain(&self) {
        self.waker.events.store(false, Ordering::Release);
        let Some(worker) = &self.worker else { return };
        let mut out = Out::new();
        while let Ok(note) = worker.notes.try_recv() {
            self.note(note, &mut out);
        }
        let failed = self.state.borrow_mut().failed.take();
        if let Some(error) = failed {
            let (code, http) = self.state.borrow().clues.error(error);
            fail(&self.state, &mut out, code, http);
            self.area.queue_render();
        }
        self.emit(out);
    }

    fn note(&self, note: Note, out: &mut Out) {
        match note {
            Note::Loading(id) => {
                let mut st = self.state.borrow_mut();
                st.loads = st.loads.saturating_sub(1);
                if st.loads > 0 {
                    return; // a newer load is on its way
                }
                log::debug!("iptv mpv: loading entry {id:?}");
                st.expected = id;
                if id.is_none() && st.url.is_some() {
                    drop(st);
                    fail(&self.state, out, map::IO_UNSPECIFIED, None);
                }
            }
            Note::Started(entry) => {
                log::debug!("iptv mpv: started entry {entry}");
                self.state.borrow_mut().entry = Some(entry);
            }
            Note::Ended {
                entry,
                reason,
                error,
            } => self.end_file(entry, reason, error, out),
            Note::Restarted { audio_out } => self.restarted(audio_out, out),
            Note::Buffering(on) => {
                let mut st = self.state.borrow_mut();
                st.buffering = on;
                if st.playing && self.current(&st) {
                    st.state(out, if on { "buffering" } else { "ready" });
                }
            }
            Note::Width(v) | Note::Height(v) => {
                let mut st = self.state.borrow_mut();
                if matches!(note, Note::Width(_)) {
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
            Note::Aspect(a) => {
                let mut st = self.state.borrow_mut();
                if st.aspect != Some(a) {
                    st.aspect = Some(a);
                    let cinema = st.picture == "cinema";
                    drop(st);
                    if cinema {
                        self.apply_picture();
                    }
                }
            }
            Note::Log { warn, text } => {
                // Output formats carry no addresses; everything else may.
                if text.starts_with("AO:") || text.starts_with("VO:") {
                    log::info!("iptv mpv: {}", text.trim_end());
                }
                if warn {
                    self.state.borrow_mut().clues.hear(&text);
                }
            }
            Note::Position(seconds) => {
                if self.playing() {
                    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
                    log::debug!("iptv mpv: position {ms} ms");
                    out.push(("position", json!({ "ms": ms })));
                }
            }
        }
    }

    fn playing(&self) -> bool {
        let st = self.state.borrow();
        st.playing && !st.paused && !st.buffering && self.current(&st)
    }

    /// Asks for the position (ms from where the source opened: mpv rebases
    /// a stream's first timestamp to 0) while the current file plays.
    fn tick(&self) {
        if self.playing() {
            self.send(Cmd::Position);
        }
    }

    fn current(&self, st: &State) -> bool {
        st.url.is_some() && st.expected.is_some() && st.entry == st.expected
    }

    fn end_file(&self, entry: i64, reason: c_int, error: c_int, out: &mut Out) {
        log::debug!("iptv mpv: ended entry {entry} (reason {reason}, error {error})");
        let mut st = self.state.borrow_mut();
        if !self.current(&st) || st.entry != Some(entry) {
            return; // a file the viewer already left
        }
        st.playing = false;
        match reason {
            ffi::MPV_END_FILE_REASON_EOF => st.state(out, "ended"),
            // Reported once the queue is empty: mpv's log messages come
            // apart from its events and may follow this one.
            ffi::MPV_END_FILE_REASON_ERROR => st.failed = Some(error),
            _ => {}
        }
    }

    fn restarted(&self, ao: Option<String>, out: &mut Out) {
        let mut st = self.state.borrow_mut();
        if !self.current(&st) {
            return;
        }
        st.playing = true;
        st.shutter = false;
        if !st.buffering {
            st.state(out, "ready");
        }
        if ao.is_some() && ao != st.audio_out {
            log::info!("iptv mpv: audio output {}", ao.as_deref().unwrap_or("?"));
            st.audio_out = ao;
        }
        drop(st);
        self.area.queue_render();
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
        // The control thread stops first, then the render context goes, then
        // the handle (its last owner).
        self.worker.take();
        self.free_render();
        self.mpv.take();
    }
}
