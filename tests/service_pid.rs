// Integration tests for daemon service pid-file handling (stale cleanup,
// refusal to signal foreign processes) and the cross-process file lock
// (fresh-lock contention, stale-lock breaking).
//
// Ported from the deleted TS suite (runtime-controls service-stop cases plus
// the daemon-state lock semantics they shared with with_file_lock).

use parking_lot::Mutex;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use openloop::core::file_lock::with_file_lock;
use openloop::core::service_control::stop_service;

// stop_service resolves the app home from OPENLOOP_HOME; env mutation must be
// serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ol-service-pid-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct EnvHome {
    path: PathBuf,
    saved: Option<String>,
}

impl Drop for EnvHome {
    fn drop(&mut self) {
        match self.saved.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn env_home(name: &str) -> EnvHome {
    let path = fresh_dir(name);
    fs::create_dir_all(path.join("run")).unwrap();
    let saved = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &path);
    EnvHome { path, saved }
}

fn pid_path(home: &std::path::Path) -> PathBuf {
    home.join("run").join("daemon.pid")
}

fn write_pid_file(home: &std::path::Path, pid: i32) {
    fs::write(pid_path(home), format!("{pid}\n")).unwrap();
}

/// Kills and reaps the child even if an assertion panics first.
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn stop_service_removes_stale_pid_file() {
    let _guard = ENV_LOCK.lock();
    let home = env_home("stale");

    // 2^22 + 1 is beyond the kernel's maximum allocatable pid, so no live
    // process can ever own it.
    let stale_pid = 4_194_305;
    let alive = unsafe { libc::kill(stale_pid, 0) } == 0;
    assert!(
        !alive,
        "precondition failed: pid {stale_pid} unexpectedly exists"
    );
    write_pid_file(&home.path, stale_pid);

    let message = stop_service().expect("stale pid file must be cleared, not an error");

    assert!(
        message.to_lowercase().contains("stale"),
        "unexpected message: {message}"
    );
    assert!(
        !pid_path(&home.path).exists(),
        "stale pid file must be removed"
    );
}

#[test]
fn stop_service_refuses_live_non_openloop_process() {
    let _guard = ENV_LOCK.lock();
    let home = env_home("foreign");

    let mut sleeper = Sleeper(
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleep 30 should spawn"),
    );
    let foreign_pid = sleeper.0.id() as i32;
    write_pid_file(&home.path, foreign_pid);

    let err = stop_service().expect_err("stop must refuse a live non-openloop pid");

    assert!(err.contains("Refusing to stop"), "unexpected error: {err}");
    assert!(err.contains("non-openloop"), "unexpected error: {err}");
    assert!(
        pid_path(&home.path).exists(),
        "pid file must be preserved on refusal"
    );
    assert!(
        sleeper
            .0
            .try_wait()
            .expect("sleep status readable")
            .is_none(),
        "the foreign process must not be signaled"
    );
}

#[test]
fn with_file_lock_rejects_fresh_lock_held_elsewhere() {
    let dir = TempDir(fresh_dir("fresh-lock"));
    let lock_path = dir.0.join("daemon.lock");
    fs::write(&lock_path, "99999.deadbeef\n").unwrap();

    let mut critical_ran = false;
    let err = with_file_lock(&lock_path, "daemon state", || {
        critical_ran = true;
        Ok::<(), String>(())
    })
    .expect_err("a fresh foreign lock must not be acquirable");

    assert_eq!(err, "daemon state is locked by another openloop process");
    assert!(
        !critical_ran,
        "critical section must not run while another holder owns the lock"
    );
    assert_eq!(
        fs::read_to_string(&lock_path).unwrap().trim(),
        "99999.deadbeef",
        "a contender must never delete a live holder's lock"
    );
}

#[test]
fn with_file_lock_breaks_stale_lock() {
    let dir = TempDir(fresh_dir("stale-lock"));
    let lock_path = dir.0.join("daemon.lock");
    fs::write(&lock_path, "1.old\n").unwrap();

    // Age the lock past the 10-minute staleness threshold.
    let file = fs::File::options().write(true).open(&lock_path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(11 * 60))
        .unwrap();
    drop(file);

    let mut critical_ran = false;
    let out = with_file_lock(&lock_path, "daemon state", || {
        critical_ran = true;
        Ok::<i32, String>(7)
    })
    .expect("a stale lock must be broken and acquired");

    assert_eq!(out, 7);
    assert!(critical_ran, "critical section must run under the lock");
    assert!(
        !lock_path.exists(),
        "the lock must be released (unlinked) after the critical section"
    );
}
