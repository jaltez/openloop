use std::path::Path;

use super::fsutil::{read_json_file, write_json_file};
use super::types::*;

pub fn default_project_config() -> ProjectConfig {
    ProjectConfig {
        version: 1,
        project: ProjectMeta {
            alias: None,
            repo_root: None,
            initialized_at: None,
        },
        pi: PiConfig {
            model: None,
            prompt_files: vec![
                ".agents/skills/openloop/SKILL.md".to_string(),
                ".pi/SYSTEM.md".to_string(),
                ".pi/APPEND_SYSTEM.md".to_string(),
            ],
        },
        agent: Some(AgentConfig {
            agent_type: Some("pi".to_string()),
            command: None,
            extra_args: None,
        }),
        runtime: ProjectRuntime {
            use_worktree: false,
            branch_prefix: "openloop/".to_string(),
            pr_command: None,
            worktree_setup_command: None,
        },
        validation: ProjectValidation {
            custom: None,
            lint_command: None,
            test_command: None,
            typecheck_command: None,
        },
        risk: ProjectRisk {
            default_unknown_area_classification: "medium-risk".to_string(),
            require_policy_for_auto_merge: true,
        },
        issue_source: None,
        hooks: None,
        schedule: None,
        schedule_state: None,
        review: Some(ProjectReview { enabled: false }),
        verification: Some(ProjectVerification { enabled: true }),
        retention: None,
    }
}

pub fn project_config_path(project_path: &Path) -> std::path::PathBuf {
    project_path.join(".openloop").join("project.json")
}

pub fn load_project_config(project_path: &Path) -> Result<ProjectConfig, String> {
    read_json_file(&project_config_path(project_path), default_project_config())
}

pub fn save_project_config(project_path: &Path, config: &ProjectConfig) -> Result<(), String> {
    write_json_file(&project_config_path(project_path), config)
}
