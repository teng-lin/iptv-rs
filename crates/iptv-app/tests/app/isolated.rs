//! Each scenario in a process of its own, inside one test binary.
//!
//! The app keeps its data under `$HOME` and holds process-wide state
//! (statics, thread-locals, the headless engine's threads), so two
//! scenarios cannot share a process. A scenario's `#[test]` starts with
//! [`isolated!`]. Under `cargo test` that runs this binary again for just
//! this test (`--exact <name> --nocapture --test-threads=1`, [`CHILD`]
//! naming it, a fresh, empty `$HOME`), forwards the child's output (shown
//! when it fails, or with `--nocapture`) and returns `None`; in the child
//! it returns the `$HOME` to use and the scenario runs. Under
//! cargo-nextest, whose processes run one test each, the scenario runs at
//! once with a fresh `$HOME` of its own.
//!
//! Each scenario's local time is near noon ([`time_zone`]): its process
//! gets a fixed-offset `TZ`, whatever the machine's clock and zone say, so
//! no scenario's guide (a few hours either side of now) crosses midnight
//! and no time shows a weekday prefix by chance of the hour it runs.
//!
//! At most [`JOBS`] scenario processes run at once (default 6: each is a
//! headless app with threads of its own); other tests stay as parallel as
//! the harness makes them.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Condvar, Mutex, PoisonError};

use tempfile::TempDir;

/// The scenario (its full test name) a child process runs.
pub const CHILD: &str = "IPTV_RS_TEST_CHILD";
/// How many scenario processes may run at once.
pub const JOBS: &str = "IPTV_RS_TEST_JOBS";
const DEFAULT_JOBS: usize = 6;
/// A file the child writes when it reaches the scenario: a name that
/// selects no test exits successfully too.
const RAN: &str = "IPTV_RS_TEST_RAN";
/// `HH:MM`: the local time scenarios start at instead of noon (e.g. 23:50
/// to see what crosses midnight).
pub const LOCAL_TIME: &str = "IPTV_RS_TEST_LOCAL_TIME";

/// `let Some(home) = isolated!() else { return };` first in a scenario:
/// the rest runs in a process of its own, `home` its `$HOME`.
macro_rules! isolated {
    () => {{
        fn here() {}
        $crate::isolated::run(::std::any::type_name_of_val(&here))
    }};
}

/// The scenario's `$HOME`, fresh and empty, removed after it.
pub enum Home {
    /// Made by the parent process, which removes it.
    Given(PathBuf),
    /// Made here (one test per process).
    Owned(TempDir),
}

impl Home {
    pub fn path(&self) -> &Path {
        match self {
            Home::Given(path) => path,
            Home::Owned(dir) => dir.path(),
        }
    }

    /// The app's native data directory inside this scenario's home.
    pub fn data_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.path().join("Library/Application Support/iptv-rs")
        } else {
            self.path().join(".local/share/iptv-rs")
        }
    }
}

/// `here` is the type name of a function nested in the test:
/// `app::<module>::<test>::here`.
pub fn run(here: &str) -> Option<Home> {
    initialize_key();
    let name = here
        .strip_suffix("::here")
        .and_then(|p| p.split_once("::"))
        .map(|(_crate, name)| name)
        .unwrap_or_else(|| panic!("not a test's function: {here}"));
    match std::env::var(CHILD) {
        Ok(child) if child == name => {
            let home = std::env::var_os("HOME").expect("the parent sets HOME");
            let ran = std::env::var_os(RAN).expect("the parent names the marker");
            std::fs::write(ran, name).unwrap();
            return Some(Home::Given(PathBuf::from(home)));
        }
        Ok(child) => panic!("the process for {child} reached {name}"),
        Err(_) => {}
    }
    if one_test_per_process() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: this process runs this one test, which sets HOME before
        // anything reads the environment.
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("TZ", time_zone());
        }
        return Some(Home::Owned(home));
    }
    let _slot = Slot::take();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let ran = dir.path().join("ran");
    std::fs::create_dir(&home).unwrap();
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(&exe)
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .arg("--include-ignored")
        .env(CHILD, name)
        .env(RAN, &ran)
        .env("HOME", &home)
        .env("TZ", time_zone())
        .output()
        .unwrap_or_else(|e| panic!("{name}: cannot run {}: {e}", exe.display()));
    // Through the test harness: shown when this test fails, or with
    // --nocapture.
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        out.status.success(),
        "{name} failed in its own process ({}); its output is above",
        out.status
    );
    assert!(ran.exists(), "{name}: the child process ran no such test");
    None
}

