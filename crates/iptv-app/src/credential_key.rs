//! The database key never lives in application preferences or the database tree.
//! Blocking platform credential access runs exclusively on the store worker.
use std::path::Path;

use iptv_store_sqlite::{DatabaseKey, Store, StoreError};
#[cfg(all(target_os = "linux", not(feature = "test-support")))]
mod linux;
#[cfg(all(
    any(target_os = "macos", target_os = "ios"),
    not(feature = "test-support")
))]
use keyring::Entry;
#[cfg(all(target_os = "linux", not(feature = "test-support")))]
use linux::Entry;

#[cfg(all(target_os = "android", not(feature = "test-support")))]
mod android;

#[cfg(feature = "test-support")]
static TEST_KEY: std::sync::OnceLock<DatabaseKey> = std::sync::OnceLock::new();

/// Explicit synthetic key injection, absent from production builds.
#[cfg(feature = "test-support")]
pub fn install_test_key(bytes: [u8; 32]) {
    let _ = TEST_KEY.set(DatabaseKey::from_bytes(bytes));
}

pub(crate) fn open_database(path: &Path) -> Result<Store, StoreError> {
    #[cfg(feature = "test-support")]
    {
        let key = TEST_KEY.get().ok_or(StoreError::Unavailable(
            "synthetic database key was not installed",
        ))?;
        let store = Store::open_encrypted(path, key)?;
        seal_documents(store, path)
    }
    #[cfg(not(feature = "test-support"))]
    {
        let key = platform_key(path)?;
        let store = Store::open_encrypted(path, &key)?;
        seal_documents(store, path)
    }
}

#[cfg(all(target_os = "android", not(feature = "test-support")))]
fn platform_key(_: &Path) -> Result<DatabaseKey, StoreError> {
    android::database_key()
}

#[cfg(all(
    not(feature = "test-support"),
    any(target_os = "linux", target_os = "macos", target_os = "ios")
))]
fn platform_key(path: &Path) -> Result<DatabaseKey, StoreError> {
    use std::io::Read;
    use zeroize::Zeroizing;
    // Serialize first use across processes, before creating the keyring entry.
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".key-lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(Path::new(&lock_name))
        .map_err(|_| StoreError::Unavailable("cannot lock the credential broker"))?;
    lock.lock()
        .map_err(|_| StoreError::Unavailable("cannot lock the credential broker"))?;
    let entry = Entry::new("org.iptv-rs.database", "catalog-v1")
        .map_err(|_| StoreError::Unavailable("platform credential broker could not be opened"))?;
    let secret = match entry.get_secret() {
        Ok(secret) => Zeroizing::new(secret),
        Err(keyring::Error::NoEntry) => {
            let mut header = [0; 16];
            match std::fs::File::open(path) {
                Ok(mut file) => {
                    let count = file
                        .read(&mut header)
                        .map_err(|_| StoreError::Unavailable("cannot inspect existing database"))?;
                    if count != 0 && &header != b"SQLite format 3\0" {
                        return Err(StoreError::Unavailable(
                            "database key is missing from the platform credential broker",
                        ));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(StoreError::Unavailable("cannot inspect existing database")),
            }
            let mut bytes = Zeroizing::new(vec![0; 32]);
            getrandom::fill(&mut bytes)
                .map_err(|_| StoreError::Unavailable("secure random generator failed"))?;
            // UTF-8 encoding also supports KDE Wallet's Secret Service backend.
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let encoded = Zeroizing::new(
                bytes
                    .iter()
                    .flat_map(|byte| [HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]])
                    .collect::<Vec<_>>(),
            );
            entry.set_secret(&encoded).map_err(|_| {
                StoreError::Unavailable(
                    "platform credential broker could not save the database key",
                )
            })?;
            encoded
        }
        Err(_) => {
            return Err(StoreError::Unavailable(
                "platform credential broker is locked or unavailable",
            ));
        }
    };
    let mut bytes = [0; 32];
    if secret.len() != bytes.len() * 2 {
        return Err(StoreError::Unavailable(
            "platform database key has an invalid length",
        ));
    }
    for (byte, pair) in bytes.iter_mut().zip(secret.as_chunks::<2>().0.iter()) {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        };
        let (Some(high), Some(low)) = (digit(pair[0]), digit(pair[1])) else {
            zeroize::Zeroize::zeroize(&mut bytes);
            return Err(StoreError::Unavailable(
                "platform database key has an invalid encoding",
            ));
        };
        *byte = (high << 4) | low;
    }
    let key = DatabaseKey::from_bytes(bytes);
    zeroize::Zeroize::zeroize(&mut bytes);
    Ok(key)
}

#[cfg(all(
    not(feature = "test-support"),
    not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))
))]
fn platform_key(_: &Path) -> Result<DatabaseKey, StoreError> {
    Err(StoreError::Unavailable(
        "no platform credential broker for this target",
    ))
}

