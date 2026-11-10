//! The guide's look, headless: programme titles are set in the bundled
//! Roboto Condensed, channel names and the rest in Roboto (the app's
//! default family), every list falling back to the system's sans-serif;
//! the sizes are the TiviMate-like ones; a name too long for the channel
//! column ends in two dots; rows are 58 dp apart.
//!
//! Synthetic data only: a playlist picked from a file, with a local XMLTV
//! guide.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iptv_epg::format_xmltv_time;
use nsh_core::picker::PickedFile;
use nsh_core::{ImageData, Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const IMPORT: Duration = Duration::from_secs(30);
const LONG: &str = "Synthetic Channel With A Name Far Too Long For The Column";

fn guide(now: i64) -> String {
    let mut xml = String::from("<?xml version=\"1.0\"?><tv>");
    for (id, title) in [("one", "Quiet Harbour"), ("two", "Golden Engine")] {
        xml.push_str(&format!(
            "<channel id=\"{id}\"><display-name>{id}</display-name></channel>\
             <programme channel=\"{id}\" start=\"{}\" stop=\"{}\"><title>{title}</title></programme>",
            format_xmltv_time(now - 1800),
            format_xmltv_time(now + 1800)
        ));
    }
    xml + "</tv>"
}

#[test]
fn the_guide_uses_the_bundled_fonts_and_proportions() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary, before anything reads the
    // environment.
    unsafe { std::env::set_var("HOME", home.path()) };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let guide_path = home.path().join("guide.xml");
    std::fs::write(&guide_path, guide(now)).unwrap();
    let playlist = format!(
        "#EXTM3U url-tvg=\"{}\"\n\
         #EXTINF:-1 tvg-id=\"one\" group-title=\"News\",Test Channel One\nhttp://stream.example.test/1.m3u8\n\
         #EXTINF:-1 tvg-id=\"two\" group-title=\"News\",{LONG}\nhttp://stream.example.test/2.m3u8\n",
        guide_path.display()
    );
    let config = EngineConfig {
        viewport: Size::new(px(1280.0), px(800.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    };
    let mut app = Headless::start(config).unwrap();
    app.on_pick(move |_| {
        vec![PickedFile::new(
            "synthetic.m3u",
            "audio/x-mpegurl",
            ImageData::new(playlist.as_bytes().to_vec()),
        )]
    });
    let step = |r: Result<(), String>| r.unwrap_or_else(|e| panic!("{e}"));
    step(app.wait_for("text:No playlists yet"));
    step(app.press("text:Add playlist"));
    step(app.press("text:Playlist file"));
    step(app.press("text:Choose a file…"));
    step(app.wait_for("text:synthetic.m3u"));
    step(app.press("text:Add playlist"));
    step(app.wait_for_within("text:Playlist added", IMPORT));
    step(app.press("text:Watch"));
    step(app.wait_for_within("text:Golden Engine", IMPORT));

    // The family, size and weight of every span reading `text` (hidden
    // layers, such as the channel list, are mounted too).
    let styles = |text: &str| {
        let mut out = Vec::new();
        while let Ok(style) = app
            .screen()
            .span_style(&format!("text:{text} >> nth={}", out.len()))
        {
            let family = style.font_family.map(|f| f.name().to_string());
            out.push((family, style.font_size.0, style.font_weight));
        }
        out
    };
    let condensed = (
        Some("'Roboto Condensed', Roboto, sans-serif".to_owned()),
        26.0,
        400,
    );
    let roboto = Some("Roboto, sans-serif".to_owned());
    // Programme titles: Roboto Condensed 26.
    assert!(
        styles("Golden Engine").contains(&condensed),
        "{:?}",
        styles("Golden Engine")
    );
    // Channel names: Roboto 26.
    assert!(styles("Test Channel One").contains(&(roboto.clone(), 26.0, 400)));
    // The focused programme's title: in its cell, and in the header in
    // Roboto Medium 27.
    let focused = styles("Quiet Harbour");
    assert!(focused.contains(&condensed), "{focused:?}");
    assert!(
        focused.contains(&(roboto.clone(), 27.0, 500)),
        "{focused:?}"
    );
    // Text without a family of its own takes the app's default (font-sans).
    let default = styles("+ Add playlist");
    assert!(
        default.iter().all(|(f, _, _)| f.is_none() || *f == roboto),
        "default family: {default:?}"
    );

    // The long name is cut with two dots; the short one is whole.
    let cut = app
        .screen()
        .texts()
        .into_iter()
        .find(|t| t.starts_with("Synthetic Channel") && t.ends_with(".."))
        .map(str::to_owned);
    assert!(cut.is_some(), "a cut name: {:?}", app.screen().texts());
    assert!(!cut.unwrap().ends_with("…"));
    assert!(app.screen().matches("text:Test Channel One").unwrap());

    // Rows 58 dp apart: the two channel numbers.
    let numbers: Vec<f32> = app
        .screen()
        .layout_nodes()
        .into_iter()
        .filter(|n| {
            n.text
                .as_ref()
                .is_some_and(|t| ["1", "2"].contains(&&*t.text.text))
        })
        .map(|n| n.bounds.origin.y.0)
        .collect();
    assert_eq!(numbers.len(), 2, "{numbers:?}");
    assert!((numbers[1] - numbers[0] - 58.0).abs() < 0.5, "{numbers:?}");
}