/// cargo-nextest runs each test in a process of its own.
fn one_test_per_process() -> bool {
    match std::env::var("NEXTEST_EXECUTION_MODE") {
        Ok(mode) => mode == "process-per-test",
        Err(_) => std::env::var_os("NEXTEST").is_some_and(|v| v == "1"),
    }
}

static RUNNING: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

/// One of the [`JOBS`] places for a scenario process.
struct Slot;

impl Slot {
    fn take() -> Self {
        let max = jobs();
        let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        while *running >= max {
            running = FREED.wait(running).unwrap_or_else(PoisonError::into_inner);
        }
        *running += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
        FREED.notify_one();
    }
}

/// [`JOBS`] if set (a positive number), else 4.
fn jobs() -> usize {
    std::env::var(JOBS)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_JOBS)
}

/// A POSIX `TZ` with a fixed offset (no daylight saving) that makes the
/// local time now noon, or [`LOCAL_TIME`]: the app reads its offset
/// through the C library, which follows `TZ`. Timestamps stay real; only
/// the local wall clock moves.
pub fn time_zone() -> String {
    let wanted = std::env::var(LOCAL_TIME).ok().map_or(12 * 60, |t| {
        let (h, m) = t
            .split_once(':')
            .and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)))
            .unwrap_or_else(|| panic!("{LOCAL_TIME} wants HH:MM, not {t:?}"));
        h * 60 + m
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    posix_tz(wanted, now / 60 % (24 * 60))
}

/// The `TZ` whose local minute of the day is `wanted` when UTC's is
/// `utc` (offsets kept within ±12 h: the date may differ, not the time).
fn posix_tz(wanted: i64, utc: i64) -> String {
    let mut east = (wanted - utc).rem_euclid(24 * 60);
    if east > 12 * 60 {
        east -= 24 * 60;
    }
    // POSIX counts west of Greenwich as positive.
    let west = -east;
    let sign = if west < 0 { '-' } else { '+' };
    format!("IPT{sign}{:02}:{:02}", west.abs() / 60, west.abs() % 60)
}

#[test]
fn the_time_zone_puts_local_time_where_wanted() {
    // UTC 02:00, wanted 12:00: ten hours east.
    assert_eq!(posix_tz(12 * 60, 2 * 60), "IPT-10:00");
    // UTC 21:47, wanted 12:00: 9 h 47 min west.
    assert_eq!(posix_tz(12 * 60, 21 * 60 + 47), "IPT+09:47");
    // UTC 12:00: UTC itself.
    assert_eq!(posix_tz(12 * 60, 12 * 60), "IPT+00:00");
    // Wanted 23:50 at UTC 00:10: 23 h 40 min east is 20 min west.
    assert_eq!(posix_tz(23 * 60 + 50, 10), "IPT+00:20");
}

/// Generated fixture data uses a fixed synthetic key, never the user's keychain.
pub fn initialize_key() {
    iptv_app::install_test_key([0x54; 32]);
}

pub fn open_database(
    path: impl AsRef<Path>,
) -> iptv_store_sqlite::Result<iptv_store_sqlite::Store> {
    initialize_key();
    iptv_store_sqlite::Store::open_encrypted(
        path,
        &iptv_store_sqlite::DatabaseKey::from_bytes([0x54; 32]),
    )
}
