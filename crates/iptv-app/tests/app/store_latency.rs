//! Real SQLCipher write contention must not delay guide input or rendering.
//! All 10,000 channels and seven days of programmes are generated locally.
use nsh_core::{ImageData, Size, element::Root, picker::PickedFile, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn guide_input_stays_under_budget_while_the_database_writer_is_locked() {
    let Some(home) = isolated!() else { return };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let guide_path = home.path().join("synthetic-guide.xml");
    let mut guide = String::from("<tv>");
    let mut playlist = format!("#EXTM3U url-tvg=\"{}\"\n", guide_path.display());
    for channel in 1..=10_000 {
        playlist.push_str(&format!("#EXTINF:-1 tvg-id=\"c{channel}\" group-title=\"Synthetic\",Channel {channel:05}\nhttp://stream.example.test/{channel}\n"));
        guide.push_str(&format!(
            "<channel id=\"c{channel}\"><display-name>Channel {channel:05}</display-name></channel>"
        ));
        for day in -3..4 {
            let start = now - 3600 + day * 86400;
            guide.push_str(&format!("<programme channel=\"c{channel}\" start=\"{}\" stop=\"{}\"><title>Synthetic day {day}</title></programme>", iptv_epg::format_xmltv_time(start), iptv_epg::format_xmltv_time(start + 86400)));
        }
    }
    guide.push_str("</tv>");
    std::fs::write(guide_path, guide).unwrap();
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(1280.0), px(720.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    })
    .unwrap();
    app.on_pick(move |_| {
        vec![PickedFile::new(
            "synthetic.m3u",
            "audio/x-mpegurl",
            ImageData::new(playlist.as_bytes().to_vec()),
        )]
    });
    app.wait_for("text:No playlists yet").unwrap();
    app.press("text:Add playlist").unwrap();
    app.wait_for("text:Playlist file").unwrap();
    app.press("text:Playlist file").unwrap();
    app.press("text:Choose a file…").unwrap();
    app.press("text:Add playlist").unwrap();
    app.wait_for_within("text:Playlist added", Duration::from_secs(120))
        .unwrap();
    app.press("text:Watch").unwrap();
    app.wait_for_within("text:Synthetic day 0", Duration::from_secs(120))
        .unwrap();
    for _ in 0..40 {
        app.press_key("down").unwrap();
    }
    app.idle(Duration::from_millis(150)).unwrap();

    let path = home.data_dir().join("iptv.sqlite");
    let lock = rusqlite::Connection::open(&path).unwrap();
    lock.pragma_update(None, "key", format!("x'{}'", "54".repeat(32)))
        .unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    // This action queues a real database write while the other connection owns
    // SQLite's writer lock. The UI change and later navigation must still render.
    app.press_key("m").unwrap();
    app.wait_for("text:Add to favourites").unwrap();
    app.press("text:Add to favourites").unwrap();
    let before: i64 = lock
        .query_row("SELECT count(*) FROM favourites", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, 0, "the pending write cannot pass the held lock");
    let timing = Arc::new(Mutex::new((None::<Instant>, None::<Duration>)));
    let observed = timing.clone();
    app.observe(move |step, batch| {
        if step.contains("keyboard.press(down)") && batch.items.iter().any(|item| matches!(item, nsh_core::MountItem::Text { text, .. } if text.text.starts_with("keys "))) {
            let mut timing = observed.lock().unwrap();
            if timing.1.is_none() { timing.1 = timing.0.map(|start| start.elapsed()); }
        }
    });
    let mut maximum = Duration::ZERO;
    for _ in 0..24 {
        *timing.lock().unwrap() = (Some(Instant::now()), None);
        app.press_key("down").unwrap();
        let elapsed = timing
            .lock()
            .unwrap()
            .1
            .expect("navigation produced a mounted frame");
        maximum = maximum.max(elapsed);
    }
    lock.execute_batch("ROLLBACK").unwrap();
    eprintln!(
        "10k channels / 7 days, held SQLite writer: maximum key-to-mounted-frame {maximum:?}"
    );
    assert!(
        maximum < Duration::from_millis(50),
        "guide input budget exceeded: {maximum:?}"
    );
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let saved: i64 = lock
            .query_row("SELECT count(*) FROM favourites", [], |row| row.get(0))
            .unwrap();
        if saved == 1 {
            break;
        }
        assert!(
            Instant::now() < end,
            "queued favourite was not saved after unlocking"
        );
        app.wait(Duration::from_millis(50)).unwrap();
    }
}

#[test]
fn root_shutdown_drains_a_favourite_write_that_is_waiting_for_sqlite() {
    use crate::personalization::{menu, start};
    let Some(home) = isolated!() else { return };
    let mut fixture = start(home, iptv_settings::Settings::new(), false);
    let lock = rusqlite::Connection::open(&fixture.path).unwrap();
    lock.pragma_update(None, "key", format!("x'{}'", "54".repeat(32)))
        .unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    menu(&mut fixture.app, "Remove from favourites");
    let source = fixture.source;
    let path = fixture.path.clone();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        lock.execute_batch("ROLLBACK").unwrap();
    });
    drop(fixture.app);
    let store = crate::isolated::open_database(path).unwrap();
    let favorites = store
        .load_channels(
            source,
            &iptv_store_sqlite::ChannelQuery {
                favourites_only: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        favorites.len(),
        2,
        "shutdown must drain the accepted edit before returning"
    );
    release.join().unwrap();
}
