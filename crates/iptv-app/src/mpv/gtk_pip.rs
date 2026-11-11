//! Linux: the picture-in-picture window's `tv.pip` (`pip`). Its widget is
//! a box; while a player is wanted there it holds a `GtkGLArea` of its own
//! whose GL context is the player's area's (`create-context` hands it
//! over), so mpv's render context, made in that context, draws in it as
//! in the player's area: the same frames, nothing reopened. The player's
//! area stays in the main window (shrunk to nothing by the layout), which
//! keeps the context alive; GTK's GL contexts all share the display's, so
//! the window's renderer takes the area's frames like any other's.
//!
//! Moving the player's own area into the window instead would unrealize
//! it, and its GL context with mpv's render context would go: mpv drops
//! the video output then, and the stream would have to open again.
//!
//! On X11 the window asks to stay above the others and on every
//! workspace (`desktop/x11.rs`); on Wayland a client cannot, and the
//! compositor's rules for windows titled as browsers' picture in picture
//! do it.

use std::cell::RefCell;
use std::rc::Rc;

use nsh_platform_linux::gtk4::{self as gtk, glib, prelude::*};
use nsh_platform_linux::{Components, NativeComponent};
use serde_json::Value;

use super::find;

thread_local! {
    /// The window's view (one window), and the area it holds for a player.
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

struct Host {
    frame: gtk::Box,
    area: Option<(u64, gtk::GLArea)>,
}

/// Registers `tv.pip`.
pub(super) fn register(components: &Components) {
    components.register(
        super::pip::VIEW,
        Rc::new(|_| Box::new(PipView::new()) as Box<dyn NativeComponent>),
    );
}

/// The component NSH holds.
struct PipView(gtk::Box);

impl PipView {
    fn new() -> PipView {
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.set_hexpand(true);
        frame.set_vexpand(true);
        // Presses go to the view (its drag region and controls).
        frame.set_can_target(false);
        frame.set_focusable(false);
        frame.connect_map(|frame| {
            let frame = frame.clone();
            // Once the window's surface is up.
            glib::idle_add_local_once(move || {
                if let Some(window) = frame.root().and_downcast::<gtk::Window>() {
                    crate::desktop::x11::keep_above(&window);
                }
            });
        });
        HOST.with(|h| {
            *h.borrow_mut() = Some(Host {
                frame: frame.clone(),
                area: None,
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
            frame,
            area: Some((_, area)),
        }) = host
        {
            frame.remove(&area);
        }
    }
}

impl NativeComponent for PipView {
    fn widget(&self) -> gtk::Widget {
        self.0.clone().upcast()
    }

    fn set_prop(&mut self, _name: &str, _value: Option<&Value>) {}
}

/// Gives the window's view an area for the wanted player, or takes it
/// away.
pub(super) fn sync() {
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        let Some(host) = h.as_mut() else { return };
        let wanted = super::pip::wanted().filter(|id| find(*id).is_some());
        if host.area.as_ref().map(|(id, _)| *id) == wanted {
            return;
        }
        if let Some((_, area)) = host.area.take() {
            host.frame.remove(&area);
        }
        if let Some(id) = wanted {
            let area = area_for(id);
            host.frame.append(&area);
            host.area = Some((id, area));
            log::info!("iptv mpv {id}: drawing in the picture-in-picture window");
        }
    });
}

/// Player `id` has a new frame: the window's area draws it too.
pub(super) fn redraw(id: u64) {
    HOST.with(|h| {
        if let Some(Host {
            area: Some((shown, area)),
            ..
        }) = h.borrow().as_ref()
            && *shown == id
        {
            area.queue_render();
        }
    });
}

/// An area drawing player `id`'s frames with its area's GL context.
fn area_for(id: u64) -> gtk::GLArea {
    let area = gtk::GLArea::new();
    area.set_auto_render(false);
    area.set_has_depth_buffer(false);
    area.set_has_stencil_buffer(false);
    area.set_can_target(false);
    area.set_focusable(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.connect_create_context(move |_| {
        let context = find(id).and_then(|p| p.video.context());
        if context.is_none() {
            log::warn!("iptv mpv {id}: no GL context to share with the window");
        }
        context
    });
    area.connect_render(move |area, _| {
        if let Some(p) = find(id) {
            let scale = area.scale_factor();
            p.draw(area.width() * scale, area.height() * scale);
        }
        glib::Propagation::Stop
    });
    area.connect_realize(|area| area.queue_render());
    area
}
