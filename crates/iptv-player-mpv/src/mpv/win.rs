//! Windows composition: libmpv's software render API supplies BGRA frames
//! drawn as ordinary Direct2D content. No child HWND can cover the guide.
//! Each player retains its render context across layout changes; PiP reads
//! the same player's frame, never opening another stream.
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Inner, NAME, ffi, find, on_frame, param, pip};
pub use nsh_platform_windows::ComponentContext;
use nsh_platform_windows::{Components, NativeComponent, windows};
use serde_json::Value;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct2D::{
            Common::{
                D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
            },
            D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
            ID2D1DeviceContext,
        },
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    },
    UI::WindowsAndMessaging::SetTimer,
};

// mpv/render.h: the stable software render parameters.
const SW_SIZE: i32 = 17;
const SW_FORMAT: i32 = 18;
const SW_STRIDE: i32 = 19;
const SW_POINTER: i32 = 20;
type Job = Box<dyn FnOnce() + Send>;
static QUEUE: Mutex<Vec<Job>> = Mutex::new(Vec::new());
thread_local! {
    static TIMER: Cell<usize> = const { Cell::new(0) };
    static PIP: RefCell<Option<ComponentContext>> = const { RefCell::new(None) };
}

pub fn register(components: &Components) {
    TIMER.with(|timer| {
        if timer.get() == 0 {
            // SAFETY: timer is owned by this UI thread; callback has the Win32 ABI.
            let id = unsafe { SetTimer(None, 0, 16, Some(pump)) };
            assert_ne!(id, 0, "Could not start the player UI timer");
            timer.set(id);
        }
    });
    components.register(
        NAME,
        Rc::new(|ctx| {
            let redraw = ctx.clone();
            let inner = Inner::new(ctx, |id| Video::new(id, redraw));
            inner.create_render();
            Box::new(Player(inner))
        }),
    );
    components.register(
        pip::VIEW,
        Rc::new(|ctx| {
            PIP.with(|p| *p.borrow_mut() = Some(ctx));
            Box::new(Pip)
        }),
    );
}

pub(super) fn on_main(f: impl FnOnce() + Send + 'static) {
    QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Box::new(f));
}

/// The UI timer: runs the jobs queued for the main thread, then ticks the
/// players. Windows calls it from its message dispatch, and a panic must
/// not unwind out of an `extern "system"` function (the process aborts):
/// a job or a round of ticks that panics is logged and dropped, and the
/// next ones run.
unsafe extern "system" fn pump(_: HWND, _: u32, _: usize, _: u32) {
    let jobs = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
    for job in jobs {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
            log::error!("iptv mpv: a main-thread job panicked; the player timer goes on");
        }
    }
    let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let players: Vec<_> =
            super::PLAYERS.with(|p| p.borrow().values().filter_map(|p| p.upgrade()).collect());
        for player in players {
            if player.video.tick.get().elapsed() >= Duration::from_secs(1) {
                player.video.tick.set(Instant::now());
                player.tick();
            }
        }
    }));
    if ticked.is_err() {
        log::error!("iptv mpv: a player tick panicked; the player timer goes on");
    }
}

pub(super) fn every_second(_: u64, _: fn(&Inner)) {
    // The UI timer ticks every live player. No per-player timers outlive it.
}

pub(super) fn pip_sync() {
    PIP.with(|p| {
        if let Some(ctx) = p.borrow().as_ref() {
            ctx.redraw();
        }
    });
}

pub(super) struct Video {
    id: u64,
    ctx: ComponentContext,
    tick: Cell<Instant>,
    dirty: Cell<bool>,
    frame: RefCell<Frame>,
}
#[derive(Default)]
struct Frame {
    pixels: Vec<PixelBlock>,
    width: i32,
    height: i32,
}
// libmpv requests 64-byte aligned rows and pointers for SIMD scaling.
#[repr(C, align(64))]
#[derive(Clone)]
struct PixelBlock([u8; 64]);
impl Video {
    fn new(id: u64, ctx: ComponentContext) -> Self {
        Self {
            id,
            ctx,
            tick: Cell::new(Instant::now()),
            dirty: Cell::new(true),
            frame: RefCell::default(),
        }
    }
    pub(super) fn redraw(&self) {
        self.dirty.set(true);
        self.ctx.redraw();
        if pip::wanted() == Some(self.id) {
            pip_sync();
        }
    }
    pub(super) fn make_current(&self) {} // Software contexts have no thread-local GPU state.
}

