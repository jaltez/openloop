// Round-2 regression anchors: one deterministic assertion per one-line
// review fix, so reverting any of them fails here instead of silently.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use openloop::core as c;

#[test]
fn nested_github_token_prefixes_are_retested() {
    // TS regex /gh[ps]_[A-Za-z0-9]{36,}/g retries every start position:
    // a ghp_ token nested inside a short ghs_ run must still count.
    let long = "A".repeat(36);
    let patch = format!("+token = \"ghs_ghp_{long}\"");
    let findings = c::review::check_secrets(&patch);
    let github = findings
        .iter()
        .filter(|f| f.rule.contains("github-token"))
        .count();
    assert!(github >= 1, "nested token must be detected: {findings:?}");
}

#[test]
fn untracked_binary_and_edge_line_counts() {
    let dir = std::env::temp_dir().join(format!("ol-r2-lines-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init", "-q"]);
    fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-q", "-m", "seed"]);

    // Binary content with 2 newlines counts 2 lines, not 0 (read-bytes fix).
    fs::write(dir.join("blob.bin"), [0xFF, b'\n', 0x00, b'\n']).unwrap();
    // CRLF content: each \n still counts.
    fs::write(dir.join("crlf.txt"), b"a\r\nb\r\n").unwrap();
    // Empty file: TS "".split("\n").length - 0 === 1 (quirk kept for parity).
    fs::write(dir.join("empty.txt"), b"").unwrap();

    let state = c::git::get_git_working_tree_state(&dir).unwrap();
    assert!(state.dirty);
    let stat = c::git::get_git_diff_stat(&dir, Some("HEAD"), None).unwrap_or_default();
    let find = |name: &str| stat.iter().find(|e| e.file == name).map(|e| e.added);
    assert_eq!(find("blob.bin"), Some(2));
    assert_eq!(find("crlf.txt"), Some(2));
    assert_eq!(find("empty.txt"), Some(1));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn active_run_ids_are_never_reused() {
    // Spawn two short-lived children in their own groups, interleave
    // register/unregister and confirm the third id differs from the second.
    let pgid = |secs: &str| {
        let mut cmd = Command::new("sleep");
        cmd.arg(secs)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        use std::os::unix::process::CommandExt;
        let mut child = cmd.process_group(0).spawn().unwrap();
        let pgid = child.id() as i32;
        let _ = child.wait();
        pgid
    };
    let a = pgid("0");
    let id_a = c::active_runs::register_active_run("r2a", a);
    let b = pgid("0");
    let id_b = c::active_runs::register_active_run("r2b", b);
    c::active_runs::unregister_active_run(id_a);
    let c3 = pgid("0");
    let id_c = c::active_runs::register_active_run("r2c", c3);
    assert_ne!(id_b, id_c, "ids must never be recycled");
    c::active_runs::unregister_active_run(id_b);
    c::active_runs::unregister_active_run(id_c);
}

#[test]
fn cron_huge_step_does_not_overflow() {
    // "0 0 1/2147483647 * *" must parse (empty match set for dom) without
    // panicking in debug builds (checked_add fix).
    use chrono::TimeZone;
    let epoch = chrono::Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let ok = c::cron::matches_five_field_cron("0 0 1/2147483647 * *", epoch);
    // Either arm is fine — the regression is the panic.
    let _ = ok;
}

#[test]
fn sanitize_observation_key_matches_ts_regex() {
    use openloop::core::scheduler;
    // Named via the pub wrapper build_prompt exercises; if the fn itself is
    // private, assert through a stable public seam instead. Direct call is
    // possible because the fn is module-private — so use the note-format
    // invariant via should_stop_for_no_progress inputs instead.

    // Direct pin: replicate through the pub task list path is overkill;
    // the sanitize fn is `fn`-private, so drive it via its only consumer.
    // Reconstruct the exact keys TS would write for these signatures:
    let cases = [
        ("a: b. c", "a-b-c"),
        ("err: __run_timeout__", "err-run-timeout"),
        ("!!!", "observation"),
    ];
    for (input, expected) in cases {
        // should_stop_for_no_progress notes use
        // "openloop:no-progress:failure-<key>"; expose the key by asserting
        // the note the counter WOULD write stays stable across runs.
        let task = task_with_signature(input);
        let np = scheduler::NoProgressInput {
            previous_failure_signature: Some(input),
            previous_promotion_decision: Some(c::types::PromotionDecision::Blocked),
            current_promotion_decision: c::types::PromotionDecision::Blocked,
            before_fingerprint: Some("same".into()),
            after_fingerprint: Some("same".into()),
            no_progress_repeat_limit: 1,
        };
        let mut t2 = task.clone();
        let stop = scheduler::should_stop_for_no_progress(&mut t2, &np);
        assert!(stop, "input {input:?} must count (same fingerprint)");
        // The written note embeds the sanitized key twice through both
        // failure signatures — assert the exact TS-shaped key survives.
        let note = format!("openloop:no-progress:failure-{}", expected);
        assert!(
            t2.notes
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .any(|n| n.contains(&note)),
            "key must match TS form {note:?} in {:?}",
            t2.notes
        );
    }
}

fn task_with_signature(signature: &str) -> c::types::ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    c::types::ProjectTask {
        id: "r2-key".into(),
        title: "key".into(),
        kind: c::types::TaskKind::Feature,
        status: c::types::TaskStatus::InProgress,
        risk: c::types::TaskRisk::LowRisk,
        scope: None,
        source: c::types::TaskSource {
            source_type: "human".into(),
            r#ref: "r2".into(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec![],
        attempts: 1,
        last_failure_signature: Some(signature.to_string()),
        promotion: c::types::TaskPromotion::ManualOnly,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn task_add_rejects_empty_title_and_update_noops() {
    let home = std::env::temp_dir().join(format!("ol-r2-title-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let project = home.join("proj");
    fs::create_dir_all(project.join(".openloop")).unwrap();
    run_git(&project, &["init", "-q"]);
    fs::write(
        home.join("projects.json"),
        serde_json::json!({"version": 1, "projects": [
            {"alias": "p1", "path": project.to_string_lossy(), "initialized": true,
             "createdAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-01-01T00:00:00.000Z"}
        ]})
        .to_string(),
    )
    .unwrap();

    // add --title "" with no --from-ref exits 1 (TS truthiness).
    let out = Command::new(env!("CARGO_BIN_EXE_openloop"))
        .env("OPENLOOP_HOME", &home)
        .args(["task", "add", "-p", "p1", "--title", ""])
        .output()
        .unwrap();
    assert!(!out.status.success(), "empty --title must fail");

    // update --title "" is a no-op (title not blanked).
    let now = "2026-01-01T00:00:00.000Z";
    let ledger = serde_json::json!({"version": 1, "updatedAt": now, "tasks": [
        {"id": "t1", "title": "original", "kind": "feature", "status": "ready",
         "risk": "low-risk", "source": {"type": "human", "ref": "r2"},
         "acceptanceCriteria": [], "attempts": 0, "promotion": "manual-only",
         "createdAt": now, "updatedAt": now}
    ]});
    fs::write(
        project.join(".openloop").join("tasks.json"),
        ledger.to_string(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_openloop"))
        .env("OPENLOOP_HOME", &home)
        .args(["task", "update", "-p", "p1", "--task", "t1", "--title", ""])
        .output()
        .unwrap();
    assert!(out.status.success());
    let persisted = fs::read_to_string(project.join(".openloop").join("tasks.json")).unwrap();
    assert!(
        persisted.contains("\"original\""),
        "title must survive an empty --title update"
    );
    fs::remove_dir_all(&home).ok();
}

#[test]
fn events_since_zero_means_no_cutoff() {
    let home = std::env::temp_dir().join(format!("ol-r2-events-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("run")).unwrap();
    for ts in ["2026-01-01T00:00:00.000Z", "2099-01-01T00:00:00.000Z"] {
        c::event_log::append_event(
            &c::types::OpenLoopEvent {
                ts: ts.to_string(),
                event: "probe".into(),
                project: Some("p1".into()),
                task_id: None,
                exit_code: None,
                mode: None,
                role: None,
                stopped_by: None,
                pid: None,
                task_status: None,
                detail: None,
                hook_event: None,
                hook_type: None,
                hook_scope: None,
                error: None,
                digest: None,
                budget_snapshot_usd: None,
                pending_promotions: None,
                failed_checks: None,
                extra: Default::default(),
            },
            Some(home.to_str().unwrap()),
        )
        .unwrap();
    }
    // since 0 → no cutoff (TS truthiness): both events (old and far-future)
    // are returned.
    let all = c::event_log::read_recent_events(&c::event_log::ReadEventsOptions {
        since_ms: Some(0),
        project: None,
        limit: None,
        override_home: Some(home.to_string_lossy().as_ref()),
    });
    assert_eq!(all.len(), 2, "since_ms=0 must disable the cutoff");
    fs::remove_dir_all(&home).ok();
}

#[test]
fn parse_since_ms_overflow_falls_back_instead_of_panicking() {
    // Via the binary: an absurd window must not panic (debug) or wrap.
    let home = std::env::temp_dir().join(format!("ol-r2-since-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("run")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_openloop"))
        .env("OPENLOOP_HOME", &home)
        .args(["events", "--since", "999999999999999d"])
        .output()
        .unwrap();
    assert!(
        out.status.success() || out.status.code() == Some(1),
        "must not panic: stderr = {}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::remove_dir_all(&home).ok();
}

#[test]
fn integral_floats_serialize_without_trailing_zero() {
    // The byte-parity fix: an integral f64 renders as "1", not "1.0".
    let home = std::env::temp_dir().join(format!("ol-r2-float-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("run")).unwrap();
    let path = home.join("float.json");
    let value = serde_json::json!({ "cost": 1.0 });
    c::fsutil::write_json_file(&path, &value).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains("\"cost\": 1") && !raw.contains("\"cost\": 1.0"),
        "{raw}"
    );
    fs::remove_dir_all(&home).ok();
}

#[test]
fn json_state_files_keep_declaration_key_order() {
    // preserve_order regression: a struct round-trip must not alphabetize
    // keys (TaskLedger declares version, updatedAt, tasks).
    let home = std::env::temp_dir().join(format!("ol-r2-order-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let project = home.join("proj");
    fs::create_dir_all(project.join(".openloop")).unwrap();
    let now = "2026-01-01T00:00:00.000Z";
    let ledger = c::types::TaskLedger {
        version: 1,
        updated_at: now.to_string(),
        tasks: Vec::new(),
    };
    let path = project.join(".openloop").join("tasks.json");
    c::fsutil::write_json_file(&path, &ledger).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let version_pos = raw.find("\"version\"").expect("version key");
    let updated_pos = raw.find("\"updatedAt\"").expect("updatedAt key");
    let tasks_pos = raw.find("\"tasks\"").expect("tasks key");
    assert!(
        version_pos < updated_pos && updated_pos < tasks_pos,
        "keys must keep declaration order: {raw}"
    );
    fs::remove_dir_all(&home).ok();
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?} failed");
}

// Silence unused warnings when a helper is only used by one cfg path.
#[allow(dead_code)]
fn _unused(_: PathBuf) {}
