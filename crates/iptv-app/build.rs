//! Where the linker finds libmpv (`src/mpv/ffi.rs` links `mpv`). Linux
//! distributions install it on the default search path; on macOS it comes
//! from Homebrew (`brew install mpv`), whose prefix is not searched:
//! `pkg-config`'s answer, else Homebrew's default prefixes. The library's
//! install name is absolute, so the app finds it at run time too.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let pkg = std::process::Command::new("pkg-config")
        .args(["--variable=libdir", "mpv"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let dirs = pkg
        .into_iter()
        .chain(["/opt/homebrew/lib".to_owned(), "/usr/local/lib".to_owned()]);
    for dir in dirs {
        if std::path::Path::new(&dir).join("libmpv.dylib").exists() {
            println!("cargo:rustc-link-search=native={dir}");
            return;
        }
    }
    println!("cargo:warning=libmpv not found: install it with `brew install mpv`");
}
