use super::types::*;

pub fn get_configured_validation_names(project_config: &ProjectConfig) -> Vec<ValidationName> {
    let mut configured = Vec::new();
    if project_config.validation.lint_command.is_some() {
        configured.push(ValidationName::Lint);
    }
    if project_config.validation.test_command.is_some() {
        configured.push(ValidationName::Test);
    }
    if project_config.validation.typecheck_command.is_some() {
        configured.push(ValidationName::Typecheck);
    }
    configured
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
