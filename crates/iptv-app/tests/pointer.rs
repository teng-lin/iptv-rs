//! What a click or a tap reaches, on the app run headless: the overlay
//! layers mounted while hidden (the channel list, the player panels, the
//! choosers) must leave the guide, the rail and the header to the
//! pointer. NSH's pointer events inherit as CSS's, so a view saying
//! `PointerEvents::Auto` inside a hidden layer's `None` takes presses
//! through it, and on the desktop a press goes to the topmost view under
//! the pointer; `Headless::press` fails when another view covers its
//! target there, as a real click would miss it.
//!
//! Synthetic data only: a three-channel playlist "picked" from a file.

use std::time::Duration;

use nsh_core::picker::PickedFile;
use nsh_core::{ImageData, Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const PLAYLIST: &str = "#EXTM3U
#EXTINF:-1 tvg-id=\"one\" group-title=\"News\",Test Channel One
http://stream.example.test/one.m3u8
#EXTINF:-1 tvg-id=\"two\" group-title=\"News\",Test Channel Two
http://stream.example.test/two.m3u8
#EXTINF:-1 tvg-id=\"three\" group-title=\"Sport\",Test Channel Three
http://stream.example.test/three.m3u8
";

const IMPORT: Duration = Duration::from_secs(20);

#[test]
fn clicks_reach_the_guide_under_the_hidden_layers() {
    // The app keeps its files under $HOME: a fresh, empty one.
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary, before anything reads the
    // environment.
    unsafe { std::env::set_var("HOME", home.path()) };

    let config = EngineConfig {
        viewport: Size::new(px(1280.0), px(720.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    };
    let mut app = Headless::start(config).unwrap();
    app.on_pick(|_| {
        vec![PickedFile::new(
            "synthetic.m3u",
            "audio/x-mpegurl",
            ImageData::new(PLAYLIST.as_bytes().to_vec()),
        )]
    });
    let step = |r: Result<(), String>| r.unwrap_or_else(|e| panic!("{e}"));

    // First run: add the playlist from a file.
    step(app.wait_for("text:No playlists yet"));
    step(app.press("text:Add playlist"));
    step(app.press("text:Playlist file"));
    step(app.press("text:Choose a file…"));
    step(app.wait_for("text:synthetic.m3u"));
    step(app.press("text:Add playlist"));
    step(app.wait_for_within("text:Playlist added", IMPORT));
    step(app.press("text:Watch"));

    // The guide: a channel's name plays it (the name column lies under
    // the hidden channel list's panel).
    step(app.wait_for("text:Test Channel Two"));
    step(app.press("text:Test Channel Two"));
    step(app.wait_for("text:▶"));

    // The list name in the header (under the hidden player panels' top
    // bar) opens the groups and the rail; a group shows its channels.
    step(app.press("text:☰  All channels"));
    step(app.press("text:Sport"));
    step(app.wait_for("text:Test Channel Three"));
    step(app.wait_for_gone("text:Test Channel One", Duration::from_secs(2)));

    // The header's add button (under the hidden player panels' top bar
    // too) opens the wizard.
    step(app.press("text:+ Add playlist"));
    step(app.wait_for("text:Playlist link"));
}
