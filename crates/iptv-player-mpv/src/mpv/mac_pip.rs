//! macOS: the picture-in-picture window's `tv.pip` (`pip`). The player's
//! view (layer-hosting, its `CAOpenGLLayer` drawing with the CGL context
//! the player owns, `mac`) sits in a frame view of its own, the one NSH
//! hosts and lays out. While the window wants the player, that view moves
//! from its frame into the window's (`tv.pip`'s) frame, and back after:
//! AppKit moves views between windows as they are, the layer asks the
//! player for its context again if it asks at all (`copyCGLContextFor…`
//! hands out the same one), so mpv's render context, made in it, goes on
//! drawing; nothing reopens. Both frames let the pointer through.
//!
//! The window keeps the stream's shape when the user resizes it
//! (`contentAspectRatio`).

use std::cell::RefCell;
use std::rc::Rc;

use nsh_platform_macos::{Components, NativeComponent};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSResponder, NSView};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use serde_json::Value;

use super::find;

define_class!(
    /// A view that holds the player's view: the pointer passes through
    /// to NSH's view under it, as through the player's own.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "IptvMpvFrame"]
    pub(super) struct Frame;

    unsafe impl NSObjectProtocol for Frame {}

    impl Frame {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl Frame {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Frame> {
        let this = Frame::alloc(mtm).set_ivars(());
        // SAFETY: NSView's designated initializer.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    /// Puts `view` in this frame, filling it as it resizes.
    pub(super) fn hold(&self, view: &NSView) {
        view.removeFromSuperview();
        view.setFrame(self.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        self.addSubview(view);
    }
}

thread_local! {
    /// The window's frame (one window), and the player whose view it holds.
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

struct Host {
    frame: Retained<Frame>,
    /// The player shown, and its view.
    shown: Option<(u64, Retained<NSView>)>,
    /// The window's aspect is set.
    shaped: bool,
}

/// Registers `tv.pip`.
pub(super) fn register(components: &Components) {
    components.register(
        super::pip::VIEW,
        Rc::new(|_| Box::new(PipView::new()) as Box<dyn NativeComponent>),
    );
}

/// The component NSH holds.
struct PipView(Retained<Frame>);

impl PipView {
    fn new() -> PipView {
        let mtm = MainThreadMarker::new().expect("NSH makes components on the main thread");
        let frame = Frame::new(mtm);
        HOST.with(|h| {
            *h.borrow_mut() = Some(Host {
                frame: frame.clone(),
                shown: None,
                shaped: false,
            })
        });
        sync();
        PipView(frame)
    }
}

impl Drop for PipView {
    fn drop(&mut self) {
        let host = HOST.with(|h| h.borrow_mut().take());
        if let Some(Host {
            shown: Some((id, view)),
            ..
        }) = host
        {
            give_back(id, &view);
        }
    }
}

impl NativeComponent for PipView {
    fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.0.clone())
    }

    fn set_prop(&mut self, _name: &str, _value: Option<&Value>) {}
}

/// Player `id`'s view goes back into its own frame (or out of the window
/// when the player is gone).
fn give_back(id: u64, view: &NSView) {
    match find(id) {
        Some(p) => p.video.frame().hold(view),
        None => view.removeFromSuperview(),
    }
    view.setNeedsDisplay(true);
}

/// Moves the wanted player's view into the window's frame, or back.
pub(super) fn sync() {
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        let Some(host) = h.as_mut() else { return };
        let wanted = super::pip::wanted().and_then(|id| find(id).map(|p| (id, p)));
        if host.shown.as_ref().map(|(id, _)| *id) == wanted.as_ref().map(|(id, _)| *id) {
            return;
        }
        if let Some((id, view)) = host.shown.take() {
            give_back(id, &view);
        }
        if let Some((id, p)) = wanted {
            let view = p.video.view();
            host.frame.hold(&view);
            view.setNeedsDisplay(true);
            p.video.redraw();
            host.shown = Some((id, view));
            host.shaped = false;
            log::info!("iptv mpv {id}: drawing in the picture-in-picture window");
        }
    });
}

/// Player `id` drew a frame: the window takes the stream's shape once it
/// is on screen and the shape is known.
pub(super) fn drew(id: u64, width: i64, height: i64) {
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        let Some(host) = h.as_mut() else { return };
        if host.shaped || host.shown.as_ref().is_none_or(|(shown, _)| *shown != id) {
            return;
        }
        let Some(window) = host.frame.window() else {
            return;
        };
        let known = width > 0 && height > 0;
        let shape = if known {
            NSSize::new(width as f64, height as f64)
        } else {
            NSSize::new(16.0, 9.0)
        };
        window.setContentAspectRatio(shape);
        host.shaped = known;
    });
}