#[cfg(all(test, target_os = "linux", not(feature = "test-support")))]
mod tests {
    use super::*;
    /// Run only inside a new dbus-run-session with an isolated unlocked
    /// gnome-keyring-daemon. Never touches the user's login keyring.
    #[test]
    #[ignore = "requires an isolated Secret Service daemon"]
    fn isolated_secret_service_persists_key_and_reopens_catalog() {
        assert_eq!(
            std::env::var("IPTV_TEST_ISOLATED_BROKER").as_deref(),
            Ok("1")
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let first = platform_key(&path).unwrap();
        let mut store = Store::open_encrypted(&path, &first).unwrap();
        store.set_combined_last_channel("synthetic-secret").unwrap();
        drop(store);
        drop(first);
        let second = platform_key(&path).unwrap();
        let reopened = Store::open_encrypted(&path, &second).unwrap();
        assert_eq!(
            reopened.combined_last_channel().unwrap().as_deref(),
            Some("synthetic-secret")
        );
        assert!(
            !std::fs::read(&path)
                .unwrap()
                .windows(16)
                .any(|bytes| bytes == b"synthetic-secret")
        );
    }
}

#[cfg(all(test, feature = "test-support"))]
mod test_support_tests {
    #[test]
    fn absent_synthetic_key_never_reaches_the_platform_broker() {
        // This unit-test process never installs the integration harness key.
        assert!(super::TEST_KEY.get().is_none());
        let directory = tempfile::tempdir().unwrap();
        let result = super::open_database(&directory.path().join("synthetic.sqlite"));
        assert!(matches!(
            result,
            Err(iptv_store_sqlite::StoreError::Unavailable(
                "synthetic database key was not installed"
            ))
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

fn seal_documents(store: Store, path: &Path) -> Result<Store, StoreError> {
    if let Some(root) = path.parent() {
        crate::private_files::cleanup_temporary(&root.join("playlists"))?;
    }
    if !store.private_documents_sealed()? {
        if let Some(root) = path.parent() {
            crate::private_files::seal_tree(&store, &root.join("playlists"))?;
        }
        store.mark_private_documents_sealed()?;
    }
    Ok(store)
}

/// Revalidate an encrypted pending restore; strictly validate and seal legacy
/// plaintext pending content before it can become active.
#[cfg(not(test))]
pub(crate) fn validate_staged_database(path: &Path) -> Result<(), StoreError> {
    #[cfg(feature = "test-support")]
    let key = TEST_KEY.get().cloned().ok_or(StoreError::Unavailable(
        "synthetic database key was not installed",
    ))?;
    #[cfg(not(feature = "test-support"))]
    let key = platform_key(path)?;
    validate_staged_with_key(path, &key)
}

fn validate_staged_with_key(path: &Path, key: &DatabaseKey) -> Result<(), StoreError> {
    use std::io::Read;
    let mut header = [0; 16];
    let mut file = std::fs::File::open(path)
        .map_err(|_| StoreError::Unavailable("pending restore cannot be read"))?;
    let plaintext = file.read_exact(&mut header).is_ok() && &header == b"SQLite format 3\0";
    drop(file);
    if plaintext {
        Store::validate_snapshot(path)?;
        // Commit this BEFORE encryption. A crash after database conversion but
        // before document sealing must still resume sealing on the next open,
        // even when the portable source device had already set this marker.
        let store = Store::open(path)?;
        store.mark_private_documents_unsealed()?;
        drop(store);
    } else {
        Store::validate_encrypted_snapshot(path, key)?;
    }
    let store = Store::open_encrypted(path, key)?;
    drop(seal_documents(store, path)?);
    Store::validate_encrypted_snapshot(path, key)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::io::Read;

    #[test]
    #[cfg(unix)]
    fn legacy_pending_resumes_document_sealing_after_database_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iptv.sqlite");
        let store = Store::open(&path).unwrap();
        store.mark_private_documents_sealed().unwrap();
        drop(store);
        let documents = dir.path().join("playlists");
        std::fs::create_dir(&documents).unwrap();
        let document = documents.join("synthetic.m3u");
        let bytes = b"#EXTM3U\nhttps://synthetic.example.test/private\n";
        std::fs::write(&document, bytes).unwrap();
        let blocked = documents.join("linked");
        std::os::unix::fs::symlink(&document, &blocked).unwrap();
        let key = DatabaseKey::from_bytes([0x62; 32]);
        // The database conversion succeeds, but a refused document prevents
        // publication. Restart must not trust the source device's old marker.
        assert!(validate_staged_with_key(&path, &key).is_err());
        Store::validate_encrypted_snapshot(&path, &key).unwrap();
        assert!(
            !Store::open_encrypted(&path, &key)
                .unwrap()
                .private_documents_sealed()
                .unwrap()
        );
        std::fs::remove_file(blocked).unwrap();
        validate_staged_with_key(&path, &key).unwrap();
        let store = Store::open_encrypted(&path, &key).unwrap();
        assert!(store.private_documents_sealed().unwrap());
        let mut actual = Vec::new();
        store
            .read_private_file(&document)
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, bytes);
    }

    #[test]
    fn sealed_store_still_cleans_interrupted_new_document_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iptv.sqlite");
        let key = DatabaseKey::from_bytes([0x63; 32]);
        let store = Store::open_encrypted(&path, &key).unwrap();
        store.mark_private_documents_sealed().unwrap();
        let documents = dir.path().join("playlists");
        std::fs::create_dir(&documents).unwrap();
        for name in [".iptv-sealing-new", ".iptv-sealing-new-journal"] {
            std::fs::write(documents.join(name), b"interrupted encrypted write").unwrap();
        }
        drop(seal_documents(store, &path).unwrap());
        assert_eq!(std::fs::read_dir(documents).unwrap().count(), 0);
    }
}
