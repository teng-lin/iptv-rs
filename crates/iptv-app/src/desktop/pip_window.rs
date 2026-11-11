//! The desktop's picture in picture (`pip.rs`): the app's window becomes a
//! small borderless window that floats above the others, in the bottom
//! right corner of its display's usable area, showing only the video; it
//! goes back to its own place, size and mode (full screen too) after.

use std::cell::RefCell;

use nsh_core::desktop::{WindowAttributes, WindowFrame, WindowFrameStyle, WindowLevel};
use nsh_core::element::desktop::display;
use nsh_core::prelude::*;

/// The window's size in picture in picture (points, 16:9).
const SIZE: (f32, f32) = (480.0, 270.0);
/// Its distance from the display's usable edges.
const MARGIN: f32 = 24.0;

thread_local! {
    /// What the window was before: restored by [`leave`].
    static BEFORE: RefCell<Option<nsh_core::desktop::WindowPlacement>> = const { RefCell::new(None) };
}

fn attributes(frame: WindowFrameStyle, level: WindowLevel) -> WindowAttributes {
    let mut a = WindowAttributes::default();
    a.frame = Some(frame);
    a.level = Some(level);
    a
}

/// Makes the window small, borderless and always on top; false off
/// desktops (no window) or when it already is.
pub fn enter() -> bool {
    super::with_window(|w| {
        let Some(state) = untracked(|| w.state()) else {
            return false;
        };
        if BEFORE.with(|b| b.borrow().is_some()) {
            return false;
        }
        BEFORE.with(|b| *b.borrow_mut() = Some(state.placement.clone()));
        log::info!("iptv window: picture in picture");
        if state.mode != nsh_core::desktop::WindowMode::Normal {
            w.set_mode(nsh_core::desktop::WindowMode::Normal);
        }
        w.set(attributes(WindowFrameStyle::None, WindowLevel::Floating));
        let area = state
            .display
            .and_then(display)
            .map_or(state.bounds, |d| d.visible_bounds);
        let x = area.origin.x.0 + area.size.width.0 - SIZE.0 - MARGIN;
        let y = area.origin.y.0 + area.size.height.0 - SIZE.1 - MARGIN;
        w.set_bounds(WindowFrame {
            x: Some(px(x.max(area.origin.x.0))),
            y: Some(px(y.max(area.origin.y.0))),
            width: Some(px(SIZE.0)),
            height: Some(px(SIZE.1)),
            animated: None,
        });
        true
    })
    .unwrap_or(false)
}

/// The window as it was before [`enter`].
pub fn leave() {
    let Some(placement) = BEFORE.with(|b| b.borrow_mut().take()) else {
        return;
    };
    super::with_window(|w| {
        log::info!("iptv window: back from picture in picture");
        w.set(attributes(WindowFrameStyle::Default, WindowLevel::Normal));
        w.set_placement(placement);
    });
}
