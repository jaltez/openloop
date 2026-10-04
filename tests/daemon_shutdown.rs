// Integration regression test: SIGTERM during an active agent run must
// interrupt the run promptly (kill the agent's process group, remove the pid
// file, and exit) instead of hanging until the run timeout.
//
// Requires the compiled binary (CARGO_BIN_EXE_openloop) and git.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openloop"))
}

struct Sandbox {
    home: PathBuf,
    target: PathBuf,
    #[allow(dead_code)]
    original_openloop_home: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.target);
    }
}

fn run_in(dir: &Path, env_home: &Path, args: &[&str]) -> bool {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("OPENLOOP_HOME", env_home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn make_sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-sigterm-home-{name}-{}", std::process::id()));
    let target =
        std::env::temp_dir().join(format!("ol-sigterm-target-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&target).unwrap();

    // Minimal git repo so template init and ledger operations work.
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(&target)
            .output()
            .unwrap();
    }
    std::fs::write(target.join("README.md"), "hello\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&target)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(&target)
        .output()
        .unwrap();

    // Sleeping agent: a script that never finishes on its own.
    let agent = target.join("sleep-agent.sh");
    std::fs::write(&agent, "#!/bin/sh\nsleep 300\n").unwrap();
    make_executable(&agent);

    Sandbox {
        home,
        target,
        original_openloop_home: None,
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[test]
fn sigterm_interrupts_active_agent_run() {
    let sandbox = make_sandbox("a");
    let home = &sandbox.home;
    let target = &sandbox.target;
    let agent = target.join("sleep-agent.sh");

    assert!(run_in(
        target,
        home,
        &["project", "add", "sig", &target.to_string_lossy(), "--init"]
    ));
    assert!(run_in(
        target,
        home,
        &[
            "task",
            "add",
            "--project",
            "sig",
            "--title",
            "slow work",
            "--risk",
            "low-risk"
        ]
    ));
    // Plan the task once so it becomes ready.
    run_in(target, home, &["run-once", "--project", "sig"]);
    run_in(
        target,
        home,
        &[
            "task",
            "update",
            "--project",
            "sig",
            "--task",
            "slow-work",
            "--status",
            "ready",
        ],
    );
    assert!(run_in(
        target,
        home,
        &[
            "config",
            "project-set-agent",
            "sig",
            "custom",
            "--command",
            &agent.to_string_lossy(),
        ]
    ));

    // Start the daemon worker in the background.
    let mut daemon = Command::new(bin())
        .args(["daemon", "worker"])
        .env("OPENLOOP_HOME", home)
        .current_dir(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait until the daemon has picked the task and spawned the sleeping
    // agent (the run summary or the in_progress flip both indicate it).
    let deadline = Instant::now() + Duration::from_secs(30);
    let agent_spawned = loop {
        let agent_running = Command::new("pgrep")
            .args(["-f", "sleep 300"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if agent_running || Instant::now() >= deadline {
            break agent_running;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(agent_spawned, "agent did not start within 30s");

    // SIGTERM the daemon: it must exit quickly (not wait for the 30-min run
    // timeout), remove its pid file, and take the agent down with it.
    let start = Instant::now();
    unsafe {
        libc::kill(daemon.id() as i32, libc::SIGTERM);
    }
    let start_exit = Instant::now();
    let mut exited = false;
    while start_exit.elapsed() < Duration::from_secs(10) {
        match daemon.try_wait() {
            Ok(Some(_)) => {
                exited = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }
    if !exited {
        let _ = daemon.kill();
    }
    let _ = daemon.wait();
    assert!(exited, "daemon did not exit within 10s of SIGTERM");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "shutdown took {:?}",
        start.elapsed()
    );

    // The pid file must be cleaned up on shutdown.
    let pid_path = home.join("run").join("daemon.pid");
    assert!(!pid_path.exists(), "pid file survived shutdown");

    // The sleeping agent must be gone shortly after (SIGTERM to the group).
    let agent_deadline = Instant::now() + Duration::from_secs(8);
    let agent_gone = loop {
        let agent_running = Command::new("pgrep")
            .args(["-f", "sleep 300"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !agent_running || Instant::now() >= agent_deadline {
            break !agent_running;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    // Best effort: clean up any stray sleep in case of a test failure path.
    let _ = Command::new("pkill").arg("-f").arg("sleep 300").output();
    assert!(agent_gone, "agent process group survived the daemon");
}
