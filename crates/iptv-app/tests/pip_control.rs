//! The control strip's Picture in picture, run headless as a desktop app
//! (keys as on a TV): Menu over full-screen video shows the button; OK on
//! it turns the app's window into the small picture-in-picture one
//! (`pip.rs`, `desktop/pip_window.rs`), and Back (Escape) or a double
//! click on the video brings the window back as it was, the player still
//! full screen. Android's system window is
//! checked on a device.
//!
//! Synthetic data only: a two-channel playlist "picked" from a file.

use std::time::Duration;

use nsh_core::picker::PickedFile;
use nsh_core::platform::{Idiom, Os};
use nsh_core::{ImageData, Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const PLAYLIST: &str = "#EXTM3U
#EXTINF:-1 tvg-id=\"one\" group-title=\"News\",Test Channel One
http://stream.example.test/one.m3u8
#EXTINF:-1 tvg-id=\"two\" group-title=\"News\",Test Channel Two
http://stream.example.test/two.m3u8
";

const STRIP: &str = "text:/M or Esc hide/";
const PIP: &str = "text:Picture in picture";

#[test]
fn the_strip_turns_the_desktop_window_into_picture_in_picture_and_back() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary, before anything reads the
    // environment.
    unsafe { std::env::set_var("HOME", home.path()) };
    let mut config = EngineConfig {
        viewport: Size::new(px(960.0), px(540.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    };
    // A desktop app: it has a window of its own.
    config.environment.os = Os::Linux;
    let mut app = Headless::start(config).unwrap();
    app.on_pick(|_| {
        vec![PickedFile::new(
            "synthetic.m3u",
            "audio/x-mpegurl",
            ImageData::new(PLAYLIST.as_bytes().to_vec()),
        )]
    });
    let step = |r: Result<(), String>| r.unwrap_or_else(|e| panic!("{e}"));

    step(app.wait_for("text:No playlists yet"));
    step(app.press("text:Add playlist"));
    step(app.press("text:Playlist file"));
    step(app.press("text:Choose a file…"));
    step(app.wait_for("text:synthetic.m3u"));
    step(app.press("text:Add playlist"));
    step(app.wait_for_within("text:Playlist added", Duration::from_secs(20)));
    step(app.press("text:Watch"));
    step(app.wait_for("text:Test Channel One"));
    step(app.set_environment("a TV", |e| e.idiom = Idiom::Tv));

    // Full screen, then Menu: the strip, with its Picture in picture.
    step(app.press_key("enter"));
    step(app.wait_for("text:▶"));
    step(app.press_key("enter"));
    step(app.wait_for("id:player-banner"));
    step(app.press_key("m"));
    step(app.wait_for(STRIP));
    step(app.wait_for(PIP));

    let size = |app: &mut Headless| {
        let state = app.window_state("main").unwrap().expect("a desktop window");
        (state.bounds.size.width.0, state.bounds.size.height.0)
    };
    let before = size(&mut app);

    // OK on it (Search, Channels, Recordings, Multiview, then it): the
    // desktop's window becomes the small picture-in-picture one, video
    // only.
    for _ in 0..4 {
        step(app.press_key("right"));
    }
    step(app.press_key("enter"));
    app.idle(Duration::from_millis(300)).unwrap();
    assert_eq!(size(&mut app), (480.0, 270.0), "the small window");

    // Back (Escape): the window as it was, the player full screen.
    step(app.back());
    app.idle(Duration::from_millis(300)).unwrap();
    assert_eq!(size(&mut app), before, "back to its size");
    assert!(
        app.screen().matches("id:video").unwrap(),
        "the video layer stays"
    );
    step(app.press_key("m"));
    step(app.wait_for(STRIP));

    // Again, and a double click on the video brings it back too (a single
    // click does nothing in the small window).
    for _ in 0..4 {
        step(app.press_key("right"));
    }
    step(app.press_key("enter"));
    app.idle(Duration::from_millis(300)).unwrap();
    assert_eq!(size(&mut app), (480.0, 270.0));
    step(app.press("id:video"));
    step(app.press("id:video"));
    app.idle(Duration::from_millis(300)).unwrap();
    assert_eq!(size(&mut app), before, "a double click: back");
}
