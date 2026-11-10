//! Remote and keyboard input for iptv-rs, independent of any UI toolkit.
//!
//! Pipeline: platform key events → [`RemoteKey`] + [`Phase`] →
//! [`PressDetector`] (short / long) → [`Keymap::resolve`] in the current
//! [`Context`] → [`Action`]. The app applies actions to its state; shells only
//! translate their key events into `RemoteKey`s.

mod back;
mod key;
mod keymap;
mod press;

pub use back::BackGuard;
pub use key::RemoteKey;
pub use keymap::{Action, Binding, Context, Keymap};
pub use press::{DEFAULT_LONG_PRESS_MS, Phase, Press, PressDetector};
