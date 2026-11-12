//! Explicit browsing policy for older player journeys. Default delay/close
//! behavior is exercised by the personalization integration tests.
pub fn two_step_preview(home: &std::path::Path) {
    let mut s = iptv_settings::Settings::new();
    s.set("guide.stay_on_guide", iptv_settings::Value::Bool(true))
        .unwrap();
    s.set("guide.stay_on_list", iptv_settings::Value::Bool(true))
        .unwrap();
    s.set("guide.switch_delay", iptv_settings::Value::ms(0))
        .unwrap();
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/iptv-rs")
    } else {
        home.join(".local/share/iptv-rs")
    };
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), s.to_json_string()).unwrap();
}
