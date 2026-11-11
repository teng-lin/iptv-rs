mod personalization;
use iptv_settings::{Settings, Value};
use nsh_core::ViewKind;
use personalization::*;
use std::time::Duration;
#[test]
fn overlay_stay_policy_is_independent_and_back_returns_to_video() {
    let mut s = Settings::new();
    s.set("guide.browse_mode", Value::choice("overlay"))
        .unwrap();
    s.set("guide.stay_on_guide", Value::Bool(false)).unwrap();
    s.set("guide.stay_on_guide_overlay", Value::Bool(true))
        .unwrap();
    let mut f = start(s, true);
    let app = &mut f.app;
    step(app.press_key("enter"));
    step(app.run_for(Duration::from_millis(500)));
    let players: Vec<_> = app
        .screen()
        .layout_nodes()
        .into_iter()
        .filter(|n| n.kind == ViewKind::Native)
        .collect();
    assert_eq!(players.len(), 1);
    let id = players[0].id;
    assert_eq!(players[0].bounds.size.width.0, 1920.0);
    // Guide menu confirms it stayed in browsing despite the preview policy being Off.
    step(app.press_key("m"));
    step(app.wait_for("text:Manage favourites…"));
    step(app.back());
    step(app.back());
    step(app.press_key("enter"));
    step(app.wait_for("id:player-card"));
    step(app.back());
    step(app.back());
    // Re-entering the guide keeps its configured overlay layout and surface identity.
    step(app.press_key("m"));
    step(app.wait_for("text:Manage favourites…"));
    let player = app
        .screen()
        .layout_nodes()
        .into_iter()
        .find(|n| n.kind == ViewKind::Native)
        .unwrap();
    assert_eq!(player.id, id);
    assert_eq!(player.bounds.size.width.0, 1920.0);
}
