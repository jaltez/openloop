use parking_lot::Mutex;

/// Registry of live agent runs so daemon shutdown can kill the whole process
/// group of every running agent instead of orphaning it.
static ACTIVE_RUNS: Mutex<Vec<ActiveRun>> = Mutex::new(Vec::new());

pub struct ActiveRun {
    pub id: usize,
    pub project_alias: String,
    pub pgid: i32,
}

/// Register a live process group. Returns an unregister closure id.
pub fn register_active_run(project_alias: &str, pgid: i32) -> usize {
    let mut runs = ACTIVE_RUNS.lock();
    let id = runs.len() + 1;
    runs.push(ActiveRun {
        project_alias: project_alias.to_string(),
        pgid,
        id,
    });
    id
}

pub fn unregister_active_run(id: usize) {
    let mut runs = ACTIVE_RUNS.lock();
    runs.retain(|r| r.id != id);
}

/// Kill every registered run. Best effort — failures never block shutdown.
pub fn kill_active_runs() {
    for run in ACTIVE_RUNS.lock().drain(..) {
        super::providers::kill_process_group(run.pgid, libc::SIGTERM);
    }
}

pub fn list_active_runs() -> Vec<ActiveRun> {
    ACTIVE_RUNS.lock().clone()
}

impl Clone for ActiveRun {
    fn clone(&self) -> Self {
        ActiveRun {
            project_alias: self.project_alias.clone(),
            pgid: self.pgid,
            id: self.id,
        }
    }
}
