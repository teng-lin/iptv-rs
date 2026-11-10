//! The desktop's window and mouse around the player: window full screen
//! (F11 is the platform's own shortcut; the panels' button, a double click
//! on the video and Escape are ours) and mouse movement over full-screen
//! video, which shows the player panels. Off desktops the window has no
//! state, so these read "not full screen" and do nothing.

use std::cell::{Cell, RefCell};

use nsh_core::desktop::WindowMode;
use nsh_core::element::desktop::{WindowRef, current_window};

thread_local! {
    static WINDOW: RefCell<Option<WindowRef>> = const { RefCell::new(None) };
    /// Where the mouse last was (window points).
    static POINTER: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}

/// Keeps the app's window; call while the app renders.
pub fn bind() {
    WINDOW.with(|w| *w.borrow_mut() = Some(current_window()));
}

/// Whether the window is in full screen (reading subscribes).
pub fn fullscreen() -> bool {
    WINDOW.with(|w| {
        w.borrow()
            .as_ref()
            .and_then(WindowRef::state)
            .is_some_and(|s| s.mode == WindowMode::FullScreen)
    })
}

pub fn set_fullscreen(on: bool) {
    WINDOW.with(|w| {
        if let Some(w) = w.borrow().as_ref() {
            log::info!("iptv window: full screen {on}");
            w.set_mode(if on {
                WindowMode::FullScreen
            } else {
                WindowMode::Normal
            });
        }
    });
}

pub fn toggle_fullscreen() {
    set_fullscreen(!nsh_core::prelude::untracked(fullscreen));
}

/// The mouse is at (`x`, `y`) at `now_ms`: whether it moved, at most every
/// [`POINTER_EVERY_MS`] (moves come once per frame; a redraw under a
/// resting mouse reports the same place again and must not keep the panels
/// up).
pub fn pointer_moved(x: f32, y: f32, now_ms: u64) -> bool {
    let moved = POINTER.with(|p| {
        let moved = p
            .get()
            .is_none_or(|(px, py)| (px - x).abs() + (py - y).abs() >= 2.0);
        if moved {
            p.set(Some((x, y)));
        }
        moved
    });
    moved
        && LAST_MOVE.with(|last| {
            let due = last
                .get()
                .is_none_or(|at| now_ms.saturating_sub(at) >= POINTER_EVERY_MS);
            if due {
                last.set(Some(now_ms));
            }
            due
        })
}

/// How often mouse movement restarts the panels' timer at most.
const POINTER_EVERY_MS: u64 = 250;

thread_local! {
    static LAST_MOVE: Cell<Option<u64>> = const { Cell::new(None) };
}
