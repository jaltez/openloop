use include_dir::{include_dir, Dir};

use super::fsutil::{copy_tree, ensure_dir, file_exists, read_json_file, write_json_file};
use super::now_iso;
use super::stack_detection::detect_validation_commands;
use super::types::LinkedProject;

static TEMPLATES_PROJECT: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates/project");

/// Version of the embedded control-plane templates. Bump when templates
/// change; `openloop project upgrade` brings linked repos up to it.
pub const TEMPLATE_VERSION: u32 = 1;

/// Unique-per-call temp dir for template extraction: concurrent invocations
/// (parallel test runs in one process) must not share a directory, so a
/// process-wide counter disambiguates within the same millisecond.
fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("{prefix}-{}-{id}", std::process::id()))
}

/// The marker file stamped into `.openloop/` at init/upgrade time.
fn template_version_path(project_path: &std::path::Path) -> std::path::PathBuf {
    project_path.join(".openloop").join("template-version")
}

/// The project's template version, or None when it predates versioning.
pub fn template_version_of(project_path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(template_version_path(project_path))
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
}

fn stamp_template_version(project_path: &std::path::Path) -> Result<(), String> {
    let path = template_version_path(project_path);
    if let Some(parent) = path.parent() {
        let _ = ensure_dir(parent);
    }
    std::fs::write(&path, format!("{TEMPLATE_VERSION}\n")).map_err(|e| e.to_string())
}

/// Copy the current control-plane files into a timestamped backup directory.
/// Returns the backup path (shared by --force init and upgrade).
fn backup_control_plane(project_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let openloop_dir = project_path.join(".openloop");
    if !file_exists(&openloop_dir) {
        return None;
    }
    let backup_dir = openloop_dir
        .join("backup")
        .join(now_iso().replace([':', '.'], "-"));
    ensure_dir(&backup_dir).ok()?;
    if let Ok(entries) = std::fs::read_dir(&openloop_dir) {
        for entry in entries.flatten() {
            if entry.file_name() == "backup" {
                continue;
            }
            let src = entry.path();
            let dest = backup_dir.join(entry.file_name());
            if src.is_dir() {
                let _ = super::fsutil::copy_tree(&src, &dest, true);
            } else {
                let _ = std::fs::copy(&src, &dest);
            }
        }
    }
    Some(backup_dir)
}

