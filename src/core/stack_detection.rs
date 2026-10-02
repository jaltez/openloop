use std::path::Path;

use super::fsutil::file_exists;

#[derive(Debug, Clone, Default)]
pub struct DetectedValidationCommands {
    pub lint_command: Option<String>,
    pub test_command: Option<String>,
    pub typecheck_command: Option<String>,
}

pub fn detect_validation_commands(
    project_path: &Path,
) -> Result<DetectedValidationCommands, String> {
    let package_json_path = project_path.join("package.json");
    if file_exists(&package_json_path) {
        return detect_node_validation_commands(project_path, &package_json_path);
    }

    let pyproject_path = project_path.join("pyproject.toml");
    let requirements_path = project_path.join("requirements.txt");
    if file_exists(&pyproject_path) || file_exists(&requirements_path) {
        return Ok(DetectedValidationCommands {
            lint_command: Some("ruff check .".to_string()),
            test_command: Some("pytest".to_string()),
            typecheck_command: Some("mypy .".to_string()),
        });
    }

    if file_exists(&project_path.join("go.mod")) {
        return Ok(DetectedValidationCommands {
            lint_command: Some("golangci-lint run".to_string()),
            test_command: Some("go test ./...".to_string()),
            typecheck_command: Some("go vet ./...".to_string()),
        });
    }

    if file_exists(&project_path.join("Cargo.toml")) {
        return Ok(DetectedValidationCommands {
            lint_command: Some("cargo clippy".to_string()),
            test_command: Some("cargo test".to_string()),
            typecheck_command: Some("cargo check".to_string()),
        });
    }

    if file_exists(&project_path.join("build.gradle"))
        || file_exists(&project_path.join("build.gradle.kts"))
        || file_exists(&project_path.join("pom.xml"))
    {
        if file_exists(&project_path.join("build.gradle"))
            || file_exists(&project_path.join("build.gradle.kts"))
        {
            return Ok(DetectedValidationCommands {
                lint_command: None,
                test_command: Some("./gradlew test".to_string()),
                typecheck_command: Some("./gradlew compileJava".to_string()),
            });
        }
        return Ok(DetectedValidationCommands {
            lint_command: None,
            test_command: Some("mvn test".to_string()),
            typecheck_command: Some("mvn compile".to_string()),
        });
    }

    if file_exists(&project_path.join("Gemfile")) {
        return Ok(DetectedValidationCommands {
            lint_command: Some("bundle exec rubocop".to_string()),
            test_command: Some("bundle exec rspec".to_string()),
            typecheck_command: None,
        });
    }

    if find_dotnet_project(project_path) {
        return Ok(DetectedValidationCommands {
            lint_command: None,
            test_command: Some("dotnet test".to_string()),
            typecheck_command: Some("dotnet build --no-restore".to_string()),
        });
    }

    Ok(DetectedValidationCommands::default())
}

fn detect_node_validation_commands(
    project_path: &Path,
    package_json_path: &Path,
) -> Result<DetectedValidationCommands, String> {
    // TS parity: JSON.parse throws on a malformed package.json.
    let raw = std::fs::read_to_string(package_json_path).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let scripts = parsed
        .get("scripts")
        .and_then(|s| s.as_object())
        .cloned()
        .unwrap_or_default();
    let runner = detect_node_runner(project_path);

    let has = |key: &str| {
        scripts
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty()) // TS truthiness: "" scripts are falsy
            .unwrap_or(false)
    };
    Ok(DetectedValidationCommands {
        lint_command: has("lint").then(|| format!("{runner} run lint")),
        test_command: has("test").then(|| format!("{runner} run test")),
        typecheck_command: if has("typecheck") {
            Some(format!("{runner} run typecheck"))
        } else if has("check") {
            Some(format!("{runner} run check"))
        } else {
            None
        },
    })
}

fn detect_node_runner(project_path: &Path) -> &'static str {
    const CANDIDATES: [(&str, &str); 4] = [
        ("bun.lock", "bun"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
    ];
    for (filename, runner) in CANDIDATES {
        if file_exists(&project_path.join(filename)) {
            return runner;
        }
    }
    "npm"
}

fn find_dotnet_project(project_path: &Path) -> bool {
    match std::fs::read_dir(project_path) {
        Ok(entries) => entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.ends_with(".csproj") || name.ends_with(".sln") || name.ends_with(".fsproj")
        }),
        Err(_) => false,
    }
}
