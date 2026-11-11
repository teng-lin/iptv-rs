//! The mpv instance: created with the player's options, then commands and
//! properties by name.

use std::ffi::{CStr, CString};

use super::{DEFAULT_UA, ffi};

/// An mpv instance.
pub struct Mpv(pub *mut ffi::mpv_handle);

// SAFETY: libmpv's client API may be used from any thread; the handle is
// destroyed once, when the last owner drops it.
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

impl Mpv {
    pub fn new() -> Option<Mpv> {
        // SAFETY: mpv refuses to start under a locale whose numbers are not
        // C's (GTK set the user's); only number formatting changes.
        let h = unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
            ffi::mpv_create()
        };
        if h.is_null() {
            return None;
        }
        let mpv = Mpv(h);
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let hwdec = env("IPTV_RS_HWDEC").unwrap_or_else(|| "auto-copy-safe".to_owned());
        let mut options = vec![
            ("vo", "libmpv".to_owned()),
            ("idle", "yes".to_owned()),
            ("keep-open", "no".to_owned()),
            ("terminal", "no".to_owned()),
            ("config", "no".to_owned()),
            ("load-scripts", "no".to_owned()),
            ("input-default-bindings", "no".to_owned()),
            ("input-vo-keyboard", "no".to_owned()),
            ("osd-level", "0".to_owned()),
            ("audio-display", "no".to_owned()),
            ("network-timeout", "15".to_owned()),
            // Opens a live stream's spare entry once the current one was
            // read to its end (`worker::Bridge`).
            ("prefetch-playlist", "yes".to_owned()),
            ("user-agent", DEFAULT_UA.to_owned()),
            ("hwdec", hwdec),
        ];
        // No youtube-dl hook (MPVKit's iOS build has none to turn off).
        if !cfg!(target_os = "ios") {
            options.push(("ytdl", "no".to_owned()));
        }
        // A tablet's GPU draws every frame on the main thread: mpv's fast
        // profile (bilinear scaling, no dithering or debanding).
        if cfg!(target_os = "ios") {
            options.insert(0, ("profile", "fast".to_owned()));
        }
        if let Some(ao) = env("IPTV_RS_AUDIO") {
            options.push(("ao", ao));
        } else if cfg!(all(target_os = "ios", target_abi = "sim")) {
            // The iOS simulator plays no sound through mpv: its audio server
            // timed AudioUnit's output out as a stream reopened (and
            // AudioToolbox aborted the app), and AVFoundation's output never
            // advanced, holding the video. Devices keep mpv's own choice
            // (AudioUnit).
            options.push(("ao", "null".to_owned()));
        }
        for (k, v) in &options {
            mpv.option(k, v);
        }
        // SAFETY: a created, uninitialized handle.
        if unsafe { ffi::mpv_initialize(h) } < 0 {
            log::error!("iptv mpv: could not initialize");
            return None;
        }
        // Ids are [`super::worker::convert`]'s.
        let observe = [
            (1, c"paused-for-cache", ffi::MPV_FORMAT_FLAG),
            (2, c"width", ffi::MPV_FORMAT_INT64),
            (3, c"height", ffi::MPV_FORMAT_INT64),
            (4, c"video-params/aspect", ffi::MPV_FORMAT_DOUBLE),
            // A change notice only: read as JSON when it comes.
            (5, c"track-list", ffi::MPV_FORMAT_NONE),
            (6, c"container-fps", ffi::MPV_FORMAT_DOUBLE),
            (7, c"audio-params/channel-count", ffi::MPV_FORMAT_INT64),
        ];
        // SAFETY: an initialized handle and NUL-terminated names.
        unsafe {
            for (id, name, format) in observe {
                ffi::mpv_observe_property(h, id, name.as_ptr(), format);
            }
            ffi::mpv_request_log_messages(h, c"info".as_ptr());
        }
        Some(mpv)
    }

    fn option(&self, name: &str, value: &str) {
        let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        // SAFETY: a live handle and NUL-terminated strings.
        if unsafe { ffi::mpv_set_option_string(self.0, n.as_ptr(), v.as_ptr()) } < 0 {
            log::warn!("iptv mpv: option {name} not set");
        }
    }

    pub fn set(&self, name: &str, value: &str) {
        let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        // SAFETY: a live handle and NUL-terminated strings.
        if unsafe { ffi::mpv_set_property_string(self.0, n.as_ptr(), v.as_ptr()) } < 0 {
            log::warn!("iptv mpv: property {name} not set");
        }
    }

    pub fn get_f64(&self, name: &CStr) -> Option<f64> {
        let mut v = 0.0_f64;
        self.get_raw(name, ffi::MPV_FORMAT_DOUBLE, (&mut v as *mut f64).cast())
            .then_some(v)
            .filter(|v| v.is_finite())
    }

    pub fn get_i64(&self, name: &CStr) -> Option<i64> {
        let mut v = 0_i64;
        self.get_raw(name, ffi::MPV_FORMAT_INT64, (&mut v as *mut i64).cast())
            .then_some(v)
    }

    /// Reads property `name` in `format` into `data` (sized for it).
    fn get_raw(&self, name: &CStr, format: std::ffi::c_int, data: *mut std::ffi::c_void) -> bool {
        // SAFETY: a live handle; callers pass storage of `format`'s type.
        unsafe { ffi::mpv_get_property(self.0, name.as_ptr(), format, data) >= 0 }
    }

    pub fn get(&self, name: &CStr) -> Option<String> {
        // SAFETY: a live handle; the result is mpv's to free.
        unsafe {
            let p = ffi::mpv_get_property_string(self.0, name.as_ptr());
            if p.is_null() {
                return None;
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            ffi::mpv_free(p.cast());
            Some(s)
        }
    }

    /// Interrupts `mpv_wait_event` (safe on the render thread).
    pub fn wakeup(&self) {
        // SAFETY: a live handle.
        unsafe { ffi::mpv_wakeup(self.0) };
    }

    /// Runs a command; whether mpv accepted it.
    pub fn command(&self, args: &[&str]) -> bool {
        let Ok(owned) = args
            .iter()
            .map(|a| CString::new(*a))
            .collect::<Result<Vec<_>, _>>()
        else {
            return false;
        };
        let mut ptrs: Vec<*const std::ffi::c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: a live handle and a NULL-terminated array of C strings.
        unsafe { ffi::mpv_command(self.0, ptrs.as_mut_ptr()) >= 0 }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // SAFETY: the handle is ours; nothing uses it afterwards.
        unsafe { ffi::mpv_terminate_destroy(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mpv() -> Mpv {
        let mpv = Mpv::new().expect("libmpv");
        mpv.set("ao", "null");
        mpv
    }

    /// The instance renders through the app (no window of its own), stays
    /// idle between files, reads no user configuration and prefetches the
    /// next entry (the live bridge's spare).
    #[test]
    fn a_new_instance_has_the_players_options() {
        let mpv = mpv();
        for (name, want) in [
            (c"vo", "libmpv"),
            (c"idle", "yes"),
            (c"keep-open", "no"),
            (c"load-scripts", "no"),
            (c"input-default-bindings", "no"),
            (c"osd-level", "0"),
            (c"audio-display", "no"),
            (c"prefetch-playlist", "yes"),
            (c"ytdl", "no"),
            (c"user-agent", DEFAULT_UA),
        ] {
            assert_eq!(mpv.get(name).as_deref(), Some(want), "{name:?}");
        }
        assert_eq!(mpv.get_f64(c"network-timeout"), Some(15.0));
        if std::env::var_os("IPTV_RS_HWDEC").is_none() {
            assert_eq!(mpv.get(c"hwdec").as_deref(), Some("auto-copy-safe"));
        }
        // Numbers parse as C's whatever the user's locale.
        assert!(mpv.command(&["set", "volume", "50.5"]));
        assert_eq!(mpv.get_f64(c"volume"), Some(50.5));
    }

    /// Unknown names and strings C cannot carry fail without effect.
    #[test]
    fn bad_names_are_refused() {
        let mpv = mpv();
        mpv.set("no-such-property", "1");
        mpv.set("volume\0", "1");
        mpv.option("no-such-option", "1");
        mpv.option("volume", "1\0");
        assert_eq!(mpv.get_f64(c"volume"), Some(100.0));
        assert!(!mpv.command(&["no-such-command"]));
        assert!(!mpv.command(&["set", "volume\0", "1"]));
        assert_eq!(mpv.get(c"no-such-property"), None);
        assert_eq!(mpv.get_i64(c"no-such-property"), None);
        // Unavailable without a file.
        assert_eq!(mpv.get_f64(c"time-pos"), None);
        assert_eq!(mpv.get_i64(c"playlist-count"), Some(0));
        mpv.wakeup();
    }
}
