//! Multiview's tile menu on a TV, run headless with the TV idiom (keys
//! stand in for the remote): OK opens a panel at the right edge,
//! vertically centred, 208 dp wide with 40 dp rows, with no backdrop over
//! the tiles; its items in the reference's order and words, the focus the
//! light pill with dark text. Pause holds that tile alone (⏸ on it) and
//! Play resumes it; Search and add opens the search screen over the grid,
//! whose result becomes a new tile, and Back from it adds nothing.
//!
//! Synthetic data only: a three-channel playlist "picked" from a file.

use std::collections::HashMap;
use std::time::Duration;

use nsh_core::picker::PickedFile;
use nsh_core::platform::Idiom;
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
const TILE: &str = "id:multiview-tile";
const MENU: &str = "id:multiview-menu";
const PAUSED: &str = "id:multiview-paused";
/// A TV's window (dp).
const W: f32 = 960.0;
const H: f32 = 540.0;

fn tiles(app: &Headless) -> usize {
    app.screen().count(TILE).unwrap()
}

/// The menu panel's bounds (x, y, width, height).
fn panel(app: &Headless) -> (f32, f32, f32, f32) {
    let node = app
        .screen()
        .layout_nodes()
        .into_iter()
        .find(|n| n.test_id.as_deref() == Some("multiview-menu"))
        .expect("the menu panel");
    let b = node.bounds;
    (b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0)
}

/// The panel's texts, top to bottom, with their tops.
fn items(app: &Headless) -> Vec<(String, f32)> {
    let nodes = app.screen().layout_nodes();
    let panel = nodes
        .iter()
        .find(|n| n.test_id.as_deref() == Some("multiview-menu"))
        .expect("the menu panel")
        .id;
    let parent: HashMap<_, _> = nodes.iter().map(|n| (n.id, n.parent)).collect();
    let inside = |mut id| {
        while let Some(&up) = parent.get(&id) {
            if up == panel {
                return true;
            }
            id = up;
        }
        false
    };
    let mut found: Vec<(String, f32)> = nodes
        .iter()
        .filter(|n| inside(n.id))
        .filter_map(|n| Some((n.text.as_ref()?.text.text.to_string(), n.bounds.origin.y.0)))
        .collect();
    found.sort_by(|a, b| a.1.total_cmp(&b.1));
    found
}

fn labels(app: &Headless) -> Vec<String> {
    items(app).into_iter().map(|(t, _)| t).collect()
}

fn colour(app: &Headless, label: &str) -> nsh_core::Color {
    app.screen()
        .span_style(&format!("{MENU} >> text:{label}"))
        .unwrap()
        .color
}

