use openloop::core::scheduler::{run_worktree_setup_command, SetupOutcome};

#[test]
fn successful_setup_returns_exit_code() {
    let outcome = run_worktree_setup_command(std::path::Path::new("."), "exit 0", Some(5000)).unwrap();
    assert!(matches!(outcome, SetupOutcome::Exit(0)));
    let outcome = run_worktree_setup_command(std::path::Path::new("."), "exit 3", Some(5000)).unwrap();
    assert!(matches!(outcome, SetupOutcome::Exit(3)));
}

#[test]
fn hung_setup_times_out_and_is_killed() {
    // Regression: the setup command used to run without any timeout, so a
    // hung setup blocked the single-threaded daemon forever.
    let start = std::time::Instant::now();
    let outcome = run_worktree_setup_command(std::path::Path::new("."), "sleep 30", Some(300)).unwrap();
    assert!(matches!(outcome, SetupOutcome::TimedOut));
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "must not wait for the full sleep");
}

#[test]
fn timeout_none_runs_to_completion() {
    let outcome = run_worktree_setup_command(std::path::Path::new("."), "sleep 0.1", None).unwrap();
    assert!(matches!(outcome, SetupOutcome::Exit(0)));
}
