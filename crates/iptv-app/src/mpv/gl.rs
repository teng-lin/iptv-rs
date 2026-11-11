//! OpenGL entry points for mpv's renderer and the few calls the player
//! makes itself. On Linux they are resolved in GTK's GL context: through
//! EGL when GTK uses it (Wayland, and X11 by default), else GLX, else the
//! process's symbols. On macOS they are OpenGL.framework's, on iOS
//! OpenGLES.framework's.

use std::ffi::{CStr, c_char, c_void};
#[cfg(target_os = "linux")]
use std::sync::OnceLock;

const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
#[cfg(target_os = "macos")]
const GL_VIEWPORT: u32 = 0x0BA2;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

#[cfg(target_os = "linux")]
type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *mut c_void;
#[cfg(target_os = "linux")]
type EglGetCurrentContext = unsafe extern "C" fn() -> *mut c_void;

/// The loaders found in the process (libEGL, libGL's GLX).
#[cfg(target_os = "linux")]
struct Loaders {
    egl_proc: Option<EglGetProcAddress>,
    egl_current: Option<EglGetCurrentContext>,
    glx_proc: Option<EglGetProcAddress>,
}

#[cfg(target_os = "linux")]
fn sym(lib: &CStr, name: &CStr) -> *mut c_void {
    // SAFETY: dlopen/dlsym with NUL-terminated names; a library GTK already
    // loaded is only referenced again (never closed).
    unsafe {
        let handle = libc::dlopen(lib.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        libc::dlsym(handle, name.as_ptr())
    }
}

#[cfg(target_os = "linux")]
fn loaders() -> &'static Loaders {
    static LOADERS: OnceLock<Loaders> = OnceLock::new();
    LOADERS.get_or_init(|| {
        let egl_proc = sym(c"libEGL.so.1", c"eglGetProcAddress");
        let egl_current = sym(c"libEGL.so.1", c"eglGetCurrentContext");
        let glx_proc = sym(c"libGL.so.1", c"glXGetProcAddressARB");
        // SAFETY: the symbols have these C signatures.
        unsafe {
            Loaders {
                egl_proc: (!egl_proc.is_null())
                    .then(|| std::mem::transmute::<*mut c_void, EglGetProcAddress>(egl_proc)),
                egl_current: (!egl_current.is_null())
                    .then(|| std::mem::transmute::<*mut c_void, EglGetCurrentContext>(egl_current)),
                glx_proc: (!glx_proc.is_null())
                    .then(|| std::mem::transmute::<*mut c_void, EglGetProcAddress>(glx_proc)),
            }
        }
    })
}

/// `name`'s address in the current GL context (null if unknown).
#[cfg(target_os = "linux")]
fn address(name: &CStr) -> *mut c_void {
    let l = loaders();
    // SAFETY: loader functions called with a NUL-terminated name.
    unsafe {
        let egl = l.egl_current.is_some_and(|f| !f().is_null());
        if egl && let Some(f) = l.egl_proc {
            let p = f(name.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
        if !egl && let Some(f) = l.glx_proc {
            let p = f(name.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
        libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr())
    }
}

/// `name`'s address in OpenGL.framework (null if unknown): macOS has one
/// OpenGL implementation, whatever the context.
#[cfg(target_os = "macos")]
fn address(name: &CStr) -> *mut c_void {
    static FRAMEWORK: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let handle = *FRAMEWORK.get_or_init(|| {
        // SAFETY: dlopen with a NUL-terminated path; never closed.
        unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/OpenGL.framework/OpenGL".as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_LOCAL,
            ) as usize
        }
    });
    if handle == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: a live handle and a NUL-terminated name.
    unsafe { libc::dlsym(handle as *mut c_void, name.as_ptr()) }
}

/// `name`'s address in OpenGLES.framework (null if unknown).
#[cfg(target_os = "ios")]
fn address(name: &CStr) -> *mut c_void {
    super::ios::address(name)
}

/// mpv's `get_proc_address`.
pub unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: mpv passes a NUL-terminated function name.
    address(unsafe { CStr::from_ptr(name) })
}

/// The GL calls the player makes around mpv's rendering.
#[derive(Clone, Copy)]
pub struct Gl {
    get_integerv: unsafe extern "C" fn(u32, *mut i32),
    clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    clear: unsafe extern "C" fn(u32),
}

impl Gl {
    /// Resolves them in the current context; `None` if one is missing.
    pub fn load() -> Option<Gl> {
        let (a, b, c) = (
            address(c"glGetIntegerv"),
            address(c"glClearColor"),
            address(c"glClear"),
        );
        if a.is_null() || b.is_null() || c.is_null() {
            return None;
        }
        // SAFETY: GL's documented signatures.
        unsafe {
            Some(Gl {
                get_integerv: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32, *mut i32)>(
                    a,
                ),
                clear_color: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(f32, f32, f32, f32),
                >(b),
                clear: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32)>(c),
            })
        }
    }

    /// The framebuffer bound for drawing (GTK's for the area's render,
    /// CA's for the layer's).
    pub fn framebuffer(&self) -> i32 {
        let mut fbo = 0;
        // SAFETY: called with GTK's context current, into a valid i32.
        unsafe { (self.get_integerv)(GL_FRAMEBUFFER_BINDING, &mut fbo) };
        fbo
    }

    /// The viewport's size in pixels: the size of the framebuffer a
    /// `CAOpenGLLayer` draws into.
    #[cfg(target_os = "macos")]
    pub fn viewport(&self) -> (i32, i32) {
        let mut v = [0_i32; 4];
        // SAFETY: called with a context current, into room for 4 values.
        unsafe { (self.get_integerv)(GL_VIEWPORT, v.as_mut_ptr()) };
        (v[2], v[3])
    }

    /// Fills the bound framebuffer with black.
    pub fn black(&self) {
        // SAFETY: called with GTK's context current.
        unsafe {
            (self.clear_color)(0.0, 0.0, 0.0, 1.0);
            (self.clear)(GL_COLOR_BUFFER_BIT);
        }
    }
}
