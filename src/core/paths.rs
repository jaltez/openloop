use std::path::{Path, PathBuf};

pub fn app_home(override_home: Option<&str>) -> PathBuf {
    if let Some(o) = override_home {
        if !o.is_empty() {
            return PathBuf::from(o);
        }
    }
    if let Ok(env) = std::env::var("OPENLOOP_HOME") {
        if !env.is_empty() {
            return PathBuf::from(env);
        }
    }
    // TS os.homedir() parity: fall back to the passwd entry when $HOME is
    // unset (e.g. systemd units, `env -i`), never to the current directory.
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(passwd_home)
        .unwrap_or_else(|| {
            // Neither $HOME nor a passwd entry exists (arbitrary-UID
            // containers). TS (os.homedir) fails loudly here; never
            // silently relocate state into the current directory.
            panic!("Cannot determine home directory: set $HOME or $OPENLOOP_HOME.")
        });
    Path::new(&home).join(".openloop")
}

fn passwd_home() -> Option<String> {
    unsafe {
        let uid = libc::getuid();
        let pw = libc::getpwuid(uid);
        if pw.is_null() {
            return None;
        }
        let dir = (*pw).pw_dir;
        if dir.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(dir)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }
}

pub fn global_config_path(override_home: Option<&str>) -> PathBuf {
    app_home(override_home).join("config.json")
}

pub fn projects_registry_path(override_home: Option<&str>) -> PathBuf {
    app_home(override_home).join("projects.json")
}

pub fn runtime_dir(override_home: Option<&str>) -> PathBuf {
    app_home(override_home).join("run")
}

pub fn daemon_pid_path(override_home: Option<&str>) -> PathBuf {
    runtime_dir(override_home).join("daemon.pid")
}

pub fn daemon_state_path(override_home: Option<&str>) -> PathBuf {
    runtime_dir(override_home).join("daemon-state.json")
}

pub fn daemon_log_path(override_home: Option<&str>) -> PathBuf {
    runtime_dir(override_home).join("daemon.log")
}

pub fn project_openloop_dir(project_path: &Path) -> PathBuf {
    project_path.join(".openloop")
}

pub fn project_pi_dir(project_path: &Path) -> PathBuf {
    project_path.join(".pi")
}
