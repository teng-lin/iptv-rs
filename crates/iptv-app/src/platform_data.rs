//! Platform-owned app storage location. Android supplies its actual user-scoped
//! files directory before the Rust root starts; package names are not paths.

use std::path::PathBuf;

#[cfg(target_os = "android")]
static ANDROID_FILES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub(crate) fn data_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        ANDROID_FILES
            .get()
            .expect("Android host must initialize its files directory")
            .clone()
    }
    #[cfg(not(target_os = "android"))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            home.join("Library/Application Support/iptv-rs")
        } else {
            home.join(".local/share/iptv-rs")
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_hantekllc_iptvrs_host_TvActivity_initializeDataDirectory(
    mut env: jni::JNIEnv,
    _activity: jni::objects::JObject,
    path: jni::objects::JString,
) {
    let path = match env.get_string(&path) {
        Ok(path) => PathBuf::from(String::from(path)),
        Err(_) => return, // JNI retains the conversion exception.
    };
    if !path.is_absolute() {
        let _ = env.throw_new(
            "java/lang/IllegalArgumentException",
            "Expected absolute files directory",
        );
        return;
    }
    if let Some(existing) = ANDROID_FILES.get() {
        if existing != &path {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "Files directory changed in one process",
            );
        }
    } else {
        let _ = ANDROID_FILES.set(path);
    }
}
