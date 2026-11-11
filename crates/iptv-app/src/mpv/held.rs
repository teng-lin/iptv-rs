//! The player's last picture, kept to show while mpv has none of the
//! stream: from a live stream's drop or a reopen of the same stream until
//! the new file's first frame, and after a file ended. mpv's render API
//! draws black then (its video output starts over with every
//! `loadfile`), and a platform's view keeps nothing of its own: GTK's
//! `GtkGLArea` draws each frame into a texture of its pool, a
//! `CAOpenGLLayer` or `CAEAGLLayer` into a fresh framebuffer, so not
//! drawing would show stale or undefined pixels, never the last frame.
//!
//! So mpv draws every frame into a framebuffer of ours, which is then
//! copied into the view's (one blit per frame); while there is no new
//! frame, the copy is all that is drawn. One GL context may draw a player
//! at two sizes (its view and the picture-in-picture window's, `gtk_pip`),
//! so a framebuffer is kept per size, two at most; the latest is the one
//! shown. Without framebuffer blits (an old GL), mpv draws straight into
//! the view's framebuffer as before and nothing is held.

use std::ffi::c_void;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_RGBA8: u32 = 0x8058;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_LINEAR: u32 = 0x2601;

/// Sizes held at once: the player's view and the picture-in-picture
/// window's.
const SIZES: usize = 2;

type Gen = unsafe extern "C" fn(i32, *mut u32);
type Delete = unsafe extern "C" fn(i32, *const u32);
type Bind = unsafe extern "C" fn(u32, u32);
type Blit = unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32);

/// The GL calls for our framebuffers (GL 3.0, OpenGL ES 3.0).
#[derive(Clone, Copy)]
struct Fns {
    gen_framebuffers: Gen,
    delete_framebuffers: Delete,
    bind_framebuffer: Bind,
    gen_renderbuffers: Gen,
    delete_renderbuffers: Delete,
    bind_renderbuffer: Bind,
    renderbuffer_storage: unsafe extern "C" fn(u32, u32, i32, i32),
    framebuffer_renderbuffer: unsafe extern "C" fn(u32, u32, u32, u32),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    blit_framebuffer: Blit,
}

impl Fns {
    /// Resolves them in the current context; `None` if one is missing.
    fn load() -> Option<Fns> {
        let f = |name: &std::ffi::CStr| Some(super::gl::address(name)).filter(|p| !p.is_null());
        macro_rules! f {
            ($name:literal, $ty:ty) => {
                // SAFETY: GL's documented signature for `$name`.
                unsafe { std::mem::transmute::<*mut c_void, $ty>(f($name)?) }
            };
        }
        Some(Fns {
            gen_framebuffers: f!(c"glGenFramebuffers", Gen),
            delete_framebuffers: f!(c"glDeleteFramebuffers", Delete),
            bind_framebuffer: f!(c"glBindFramebuffer", Bind),
            gen_renderbuffers: f!(c"glGenRenderbuffers", Gen),
            delete_renderbuffers: f!(c"glDeleteRenderbuffers", Delete),
            bind_renderbuffer: f!(c"glBindRenderbuffer", Bind),
            renderbuffer_storage: f!(
                c"glRenderbufferStorage",
                unsafe extern "C" fn(u32, u32, i32, i32)
            ),
            framebuffer_renderbuffer: f!(
                c"glFramebufferRenderbuffer",
                unsafe extern "C" fn(u32, u32, u32, u32)
            ),
            check_framebuffer_status: f!(
                c"glCheckFramebufferStatus",
                unsafe extern "C" fn(u32) -> u32
            ),
            blit_framebuffer: f!(c"glBlitFramebuffer", Blit),
        })
    }
}

/// A framebuffer of ours, `w`×`h`.
struct Frame {
    w: i32,
    h: i32,
    fbo: u32,
    rb: u32,
}

/// The pictures held for one player, in its GL context.
#[derive(Default)]
pub struct Held {
    /// Resolved on first use, in the context (`Some(None)`: unsupported).
    fns: Option<Option<Fns>>,
    /// The latest first.
    frames: Vec<Frame>,
}

impl Held {
    /// The framebuffer mpv draws its next `w`×`h` frame into (the one held
    /// from then on), bound; `None` (with `view` bound again) to draw
    /// straight into the view's framebuffer `view`.
    pub fn target(&mut self, view: i32, w: i32, h: i32) -> Option<i32> {
        let f = self.fns()?;
        if w <= 0 || h <= 0 {
            return None;
        }
        if let Some(i) = self.frames.iter().position(|s| (s.w, s.h) == (w, h)) {
            let frame = self.frames.remove(i);
            let fbo = frame.fbo;
            self.frames.insert(0, frame);
            return Some(fbo as i32);
        }
        if self.frames.len() >= SIZES
            && let Some(old) = self.frames.pop()
        {
            delete(&f, &old);
        }
        let (mut fbo, mut rb) = (0, 0);
        // SAFETY: the context is current; ids into valid u32s.
        let complete = unsafe {
            (f.gen_framebuffers)(1, &mut fbo);
            (f.gen_renderbuffers)(1, &mut rb);
            (f.bind_renderbuffer)(GL_RENDERBUFFER, rb);
            (f.renderbuffer_storage)(GL_RENDERBUFFER, GL_RGBA8, w, h);
            (f.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (f.framebuffer_renderbuffer)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, rb);
            (f.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE
        };
        let frame = Frame { w, h, fbo, rb };
        if !complete {
            log::warn!("iptv mpv: no framebuffer of {w}x{h} to hold the picture in");
            delete(&f, &frame);
            // SAFETY: the view's framebuffer, as it was bound.
            unsafe { (f.bind_framebuffer)(GL_FRAMEBUFFER, view as u32) };
            return None;
        }
        self.frames.insert(0, frame);
        Some(fbo as i32)
    }

    /// Copies the latest picture held into the view's framebuffer `view`,
    /// `w`×`h` (scaled if it was drawn at another size), and leaves `view`
    /// bound; whether there was one.
    pub fn show(&self, view: i32, w: i32, h: i32) -> bool {
        let (Some(Some(f)), Some(frame)) = (self.fns, self.frames.first()) else {
            return false;
        };
        // SAFETY: the context is current; both framebuffers are complete.
        unsafe {
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, frame.fbo);
            (f.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, view as u32);
            (f.blit_framebuffer)(
                0,
                0,
                frame.w,
                frame.h,
                0,
                0,
                w,
                h,
                GL_COLOR_BUFFER_BIT,
                GL_LINEAR,
            );
            (f.bind_framebuffer)(GL_FRAMEBUFFER, view as u32);
        }
        true
    }

    /// Lets go of the pictures (their GL context is current: it is going).
    pub fn clear(&mut self) {
        if let Some(Some(f)) = self.fns {
            for frame in self.frames.drain(..) {
                delete(&f, &frame);
            }
        }
        self.frames.clear();
        // The next context resolves them again.
        self.fns = None;
    }

    fn fns(&mut self) -> Option<Fns> {
        *self.fns.get_or_insert_with(|| {
            let fns = Fns::load();
            if fns.is_none() {
                log::warn!("iptv mpv: no framebuffer blits: the last picture is not held");
            }
            fns
        })
    }
}

fn delete(f: &Fns, frame: &Frame) {
    // SAFETY: ours, in the current context.
    unsafe {
        (f.delete_framebuffers)(1, &frame.fbo);
        (f.delete_renderbuffers)(1, &frame.rb);
    }
}