impl Inner {
    fn create_render(&self) -> bool {
        let Some(mpv) = &self.mpv else { return false };
        let mut params = [
            param(ffi::MPV_RENDER_PARAM_API_TYPE, c"sw".as_ptr().cast_mut()),
            param(
                ffi::MPV_RENDER_PARAM_INVALID,
                std::ptr::null_mut::<c_void>(),
            ),
        ];
        let mut render = std::ptr::null_mut();
        // SAFETY: live mpv handle and a terminated parameter array.
        let result =
            unsafe { ffi::mpv_render_context_create(&mut render, mpv.0, params.as_mut_ptr()) };
        if result < 0 || render.is_null() {
            log::error!("iptv mpv: no software render context ({result})");
            return false;
        }
        self.render.set(render);
        // SAFETY: waker stays alive until free_render removes this callback.
        unsafe {
            ffi::mpv_render_context_set_update_callback(
                render,
                Some(on_frame),
                Arc::as_ptr(&self.waker) as *mut c_void,
            )
        };
        true
    }

    fn draw_windows(&self, dc: &ID2D1DeviceContext, size: (f32, f32), pip_view: bool) {
        let Some((width, height)) = output_size(size, pip_view, pip::wanted() == Some(self.id))
        else {
            return;
        };
        // The shutter stays opaque while tuning, stopped, or without a frame.
        // Drawing nothing here would expose the themed panels below the player.
        // SAFETY: the live draw context and finite component bounds belong to this UI thread.
        unsafe {
            if let Ok(black) = dc.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                None,
            ) {
                dc.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: size.0,
                        bottom: size.1,
                    },
                    &black,
                );
            }
        }
        let (shutter, playing) = {
            let st = self.state.borrow();
            (st.shutter, st.playing)
        };
        if shutter || self.render.get().is_null() {
            return;
        }
        let mut frame = self.video.frame.borrow_mut();
        let changed = (frame.width, frame.height) != (width, height);
        if playing && (self.video.dirty.replace(false) || changed) {
            frame.width = width;
            frame.height = height;
            // 64-byte aligned stride is accepted by the software renderer.
            let mut stride = (width as usize * 4 + 63) & !63;
            frame
                .pixels
                .resize(stride * height as usize / 64, PixelBlock([0; 64]));
            let mut dimensions = [width, height];
            let mut block = 0i32;
            let mut params = [
                param(SW_SIZE, dimensions.as_mut_ptr()),
                param(SW_FORMAT, c"bgr0".as_ptr().cast_mut()),
                param(SW_STRIDE, &mut stride),
                param(SW_POINTER, frame.pixels.as_mut_ptr()),
                param(ffi::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &mut block),
                param(
                    ffi::MPV_RENDER_PARAM_INVALID,
                    std::ptr::null_mut::<c_void>(),
                ),
            ];
            // SAFETY: buffer covers stride * height bytes; renderer is used only on this UI thread.
            unsafe { ffi::mpv_render_context_render(self.render.get(), params.as_mut_ptr()) };
        }
        if frame.pixels.is_empty() {
            return;
        }
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            ..Default::default()
        };
        // SAFETY: pixels live through the synchronous upload; dc is NSH's current draw context.
        unsafe {
            if let Ok(bitmap) = dc.CreateBitmap(
                D2D_SIZE_U {
                    width: frame.width as u32,
                    height: frame.height as u32,
                },
                Some(frame.pixels.as_ptr().cast()),
                (frame.width as u32 * 4 + 63) & !63,
                &properties,
            ) {
                let rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: size.0,
                    bottom: size.1,
                };
                dc.DrawBitmap(
                    &bitmap,
                    Some(&rect),
                    1.0,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    None,
                    None,
                );
            }
        }
    }
}

