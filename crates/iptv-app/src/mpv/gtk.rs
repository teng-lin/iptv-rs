//! The Linux desktop's side of `tv.player`: the player draws into a
//! `GtkGLArea` (in-process and compositor-independent: Wayland and X11)
//! when GTK renders it, and makes its render context when the area is
//! realized. GTK's main loop runs what mpv's threads ask for.

use std::rc::Rc;

pub use nsh_platform_linux::ComponentContext;
use nsh_platform_linux::gtk4::{self as gtk, glib, prelude::*};
use nsh_platform_linux::{Components, NativeComponent};
use serde_json::Value;

use super::{Inner, NAME, find};

/// Registers `tv.player`.
pub fn register(components: &Components) {
    components.register(
        NAME,
        Rc::new(|ctx| Box::new(Player::new(ctx)) as Box<dyn NativeComponent>),
    );
}

/// Runs `f` on GTK's main loop.
pub(super) fn on_main(f: impl FnOnce() + Send + 'static) {
    let mut f = Some(f);
    glib::idle_add_full(glib::Priority::DEFAULT, move || {
        if let Some(f) = f.take() {
            f();
        }
        glib::ControlFlow::Break
    });
}

/// Calls `f` once a second while player `id` lives.
pub(super) fn every_second(id: u64, f: fn(&Inner)) {
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let Some(p) = find(id) else {
            return glib::ControlFlow::Break;
        };
        f(&p);
        glib::ControlFlow::Continue
    });
}

/// The area the player draws in.
pub(super) struct Video(gtk::GLArea);

impl Video {
    fn new() -> Video {
        let area = gtk::GLArea::new();
        area.set_auto_render(false);
        area.set_has_depth_buffer(false);
        area.set_has_stencil_buffer(false);
        // Touches go to the view.
        area.set_can_target(false);
        area.set_focusable(false);
        Video(area)
    }

    /// Draws again soon.
    pub(super) fn redraw(&self) {
        self.0.queue_render();
    }

    /// Makes the area's GL context current, if it has one.
    pub(super) fn make_current(&self) {
        if self.0.is_realized() {
            self.0.make_current();
        }
    }
}

/// The component NSH holds.
struct Player(Rc<Inner>);

impl Player {
    fn new(ctx: ComponentContext) -> Self {
        let inner = Inner::new(ctx, |_| Video::new());
        let area = &inner.video.0;
        let weak = Rc::downgrade(&inner);
        area.connect_realize(move |_| {
            if let Some(p) = weak.upgrade() {
                realize(&p);
            }
        });
        let weak = Rc::downgrade(&inner);
        area.connect_unrealize(move |_| {
            if let Some(p) = weak.upgrade() {
                // The video output goes with the context: a source reopens
                // when the area is realized again.
                p.free_render();
            }
        });
        let weak = Rc::downgrade(&inner);
        area.connect_render(move |area, _| {
            if let Some(p) = weak.upgrade() {
                let scale = area.scale_factor();
                p.draw(area.width() * scale, area.height() * scale);
            }
            glib::Propagation::Stop
        });
        Player(inner)
    }
}

fn realize(p: &Inner) {
    if p.mpv.is_none() {
        return;
    }
    let area = &p.video.0;
    area.make_current();
    if let Some(e) = area.error() {
        log::error!("iptv mpv: no GL context: {e}");
        return;
    }
    if p.create_render() {
        log::info!("iptv mpv: rendering through GTK's GL context");
    }
}

impl NativeComponent for Player {
    fn widget(&self) -> gtk::Widget {
        self.0.video.0.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        self.0.set_prop(name, value);
    }
}
