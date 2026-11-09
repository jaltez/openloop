use super::fsutil::{read_json_file, write_json_file};
use super::now_iso;
use super::paths::projects_registry_path;
use super::types::*;

fn empty_registry() -> ProjectsRegistry {
    ProjectsRegistry {
        version: 1,
        projects: Vec::new(),
    }
}

pub fn load_projects_registry(app_home_override: Option<&str>) -> Result<ProjectsRegistry, String> {
    read_json_file(&projects_registry_path(app_home_override), empty_registry())
}

pub fn save_projects_registry(
    registry: &ProjectsRegistry,
    app_home_override: Option<&str>,
) -> Result<(), String> {
    write_json_file(&projects_registry_path(app_home_override), registry)
}

pub fn add_project(
    alias: &str,
    project_path: &str,
    app_home_override: Option<&str>,
) -> Result<LinkedProject, String> {
    let mut registry = load_projects_registry(app_home_override)?;
    if registry.projects.iter().any(|p| p.alias == alias) {
        return Err(format!("Project alias already exists: {alias}"));
    }
    let now = now_iso();
    let project = LinkedProject {
        alias: alias.to_string(),
        path: project_path.to_string(),
        default_branch: None,
        initialized: false,
        created_at: now.clone(),
        updated_at: now,
    };
    registry.projects.push(project.clone());
    save_projects_registry(&registry, app_home_override)?;
    Ok(project)
}

pub fn list_projects(app_home_override: Option<&str>) -> Result<Vec<LinkedProject>, String> {
    let mut registry = load_projects_registry(app_home_override)?;
    registry.projects.sort_by(|a, b| a.alias.cmp(&b.alias));
    Ok(registry.projects)
}

pub fn get_project(alias: &str, app_home_override: Option<&str>) -> Result<LinkedProject, String> {
    let registry = load_projects_registry(app_home_override)?;
    registry
        .projects
        .into_iter()
        .find(|p| p.alias == alias)
        .ok_or_else(|| format!("Unknown project alias: {alias}"))
}

pub fn mark_project_initialized(
    alias: &str,
    app_home_override: Option<&str>,
) -> Result<LinkedProject, String> {
    let mut registry = load_projects_registry(app_home_override)?;
    let project = registry
        .projects
        .iter_mut()
        .find(|p| p.alias == alias)
        .ok_or_else(|| format!("Unknown project alias: {alias}"))?;
    project.initialized = true;
    project.updated_at = now_iso();
    let result = project.clone();
    save_projects_registry(&registry, app_home_override)?;
    Ok(result)
}

pub fn project_exists(alias: &str, app_home_override: Option<&str>) -> Result<bool, String> {
    let registry = load_projects_registry(app_home_override)?;
    Ok(registry.projects.iter().any(|p| p.alias == alias))
}

pub fn update_project_path(
    alias: &str,
    new_path: &str,
    app_home_override: Option<&str>,
) -> Result<LinkedProject, String> {
    let mut registry = load_projects_registry(app_home_override)?;
    let project = registry
        .projects
        .iter_mut()
        .find(|p| p.alias == alias)
        .ok_or_else(|| format!("Unknown project alias: {alias}"))?;
    project.path = new_path.to_string();
    project.updated_at = now_iso();
    let result = project.clone();
    save_projects_registry(&registry, app_home_override)?;
    Ok(result)
}

pub fn remove_project(alias: &str, app_home_override: Option<&str>) -> Result<(), String> {
    let mut registry = load_projects_registry(app_home_override)?;
    let index = registry
        .projects
        .iter()
        .position(|p| p.alias == alias)
        .ok_or_else(|| format!("Unknown project alias: {alias}"))?;
    registry.projects.remove(index);
    save_projects_registry(&registry, app_home_override)
}