// Only the active view chooses the render target. Reusing the main window's
// old letterboxed frame in a differently shaped PiP window would stretch it.
fn output_size(size: (f32, f32), pip_view: bool, in_pip: bool) -> Option<(i32, i32)> {
    if pip_view != in_pip
        || !size.0.is_finite()
        || !size.1.is_finite()
        || size.0 <= 0.0
        || size.1 <= 0.0
    {
        return None;
    }
    let scale = (1920.0 / size.0).min(1080.0 / size.1).min(1.0);
    Some((
        (size.0 * scale).round().max(1.0) as i32,
        (size.1 * scale).round().max(1.0) as i32,
    ))
}

struct Player(Rc<Inner>);
impl NativeComponent for Player {
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        self.0.set_prop(name, value);
    }
    fn draw(&self, dc: &ID2D1DeviceContext, size: (f32, f32)) {
        self.0.draw_windows(dc, size, false);
    }
}
struct Pip;
impl NativeComponent for Pip {
    fn set_prop(&mut self, _: &str, _: Option<&Value>) {}
    fn draw(&self, dc: &ID2D1DeviceContext, size: (f32, f32)) {
        if let Some(player) = pip::wanted().and_then(find) {
            player.draw_windows(dc, size, true);
        }
    }
}
impl Drop for Pip {
    fn drop(&mut self) {
        PIP.with(|p| p.borrow_mut().take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_and_resizing_pip_uses_the_active_surfaces_aspect_ratio() {
        assert_eq!(
            output_size((1280.0, 720.0), false, false),
            Some((1280, 720))
        );
        assert_eq!(output_size((1280.0, 720.0), false, true), None);
        assert_eq!(output_size((600.0, 450.0), true, true), Some((600, 450)));
        assert_eq!(output_size((800.0, 450.0), true, true), Some((800, 450)));
        assert_eq!(
            output_size((1280.0, 720.0), false, false),
            Some((1280, 720))
        );
        assert_eq!(output_size((600.0, 450.0), true, false), None);
        assert_eq!(
            output_size((3840.0, 2160.0), false, false),
            Some((1920, 1080))
        );
    }

    #[test]
    fn software_renderer_respects_aligned_rows_and_buffer_bounds() {
        let mpv = super::super::handle::Mpv::new().expect("libmpv initializes");
        let mut render = std::ptr::null_mut();
        let mut create = [
            param(ffi::MPV_RENDER_PARAM_API_TYPE, c"sw".as_ptr().cast_mut()),
            param(
                ffi::MPV_RENDER_PARAM_INVALID,
                std::ptr::null_mut::<c_void>(),
            ),
        ];
        // SAFETY: live handle, terminated array, writable context output.
        let created =
            unsafe { ffi::mpv_render_context_create(&mut render, mpv.0, create.as_mut_ptr()) };
        assert_eq!(created, 0);
        let mut blocks = vec![PixelBlock([0xa5; 64]); 5];
        let pointer = blocks[1].0.as_mut_ptr();
        assert_eq!(pointer as usize % 64, 0);
        let mut size = [7i32, 3];
        let mut stride = 64usize;
        let mut block = 0i32;
        let mut params = [
            param(SW_SIZE, size.as_mut_ptr()),
            param(SW_FORMAT, c"bgr0".as_ptr().cast_mut()),
            param(SW_STRIDE, &mut stride),
            param(SW_POINTER, pointer),
            param(ffi::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &mut block),
            param(
                ffi::MPV_RENDER_PARAM_INVALID,
                std::ptr::null_mut::<c_void>(),
            ),
        ];
        // SAFETY: three aligned rows are writable, with guard blocks on each end.
        let result = unsafe { ffi::mpv_render_context_render(render, params.as_mut_ptr()) };
        // SAFETY: free the render context before the mpv handle is dropped.
        unsafe { ffi::mpv_render_context_free(render) };
        assert_eq!(result, 0);
        assert_eq!(blocks[0].0, [0xa5; 64]);
        assert_eq!(blocks[4].0, [0xa5; 64]);
        assert!(
            blocks[1].0[..28]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [0, 0, 0])
        );
    }
}