#[test]
fn the_tile_menu_is_a_side_panel_with_pause_and_search_and_add() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary, before anything reads the
    // environment.
    unsafe { std::env::set_var("HOME", home.path()) };
    let config = EngineConfig {
        viewport: Size::new(px(W), px(H)),
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
    let gone = |app: &mut Headless, s: &str| app.wait_for_gone(s, Duration::from_secs(2));

    step(app.wait_for("text:No playlists yet"));
    step(app.press("text:Add playlist"));
    step(app.press("text:Playlist file"));
    step(app.press("text:Choose a file…"));
    step(app.wait_for("text:synthetic.m3u"));
    step(app.press("text:Add playlist"));
    step(app.wait_for_within("text:Playlist added", IMPORT));
    step(app.press("text:Watch"));
    step(app.wait_for("text:Test Channel One"));

    // Multiview from full-screen video (the play bar's button: the
    // harness starts as a desktop), Channel One the first tile; then a
    // TV, the channel list picking the next tile by keys.
    step(app.press("text:Test Channel One"));
    step(app.wait_for("text:▶"));
    step(app.press("text:Test Channel One"));
    step(app.press("label:Video"));
    step(app.press("label:Multiview"));
    step(app.wait_for(TILE));
    step(app.set_environment("a TV", |e| e.idiom = Idiom::Tv));
    // The harness's window size, again after the environment changed.
    step(app.set_viewport(Size::new(px(W), px(H))));
    step(app.wait_for("text:3  Test Channel Three"));
    step(app.press_key("down"));
    step(app.press_key("down"));
    step(app.press_key("enter"));
    step(app.wait_for(&format!("{TILE} >> text:/Test Channel Three$/")));
    assert_eq!(tiles(&app), 2);

    // OK: the panel at the right edge, centred, with the reference's items.
    step(app.press_key("enter"));
    step(app.wait_for(MENU));
    let (x, y, w, h) = panel(&app);
    assert!((w - 208.0).abs() < 0.5, "208 dp wide: {w}");
    assert!((W - (x + w) - 8.0).abs() < 0.5, "8 dp from the edge: {x}");
    assert!((y + h / 2.0 - H / 2.0).abs() < 1.0, "centred: {y} + {h}");
    assert_eq!(
        labels(&app),
        [
            "Add screen",
            "Search and add",
            "Change channel",
            "Pause",
            "Enlarge screen",
            "Full screen",
            "Remove screen"
        ]
    );
    let rows: Vec<f32> = items(&app).iter().map(|i| i.1).collect();
    for pair in rows.windows(2) {
        assert!(
            (pair[1] - pair[0] - 40.0).abs() < 0.5,
            "40 dp rows: {rows:?}"
        );
    }
    // The focus: the light pill's dark text; the others the lists' light.
    let dark = nsh_core::Color((iptv_app::theme::FOCUSED_TEXT << 8) | 0xff);
    let light = nsh_core::Color((iptv_app::theme::LIST_TEXT << 8) | 0xff);
    assert_eq!(colour(&app, "Add screen"), dark);
    assert_eq!(colour(&app, "Pause"), light);
    // No backdrop: the tiles are the only full views under the panel.
    let covering = app
        .screen()
        .layout_nodes()
        .into_iter()
        .filter(|n| {
            n.test_id.is_none()
                && n.bounds.origin.x.0 <= 0.0
                && n.bounds.size.width.0 >= W
                && n.bounds.size.height.0 >= H
        })
        .count();
    step(app.press_key("backspace"));
    step(gone(&mut app, MENU));
    let without = app
        .screen()
        .layout_nodes()
        .into_iter()
        .filter(|n| {
            n.test_id.is_none()
                && n.bounds.origin.x.0 <= 0.0
                && n.bounds.size.width.0 >= W
                && n.bounds.size.height.0 >= H
        })
        .count();
    assert_eq!(covering, without, "the menu adds nothing over the screen");

    // Pause (the fourth item): that tile alone shows ⏸; Play resumes it.
    step(app.press_key("enter"));
    for _ in 0..3 {
        step(app.press_key("down"));
    }
    assert_eq!(colour(&app, "Pause"), dark);
    step(app.press_key("enter"));
    step(gone(&mut app, MENU));
    step(app.wait_for(&format!("{TILE} >> {PAUSED}")));
    assert_eq!(app.screen().count(PAUSED).unwrap(), 1, "one tile paused");
    step(app.press_key("enter"));
    step(app.wait_for(&format!("{MENU} >> text:Play")));
    assert_eq!(labels(&app)[3], "Play");
    for _ in 0..3 {
        step(app.press_key("down"));
    }
    step(app.press_key("enter"));
    step(gone(&mut app, PAUSED));

    // Search and add: the search screen over the grid; a result becomes
    // a new tile and multiview is back.
    step(app.press_key("enter"));
    step(app.press_key("down"));
    step(app.press_key("enter"));
    step(app.wait_for("id:search-field"));
    step(app.wait_for("text:Search and add"));
    step(app.type_text("id:search-field", "Two"));
    step(app.wait_for("text:2   Test Channel Two"));
    step(app.press("text:2   Test Channel Two"));
    step(gone(&mut app, "id:search-field"));
    step(app.wait_for(&format!("{TILE} >> text:/Test Channel Two$/")));
    assert_eq!(tiles(&app), 3);

    // Back from it adds nothing: the grid as it was.
    step(app.press_key("enter"));
    step(app.press_key("down"));
    step(app.press_key("enter"));
    step(app.wait_for("id:search-field"));
    // Nothing typed: one Back closes it.
    step(app.press_key("backspace"));
    step(gone(&mut app, "id:search-field"));
    assert_eq!(tiles(&app), 3);
    assert!(!app.screen().matches(MENU).unwrap());
    // A single tile's menu would be shorter; here three of four: Remove
    // screen is last, Full screen before it.
    step(app.press_key("enter"));
    step(app.wait_for(MENU));
    let l = labels(&app);
    assert_eq!(&l[l.len() - 2..], ["Full screen", "Remove screen"]);
}
