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
//! At most [`JOBS`] scenario processes run at once (default 4: each is a
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
const DEFAULT_JOBS: usize = 4;
/// A file the child writes when it reaches the scenario: a name that
/// selects no test exits successfully too.
const RAN: &str = "IPTV_RS_TEST_RAN";

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
}

/// `here` is the type name of a function nested in the test:
/// `app::<module>::<test>::here`.
pub fn run(here: &str) -> Option<Home> {
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
        unsafe { std::env::set_var("HOME", home.path()) };
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
