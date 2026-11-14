//! Shell version and Windows resource stamping.
mod build_version;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_version.rs");
    build_version::emit();
    if let Ok("windows") = std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        println!("cargo:rerun-if-changed=windows.rc");
        println!("cargo:rerun-if-changed=icon.ico");
        embed_resource::compile_for("windows.rc", ["iptv-app"], embed_resource::NONE)
            .manifest_required()
            .expect("compile Windows app icon resource");
    }
}
