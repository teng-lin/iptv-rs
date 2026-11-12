//! Whole-app snapshots. Restores are staged, then activated before any
//! settings, SQLite connection, import job or player is started. Directory
//! renames keep the previous state recoverable across an interrupted swap.
use std::cell::Cell;

mod instance;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use iptv_settings::Settings;
use iptv_store_sqlite::Store;
#[cfg(target_os = "android")]
use serde_json::Value;
use serde_json::json;

const MAGIC: &[u8; 9] = b"PATINABK\x01";
use iptv_import::backup_manifest::{
    self, MAX_BYTES, MAX_FILES, MAX_MANIFEST, allowed, entry_limit,
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static STARTED: OnceLock<std::result::Result<(), StartupError>> = OnceLock::new();

#[derive(Clone, Copy)]
pub(crate) struct StartupError {
    pub message: &'static str,
    pub fatal: bool,
}
thread_local! { static BUSY: Cell<bool> = const { Cell::new(false) }; }
struct Busy;
impl Busy {
    fn begin() -> Option<Self> {
        if BUSY.with(|busy| busy.replace(true)) {
            crate::settings_view::notify("A backup operation is already running");
            None
        } else {
            Some(Self)
        }
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with(|busy| busy.set(false));
    }
}

pub(crate) fn is_pending() -> bool {
    sibling(&crate::data::data_dir(), "restore-pending").exists()
}

type Result<T> = std::result::Result<T, &'static str>;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sibling(root: &Path, suffix: &str) -> PathBuf {
    root.with_file_name(format!(
        "{}.{}",
        root.file_name().unwrap_or_default().to_string_lossy(),
        suffix
    ))
}
fn scratch(root: &Path) -> Result<Scratch> {
    let path = sibling(
        root,
        &format!(
            "backup-work-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ),
    );
    private_dir(&path).map_err(|_| "Could not create temporary backup storage")?;
    Ok(Scratch(path))
}
fn private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Could not finish writing backup storage")
}

fn collect(root: &Path, directory: &Path, names: &mut Vec<String>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|_| "Could not read local playlist files")? {
        let entry = entry.map_err(|_| "Could not read local playlist files")?;
        let kind = entry
            .file_type()
            .map_err(|_| "Could not read local playlist files")?;
        if kind.is_symlink() {
            return Err("Linked local playlist files cannot be backed up");
        }
        if kind.is_dir() {
            collect(root, &entry.path(), names)?;
        } else if kind.is_file() {
            let name = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "Invalid local playlist path")?
                .to_str()
                .ok_or("Invalid local playlist path")?
                .to_owned();
            if !allowed(&name) {
                return Err("Invalid local playlist path");
            }
            names.push(name);
            if names.len() > MAX_FILES {
                return Err("Too many local playlist files");
            }
        }
    }
    Ok(())
}

