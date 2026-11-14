//! The mpv instance: created with the player's options, then commands and
//! properties by name.

use std::ffi::{CStr, CString};

use super::{DEFAULT_UA, ffi};

/// An mpv instance.
pub struct Mpv(pub *mut ffi::mpv_handle);

/// What the player must never do, whatever a stream, a playlist or the
/// user's own mpv files ask for: read mpv's configuration, run scripts,
/// hand an address to `youtube-dl`, or open files that lie beside a
/// stream. Streams come from providers, so these are the app's sandbox:
/// an instance that cannot be given all of them is not used
/// ([`Mpv::new`] fails).
const RESTRICTIONS: [&str; 4] = ["config", "load-scripts", "ytdl", "autoload-files"];

/// Whether mpv's answer `result` to setting option `name` to `value`
/// leaves the option as the player needs it.
///
/// Runtimes built without scripting omit `load-scripts` and `ytdl`
/// entirely: there is nothing to switch off, so for these two (and only
/// when switching them off) "no such option" is as good as set. Every
/// other refusal counts, for every option.
fn option_holds(name: &str, value: &str, result: std::ffi::c_int) -> bool {
    let scripting_compiled_out = result == ffi::MPV_ERROR_OPTION_NOT_FOUND
        && value == "no"
        && matches!(name, "load-scripts" | "ytdl");
    result >= 0 || scripting_compiled_out
}

// SAFETY: libmpv's client API may be used from any thread; the handle is
// destroyed once, when the last owner drops it.
unsafe impl Send for Mpv {}
// SAFETY: libmpv serializes concurrent client API access to a live handle.
unsafe impl Sync for Mpv {}

/// mpv refuses to start under a locale whose numbers are not C's, and GTK
/// set the user's when it started. Done once, before the first instance:
/// the locale is the process's, the app sets none itself, and the toolkit
/// sets its own only at startup.
#[cfg(unix)]
fn use_c_numbers() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: `setlocale` gets a valid category and a NUL-terminated
        // name with static lifetime. It changes process-wide state that
        // other threads may be reading, which C does not make safe in
        // general; here it runs once, before the first mpv instance
        // exists (so before any of mpv's threads), and only number
        // formatting changes, to the form Rust's own formatting already
        // uses. `Once` also keeps two first players (multiview) from
        // calling it at the same time.
        unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };
    });
}

#[cfg(not(unix))]
fn use_c_numbers() {}

impl Mpv {
    /// A created handle that is not initialized yet.
    fn create() -> Option<Mpv> {
        use_c_numbers();
        // SAFETY: no preconditions; null when mpv cannot start.
        let h = unsafe { ffi::mpv_create() };
        (!h.is_null()).then(|| Mpv(h))
    }

