use std::path::Path;

pub fn resolve_model(
    project_path: &Path,
    cli_override: Option<&str>,
    app_home_override: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(m) = cli_override {
        if !m.is_empty() {
            return Ok(Some(m.to_string()));
        }
    }

    let project_config = super::project_config::load_project_config(project_path)?;
    if let Some(model) = &project_config.pi.model {
        if !model.is_empty() {
            return Ok(Some(model.clone()));
        }
    }

    let global_config = super::global_config::load_global_config(app_home_override)?;
    Ok(global_config.model.clone())
}