/// An online SQLite snapshot includes committed WAL data. The caller first
/// drains queued favorite/history writes and captures settings on the UI thread.
fn export(root: &Path, destination: &Path, settings: &str) -> Result<()> {
    validate_settings(settings)?;
    let absolute_destination = if destination.is_absolute() {
        destination.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| "Could not resolve backup destination")?
            .join(destination)
    };
    let parent = absolute_destination
        .parent()
        .ok_or("Invalid backup destination")?
        .canonicalize()
        .map_err(|_| "Could not open backup destination")?;
    let absolute_destination = parent.join(
        absolute_destination
            .file_name()
            .ok_or("Invalid backup destination")?,
    );
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "Could not open application storage")?;
    let is_prepared_export = absolute_destination.parent() == Some(canonical_root.as_path())
        && absolute_destination
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("backup-export-") && n.ends_with(".patinabackup"));
    if absolute_destination.starts_with(&canonical_root) && !is_prepared_export {
        return Err("Choose a backup destination outside application storage");
    }
    let destination = absolute_destination.as_path();
    let work = scratch(root)?;
    Store::open(root.join("iptv.sqlite"))
        .and_then(|s| s.snapshot_to(&work.0.join("iptv.sqlite")))
        .map_err(|_| "Could not snapshot the library")?;
    let mut names = vec!["iptv.sqlite".to_owned(), "settings.json".to_owned()];
    collect(root, &root.join("playlists"), &mut names)?;
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries {
            let entry = entry.map_err(|_| "Could not read application files")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if allowed(&name) && name != "iptv.sqlite" && name != "settings.json" {
                if !entry.file_type().is_ok_and(|t| t.is_file()) {
                    return Err("Linked application files cannot be backed up");
                }
                names.push(name);
            }
        }
    }
    names.sort();
    let snapshot =
        Store::open(work.0.join("iptv.sqlite")).map_err(|_| "Could not check backup sources")?;
    for source in snapshot
        .sources()
        .map_err(|_| "Could not check backup sources")?
    {
        let source = snapshot
            .source(source.id)
            .map_err(|_| "Could not check backup source")?
            .ok_or("Backup source is missing")?;
        if let iptv_providers::model::SourceConfig::M3u(config) = source.source.config {
            let location = config.location.expose();
            if let Some(local) = iptv_providers::content_url::local_path(location)
                .map_err(|_| "Backup contains an invalid local playlist URL")?
            {
                let relative = local.strip_prefix(root).map_err(|_| "A local playlist is outside application storage; add it with the file picker before backing up")?;
                if !relative
                    .to_str()
                    .is_some_and(|name| names.iter().any(|entry| entry == name))
                {
                    return Err("A local playlist file is missing; add it again before backing up");
                }
            }
        }
    }
    drop(snapshot);
    let mut entries = Vec::new();
    let mut total = 0u64;
    for name in &names {
        let len = if name == "settings.json" {
            settings.len() as u64
        } else {
            fs::metadata(if name == "iptv.sqlite" {
                work.0.join(name)
            } else {
                root.join(name)
            })
            .map_err(|_| "Could not read backup files")?
            .len()
        };
        entry_limit(name, len).map_err(|_| "Backup structured entry is too large")?;
        total = total.checked_add(len).ok_or("Backup is too large")?;
        if total > MAX_BYTES {
            return Err("Backup is too large");
        }
        entries.push(json!({"name": name, "length": len}));
    }
    let root_text = root.to_str().ok_or("Invalid application storage path")?;
    let manifest = serde_json::to_vec(
        &json!({"format": "patina-whole-app", "version": 1, "root": root_text, "files": entries}),
    )
    .map_err(|_| "Could not encode backup")?;
    if manifest.len() as u64 > MAX_MANIFEST {
        return Err("Backup file list is too large");
    }
    let temporary = sibling(
        destination,
        &format!(
            "writing-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ),
    );
    let result = (|| {
        let mut output =
            private_file(&temporary).map_err(|_| "Could not create the backup file")?;
        output
            .write_all(MAGIC)
            .and_then(|_| output.write_all(&(manifest.len() as u64).to_le_bytes()))
            .and_then(|_| output.write_all(&manifest))
            .map_err(|_| "Could not write the backup file")?;
        for (name, entry) in names.iter().zip(&entries) {
            if name == "settings.json" {
                output
                    .write_all(settings.as_bytes())
                    .map_err(|_| "Could not write settings")?;
            } else {
                let path = if name == "iptv.sqlite" {
                    work.0.join(name)
                } else {
                    root.join(name)
                };
                let expected = entry["length"].as_u64().unwrap();
                let file = File::open(path).map_err(|_| "Could not read backup files")?;
                let count = io::copy(&mut file.take(expected), &mut output)
                    .map_err(|_| "Could not write backup files")?;
                if count != expected {
                    return Err("Application files changed; try the backup again");
                }
            }
        }
        output
            .sync_all()
            .map_err(|_| "Could not finish writing the backup")?;
        fs::rename(&temporary, destination).map_err(|_| "Could not save the backup file")?;
        sync_dir(destination.parent().unwrap_or(Path::new(".")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn read_small_file(path: &Path, name: &str) -> Result<String> {
    let maximum = if name == "last_channel" {
        64 << 10
    } else {
        1 << 20
    };
    if !fs::symlink_metadata(path)
        .map_err(|_| "Could not read backup file size")?
        .file_type()
        .is_file()
    {
        return Err("Backup structured entry is not a regular file");
    }
    let file = File::open(path).map_err(|_| "Could not read backup settings")?;
    entry_limit(
        name,
        file.metadata()
            .map_err(|_| "Could not read backup file size")?
            .len(),
    )
    .map_err(|_| "Backup structured entry is too large")?;
    let mut text = String::new();
    file.take(maximum + 1)
        .read_to_string(&mut text)
        .map_err(|_| "Could not read backup settings")?;
    entry_limit(name, text.len() as u64).map_err(|_| "Backup structured entry is too large")?;
    Ok(text)
}

fn validate_settings(text: &str) -> Result<()> {
    entry_limit("settings.json", text.len() as u64)
        .map_err(|_| "Backup structured entry is too large")?;
    let (_, report) = Settings::from_json(text)
        .map_err(|_| "Backup settings are invalid or from a newer version")?;
    if !report.is_clean() {
        return Err("Backup settings are not supported by this version");
    }
    Ok(())
}

/// Validates all content before replacing an existing pending restore. No
/// running database or settings file is changed by staging or cancellation.
fn stage(root: &Path, input: &mut impl Read) -> Result<()> {
    let work = scratch(root)?;
    let mut magic = [0; 9];
    input
        .read_exact(&mut magic)
        .map_err(|_| "This is not a complete Patina backup")?;
    if &magic != MAGIC {
        return Err("This is not a supported Patina backup");
    }
    let mut size = [0; 8];
    input
        .read_exact(&mut size)
        .map_err(|_| "Backup header is incomplete")?;
    let size = u64::from_le_bytes(size);
    if size > MAX_MANIFEST {
        return Err("Backup header is too large");
    }
    let mut bytes = vec![0; size as usize];
    input
        .read_exact(&mut bytes)
        .map_err(|_| "Backup header is incomplete")?;
    let manifest = backup_manifest::parse(&bytes)?;
    let old_root = Path::new(&manifest.root);
    for entry in &manifest.files {
        let name = &entry.name;
        let len = entry.length;
        let path = work.0.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| "Could not stage backup files")?;
        }
        let mut output = private_file(&path).map_err(|_| "Could not stage backup files")?;
        if io::copy(&mut input.take(len), &mut output).map_err(|_| "Could not read backup files")?
            != len
        {
            return Err("Backup is incomplete");
        }
        output
            .sync_all()
            .map_err(|_| "Could not finish staging backup files")?;
    }
    let mut extra = [0];
    if input
        .read(&mut extra)
        .map_err(|_| "Could not read backup")?
        != 0
    {
        return Err("Backup has unexpected trailing data");
    }
    let settings = read_small_file(&work.0.join("settings.json"), "settings.json")?;
    validate_settings(&settings)?;
    Store::validate_snapshot(&work.0.join("iptv.sqlite"))
        .map_err(|_| "Backup library is invalid or from a newer version")?;
    {
        let mut store =
            Store::open(work.0.join("iptv.sqlite")).map_err(|_| "Could not open backup library")?;
        store
            .rebase_local_playlists(old_root, root)
            .map_err(|_| "Backup contains unavailable local playlist files")?;
    }
    // Check rewritten local references point to files included in this archive.
    let store =
        Store::open(work.0.join("iptv.sqlite")).map_err(|_| "Could not check backup library")?;
    for source in store
        .sources()
        .map_err(|_| "Could not check backup sources")?
    {
        for guide in store
            .guide_sources(source.id)
            .map_err(|_| "Could not check backup guide sources")?
        {
            if let Some(local) = iptv_providers::content_url::local_path(guide.url.expose())
                .map_err(|_| "Backup contains an invalid guide URL")?
            {
                let relative = local
                    .strip_prefix(root)
                    .map_err(|_| "Backup guide is outside application storage")?;
                if !work.0.join(relative).is_file() {
                    return Err("Backup is missing a local guide file");
                }
            }
        }
        let source = store
            .source(source.id)
            .map_err(|_| "Could not check backup source")?
            .ok_or("Backup source is missing")?;
        if let iptv_providers::model::SourceConfig::M3u(config) = source.source.config {
            let location = config.location.expose();
            if let Some(local) = iptv_providers::content_url::local_path(location)
                .map_err(|_| "Backup contains an invalid local playlist URL")?
            {
                let relative = local.strip_prefix(root).map_err(
                    |_| "Backup references a local playlist outside application storage",
                )?;
                if !work.0.join(relative).is_file() {
                    return Err("Backup is missing a local playlist file");
                }
            }
        }
    }
    drop(store);
    // Close/checkpoint WAL before directory swap; SQLite open on next launch
    // sees a complete database, and no staging handles remain.
    recover_replacement(root)?;
    let pending = sibling(root, "restore-pending");
    let replaced = sibling(root, "restore-replaced");
    if replaced.exists() {
        fs::remove_dir_all(&replaced).map_err(|_| "Could not replace an older pending restore")?;
    }
    if pending.exists() {
        fs::rename(&pending, &replaced).map_err(|_| "Could not replace the pending restore")?;
    }
    if fs::rename(&work.0, &pending).is_err() {
        if replaced.exists() {
            let _ = fs::rename(&replaced, &pending);
        }
        return Err("Could not schedule the restore");
    }
    sync_dir(root.parent().ok_or("Application storage has no parent")?)?;
    let _ = fs::remove_dir_all(replaced);
    Ok(())
}

fn recover_replacement(root: &Path) -> Result<()> {
    let pending = sibling(root, "restore-pending");
    let replaced = sibling(root, "restore-replaced");
    if replaced.exists() {
        if pending.exists() {
            fs::remove_dir_all(replaced)
                .map_err(|_| "Could not clean up the replaced pending restore")?;
        } else {
            fs::rename(replaced, pending).map_err(|_| "Could not recover the pending restore")?;
        }
    }
    Ok(())
}

fn cancel(root: &Path) -> Result<()> {
    // Remove fallback first, so a crash during cancellation cannot resurrect
    // a superseded scheduled restore on the next launch.
    let replaced = sibling(root, "restore-replaced");
    if replaced.exists() {
        fs::remove_dir_all(replaced).map_err(|_| "Could not cancel the pending restore")?;
    }
    let pending = sibling(root, "restore-pending");
    if pending.exists() {
        fs::remove_dir_all(pending).map_err(|_| "Could not cancel the pending restore")?;
    }
    Ok(())
}

/// Called before opening any file under `root`. If a crash happened after
/// moving the original aside, finish the validated restore, or recover it.
pub(crate) fn lock() -> Result<()> {
    instance::acquire(&crate::data::data_dir())
}

pub(crate) fn startup() -> std::result::Result<(), StartupError> {
    // Keep the first outcome, including a fatal recovery failure. Recreating
    // an engine must neither rerun activation nor bypass failed recovery.
    *STARTED.get_or_init(|| {
        let root = crate::data::data_dir();
        activate(&root).map_err(|message| StartupError {
            message,
            fatal: !root.exists(),
        })
    })
}

fn activate(root: &Path) -> Result<()> {
    recover_replacement(root)?;
    // No native export is active before App starts. A killed process can leave
    // a private prepared archive; never retain those credential-bearing copies.
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name.starts_with("backup-export-") || name.starts_with("backup-import-"))
                && name.ends_with(".patinabackup")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let pending = sibling(root, "restore-pending");
    let previous = sibling(root, "restore-previous");
    if previous.exists() && !root.exists() {
        fs::rename(&previous, root).map_err(|_| "Could not recover application storage")?;
    }
    if previous.exists() && root.exists() {
        // The new directory already became active before interruption.
        fs::remove_dir_all(&previous).map_err(|_| "Could not finish the previous restore")?;
    }
    if !pending.exists() {
        if !root.exists() && previous.exists() {
            fs::rename(previous, root).map_err(|_| "Could not recover application storage")?;
        }
        return Ok(());
    }
    // Revalidate persisted staging before moving the working state aside.
    for entry in fs::read_dir(&pending).map_err(|_| "Could not read pending backup")? {
        let entry = entry.map_err(|_| "Could not read pending backup entry")?;
        if let Some(name) = entry.file_name().to_str() {
            entry_limit(
                name,
                entry
                    .metadata()
                    .map_err(|_| "Could not read backup file size")?
                    .len(),
            )
            .map_err(|_| "Backup structured entry is too large")?;
        }
    }
    Store::validate_snapshot(&pending.join("iptv.sqlite"))
        .map_err(|_| "The pending backup library is invalid; current data was kept")?;
    validate_settings(&read_small_file(
        &pending.join("settings.json"),
        "settings.json",
    )?)?;
    if root.exists() {
        fs::rename(root, &previous)
            .map_err(|_| "Could not prepare the restore; current data was kept")?;
    }
    if fs::rename(&pending, root).is_err() {
        if previous.exists() {
            let _ = fs::rename(&previous, root);
        }
        return Err("Could not activate the restore; current data was kept");
    }
    sync_dir(root.parent().ok_or("Application storage has no parent")?)?;
    if previous.exists() {
        fs::remove_dir_all(previous)
            .map_err(|_| "Restore succeeded; could not remove the previous private copy")?;
    }
    Ok(())
}

async fn worker<T: Send + 'static>(job: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx.await.map_err(|_| "The backup task stopped")?
}

pub(crate) fn export_ui() {
    crate::data::spawn_in_app(async move {
        let Some(_busy) = Busy::begin() else {
            return;
        };
        let answer = nsh_base::alert_dialog().title("Back up Patina")
            .description("The backup includes playlist logins and passwords, settings, channel arrangements, history and cached guide data. Keep the file private.")
            .cancel("cancel", "Cancel").action("export", "Choose file…").show().await;
        if !answer.is_ok_and(|a| a.action.as_deref() == Some("export")) {
            return;
        }
        if !crate::favorites::flush().await {
            crate::settings_view::notify("Recent channel changes could not be saved; try again");
            return;
        }
        let settings = crate::settings::get().to_json_string();
        let root = crate::data::data_dir();
        crate::settings_view::notify("Writing backup…");
        let result = save_export(root, settings).await;
        crate::settings_view::notify(match result {
            Ok(true) => "Backup saved",
            Ok(false) => "Backup cancelled",
            Err(message) => message,
        });
    });
}

pub(crate) fn restore_ui(ui: crate::Ui) {
    crate::data::spawn_in_app(async move {
        let Some(_busy) = Busy::begin() else {
            return;
        };
        let root = crate::data::data_dir();
        if sibling(&root, "restore-pending").exists() {
            let answer = nsh_base::alert_dialog().title("Restore scheduled")
                .description("The backup replaces current data when you next open Patina. Changes made before then will be replaced.")
                .cancel("keep", "Keep scheduled").action("cancel", "Cancel restore").action("replace", "Choose another backup…").show().await;
            match answer.ok().and_then(|a| a.action) {
                Some(action) if action.as_str() == "cancel" => {
                    let result = worker(move || cancel(&root)).await;
                    crate::settings_view::refresh(ui);
                    crate::settings_view::notify(match result {
                        Ok(()) => "Scheduled restore cancelled",
                        Err(message) => message,
                    });
                    return;
                }
                Some(action) if action.as_str() == "replace" => {}
                _ => {
                    offer_restart().await;
                    return;
                }
            }
        }
        let file = match select_import(root.clone()).await {
            Ok(Some(file)) => file,
            Ok(None) => return,
            Err(message) => {
                crate::settings_view::notify(message);
                return;
            }
        };
        let answer = nsh_base::alert_dialog().title("Replace data on next launch?")
            .description("This backup will replace all playlists, settings, favorites, groups and history when you reopen Patina. You can cancel the scheduled restore from Settings before closing the app.")
            .cancel("cancel", "Cancel").destructive("restore", "Schedule restore").show().await;
        if !answer.is_ok_and(|a| a.action.as_deref() == Some("restore")) {
            return;
        }
        crate::settings_view::notify("Checking backup…");
        let result = worker(move || {
            let mut input =
                File::open(&file.path).map_err(|_| "Could not read the chosen backup")?;
            stage(&root, &mut input)
        })
        .await;
        crate::settings_view::refresh(ui);
        crate::settings_view::notify(match result {
            Ok(()) => "Restore scheduled. Close and reopen Patina to apply it",
            Err(message) => message,
        });
        if result.is_ok() {
            offer_restart().await;
        }
    });
}

#[cfg(test)]
mod tests;

#[cfg(not(target_os = "android"))]
async fn save_export(root: PathBuf, settings: String) -> Result<bool> {
    use nsh_core::file_dialog::{FileFilter, SaveDialog};
    let destination = nsh_core::element::pick_save_path(
        SaveDialog::new()
            .file_name("Patina.patinabackup")
            .filter(FileFilter::new("Patina backup", ["patinabackup"])),
    )
    .await;
    let destination = match destination {
        Ok(Some(path)) => path,
        Ok(None) => return Ok(false),
        Err(_) => return Err("Saving backup files is unavailable on this device"),
    };
    worker(move || export(&root, &destination, &settings)).await?;
    Ok(true)
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use nsh_core::prelude::*;
    use std::cell::RefCell;
    thread_local! {
        static REQUEST: Cell<Option<Signal<Value>>> = const { Cell::new(None) };
        static ANSWER: RefCell<Option<(String, futures_channel::oneshot::Sender<Result<bool>>)>> = const { RefCell::new(None) };
    }
    pub fn layer() -> impl IntoElement {
        let request = signal(Value::Null);
        REQUEST.with(|slot| slot.set(Some(request)));
        native("tv.backup")
            .absolute()
            .w(px(0.))
            .h(px(0.))
            .prop("archivePath", move || request.get()["archivePath"].clone())
            .prop("requestId", move || request.get()["requestId"].clone())
            .prop("operation", move || request.get()["operation"].clone())
            .on_native_event(move |event: &NativeEvent| {
                if event.name.as_ref() != "complete" {
                    return;
                }
                ANSWER.with(|slot| {
                    let mut answer = slot.borrow_mut();
                    if answer.as_ref().is_some_and(|(id, _)| {
                        event.data["requestId"].as_str() == Some(id.as_str())
                    }) {
                        let (_, tx) = answer.take().unwrap();
                        let value = if event.data["success"].as_bool() == Some(true) {
                            Ok(true)
                        } else if event.data["cancelled"].as_bool() == Some(true) {
                            Ok(false)
                        } else {
                            Err("Could not read or write the chosen backup document")
                        };
                        let _ = tx.send(value);
                        request.set(Value::Null);
                    }
                });
            })
    }
    async fn document(request_id: String, path: &Path, operation: &str) -> Result<bool> {
        let Some(request) = REQUEST.with(Cell::get) else {
            return Err("The document service is unavailable");
        };
        let (tx, rx) = futures_channel::oneshot::channel();
        ANSWER.with(|slot| *slot.borrow_mut() = Some((request_id.clone(), tx)));
        request.set(json!({"requestId": request_id, "archivePath": path.to_string_lossy(), "operation": operation}));
        rx.await.map_err(|_| "The document service stopped")?
    }
    pub async fn save(root: PathBuf, settings: String) -> Result<bool> {
        let request_id = format!("{}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
        let path = root.join(format!("backup-export-{request_id}.patinabackup"));
        let prepared = path.clone();
        worker(move || export(&root, &prepared, &settings)).await?;
        let result = document(request_id, &path, "export").await;
        let _ = fs::remove_file(path);
        result
    }
    pub async fn close() {
        let request_id = format!("{}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
        let _ = document(request_id, &crate::data::data_dir(), "close").await;
    }
    pub async fn select(root: PathBuf) -> Result<Option<ImportFile>> {
        let request_id = format!("{}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
        let path = root.join(format!("backup-import-{request_id}.patinabackup"));
        let file = ImportFile {
            path,
            temporary: true,
        };
        if document(request_id, &file.path, "import").await? {
            Ok(Some(file))
        } else {
            Ok(None)
        }
    }
}

#[cfg(target_os = "android")]
async fn save_export(root: PathBuf, settings: String) -> Result<bool> {
    android::save(root, settings).await
}

pub(crate) fn service_layer() -> impl nsh_core::prelude::IntoElement {
    #[cfg(target_os = "android")]
    {
        android::layer()
    }
    #[cfg(not(target_os = "android"))]
    {
        use nsh_core::prelude::*;
        view().absolute().w(px(0.)).h(px(0.))
    }
}

struct ImportFile {
    path: PathBuf,
    temporary: bool,
}
impl Drop for ImportFile {
    fn drop(&mut self) {
        if self.temporary {
            let _ = fs::remove_file(&self.path);
        }
    }
}
#[cfg(not(target_os = "android"))]
async fn select_import(_root: PathBuf) -> Result<Option<ImportFile>> {
    use nsh_core::file_dialog::{FileFilter, OpenDialog};
    match nsh_core::element::pick_paths(
        OpenDialog::files().filter(FileFilter::new("Patina backup", ["patinabackup"])),
    )
    .await
    {
        Ok(Some(paths)) => Ok(paths.into_iter().next().map(|path| ImportFile {
            path,
            temporary: false,
        })),
        Ok(None) => Ok(None),
        Err(_) => Err("Opening backup documents is unavailable on this device"),
    }
}
#[cfg(target_os = "android")]
async fn select_import(root: PathBuf) -> Result<Option<ImportFile>> {
    android::select(root).await
}

async fn offer_restart() {
    #[cfg(target_os = "android")]
    {
        let answer = nsh_base::alert_dialog().title("Restore scheduled")
            .description("Close Patina, then open it again to apply the backup. Keep the app open to cancel the scheduled restore from Settings first.")
            .cancel("keep", "Keep app open").action("close", "Close Patina").show().await;
        if answer.is_ok_and(|a| a.action.as_deref() == Some("close")) {
            android::close().await;
        }
    }
}
