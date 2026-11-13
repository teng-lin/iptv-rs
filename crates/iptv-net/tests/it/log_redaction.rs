//! Generic URL diagnostics never reveal synthetic path or query credentials.
use super::support;

use iptv_import::{CancelToken, ImportOptions, NoProgress, import_source};
use iptv_providers::{
    SecretUrl,
    model::{M3uSourceConfig, SourceConfig},
};
use std::sync::Mutex;

struct Capture(Mutex<Vec<String>>);
static LOG: Capture = Capture(Mutex::new(Vec::new()));
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, r: &log::Record<'_>) {
        if r.target().starts_with("iptv_") {
            self.0.lock().unwrap().push(r.args().to_string());
        }
    }
    fn flush(&self) {}
}

#[tokio::test]
async fn generic_playlist_request_logs_only_origin() {
    const CHILD: &str = "IPTV_LOG_REDACTION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "log_redaction::generic_playlist_request_logs_only_origin",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "isolated log regression failed: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    log::set_logger(&LOG).unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    let server = support::TestServer::http(|_| {
        support::Resp::ok("#EXTM3U\n#EXTINF:-1,Synthetic\nhttps://media.example.test/live.ts\n")
    })
    .await;
    let config = SourceConfig::M3u(M3uSourceConfig {
        location: SecretUrl::new(server.url(
            "/generated-path-token/list.m3u?api_key=generated-query-token#generated-fragment-token",
        )),
        user_agent: None,
        referrer: None,
    });
    import_source(
        &iptv_net::NetClient::with_defaults().unwrap(),
        &config,
        &ImportOptions::new(0),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .await
    .unwrap();
    let lines = LOG.0.lock().unwrap().join("\n");
    assert!(lines.contains(&server.url("")), "{lines}");
    for token in [
        "generated-path-token",
        "generated-query-token",
        "generated-fragment-token",
    ] {
        assert!(!lines.contains(token), "{lines}");
    }
}