    pub fn new() -> Option<Mpv> {
        let mpv = Mpv::create()?;
        let h = mpv.0;
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
            ("autoload-files", "no".to_owned()),
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
        if !mpv.configure(&options) {
            // `mpv` drops here: `mpv_terminate_destroy` also destroys a
            // handle that was created and never initialized.
            return None;
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

    /// Sets the options of a handle that is not initialized yet. False
    /// when one of the [`RESTRICTIONS`] could not be set: the instance
    /// must not be initialized or play anything then. Any other option
    /// that mpv refuses keeps mpv's default, with a warning.
    fn configure(&self, options: &[(&'static str, String)]) -> bool {
        let mut restricted = true;
        for (name, value) in options {
            if self.option(name, value) {
                continue;
            }
            if RESTRICTIONS.contains(name) {
                log::error!("iptv mpv: restriction {name} could not be set; no player");
                restricted = false;
            } else {
                log::warn!("iptv mpv: option {name} not set");
            }
        }
        restricted
    }

    /// Whether option `name` now holds `value` ([`option_holds`]).
    fn option(&self, name: &str, value: &str) -> bool {
        let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) else {
            return false;
        };
        // SAFETY: a live handle and NUL-terminated strings.
        let result = unsafe { ffi::mpv_set_option_string(self.0, n.as_ptr(), v.as_ptr()) };
        option_holds(name, value, result)
    }

    pub fn set(&self, name: &str, value: &str) {
        let _ = self.try_set(name, value);
    }

    pub fn try_set(&self, name: &str, value: &str) -> bool {
        let (Ok(n), Ok(v)) = (CString::new(name), CString::new(value)) else {
            return false;
        };
        // SAFETY: a live handle and NUL-terminated strings.
        let ok = unsafe { ffi::mpv_set_property_string(self.0, n.as_ptr(), v.as_ptr()) } >= 0;
        if !ok {
            log::warn!("iptv mpv: property {name} not set");
        }
        ok
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
    /// idle between files, reads no user configuration, runs no scripts,
    /// loads no files beside a stream and prefetches the next entry (the
    /// live bridge's spare).
    #[test]
    fn a_new_instance_has_the_players_options() {
        let mpv = mpv();
        for (name, want) in [
            (c"vo", "libmpv"),
            (c"idle", "yes"),
            (c"keep-open", "no"),
            (c"input-default-bindings", "no"),
            (c"osd-level", "0"),
            (c"audio-display", "no"),
            (c"prefetch-playlist", "yes"),
            (c"user-agent", DEFAULT_UA),
        ] {
            assert_eq!(mpv.get(name).as_deref(), Some(want), "{name:?}");
        }
        // The restrictions every runtime has.
        for name in [c"config", c"autoload-files"] {
            assert_eq!(mpv.get(name).as_deref(), Some("no"), "{name:?}");
        }
        // A runtime with scripting compiled out has neither capability nor
        // its option. Runtimes that expose either must have it disabled.
        for name in [c"load-scripts", c"ytdl"] {
            assert!(
                matches!(mpv.get(name).as_deref(), None | Some("no")),
                "{name:?}"
            );
        }
        // All four are in the list `Mpv::new` insists on.
        assert_eq!(
            RESTRICTIONS,
            ["config", "load-scripts", "ytdl", "autoload-files"]
        );
        assert_eq!(mpv.get_f64(c"network-timeout"), Some(15.0));
        if std::env::var_os("IPTV_RS_HWDEC").is_none() {
            assert_eq!(mpv.get(c"hwdec").as_deref(), Some("auto-copy-safe"));
        }
        // Numbers parse as C's whatever the user's locale.
        assert!(mpv.command(&["set", "volume", "50.5"]));
        assert_eq!(mpv.get_f64(c"volume"), Some(50.5));
    }

    /// Only scripting that is compiled out may be missing; any other
    /// refusal of a restriction, or of any option, counts as not set.
    #[test]
    fn only_absent_scripting_options_are_excused() {
        let absent = ffi::MPV_ERROR_OPTION_NOT_FOUND;
        for name in RESTRICTIONS {
            assert!(option_holds(name, "no", 0), "{name}");
            assert!(!option_holds(name, "no", -4), "{name}: another error");
        }
        assert!(option_holds("load-scripts", "no", absent));
        assert!(option_holds("ytdl", "no", absent));
        assert!(!option_holds("config", "no", absent));
        assert!(!option_holds("autoload-files", "no", absent));
        // Asking for scripting on a runtime without it is not excused.
        assert!(!option_holds("ytdl", "yes", absent));
        assert!(!option_holds("no-such-option", "no", absent));
    }

    /// A handle that refuses a restriction is not configured: `Mpv::new`
    /// gives no player, and the handle is destroyed uninitialized.
    #[test]
    fn a_refused_restriction_fails_the_configuration() {
        let mpv = Mpv::create().expect("libmpv");
        assert!(mpv.configure(&[("config", "no".to_owned())]));
        // mpv refuses a value the flag cannot take.
        assert!(!mpv.configure(&[("autoload-files", "perhaps".to_owned())]));
        assert!(!mpv.configure(&[("idle", "yes".to_owned()), ("config", "perhaps".to_owned()),]));
        // An ordinary option it refuses only warns.
        assert!(mpv.configure(&[("no-such-option", "1".to_owned())]));
    }

    /// Unknown names and strings C cannot carry fail without effect.
    #[test]
    fn bad_names_are_refused() {
        let mpv = mpv();
        mpv.set("no-such-property", "1");
        mpv.set("volume\0", "1");
        assert!(!mpv.option("no-such-option", "1"));
        assert!(!mpv.option("volume", "1\0"));
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
