//! Portable validation shared by restore and its fuzz/property tests.
//! Parsing never touches files; callers stage only these validated entries.
use serde_json::Value;
use std::collections::BTreeSet;

pub const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
pub const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 20_000;
#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub root: String,
    pub files: Vec<Entry>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub length: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub struct EntryLimit;
pub fn entry_limit(name: &str, bytes: u64) -> Result<(), EntryLimit> {
    let max = match name {
        "settings.json" | "search-history.json" => 1 << 20,
        n if n == "last_channel" || n.starts_with("last_channel-") => 64 << 10,
        _ => MAX_BYTES,
    };
    if bytes > max { Err(EntryLimit) } else { Ok(()) }
}
pub fn allowed(name: &str) -> bool {
    !name.contains(['\\', ':', '\0'])
        && name
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
        && (matches!(
            name,
            "iptv.sqlite" | "settings.json" | "search-history.json" | "last_channel"
        ) || name
            .strip_prefix("last_channel-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            || name.starts_with("playlists/"))
}
pub fn parse(bytes: &[u8]) -> Result<Manifest, &'static str> {
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("Backup header is too large");
    }
    let manifest: Value = serde_json::from_slice(bytes).map_err(|_| "Backup header is invalid")?;
    if manifest["format"] != "patina-whole-app" || manifest["version"] != 1 {
        return Err("Backup format is not supported by this version");
    }
    let root = manifest["root"]
        .as_str()
        .ok_or("Backup storage path is missing")?;
    if !root.starts_with('/') || root.contains('\0') {
        return Err("Backup storage path is invalid");
    }
    let files = manifest["files"]
        .as_array()
        .ok_or("Backup file list is missing")?;
    if files.len() > MAX_FILES {
        return Err("Backup has too many files");
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut out = Vec::new();
    for entry in files {
        let name = entry["name"]
            .as_str()
            .ok_or("Backup file name is invalid")?;
        if !allowed(name) || !seen.insert(name) {
            return Err("Backup contains an unsafe or duplicate file path");
        }
        let length = entry["length"]
            .as_u64()
            .ok_or("Backup file size is invalid")?;
        entry_limit(name, length).map_err(|_| "Backup structured entry is too large")?;
        total = total.checked_add(length).ok_or("Backup is too large")?;
        if total > MAX_BYTES {
            return Err("Backup is too large");
        }
        out.push(Entry {
            name: name.into(),
            length,
        });
    }
    if !seen.contains("iptv.sqlite") || !seen.contains("settings.json") {
        return Err("Backup is missing the library or settings");
    }
    Ok(Manifest {
        root: root.into(),
        files: out,
    })
}
