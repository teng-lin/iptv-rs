//! The mpv instance: created with the player's options, then commands and
//! properties by name.

use std::ffi::{CStr, CString};

use super::{DEFAULT_UA, ffi};

/// An mpv instance.
pub struct Mpv(pub *mut ffi::mpv_handle);

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
            ("ytdl", "no".to_owned()),
            ("input-default-bindings", "no".to_owned()),
            ("input-vo-keyboard", "no".to_owned()),
            ("osd-level", "0".to_owned()),
            ("audio-display", "no".to_owned()),
            ("network-timeout", "15".to_owned()),
            ("user-agent", DEFAULT_UA.to_owned()),
            ("hwdec", hwdec),
        ];
        if let Some(ao) = env("IPTV_RS_AUDIO") {
            options.push(("ao", ao));
        }
        for (k, v) in &options {
            mpv.option(k, v);
        }
        // SAFETY: a created, uninitialized handle.
        if unsafe { ffi::mpv_initialize(h) } < 0 {
            log::error!("iptv mpv: could not initialize");
            return None;
        }
        let observe = [
            (1, c"paused-for-cache", ffi::MPV_FORMAT_FLAG),
            (2, c"width", ffi::MPV_FORMAT_INT64),
            (3, c"height", ffi::MPV_FORMAT_INT64),
            (4, c"video-params/aspect", ffi::MPV_FORMAT_DOUBLE),
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
