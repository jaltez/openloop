use std::fs;

use openloop::core::stack_detection::detect_validation_commands;

fn temp_project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("openloop-stack-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn node_project_detects_scripts_and_runner() {
    let dir = temp_project("node");
    fs::write(
        dir.join("package.json"),
        r#"{"scripts": {"lint": "eslint .", "test": "vitest", "typecheck": "tsc"}}"#,
    )
    .unwrap();
    fs::write(dir.join("bun.lock"), "").unwrap();
    let detected = detect_validation_commands(&dir);
    assert_eq!(detected.lint_command.as_deref(), Some("bun run lint"));
    assert_eq!(detected.test_command.as_deref(), Some("bun run test"));
    assert_eq!(detected.typecheck_command.as_deref(), Some("bun run typecheck"));
}

#[test]
fn node_runner_falls_back_to_npm() {
    let dir = temp_project("npm");
    fs::write(
        dir.join("package.json"),
        r#"{"scripts": {"lint": "eslint .", "test": "vitest", "check": "tsc"}}"#,
    )
    .unwrap();
    let detected = detect_validation_commands(&dir);
    assert_eq!(detected.lint_command.as_deref(), Some("npm run lint"));
    // typecheckCommand falls back to the check script when typecheck is absent.
    assert_eq!(detected.typecheck_command.as_deref(), Some("npm run check"));
}

#[test]
fn node_without_scripts_yields_nulls() {
    let dir = temp_project("bare");
    fs::write(dir.join("package.json"), "{}").unwrap();
    let detected = detect_validation_commands(&dir);
    assert!(detected.lint_command.is_none());
    assert!(detected.test_command.is_none());
    assert!(detected.typecheck_command.is_none());
}

#[test]
fn python_go_rust_java_dotnet_ruby() {
    let py = temp_project("py");
    fs::write(py.join("requirements.txt"), "").unwrap();
    let d = detect_validation_commands(&py);
    assert_eq!(d.lint_command.as_deref(), Some("ruff check ."));
    assert_eq!(d.test_command.as_deref(), Some("pytest"));

    let go = temp_project("go");
    fs::write(go.join("go.mod"), "").unwrap();
    let d = detect_validation_commands(&go);
    assert_eq!(d.test_command.as_deref(), Some("go test ./..."));

    let rs = temp_project("rs");
    fs::write(rs.join("Cargo.toml"), "").unwrap();
    let d = detect_validation_commands(&rs);
    assert_eq!(d.lint_command.as_deref(), Some("cargo clippy"));
    assert_eq!(d.typecheck_command.as_deref(), Some("cargo check"));

    let gradle = temp_project("gradle");
    fs::write(gradle.join("build.gradle"), "").unwrap();
    let d = detect_validation_commands(&gradle);
    assert_eq!(d.test_command.as_deref(), Some("./gradlew test"));

    let mvn = temp_project("mvn");
    fs::write(mvn.join("pom.xml"), "").unwrap();
    let d = detect_validation_commands(&mvn);
    assert_eq!(d.test_command.as_deref(), Some("mvn test"));

    let cs = temp_project("cs");
    fs::write(cs.join("App.csproj"), "").unwrap();
    let d = detect_validation_commands(&cs);
    assert_eq!(d.test_command.as_deref(), Some("dotnet test"));

    let rb = temp_project("rb");
    fs::write(rb.join("Gemfile"), "").unwrap();
    let d = detect_validation_commands(&rb);
    assert_eq!(d.lint_command.as_deref(), Some("bundle exec rubocop"));
    assert!(d.typecheck_command.is_none());
}

#[test]
fn unknown_stack_yields_nulls() {
    let dir = temp_project("unknown");
    let detected = detect_validation_commands(&dir);
    assert!(detected.lint_command.is_none());
    assert!(detected.test_command.is_none());
    assert!(detected.typecheck_command.is_none());
}
