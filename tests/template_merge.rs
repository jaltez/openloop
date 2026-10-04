// Integration tests for project template materialization: initializing into a
// repository that already has control-plane files must merge rather than
// clobber, and `--force` must back up the control plane before rewriting.

use parking_lot::{Mutex, MutexGuard};
use std::fs;
use std::path::{Path, PathBuf};

use openloop::core::templates::initialize_project_from_templates;
use openloop::core::types::LinkedProject;

// initialize_project_from_templates extracts the embedded templates into a
// process-shared temp dir (`openloop-templates-<pid>`); concurrent init calls
// from the parallel test threads of this binary must be serialized.
static INIT_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    INIT_LOCK.lock()
}

/// Per-test target directory (pid-guarded temp path), removed on drop.
struct TargetDir(PathBuf);

impl Drop for TargetDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for TargetDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn target_dir(name: &str) -> TargetDir {
    let dir = std::env::temp_dir().join(format!("ol-templates-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    TargetDir(dir)
}

fn init(alias: &str, target: &Path, force: bool) {
    let project = LinkedProject {
        alias: alias.to_string(),
        path: target.to_string_lossy().to_string(),
        default_branch: None,
        initialized: false,
        created_at: "2026-01-01T00:00:00.000Z".to_string(),
        updated_at: "2026-01-01T00:00:00.000Z".to_string(),
    };
    // The repo-root argument is unused in the Rust port: templates are embedded.
    initialize_project_from_templates(Path::new("."), &project, force).unwrap();
}

fn write_json(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn merges_into_existing_project_json_preserving_custom_fields_and_pi_model() {
    let _guard = lock();
    let target = target_dir("merge-config");

    // Pre-existing project.json with custom fields and a chosen Pi model.
    write_json(
        &target.join(".openloop").join("project.json"),
        serde_json::json!({
            "version": 1,
            "customField": "should-survive",
            "pi": { "model": "anthropic/sonnet", "promptFiles": [] },
            "validation": { "lintCommand": "stale-lint" },
        }),
    );

    init("existing", &target, false);

    let config = read_json(&target.join(".openloop").join("project.json"));
    // Init stamps the project identity block.
    assert_eq!(config["project"]["alias"].as_str(), Some("existing"));
    assert!(config["project"]["initializedAt"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    // Pre-existing scalars and nested values win over template defaults.
    assert_eq!(config["customField"].as_str(), Some("should-survive"));
    assert_eq!(config["pi"]["model"].as_str(), Some("anthropic/sonnet"));
    assert_eq!(config["pi"]["promptFiles"], serde_json::json!([]));
    // Template defaults still fill in untouched sections.
    assert_eq!(
        config["runtime"]["branchPrefix"].as_str(),
        Some("openloop/")
    );
    assert_eq!(
        config["risk"]["defaultUnknownAreaClassification"].as_str(),
        Some("medium-risk")
    );
    // Validation commands are refreshed by stack detection (an empty target
    // directory detects nothing), never kept stale from the existing config.
    assert_eq!(config["validation"]["lintCommand"], serde_json::Value::Null);
    assert!(config["validation"].get("testCommand").is_some());
    assert!(config["validation"].get("typecheckCommand").is_some());
}

#[test]
fn creates_tasks_json_from_template_with_refreshed_updated_at() {
    let _guard = lock();
    let target = target_dir("tasks-fresh");

    init("tasks-test", &target, false);

    let tasks = read_json(&target.join(".openloop").join("tasks.json"));
    assert_eq!(tasks["tasks"], serde_json::json!([]));
    assert_eq!(tasks["version"].as_i64(), Some(1));
    // The template's epoch updatedAt is refreshed, not copied verbatim.
    assert_ne!(
        tasks["updatedAt"].as_str(),
        Some("1970-01-01T00:00:00.000Z")
    );
    assert!(tasks["updatedAt"].as_str().is_some());
}

#[test]
fn preserves_existing_task_ledger_and_refreshes_updated_at() {
    let _guard = lock();
    let target = target_dir("tasks-existing");

    write_json(
        &target.join(".openloop").join("tasks.json"),
        serde_json::json!({
            "version": 1,
            "updatedAt": "2024-01-01T00:00:00.000Z",
            "tasks": [{ "id": "keep-me", "title": "Existing task" }],
        }),
    );

    init("ledger-test", &target, false);

    let tasks = read_json(&target.join(".openloop").join("tasks.json"));
    // Existing ledger entries are preserved verbatim.
    assert_eq!(
        tasks["tasks"],
        serde_json::json!([{ "id": "keep-me", "title": "Existing task" }])
    );
    // Only the timestamp is touched.
    assert_ne!(
        tasks["updatedAt"].as_str(),
        Some("2024-01-01T00:00:00.000Z")
    );
}

#[test]
fn existing_skill_md_and_ka_toml_survive_reinit_byte_for_byte() {
    let _guard = lock();
    let target = target_dir("skip-existing");

    // A user-customized agent skill already exists in the target.
    let skill = target
        .join(".agents")
        .join("skills")
        .join("openloop")
        .join("SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "# Existing Skill\n").unwrap();

    init("existing-skill", &target, false);
    // copy_tree skips existing files: the customization survives byte-for-byte.
    assert_eq!(fs::read_to_string(&skill).unwrap(), "# Existing Skill\n");

    // ka.toml is materialized from the template on first init...
    let ka = target.join(".ka").join("ka.toml");
    assert!(fs::read_to_string(&ka)
        .unwrap()
        .contains("# ka configuration for a repository linked to Openloop"));

    // ...then user-edited, and survives re-materialization without --force.
    fs::write(&ka, "# user override\n").unwrap();
    init("existing-skill", &target, false);
    assert_eq!(fs::read_to_string(&ka).unwrap(), "# user override\n");
    assert_eq!(fs::read_to_string(&skill).unwrap(), "# Existing Skill\n");
}

#[test]
fn creates_agents_md_with_openloop_section_when_absent() {
    let _guard = lock();
    let target = target_dir("agents-new");

    init("agents-new", &target, false);

    let content = fs::read_to_string(target.join("AGENTS.md")).unwrap();
    assert!(content.contains("<!-- openloop:start -->"));
    assert!(content.contains("## Openloop"));
    assert!(content.contains("<!-- openloop:end -->"));
    assert!(content.contains(".openloop/tasks.json"));
}

#[test]
fn appends_openloop_section_to_existing_agents_md() {
    let _guard = lock();
    let target = target_dir("agents-append");

    fs::write(
        target.join("AGENTS.md"),
        "# My conventions\n\nBe excellent.\n",
    )
    .unwrap();

    init("agents-append", &target, false);

    let content = fs::read_to_string(target.join("AGENTS.md")).unwrap();
    // The user's conventions are kept and the openloop section is appended
    // after them, not prepended.
    assert!(content.starts_with("# My conventions"));
    assert!(content.contains("Be excellent."));
    assert!(content.contains("## Openloop"));
    let user_at = content.find("Be excellent.").unwrap();
    let section_at = content.find("## Openloop").unwrap();
    assert!(user_at < section_at);
}

#[test]
fn replaces_stale_openloop_section_without_duplication() {
    let _guard = lock();
    let target = target_dir("agents-replace");

    // An AGENTS.md with user content around a stale openloop block.
    let stale =
        "<!-- openloop:start -->\n\n## Openloop\n\nSTALE INSTRUCTIONS\n\n<!-- openloop:end -->\n";
    fs::write(
        target.join("AGENTS.md"),
        format!("# Header\n\n{stale}\n# Footer\n"),
    )
    .unwrap();

    init("agents-replace", &target, false);
    init("agents-replace", &target, false); // re-run must not duplicate

    let content = fs::read_to_string(target.join("AGENTS.md")).unwrap();
    // Surrounding user content survives on both sides of the block.
    assert!(content.contains("# Header"));
    assert!(content.contains("# Footer"));
    // The stale block is replaced wholesale with the canonical section.
    assert!(!content.contains("STALE INSTRUCTIONS"));
    assert!(content.contains("## Openloop"));
    // Exactly one openloop section remains, markers included.
    assert_eq!(content.matches("<!-- openloop:start -->").count(), 1);
    assert_eq!(content.matches("<!-- openloop:end -->").count(), 1);
    assert_eq!(content.matches("## Openloop").count(), 1);
}

#[test]
fn force_backs_up_control_plane_before_rewriting() {
    let _guard = lock();
    let target = target_dir("force-backup");

    write_json(
        &target.join(".openloop").join("project.json"),
        serde_json::json!({
            "version": 1,
            "customField": "pre-force",
            "pi": { "model": "anthropic/sonnet" },
        }),
    );
    write_json(
        &target.join(".openloop").join("tasks.json"),
        serde_json::json!({
            "version": 1,
            "updatedAt": "2024-01-01T00:00:00.000Z",
            "tasks": [{ "id": "keep-me" }],
        }),
    );
    let skill = target
        .join(".agents")
        .join("skills")
        .join("openloop")
        .join("SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "# Existing Skill\n").unwrap();

    init("force-test", &target, true);

    // A single timestamped backup snapshot holds the pre-force control plane.
    let backup_root = target.join(".openloop").join("backup");
    let mut entries: Vec<_> = fs::read_dir(&backup_root).unwrap().flatten().collect();
    assert_eq!(entries.len(), 1);
    let backup_dir = entries.remove(0).path();
    assert!(backup_dir.is_dir());
    let backed_up_config = read_json(&backup_dir.join("project.json"));
    assert_eq!(backed_up_config["customField"].as_str(), Some("pre-force"));
    assert_eq!(
        backed_up_config["pi"]["model"].as_str(),
        Some("anthropic/sonnet")
    );
    let backed_up_tasks = read_json(&backup_dir.join("tasks.json"));
    assert_eq!(backed_up_tasks["tasks"][0]["id"].as_str(), Some("keep-me"));

    // The live control plane was rewritten from the template: custom fields
    // are gone (they live on only in the backup) and identity is re-stamped.
    let live = read_json(&target.join(".openloop").join("project.json"));
    assert!(live.get("customField").is_none());
    assert_eq!(live["project"]["alias"].as_str(), Some("force-test"));
    // Force overwrites non-config files too: the customized skill is replaced
    // by the template version.
    let live_skill = fs::read_to_string(&skill).unwrap();
    assert_ne!(live_skill, "# Existing Skill\n");
    assert!(live_skill.contains("name: openloop"));
}