pub fn extract_templates_to(temp_dir: &std::path::Path) -> Result<(), String> {
    fn extract(dir: &Dir, target: &std::path::Path) -> Result<(), String> {
        for entry in dir.entries() {
            match entry {
                include_dir::DirEntry::Dir(d) => {
                    extract(d, &target.join(d.path().file_name().unwrap_or_default()))?
                }
                include_dir::DirEntry::File(f) => {
                    let dest = target.join(f.path().file_name().unwrap_or_default());
                    if let Some(parent) = dest.parent() {
                        let _ = ensure_dir(parent);
                    }
                    std::fs::write(&dest, f.contents()).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    extract(&TEMPLATES_PROJECT, temp_dir)
}

pub fn initialize_project_from_templates(
    _repo_root: &std::path::Path,
    project: &LinkedProject,
    force: bool,
) -> Result<(), String> {
    let project_path = std::path::Path::new(&project.path);

    // W8: Back up existing control-plane files before re-materializing with --force.
    if force {
        backup_control_plane(project_path);
    }

    // Materialize the embedded templates/project into the target repo.
    let temp_dir = unique_temp_dir("openloop-templates");
    let _ = std::fs::remove_dir_all(&temp_dir);
    extract_templates_to(&temp_dir)?;
    let result = copy_tree(&temp_dir, project_path, force);
    let _ = std::fs::remove_dir_all(&temp_dir);
    result?;

    // Non-Pi providers (claude, codex, …) read AGENTS.md at the repo root.
    materialize_agents_md(project_path)?;

    // Merge template project.json with any existing one.
    let embedded_template_config: serde_json::Value = read_embedded_project_config();
    let existing_project_config: serde_json::Value = read_json_file(
        &project_path.join(".openloop").join("project.json"),
        serde_json::Value::Null,
    )?;
    let mut project_config =
        merge_json_objects(&embedded_template_config, &existing_project_config);

    let project_object = serde_json::json!({
        "alias": project.alias,
        "repoRoot": project.path,
        "initializedAt": now_iso(),
    });
    project_config["project"] = project_object;
    let detected = detect_validation_commands(project_path)?;
    project_config["validation"] = serde_json::json!({
        "lintCommand": detected.lint_command,
        "testCommand": detected.test_command,
        "typecheckCommand": detected.typecheck_command,
    });
    write_json_file(
        &project_path.join(".openloop").join("project.json"),
        &project_config,
    )?;

    let tasks_path = project_path.join(".openloop").join("tasks.json");
    if file_exists(&tasks_path) {
        let mut ledger: serde_json::Value = read_json_file(&tasks_path, serde_json::Value::Null)?;
        if !ledger.is_null() {
            ledger["updatedAt"] = serde_json::json!(now_iso());
            write_json_file(&tasks_path, &ledger)?;
        }
    }

    stamp_template_version(project_path)?;
    Ok(())
}

/// Result of `openloop project upgrade`.
#[derive(Debug, Clone)]
pub struct UpgradeReport {
    pub alias: String,
    /// Version before the upgrade (None when the project predates versioning).
    pub from: Option<u32>,
    pub to: u32,
    pub already_current: bool,
    /// Backup directory written before touching anything, when applicable.
    pub backup_dir: Option<String>,
}

/// Bring a linked project's control-plane templates up to the embedded
/// version, preserving user-owned state:
/// - `.openloop/policy.yaml` and `.openloop/project.json` are never
///   overwritten (project.json is only MERGED — existing values win, new
///   template keys land);
/// - the task ledger, runs, promotions, specs, and every other runtime
///   artifact are untouched;
/// - template-owned files (skill docs, prompts, README) are refreshed from
///   the embedded copies after a timestamped backup;
/// - the AGENTS.md openloop section is re-materialized in place.
pub fn upgrade_project_from_templates(project: &LinkedProject) -> Result<UpgradeReport, String> {
    let project_path = std::path::Path::new(&project.path);
    if !file_exists(&project_path.join(".openloop")) {
        return Err(format!(
            "Project {} is not initialized; run 'openloop project init {}' first.",
            project.alias, project.alias
        ));
    }
    let from = template_version_of(project_path);
    if from == Some(TEMPLATE_VERSION) {
        return Ok(UpgradeReport {
            alias: project.alias.clone(),
            from,
            to: TEMPLATE_VERSION,
            already_current: true,
            backup_dir: None,
        });
    }

    let backup_dir = backup_control_plane(project_path);

    // Extract, strip the user-owned files, then refresh template-owned ones.
    // tasks.json ships in the templates as the initial empty ledger — at
    // upgrade time the project's live ledger is user state and is never
    // touched. (tasks.schema.json is a reference doc and refreshes freely.)
    let temp_dir = unique_temp_dir("openloop-upgrade");
    let _ = std::fs::remove_dir_all(&temp_dir);
    extract_templates_to(&temp_dir)?;
    for user_owned in ["policy.yaml", "project.json", "tasks.json"] {
        let _ = std::fs::remove_file(temp_dir.join(".openloop").join(user_owned));
    }
    let copy = copy_tree(&temp_dir, project_path, true);
    let _ = std::fs::remove_dir_all(&temp_dir);
    copy?;

    // Merge new template keys UNDER the existing project.json (existing
    // values win; validation commands, custom gates, agent, and retention
    // settings are the user's to own).
    let embedded_template_config: serde_json::Value = read_embedded_project_config();
    let existing_project_config: serde_json::Value = read_json_file(
        &project_path.join(".openloop").join("project.json"),
        serde_json::Value::Null,
    )?;
    let merged = merge_json_objects(&embedded_template_config, &existing_project_config);
    write_json_file(
        &project_path.join(".openloop").join("project.json"),
        &merged,
    )?;

    materialize_agents_md(project_path)?;
    stamp_template_version(project_path)?;

    Ok(UpgradeReport {
        alias: project.alias.clone(),
        from,
        to: TEMPLATE_VERSION,
        already_current: false,
        backup_dir: backup_dir.map(|dir| dir.to_string_lossy().to_string()),
    })
}

fn read_embedded_project_config() -> serde_json::Value {
    if let Some(file) = TEMPLATES_PROJECT.get_file(".openloop/project.json") {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(file.contents()) {
            return v;
        }
    }
    serde_json::Value::Null
}

fn merge_json_objects(
    base: &serde_json::Value,
    override_value: &serde_json::Value,
) -> serde_json::Value {
    let mut merged = base.clone();
    if let (Some(_base_obj), Some(override_obj)) = (base.as_object(), override_value.as_object()) {
        for (key, value) in override_obj {
            let base_value = merged.get(key);
            let both_objects =
                base_value.map(|v| v.is_object()).unwrap_or(false) && value.is_object();
            if both_objects {
                let sub = merge_json_objects(base_value.unwrap(), value);
                merged[key] = sub;
            } else {
                merged[key] = value.clone();
            }
        }
    }
    merged
}

const OPENLOOP_AGENTS_SECTION_START: &str = "<!-- openloop:start -->";
const OPENLOOP_AGENTS_SECTION_END: &str = "<!-- openloop:end -->";

fn openloop_agents_section() -> String {
    [
        OPENLOOP_AGENTS_SECTION_START,
        "",
        "## Openloop",
        "",
        "This repository is linked to the Openloop control plane. Before planning or changing code, read:",
        "",
        "1. `.openloop/tasks.json` — task ledger with status, risk, scope, and acceptance criteria",
        "2. `.openloop/policy.yaml` — scope rules (`allowGlobs`, `denyGlobs`, `highRiskAreas`), risk classes, and promotion modes",
        "3. `.openloop/project.json` — validation commands and runtime settings",
        "4. Relevant spec under `.openloop/specs/` if one exists for the current task",
        "",
        "Workflow: implement with minimal, focused changes within the task's declared",
        "scope; run the project's validation commands (lint, test, typecheck) as",
        "configured in `project.json`; stop on validation failure.",
        "",
        "Do not modify runtime-managed files directly: `.openloop/tasks.json`,",
        "`.openloop/runs/`, `.openloop/promotions/`, `.openloop/promotion-results/`.",
        "",
        OPENLOOP_AGENTS_SECTION_END,
        "",
    ]
    .join("\n")
}

fn materialize_agents_md(project_path: &std::path::Path) -> Result<(), String> {
    let agents_md_path = project_path.join("AGENTS.md");
    if !file_exists(&agents_md_path) {
        std::fs::write(&agents_md_path, openloop_agents_section()).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let existing = std::fs::read_to_string(&agents_md_path).map_err(|e| e.to_string())?;
    let start_index = existing.find(OPENLOOP_AGENTS_SECTION_START);
    let end_index = existing.find(OPENLOOP_AGENTS_SECTION_END);
    let section = openloop_agents_section();
    let merged = match (start_index, end_index) {
        (Some(start), Some(end)) if end > start => {
            // Replace any previous openloop block in place.
            let mut s = String::new();
            s.push_str(&existing[..start]);
            s.push_str(&section);
            s.push_str(&existing[end + OPENLOOP_AGENTS_SECTION_END.len()..]);
            s
        }
        (Some(start), _) => {
            // Asymmetric markers (start without end): discard everything from the
            // dangling start marker onward — the openloop block is replaced wholesale.
            format!("{}\n\n{}", existing[..start].trim_end(), section)
        }
        (None, Some(end)) => {
            // Orphaned end marker: drop it and append the canonical section.
            let mut s = String::new();
            s.push_str(&existing[..end]);
            s.push_str(&section);
            s.push_str(&existing[end + OPENLOOP_AGENTS_SECTION_END.len()..]);
            s
        }
        (None, None) => format!("{}\n\n{}", existing.trim_end(), section),
    };
    std::fs::write(&agents_md_path, merged).map_err(|e| e.to_string())
}
