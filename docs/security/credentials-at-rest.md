# Catalog encryption

Production opens the catalog through the store worker and `credential_key::open_database`.
SQLCipher encrypts the complete database, including source credentials, stream and guide
URLs, headers, cached catalog rows, search indexes and WAL pages. The app's guide and
catalog scratch databases must use `Store::open_staging_connection` and
`Store::attach_staging_database`; a separately opened SQLCipher file needs the raw key
again when attached. Temporary SQLite sorting uses memory.

The random 256-bit database key is held by the platform credential broker:

- Linux: Secret Service, through stable keyring/dbus-secret-service libraries. The
  default persistent collection must be available and unlocked. A locked or missing
  broker produces a store-unavailable error; the app never falls back to plaintext.
  Unlock prompts have a maximum thirty-second wait rather than blocking forever.
- macOS and iOS: the application-specific Keychain entry. Apple builds use SQLCipher's
  CommonCrypto backend, avoiding another OpenSSL alongside MPVKit.
- Android: a non-exportable Android Keystore AES-256 key wraps the random database key
  using AES-GCM. Only the authenticated ciphertext is persisted in `noBackupFilesDir`.
  The host installs the unwrapped key through JNI before mounting the Rust app. A
  missing key, malformed ciphertext, or authentication failure leaves storage closed.

Keys and keyring payloads are zeroized on drop and never included in Debug or errors.
The Linux/Apple broker stores a hexadecimal secret so KDE Wallet's UTF-8-only Secret
Service implementation is supported. First key creation is serialized by a filesystem
lock. The app's existing instance lock prevents competing production store workers.

On opening a legacy plaintext database, the store checkpoints committed WAL data,
exports into a private encrypted sibling, applies every pending schema migration there,
checks integrity, syncs it and atomically renames it over the original. An export or
migration failure keeps the original usable. A stale encrypted scratch copy from an
interruption is discarded before retry. No plaintext rollback copy is retained after
successful migration. This cannot erase plaintext copies in pre-existing backups,
filesystem snapshots or storage-device remapping; it protects newly copied live files.

App-owned picked playlist copies also use SQLCipher, with document bytes stored in
64 KiB chunks. The importer reads these through a bounded decrypted stream; no
plaintext replacement file is needed. Existing copies are sealed atomically during a one-time migration recorded inside
the encrypted catalog, including compressed files and prior failed-import leftovers.
Routine readers never reinterpret damaged ciphertext as a new plaintext document;
symbolic links are refused. A failed file pick never writes plaintext into app storage.

Portable backups are explicitly user-exported plaintext archives so they can be
restored on another device without its original keystore. `snapshot_to` uses
`sqlcipher_export` for that conversion because SQLite's backup API cannot cross the
SQLCipher codec boundary. A restored legacy/plaintext catalog is encrypted with the
receiving device's key before the restore is handed off as persistent pending state;
its copied playlists are sealed too. Creating or validating an explicitly requested
portable export/import can use private mode-0600 plaintext scratch files, which are
removed on completion or the next startup after an interruption. This is the same
plaintext exposure intentionally authorized for the portable archive, not normal
catalog or playlist persistence. Backup passphrases remain a separate feature.
Backups need the same care as the
provider credentials they contain. Losing the OS credential store makes its existing
encrypted catalog inaccessible; a portable backup can be restored on a new installation.

`Store::open` remains an explicit plaintext API for synthetic fixtures and portable
backup validation. Production entry points use `open_encrypted`. Headless app tests
inject a synthetic key through the nondefault `test-support` feature; release builds
have no environment-variable key override or plaintext bypass. A test-support build
without an explicitly installed synthetic key fails closed without contacting any
platform credential broker.

Validation includes wrong-key/plain-reader rejection, raw database and WAL scans,
legacy schemas, a committed uncheckpointed legacy WAL, migration failure preservation,
portable backup roundtrips, and encrypted scratch attachment. The ignored Linux broker
integration test must run inside an isolated DBus session and temporary keyring data
root with `IPTV_TEST_ISOLATED_BROKER=1`; it must never target the user's login keyring.

On `tab36` (API 36, primary user), a synthetic schema-7 catalog containing a
fixture password and credential-bearing URL migrated to a 241,664-byte encrypted
file. Neither marker appeared in the copied database/WAL, and unkeyed SQLite
rejected the copy. A process restart reopened the catalog. Tampering with one
AES-GCM ciphertext byte caused a typed Keystore-unavailable error and left the
catalog hash unchanged; restoring the original wrapped ciphertext restored access.
The wrapped key was 60 bytes, with the key file, database and sidecars all mode
0600. This validates the encryption boundary before the separate Android storage
path changes are integrated; final release checks also exercise the integrated app.
