//! Runs iptv-rs on the Linux desktop backend, for fast iteration with a
//! keyboard's arrows, Enter and Escape. Android runs `App` through `nsh run`.

#[cfg(target_os = "linux")]
fn main() -> impl std::process::Termination {
    env_logger::init();
    use nsh_platform_linux as platform;
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<iptv_app::App>()),
        platform::AppOptions {
            title: "iptv-rs".to_owned(),
            app_id: "com.hantekllc.iptvrs".to_owned(),
            size: (960, 540),
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "linux"))]
fn main() {}
