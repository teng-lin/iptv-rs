//! Where the linker finds libmpv (`src/mpv/ffi.rs`). Linux distributions
//! install it on the default search path; on macOS it comes from Homebrew
//! (`brew install mpv`), whose prefix is not searched: `pkg-config`'s
//! answer, else Homebrew's default prefixes. The library's install name is
//! absolute, so the app finds it at run time too.
//!
//! iOS and iPadOS link MPVKit's static xcframeworks
//! (`tools/fetch-mpvkit.sh`, into `target/mpvkit` or `IPTV_RS_MPVKIT`):
//! libmpv and everything it pulls in, from the slice of the target (device
//! or simulator), and the system frameworks and libraries they use.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=IPTV_RS_MPVKIT");
    build_version();
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => macos(),
        Ok("ios") => ios(),
        _ => {}
    }
}

fn macos() {
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
        if Path::new(&dir).join("libmpv.dylib").exists() {
            println!("cargo:rustc-link-search=native={dir}");
            return;
        }
    }
    println!("cargo:warning=libmpv not found: install it with `brew install mpv`");
}

/// MPVKit's frameworks libmpv needs (MPVKit 1.0.0's `_MPVKit` and
/// `_FFmpeg` targets, LGPL).
const FRAMEWORKS: &[&str] = &[
    "Libmpv",
    "Libuchardet",
    "Libbluray",
    "Libavcodec",
    "Libavdevice",
    "Libavfilter",
    "Libavformat",
    "Libavutil",
    "Libswresample",
    "Libswscale",
    "Libssl",
    "Libcrypto",
    "Libass",
    "Libfreetype",
    "Libfribidi",
    "Libharfbuzz",
    "Libshaderc_combined",
    "lcms2",
    "Libplacebo",
    "Libdovi",
    "Libunibreak",
    "gmp",
    "nettle",
    "hogweed",
    "gnutls",
    "Libdav1d",
    "Libuavs3d",
];

/// The system's, as MPVKit's package links them (and OpenGL ES for the
/// view, MoltenVK's for its Vulkan).
const SYSTEM_FRAMEWORKS: &[&str] = &[
    "AVFoundation",
    "AudioToolbox",
    "CoreAudio",
    "CoreFoundation",
    "CoreMedia",
    "CoreVideo",
    "IOSurface",
    "Metal",
    "OpenGLES",
    "QuartzCore",
    "UIKit",
    "VideoToolbox",
];
const SYSTEM_LIBS: &[&str] = &["bz2", "iconv", "expat", "resolv", "xml2", "z", "c++"];

fn ios() {
    let simulator = std::env::var("TARGET").is_ok_and(|t| t.ends_with("-sim"));
    let slice = if simulator {
        "ios-arm64_x86_64-simulator"
    } else {
        "ios-arm64"
    };
    let root = std::env::var_os("IPTV_RS_MPVKIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
            manifest.join("../../target/mpvkit")
        });
    if !root.join("Libmpv.xcframework").exists() {
        println!(
            "cargo:warning=MPVKit not found in {}: run tools/fetch-mpvkit.sh",
            root.display()
        );
        return;
    }
    for name in FRAMEWORKS {
        let dir = root.join(format!("{name}.xcframework")).join(slice);
        println!("cargo:rerun-if-changed={}", dir.display());
        println!("cargo:rustc-link-search=framework={}", dir.display());
        println!("cargo:rustc-link-lib=framework={name}");
    }
    let molten = root.join("MoltenVK.xcframework").join(slice);
    println!("cargo:rustc-link-search=native={}", molten.display());
    // A universal archive: the linker takes it, rustc could not bundle it.
    println!("cargo:rustc-link-lib=static:-bundle=MoltenVK");
    for name in SYSTEM_FRAMEWORKS {
        println!("cargo:rustc-link-lib=framework={name}");
    }
    for name in SYSTEM_LIBS {
        println!("cargo:rustc-link-lib=dylib={name}");
    }
}

/// Resolve Git from the crate, including linked worktrees and detached CI checkouts.
fn build_version() {
    fn git(args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("git")
            .current_dir(std::env::var_os("CARGO_MANIFEST_DIR")?)
            .args(args)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    // HEAD changes on checkout; its symbolic ref changes on commit. Packed refs
    // can change without either file changing. Git resolves their worktree paths.
    let mut paths = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        paths.push(reference);
    }
    for path in paths {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &path]) {
            // Missing watched files make Cargo rebuild on every invocation.
            // Watch the nearest existing parent until Git creates the file.
            let mut watched = Path::new(&path);
            while !watched.exists() {
                let Some(parent) = watched.parent() else {
                    break;
                };
                watched = parent;
            }
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }
    let hash = git(&["rev-parse", "--verify", "HEAD"])
        .filter(|hash| hash.len() >= 8 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .map(|hash| hash[..8].to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=IPTV_GIT_HASH={hash}");
}
