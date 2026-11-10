//! Real data: imports a playlist and its EPG off the UI thread, saves the
//! catalog in the SQLite store, and swaps the guide to the imported
//! schedule. Until the playlist wizard lands, the source is the synthetic
//! development playlist on the local mock server (`tools/mock-server.py`
//! with `tools/make-iptv-fixtures.py`). On failure the synthetic guide stays.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_channel::oneshot;
use iptv_epg::{GuideStore, Overrides};
use iptv_guide::EpgSchedule;
use iptv_import::{CancelToken, Catalog, ImportOptions, NoProgress, import_epg, import_source};
use iptv_net::{NetClient, NetConfig};
use iptv_providers::SecretUrl;
use iptv_providers::model::{M3uSourceConfig, Source, SourceConfig};
use iptv_store_sqlite::Store;
use nsh_core::prelude::*;

use crate::Ui;

/// The synthetic development playlist (the emulator reaches the host as 10.0.2.2).
#[cfg(target_os = "android")]
const DEV_PLAYLIST: &str = "http://10.0.2.2:8089/iptv/playlist.m3u";
#[cfg(not(target_os = "android"))]
const DEV_PLAYLIST: &str = "http://127.0.0.1:8089/iptv/playlist.m3u";
const DEV_SOURCE_NAME: &str = "Dev playlist";

struct Imported {
    catalog: Catalog,
    guide: GuideStore,
    now: i64,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Where the app keeps its files: the Android app's files directory, or
/// `~/.local/share/iptv-rs` on the desktop.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        let cmdline = std::fs::read("/proc/self/cmdline").unwrap_or_default();
        let package = cmdline
            .split(|b| *b == 0)
            .next()
            .map(|p| String::from_utf8_lossy(p).into_owned());
        let package = package.unwrap_or_else(|| "com.hantekllc.iptvrs".to_owned());
        PathBuf::from(format!(
            "/data/data/{}/files",
            package.split(':').next().unwrap_or_default()
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join(".local/share/iptv-rs")
    }
}

/// Starts the import; when it finishes the guide switches to the result.
pub fn start(ui: Ui) {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run());
    });
    let _ = spawn(async move {
        match rx.await {
            Ok(Ok(imported)) => {
                let channels = imported.catalog.channels.len();
                let schedule = EpgSchedule::from_catalog(
                    &imported.catalog.groups,
                    &imported.catalog.channels,
                    imported.guide,
                    imported.now,
                );
                log::info!(
                    "iptv data: imported {channels} channels; guide switched to the playlist"
                );
                crate::set_schedule(&ui, Rc::new(schedule));
            }
            Ok(Err(reason)) => {
                log::warn!("iptv data: import failed ({reason}); keeping the synthetic guide")
            }
            Err(_) => log::warn!("iptv data: import thread ended without a result"),
        }
    });
}

/// The import itself, on its own thread with a small tokio runtime.
fn run() -> Result<Imported, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("runtime: {e}"))?;
    runtime.block_on(async {
        let net = NetClient::new(NetConfig::default())
            .map_err(|_| "network client setup failed".to_owned())?;
        let now = now_unix();
        let source = Source {
            name: DEV_SOURCE_NAME.to_owned(),
            config: SourceConfig::M3u(M3uSourceConfig {
                location: SecretUrl::new(DEV_PLAYLIST),
                user_agent: None,
                referrer: None,
            }),
        };
        let cancel = CancelToken::new();
        let catalog = import_source(
            &net,
            &source.config,
            &ImportOptions::new(now),
            &mut NoProgress,
            &cancel,
        )
        .await
        .map_err(|e| format!("playlist: {:?}", e.kind()))?;
        save(&source, &catalog).map_err(|e| format!("store: {e}"))?;
        let epg_sources = catalog.epg_sources(1);
        let epg = import_epg(
            &net,
            &epg_sources,
            &catalog.channels,
            &Overrides::default(),
            now,
            &mut NoProgress,
            &cancel,
        )
        .await
        .map_err(|e| format!("guide: {:?}", e.kind()))?;
        Ok(Imported {
            catalog,
            guide: epg.guide,
            now,
        })
    })
}

/// Saves the catalog under the dev source (created on first run).
fn save(source: &Source, catalog: &Catalog) -> Result<(), iptv_store_sqlite::StoreError> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut store = Store::open(dir.join("iptv.sqlite"))?;
    let existing = store
        .sources()?
        .into_iter()
        .find(|s| s.name == source.name)
        .map(|s| s.id);
    let id = match existing {
        Some(id) => id,
        None => store.add_source(source)?,
    };
    store.replace_catalog(
        id,
        &catalog.groups,
        &catalog.channels,
        &catalog.vod,
        &catalog.series,
    )?;
    log::info!(
        "iptv data: saved {} channels in {}",
        catalog.channels.len(),
        dir.display()
    );
    Ok(())
}
