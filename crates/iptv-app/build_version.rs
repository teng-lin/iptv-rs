//! Git version metadata shared by the Cargo build script and its standalone tests.

use std::path::Path;

/// Resolve Git from the crate, including linked worktrees and detached CI checkouts.
pub fn emit() {
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
