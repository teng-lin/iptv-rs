# Whole-app backup and restore

Settings → General → Back up writes a portable `.patinabackup` file. The
export dialog explains that the archive contains playlist logins/passwords.
The archive is not encrypted: keep it private. Temporary files, staged
restores and desktop exports use private filesystem permissions.

Settings → General → Restore a backup checks the selected file before
scheduling a replacement. Current playback and library data continue to work.
The General page shows that a restore is scheduled; opening Restore again
lets you cancel it or choose another backup. Changes made after scheduling
are replaced by the backup when the Patina process restarts. Android offers Close Patina after scheduling;
open the launcher icon again to apply. If you keep the app open, reopening
Restore offers that close action again. Desktop users quit and reopen normally.
Recreating an Activity/window in the same process does not apply a restore.

## Contents

The archive contains a consistent online SQLite snapshot, including committed
WAL pages. It preserves all database tables, including:

- Provider configurations and credentials, active playlist/All playlists mode,
  combined-view last channel, refresh policies,
  guide-source configuration, and per-channel guide assignments.
- Cached channels/groups/VOD metadata and cached EPG/programme descriptions.
- Favorites and their order, channel/group names, visibility and manual order,
  custom groups/memberships/order, and viewing history.
- Versioned settings, search history, and per-source last-channel files.
- Playlist files copied into application storage by the playlist picker.
  Their stored paths are rewritten for the destination device/home.

It does not contain media streams or video files, OS permissions, native
window geometry, running playback/decoder state, or unfinished downloads.
A local playlist referenced outside application storage must first be added
through the playlist file picker; export refuses to create an incomplete
backup of such a source. Cached guides are included, so archives may be large.

## Platforms and limits

Linux/macOS use native Save/Open dialogs. Android uses the Storage Access
Framework's Create/Open Document dialogs with no broad storage permission.
Database and file I/O, Android document copies and cancellation cleanup run
off the UI thread. Imports and exports stream their contents; the application
does not allocate the entire archive in memory.

The current iOS host has no streaming document-save/import service. Both
backup commands remain explicitly unavailable there pending that integration.
This is a platform limitation, not a claim that an app-private file is an
exported backup.

Version 1 has a fixed magic/version header, bounded JSON file manifest and
binary file payloads. Limits are 8 GiB of content, 20,000 files and 4 MiB of
manifest metadata; sufficient free storage is needed for the archive, staged
replacement and previous data. Future archive versions and unsupported
settings are rejected. Known historical database schemas are validated and
migrated while staged, before replacing current data.

## Restore safety

Every path is checked against an application-file allowlist; absolute paths,
traversal, duplicate entries, missing required files, truncated archives and
unexpected trailing data are rejected. Settings are validated, and SQLite
schema/migration history, integrity and foreign keys are checked. Linked local
files are refused. Rejected archives leave current data and an already
scheduled valid restore intact.

An exclusive application-lifetime lock lives beside the application directory.
A second process shows an already-running message before opening settings or
SQLite. The first startup outcome is retained across Activity/root recreation,
including fatal recovery failures. Activation happens before settings loading
or any database/import/player workers start. The current application directory is renamed aside and the
validated staged directory takes its place. If interrupted, the next startup
recovers the previous directory before checking a pending replacement, recovers
a scheduled restore interrupted while replacing it, or
finishes cleanup if the new directory was already installed. The persistent
player surface is unaffected while a restore is merely scheduled.

## Validation

Synthetic tests cover online WAL snapshots, retained credentials/catalogs,
custom groups/favorites/order/history, historical/newer/malformed databases,
private permissions, local-file migration to another home, cancellation,
invalid/truncated/path-traversal archives, interrupted directory swaps, and
a headless settings export → schedule → cancel → schedule → restart journey.
Android Kotlin compilation and an owned `tab36` emulator smoke run supplement
the portable/native Rust checks; no real provider data is used.
