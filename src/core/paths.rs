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
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".openloop")
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
