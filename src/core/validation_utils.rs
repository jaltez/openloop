use super::types::*;

pub fn get_configured_validation_names(project_config: &ProjectConfig) -> Vec<ValidationName> {
    super::validation::configured_steps(project_config)
        .into_iter()
        .map(|(name, _command)| name)
        .collect()
}

pub fn has_all_required_auto_merge_validations(
    validation: &[ValidationSummary],
    project_config: &ProjectConfig,
) -> bool {
    let required = get_configured_validation_names(project_config);
    if required.is_empty() {
        return false;
    }
    required.iter().all(|name| {
        validation
            .iter()
            .any(|item| item.name == *name && item.exit_code == 0)
    })
}
