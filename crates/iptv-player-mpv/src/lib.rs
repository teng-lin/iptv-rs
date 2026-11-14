//! Native libmpv adapter and the player wire contract shared with Android.
mod props;
pub use props::{PipProps, PlayerBindings, PlayerProps, SourceProps};
mod tracks;
pub use tracks::parse_tracks;

/// The PiP component shares surface zero's player and stream.
pub const PIP_VIEW: &str = "tv.pip";

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios", windows))]
mod mpv;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios", windows))]
pub use mpv::register;
